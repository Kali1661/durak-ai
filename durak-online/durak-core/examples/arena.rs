//! Play engines against each other to measure whether a change helps.
//!
//! ```text
//! cargo run --release -p durak-core --example arena -- match <A> <B> [deals] [deals per search] [playouts] [players] [deck]
//! cargo run --release -p durak-core --example arena -- positions <file> [deals per search] [playouts]
//! ```
//!
//! Engines: `random` (any legal move), `rules` (the heuristic playout player
//! on its own), `mcts` (search with random playouts, as before) and `mcts-h`
//! (search with heuristic playouts). Search settings follow a colon, e.g.
//! `mcts-h:prior=1,xnodes=100000,xstock=6,c=1.41,cut=1,d=64,p=250,k=3,kstock=6,tree=1` (see `engine`;
//! `d` and `p` give that engine its own deals and playouts per search).
//!
//! `match` plays every deal once per seat for A, so both sides get the same
//! cards, and reports A's results with a 95% margin. In two-player games both
//! engines switch to the exact solver once nothing is left to draw (as the app
//! does), so only play before that differs.
//!
//! `positions` shows what both kinds of search suggest in recorded positions
//! (see `examples/data/positions.txt`).

use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::time::Instant;

use durak_core::{
    is_exactly_solvable, policy, solve, solve_exact, Action, Card, ExactValue, GameConfig, GameState,
    Outcome, Playout, Rank, SolveParams, Suit,
};
use rand::{rngs::StdRng, Rng, SeedableRng};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Engine {
    Random,
    Rules,
    Search(Search),
}

/// Search settings that are not about effort.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Search {
    playout: Playout,
    prior: f64,
    exact_nodes: u64,
    exact_stock: u8,
    exploration: f64,
    cutoff: bool,
    /// Effort for this engine instead of the match's (0 = the match's).
    deals: u32,
    playouts: u32,
    /// Moves searched per decision below the root (0 = all), while at
    /// least `kstock` cards are left to draw.
    candidates: u8,
    kstock: u8,
    /// One tree over all deals.
    tree: bool,
}

/// `random`, `rules`, `mcts` (random playouts), `mcts-h` (heuristic
/// playouts), optionally with settings: `mcts-h:prior=1,xnodes=100000,xstock=6,c=1.41`.
fn engine(spec: &str) -> Engine {
    let (name, opts) = spec.split_once(':').unwrap_or((spec, ""));
    let playout = match name {
        "random" => return Engine::Random,
        "rules" => return Engine::Rules,
        "mcts" => Playout::Random,
        "mcts-h" => Playout::Heuristic,
        other => panic!("unknown engine {other}; use random, rules, mcts or mcts-h"),
    };
    let mut e = Search { playout, prior: 0.0, exact_nodes: 0, exact_stock: 0, exploration: 1.41, cutoff: false, deals: 0, playouts: 0, candidates: 0, kstock: 0, tree: false };
    for kv in opts.split(',').filter(|s| !s.is_empty()) {
        let (k, v) = kv.split_once('=').expect("settings look like key=value");
        match k {
            "prior" => e.prior = v.parse().unwrap(),
            "xnodes" => e.exact_nodes = v.parse().unwrap(),
            "xstock" => e.exact_stock = v.parse().unwrap(),
            "c" => e.exploration = v.parse().unwrap(),
            "cut" => e.cutoff = v == "1",
            "d" => e.deals = v.parse().unwrap(),
            "p" => e.playouts = v.parse().unwrap(),
            "k" => e.candidates = v.parse().unwrap(),
            "kstock" => e.kstock = v.parse().unwrap(),
            "tree" => e.tree = v == "1",
            other => panic!("unknown setting {other}"),
        }
    }
    Engine::Search(e)
}

#[derive(Clone, Copy)]
struct Effort {
    deals: u32,
    playouts: u32,
}

const EXACT_NODES: u64 = 300_000;

