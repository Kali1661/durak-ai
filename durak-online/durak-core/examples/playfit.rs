//! How well do the imagined players play near the end of the stock?
//!
//! ```text
//! cargo run --release -p durak-core --example playfit -- [games] [deck] [positions per solve] [most cards left to draw]
//! ```
//!
//! Plays two-player games with the heuristic playout player and stops at
//! positions with a few cards left to draw. With every card known, the exact
//! solver says who wins with perfect play and which moves win. Prints:
//!
//! 1. how well playouts from those positions predict the winner (lower
//!    loss = the imagined players are closer to good play);
//! 2. for a defender who can beat the one card on the table: how often
//!    picking it up anyway is the better move, by the card that would have to
//!    be spent - is keeping high cards and taking really right?
//! 3. where the heuristic player's own move throws away a won or drawn game:
//!    what it played and what would have been better, most frequent first.

use std::sync::Mutex;

use durak_core::{
    playout_share, policy, solve_exact_deal, Action, ExactValue, GameConfig, GameState, Phase, Playout, Rank, Style,
};
use rand::{rngs::StdRng, Rng, SeedableRng};

const KEEPS: [bool; 2] = [false, true];

struct Sample {
    stock: usize,
    y: f64,
    playouts: [f64; KEEPS.len()],
}

/// A defence with one card to beat: (stock, kind of cheapest beater, kind of
/// card to beat, cards on the way: best result by beating, result of taking).
struct Defence {
    stock: usize,
    beater: usize,
    card: usize,
    beat: u8,
    take: u8,
    /// What the heuristic player does: true = takes.
    heuristic_takes: bool,
}

/// What kind of move this is, for the table of mistakes.
fn kind(s: &GameState, a: &Action) -> &'static str {
    let trump = |c: &durak_core::Card| c.id().is_some_and(|(suit, _)| suit == s.trump);
    match a {
        Action::Attack { card } if s.table.is_empty() => {
            if trump(card) {
                "lead a trump"
            } else {
                "lead a plain card"
            }
        }
        Action::Attack { card } => {
            if trump(card) {
                "add a trump"
            } else {
                "add a plain card"
            }
        }
        Action::PassAttack | Action::LetDefend => "add nothing",
        Action::Defend { card, .. } => {
            if trump(card) {
                "beat with a trump"
            } else {
                "beat with a plain card"
            }
        }
        Action::Take => "take",
        Action::Reflect { card } => {
            if trump(card) {
                "pass on with a trump"
            } else {
                "pass on"
            }
        }
        Action::ReflectTrump { .. } => "show a trump",
        Action::Throw { card: Some(card) } => {
            if trump(card) {
                "throw in a trump"
            } else {
                "throw in a plain card"
            }
        }
        Action::Throw { card: None } => "throw in nothing",
    }
}