fn choose(state: &GameState, e: Engine, effort: Effort, seed: u64) -> Action {
    if state.num_players() == 2 && is_exactly_solvable(state) {
        if let Some(a) = exact_choice(state, effort, seed) {
            return a;
        }
    }
    match e {
        Engine::Random => {
            let acts = state.legal_actions();
            acts[StdRng::seed_from_u64(seed).gen_range(0..acts.len())].clone()
        }
        Engine::Rules => policy::heuristic_action(state).expect("game running"),
        Engine::Search(e) => search(state, e, effort, seed, None),
    }
}

/// A winning move if the solver finds one; otherwise the search, kept off proven losses.
fn exact_choice(state: &GameState, effort: Effort, seed: u64) -> Option<Action> {
    let r = solve_exact(state, EXACT_NODES).ok()??;
    if let Some(win) = r.actions.iter().find(|a| a.value == Some(ExactValue::Win)) {
        return Some(win.action.clone());
    }
    let safe: Vec<Action> =
        r.actions.iter().filter(|a| a.value != Some(ExactValue::Loss)).map(|a| a.action.clone()).collect();
    if safe.len() == 1 {
        return Some(safe[0].clone());
    }
    if safe.is_empty() {
        return Some(r.actions[0].action.clone());
    }
    // The same search for both engines here, so the endgame is a level field.
    let plain = Search { playout: Playout::Heuristic, prior: 0.0, exact_nodes: 0, exact_stock: 0, exploration: 1.41, cutoff: false, deals: 0, playouts: 0, candidates: 0, kstock: 0, tree: false };
    Some(search(state, plain, effort, seed, Some(&safe)))
}

fn search(state: &GameState, e: Search, effort: Effort, seed: u64, only: Option<&[Action]>) -> Action {
    let params = SolveParams {
        determinizations: if e.deals > 0 { e.deals } else { effort.deals },
        rollouts: if e.playouts > 0 { e.playouts } else { effort.playouts },
        playout: e.playout,
        prior_weight: e.prior,
        exact_nodes: e.exact_nodes,
        exact_stock: e.exact_stock,
        exploration: e.exploration,
        cutoff: e.cutoff,
        candidates: e.candidates,
        candidates_min_stock: e.kstock,
        shared_tree: e.tree,
        ..SolveParams::default()
    };
    let r = solve(state, params, seed).expect("search runs");
    r.actions
        .iter()
        .find(|a| only.is_none_or(|o| o.iter().any(|x| x.same_move(&a.action))))
        .unwrap_or(&r.actions[0])
        .action
        .clone()
}

/// Play one game; returns the outcome. `seats[i]` is the engine in seat i.
fn play(seed: u64, config: GameConfig, seats: &[Engine], effort: Effort) -> Outcome {
    let mut s = GameState::new_computer_game(seed, config).expect("valid config");
    for ply in 0..3000u64 {
        if s.is_terminal() {
            return s.outcome();
        }
        let actor = s.actor_to_move();
        let a = choose(&s, seats[actor.index()], effort, seed.wrapping_mul(1_000_003).wrapping_add(ply));
        s.apply(&a).expect("chosen move is legal");
    }
    Outcome::Draw // never seen; guards against an endless game
}

fn run_match(args: &[String]) {
    let a = engine(&args[0]);
    let b = engine(&args[1]);
    let deals: usize = args.get(2).map_or(100, |s| s.parse().unwrap());
    let effort = Effort {
        deals: args.get(3).map_or(8, |s| s.parse().unwrap()),
        playouts: args.get(4).map_or(250, |s| s.parse().unwrap()),
    };
    let players: usize = args.get(5).map_or(2, |s| s.parse().unwrap());
    let deck: usize = args.get(6).map_or(36, |s| s.parse().unwrap());
    let config = GameConfig {
        deck_size: deck,
        num_players: players,
        reflecting: true,
        trump_reflecting: false,
        first_trick_limit: true,
        classic_ending: true,
        cards_per_attack: 6,
    };
    println!(
        "{a:?} vs {b:?}: {deals} deals x {players} seats, {players} players, {deck} cards, search {}x{} playouts",
        effort.deals, effort.playouts
    );

    // One job per (deal, seat for A).
    let jobs: Vec<(u64, usize)> = (0..deals as u64).flat_map(|d| (0..players).map(move |seat| (d, seat))).collect();
    let next = AtomicUsize::new(0);
    // Running tally: games where A was the durak, and games finished.
    let lost = AtomicU32::new(0);
    let played = AtomicU32::new(0);
    let started = Instant::now();
    let results: Vec<(u64, usize, Outcome)> = std::thread::scope(|scope| {
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
        let handles: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let mut out = vec![];
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some(&(deal, seat)) = jobs.get(i) else { break };
                        let mut seats = vec![b; players];
                        seats[seat] = a;
                        let o = play(1000 + deal, config, &seats, effort);
                        let a_lost = matches!(o, Outcome::Durak { player } if player.index() == seat);
                        let l = lost.fetch_add(u32::from(a_lost), Ordering::Relaxed) + u32::from(a_lost);
                        let n = played.fetch_add(1, Ordering::Relaxed) + 1;
                        out.push((deal, seat, o));
                        if n % 20 == 0 {
                            eprintln!(
                                "  {n} / {} games, A the durak so far {:.1}%, {:.0} s",
                                jobs.len(),
                                100.0 * f64::from(l) / f64::from(n),
                                started.elapsed().as_secs_f64()
                            );
                        }
                    }
                    out
                })
            })
            .collect();
        handles.into_iter().flat_map(|h| h.join().unwrap()).collect()
    });

    // From A's seat: not the durak = win, the durak = loss.
    let (mut win, mut loss, mut draw) = (0, 0, 0);
    for (_, seat, o) in &results {
        match o {
            Outcome::Draw | Outcome::InProgress => draw += 1,
            Outcome::Durak { player } if player.index() == *seat => loss += 1,
            Outcome::Durak { .. } => win += 1,
        }
    }
    let n = results.len() as f64;
    // With random seats the fair share of not losing is (players-1)/players.
    let fair = (players as f64 - 1.0) / players as f64;
    let rate = loss as f64 / n;
    let margin = 1.96 * (rate * (1.0 - rate) / n).sqrt();
    println!(
        "A not the durak {win}, A the durak {loss}, draws {draw} of {} games ({:.0} s)",
        results.len(),
        started.elapsed().as_secs_f64()
    );
    println!(
        "A was the durak in {:.1}% ± {:.1}% of games (an even match: {:.1}%)",
        rate * 100.0,
        margin * 100.0,
        (1.0 - fair) * 100.0
    );
}

// ── Recorded positions ──────────────────────────────────────────────────────

fn card_json(code: &str, kind: &str) -> serde_json::Value {
    if code == "??" {
        return serde_json::json!({ "type": "unknown" });
    }
    let rank = match &code[0..1] {
        "6" => Rank::Six, "7" => Rank::Seven, "8" => Rank::Eight, "9" => Rank::Nine, "T" => Rank::Ten,
        "J" => Rank::Jack, "Q" => Rank::Queen, "K" => Rank::King, "A" => Rank::Ace,
        "2" => Rank::Two, "3" => Rank::Three, "4" => Rank::Four, "5" => Rank::Five,
        other => panic!("bad rank {other}"),
    };
    let card = if kind == "public" { Card::public(suit(&code[1..2]), rank) } else { Card::private(suit(&code[1..2]), rank) };
    serde_json::to_value(card).unwrap()
}

fn suit(letter: &str) -> Suit {
    match letter {
        "C" => Suit::Clubs, "D" => Suit::Diamonds, "H" => Suit::Hearts, "S" => Suit::Spades,
        other => panic!("bad suit {other}"),
    }
}

fn cards(list: &str, kind: &str) -> Vec<serde_json::Value> {
    list.split_whitespace().map(|c| card_json(c, kind)).collect()
}