type Mistakes = std::collections::HashMap<(usize, &'static str, &'static str), u32>;
type Decisions = std::collections::HashMap<(usize, &'static str), u32>;

fn rank(v: ExactValue) -> u8 {
    match v {
        ExactValue::Loss => 0,
        ExactValue::Draw => 1,
        ExactValue::Win => 2,
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let games: u64 = args.first().map_or(2000, |s| s.parse().unwrap());
    let deck: usize = args.get(1).map_or(24, |s| s.parse().unwrap());
    let nodes: u64 = args.get(2).map_or(2_000_000, |s| s.parse().unwrap());
    let max_stock: usize = args.get(3).map_or(6, |s| s.parse().unwrap());
    let config = GameConfig {
        deck_size: deck,
        num_players: 2,
        reflecting: true,
        trump_reflecting: false,
        first_trick_limit: true,
        classic_ending: true,
        cards_per_attack: 6,
    };

    let samples: Mutex<Vec<Sample>> = Mutex::new(vec![]);
    let defences: Mutex<Vec<Defence>> = Mutex::new(vec![]);
    let unsolved = Mutex::new(0u32);
    // (stock bucket, what the heuristic played, what was better) -> count;
    // and decisions per (stock bucket, what it played).
    let mistakes: Mutex<Mistakes> = Mutex::new(Default::default());
    let decisions: Mutex<Decisions> = Mutex::new(Default::default());
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get().saturating_sub(1).max(1)) as u64;
    std::thread::scope(|scope| {
        for t in 0..threads {
            let (samples, defences, unsolved) = (&samples, &defences, &unsolved);
            let (mistakes, decisions) = (&mistakes, &decisions);
            scope.spawn(move || {
                let (mut local, mut local_d, mut missed) = (vec![], vec![], 0u32);
                let mut local_m: Mistakes = Default::default();
                let mut local_n: Decisions = Default::default();
                for g in (t..games).step_by(threads as usize) {
                    let mut rng = StdRng::seed_from_u64(g);
                    // The solver and the playouts see every card.
                    let mut s = GameState::new_computer_game(70_000 + g, config).unwrap();
                    for _ in 0..3000 {
                        if s.is_terminal() {
                            break;
                        }
                        let stock = s.stock.len();
                        let acts = s.legal_actions();
                        let trick_start = s.phase == Phase::Attacking && s.table.is_empty();
                        let one_to_beat = s.phase == Phase::Defending && s.table.len() == 1 && s.table[0].defense.is_none();
                        if (1..=max_stock).contains(&stock) && (trick_start || one_to_beat) && rng.gen_bool(0.5) {
                            let open = reveal(&s);
                            match solve_exact_deal(&open, nodes).ok().flatten() {
                                Some(r) if r.value.is_some() && r.actions.iter().all(|a| a.value.is_some()) => {
                                    if trick_start {
                                        local.push(Sample {
                                            stock,
                                            y: f64::from(rank(r.value.unwrap())) / 2.0,
                                            playouts: KEEPS.map(|keep| {
                                                playout_share(&open, 200, &mut rng, Style { hold_trumps: keep, ..Style::default() }).unwrap_or(0.5)
                                            }),
                                        });
                                    } else {
                                        let trump = s.trump;
                                        let target = s.table[0].attack.id().unwrap();
                                        let worth = |c: &(durak_core::Suit, Rank)| c.1 as i32 + if c.0 == trump { 20 } else { 0 };
                                        let mut beat: Option<u8> = None;
                                        let mut cheapest: Option<(durak_core::Suit, Rank)> = None;
                                        let mut take = 0;
                                        for a in &r.actions {
                                            let v = rank(a.value.unwrap());
                                            match &a.action {
                                                Action::Defend { card, .. } => {
                                                    beat = Some(beat.map_or(v, |b| b.max(v)));
                                                    let id = card.id().unwrap();
                                                    if cheapest.is_none_or(|c| worth(&id) < worth(&c)) {
                                                        cheapest = Some(id);
                                                    }
                                                }
                                                Action::Take => take = v,
                                                _ => {}
                                            }
                                        }
                                        if let (Some(beat), Some(cheapest)) = (beat, cheapest) {
                                            let chosen = policy::heuristic_action(&s).unwrap();
                                            local_d.push(Defence {
                                                stock,
                                                beater: match cheapest {
                                                    (suit, _) if suit != trump => 0,
                                                    (_, r) if r <= Rank::Ten => 1,
                                                    (_, r) if r <= Rank::Queen => 2,
                                                    _ => 3,
                                                },
                                                card: match target {
                                                    (suit, _) if suit == trump => 2,
                                                    (_, r) if r >= Rank::Queen => 1,
                                                    _ => 0,
                                                },
                                                beat,
                                                take,
                                                heuristic_takes: matches!(chosen, Action::Take),
                                            });
                                        }
                                    }
                                }
                                _ => missed += 1,
                            }
                        }
                        // 3. Is the heuristic player's own move here a mistake?
                        if stock <= max_stock && acts.len() >= 2 && rng.gen_bool(0.25) {
                            let open = reveal(&s);
                            if let Some(r) = solve_exact_deal(&open, nodes / 4).ok().flatten() {
                                if r.actions.iter().all(|a| a.value.is_some()) {
                                    let chosen = policy::heuristic_action(&s).unwrap();
                                    let value = |a: &Action| {
                                        r.actions.iter().find(|x| x.action.same_move(a)).map_or(0, |x| rank(x.value.unwrap()))
                                    };
                                    let bucket = match stock {
                                        0 => 0,
                                        1..=2 => 1,
                                        _ => 2,
                                    };
                                    *local_n.entry((bucket, kind(&s, &chosen))).or_default() += 1;
                                    let best = acts.iter().max_by_key(|a| value(a)).unwrap();
                                    if value(best) > value(&chosen) {
                                        // Prefer naming a better move of another kind, if there is one.
                                        let other = acts
                                            .iter()
                                            .filter(|a| value(a) == value(best))
                                            .map(|a| kind(&s, a))
                                            .find(|k| *k != kind(&s, &chosen))
                                            .unwrap_or("another card");
                                        *local_m.entry((bucket, kind(&s, &chosen), other)).or_default() += 1;
                                    }
                                }
                            }
                        }
                        let i = policy::choose(&s, &acts, Playout::Heuristic, &mut rng);
                        s.apply(&acts[i]).unwrap();
                    }
                }
                samples.lock().unwrap().extend(local);
                defences.lock().unwrap().extend(local_d);
                *unsolved.lock().unwrap() += missed;
                for (k, v) in local_m {
                    *mistakes.lock().unwrap().entry(k).or_default() += v;
                }
                for (k, v) in local_n {
                    *decisions.lock().unwrap().entry(k).or_default() += v;
                }
            });
        }
    });
    let samples = samples.into_inner().unwrap();
    let defences = defences.into_inner().unwrap();
    println!(
        "{} trick starts and {} defences solved with 1 to {max_stock} cards left to draw ({} too big)",
        samples.len(),
        defences.len(),
        unsolved.into_inner().unwrap()
    );

    println!("\n1. Playouts against perfect play: log loss as the players are, and holding trumps back at the end");
    for (name, lo, hi) in [("all", 1, 99), ("1-2 left", 1, 2), ("3-4 left", 3, 4), ("5+ left", 5, 99)] {
        let part: Vec<&Sample> = samples.iter().filter(|s| s.stock >= lo && s.stock <= hi).collect();
        if part.is_empty() {
            continue;
        }
        let losses: Vec<String> = (0..KEEPS.len())
            .map(|k| {
                let ll = part
                    .iter()
                    .map(|s| {
                        let p = s.playouts[k].clamp(0.02, 0.98);
                        -(s.y * p.ln() + (1.0 - s.y) * (1.0 - p).ln())
                    })
                    .sum::<f64>()
                    / part.len() as f64;
                format!("{ll:.3}")
            })
            .collect();
        println!("  {name:<10} {:>6} positions: {}", part.len(), losses.join("  "));
    }

    println!("\n2. One card to beat, and the defender can: what perfect play says");
    let table = |title: &str, names: &[&str], key: &dyn Fn(&Defence) -> usize| {
        println!("  {title}");
        println!("  {:<18} {:>7} {:>14} {:>14} {:>10} {:>18}", "", "cases", "taking better", "beating better", "the same", "heuristic takes");
        for (k, name) in names.iter().enumerate() {
            let part: Vec<&Defence> = defences.iter().filter(|d| key(d) == k).collect();
            if part.is_empty() {
                continue;
            }
            let n = part.len() as f64;
            let pct = |c: usize| format!("{:.0}%", 100.0 * c as f64 / n);
            println!(
                "  {name:<18} {:>7} {:>14} {:>14} {:>10} {:>18}",
                part.len(),
                pct(part.iter().filter(|d| d.take > d.beat).count()),
                pct(part.iter().filter(|d| d.beat > d.take).count()),
                pct(part.iter().filter(|d| d.beat == d.take).count()),
                pct(part.iter().filter(|d| d.heuristic_takes).count()),
            );
        }
    };
    table("by the cheapest card that beats it", &["a plain card", "trump up to 10", "trump J or Q", "trump K or A"], &|d| d.beater);
    table("by the card to beat", &["plain up to jack", "plain Q or more", "a trump"], &|d| d.card);
    table("by cards left to draw", &["", "1", "2", "3", "4", "5", "6", "7", "8"], &|d| d.stock.min(8));

    println!("\n3. The heuristic player's moves that give away a better result");
    let mistakes = mistakes.into_inner().unwrap();
    let decisions = decisions.into_inner().unwrap();
    for (bucket, name) in ["nothing left to draw", "1-2 left to draw", "3 or more left"].iter().enumerate() {
        let total: u32 = decisions.iter().filter(|(k, _)| k.0 == bucket).map(|(_, v)| *v).sum();
        let wrong: u32 = mistakes.iter().filter(|(k, _)| k.0 == bucket).map(|(_, v)| *v).sum();
        if total == 0 {
            continue;
        }
        println!("  {name}: {total} decisions, {wrong} mistakes ({:.1}%)", 100.0 * f64::from(wrong) / f64::from(total));
        let mut rows: Vec<_> = mistakes.iter().filter(|(k, _)| k.0 == bucket).collect();
        rows.sort_by_key(|(_, v)| std::cmp::Reverse(**v));
        for ((_, played, better), n) in rows.into_iter().take(14) {
            let of = decisions.get(&(bucket, *played)).copied().unwrap_or(0);
            println!("    {n:>5}  played \"{played}\" ({:.0}% of the {of} times), better: {better}", 100.0 * f64::from(*n) / f64::from(of.max(1)));
        }
    }
}

/// The same position with every card face up (the dealt game knows them all;
/// only who may see them differs).
fn reveal(s: &GameState) -> GameState {
    let mut v = serde_json::to_value(s).unwrap();
    fn walk(v: &mut serde_json::Value) {
        match v {
            serde_json::Value::Object(m) => {
                if m.contains_key("suit") && m.contains_key("rank") {
                    m.insert("type".into(), "public".into());
                }
                for x in m.values_mut() {
                    walk(x);
                }
            }
            serde_json::Value::Array(a) => a.iter_mut().for_each(walk),
            _ => {}
        }
    }
    walk(&mut v);
    serde_json::from_value(v).unwrap()
}