/// `name|trump|attacker|phase|batch|my hand|opponent hand|table|stock|discard|played|note`
/// with space-separated cards, `??` for an unknown card and table piles as
/// `attack/defence` (or just `attack`). I am P0; the rules are the ones of the
/// game the positions came from (36 cards, reflecting, five-card first trick,
/// classic ending, any number of cards per attack).
fn parse_position(line: &str) -> (GameState, Vec<String>) {
    let f: Vec<String> = line.split('|').map(|s| s.trim().to_string()).collect();
    let attacker = &f[2];
    let defender = if attacker == "P0" { "P1" } else { "P0" };
    let phase = match f[3].as_str() { "A" => "Attacking", "D" => "Defending", "T" => "Throwing", _ => "Adding" };
    let table: Vec<serde_json::Value> = f[7]
        .split_whitespace()
        .map(|p| {
            let mut it = p.split('/');
            let attack = card_json(it.next().unwrap(), "public");
            let defense = it.next().map_or(serde_json::Value::Null, |d| card_json(d, "public"));
            serde_json::json!({ "attack": attack, "defense": defense })
        })
        .collect();
    let state = serde_json::json!({
        "trump": suit(&f[1]), "attacker": attacker, "defender": defender, "phase": phase,
        "attackers": [attacker], "current_attacker_idx": 0, "last_played_attacker": attacker, "throw_start_idx": 0,
        "hands": [cards(&f[5], "private"), cards(&f[6], "public")],
        "stock": cards(&f[8], "public"), "table": table, "discard": cards(&f[9], "public"),
        "reflected_trumps": [], "batch": f[4].parse::<u8>().unwrap(),
        "config": {
            "deck_size": 36, "num_players": 2, "trump_reflecting": false, "first_trick_limit": true,
            "classic_ending": true, "reflecting": true, "cards_per_attack": 6,
        },
    });
    let state: GameState = serde_json::from_value(state).expect("position parses");
    state.validate().expect("position is valid");
    (state, f)
}

fn label(a: &Action) -> String {
    let c = |card: &Card| {
        let (s, r) = card.id().unwrap();
        let r = match r { Rank::Ten => "10".into(), Rank::Jack => "J".into(), Rank::Queen => "Q".into(),
            Rank::King => "K".into(), Rank::Ace => "A".into(), r => (r as u8).to_string() };
        let s = match s { Suit::Clubs => "♣", Suit::Diamonds => "♦", Suit::Hearts => "♥", Suit::Spades => "♠" };
        format!("{r}{s}")
    };
    match a {
        Action::Attack { card } => format!("attack {}", c(card)),
        Action::Defend { card, .. } => format!("beat with {}", c(card)),
        Action::Reflect { card } => format!("reflect {}", c(card)),
        Action::ReflectTrump { card } => format!("show {}", c(card)),
        Action::Throw { card: Some(card) } => format!("throw {}", c(card)),
        Action::Throw { card: None } => "done".into(),
        Action::PassAttack => "pass".into(),
        Action::Take => "take".into(),
        Action::LetDefend => "let defend".into(),
    }
}

fn run_positions(args: &[String]) {
    let text = std::fs::read_to_string(&args[0]).expect("readable file");
    let effort = Effort {
        deals: args.get(1).map_or(40, |s| s.parse().unwrap()),
        playouts: args.get(2).map_or(1000, |s| s.parse().unwrap()),
    };
    for line in text.lines().filter(|l| !l.trim().is_empty() && !l.starts_with('#')) {
        let (state, f) = parse_position(line);
        println!("\n{}: played {} - {}", f[0], f[10], f[11]);
        println!("  rules player: {}", label(&policy::heuristic_action(&state).unwrap()));
        for (name, playout) in [("random playouts   ", Playout::Random), ("heuristic playouts", Playout::Heuristic)] {
            let params = SolveParams { determinizations: effort.deals, rollouts: effort.playouts, playout, ..SolveParams::default() };
            let r = solve(&state, params, 7).unwrap();
            let line: Vec<String> =
                r.actions.iter().map(|a| format!("{} {:.0}", label(&a.action), a.score * 100.0)).collect();
            println!("  {name}: {}", line.join(", "));
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("match") => run_match(&args[1..]),
        Some("positions") => run_positions(&args[1..]),
        _ => eprintln!("usage: arena match <A> <B> [deals] [deals per search] [playouts] [players] [deck]\n       arena positions <file> [deals per search] [playouts]"),
    }
}
