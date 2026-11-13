//! Mistake finder for recorded games.
//!
//! Reads the file the app's "Export all (JSON)" button saves (Games panel)
//! and checks my moves late in each two-player game against perfect play:
//! the hidden cards are dealt many ways, each deal is solved exactly, and a
//! move is flagged when another move wins clearly more of the deals. Says for
//! each flagged move whether it was the engine's suggestion or my own choice.
//!
//! `cargo run --release --example review -- <export.json> [options]`
//!
//! Options:
//!   --deals N     deals of the hidden cards per position (default 40)
//!   --nodes N     positions searched per deal before giving up (default 2000000)
//!   --stock N     check moves made with at most N cards left to draw (default 6)
//!   --gap N       flag a move when another wins N points more of the deals (default 15)
//!   --games N     only the newest N games
//!   --lost        only games I lost
//!   --all         also games against the computer (default: real games only)
//!   --threads N   positions checked at once (default: all cores but one)
//!
//! A caveat on what "wins" means here: each deal is solved as if both players
//! could see every card. That is the best available yardstick, but it is a
//! little kind to moves that only work when you know what the opponent holds.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use durak_core::{
    is_exactly_solvable, solve_exact, solve_exact_deal, Action, Card, ExactValue, GameState, PlayerId, Rank, Suit,
};
use rand::{rngs::StdRng, SeedableRng};
use serde_json::Value;

struct Options {
    deals: u64,
    nodes: u64,
    stock: usize,
    gap: f64,
    games: usize,
    lost: bool,
    all: bool,
    threads: usize,
}

/// One of my moves to check.
struct Job {
    game: usize,
    step: usize,
    state: GameState,
    legal: Vec<Action>,
    played: usize,
    /// The engine's suggestion at the time, if one was recorded.
    suggested: Option<usize>,
    /// The score the engine showed for the move I played (0..1).
    shown: Option<f64>,
}

enum Verdict {
    /// Too big to solve: nothing learned.
    TooBig,
    /// Win rate of the played move and of the best other move, over the
    /// deals where both were settled; and of the suggestion, if different.
    Checked { played: f64, best: Option<(usize, f64, f64, u32)>, suggested: Option<(f64, f64, u32)>, deals: u32, exact: bool },
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.iter().find(|a| !a.starts_with("--") && a.parse::<f64>().is_err()) else {
        eprintln!("usage: review <export.json> [--deals N] [--nodes N] [--stock N] [--gap N] [--games N] [--lost] [--all] [--threads N]");
        std::process::exit(2);
    };
    let number = |name: &str, default: f64| -> f64 {
        args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).map_or(default, |v| {
            v.parse().unwrap_or_else(|_| panic!("{name} needs a number, got {v}"))
        })
    };
    let cores = std::thread::available_parallelism().map_or(4, |n| n.get());
    let opt = Options {
        deals: number("--deals", 40.0) as u64,
        nodes: number("--nodes", 2_000_000.0) as u64,
        stock: number("--stock", 6.0) as usize,
        gap: number("--gap", 15.0) / 100.0,
        games: number("--games", f64::MAX) as usize,
        lost: args.iter().any(|a| a == "--lost"),
        all: args.iter().any(|a| a == "--all"),
        threads: number("--threads", (cores.saturating_sub(1)).max(1) as f64) as usize,
    };

    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("could not read {path}: {e}"));
    let export: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path} is not JSON: {e}"));
    let mut games: Vec<&Value> = export["games"].as_array().expect("an export of the Games panel").iter().collect();
    // Newest first, as exported.
    games.sort_by_key(|g| std::cmp::Reverse(g["startedAt"].as_f64().unwrap_or(0.0) as i64));
    let games: Vec<&Value> = games
        .into_iter()
        .filter(|g| opt.all || g["mode"] == "analyse")
        .filter(|g| g["initial"]["hands"].as_array().map_or(0, Vec::len) == 2)
        .filter(|g| !opt.lost || result(g) == "loss")
        .take(opt.games)
        .collect();

    let mut jobs = Vec::new();
    let mut unreadable = 0;
    for (gi, g) in games.iter().enumerate() {
        let steps = g["steps"].as_array().map_or(&[][..], |v| &v[..]);
        for (si, step) in steps.iter().enumerate() {
            if step["actor"] != "P0" || step["action"].is_null() {
                continue;
            }
            let before = if si == 0 { &g["initial"] } else { &steps[si - 1]["state"] };
            let Ok(state) = serde_json::from_value::<GameState>(before.clone()) else {
                unreadable += 1;
                continue;
            };
            if state.validate().is_err() || state.actor_to_move() != PlayerId::P0 || state.stock.len() > opt.stock {
                continue;
            }
            let legal = state.legal_actions();
            let find = |key: &str| legal.iter().position(|a| action_key(&serde_json::to_value(a).unwrap()) == key);
            let Some(played) = find(&action_key(&step["action"])) else {
                unreadable += 1;
                continue;
            };
            // A single legal move cannot be a mistake.
            if legal.len() < 2 {
                continue;
            }
            let hint = &step["hint"];
            let suggested = hint["best"].as_str().and_then(find);
            let shown = hint["moves"].as_array().and_then(|moves| {
                moves.iter().find(|m| m["key"].as_str() == hint["chosen"].as_str()).and_then(|m| m["score"].as_f64())
            });
            jobs.push(Job { game: gi, step: si + 1, state, legal, played, suggested, shown });
        }
    }
    println!(
        "{} two-player games, {} of my moves with at most {} cards left to draw. Checking with {} deals each on {} threads...",
        games.len(),
        jobs.len(),
        opt.stock,
        opt.deals,
        opt.threads
    );

    let started = Instant::now();
    let next = AtomicUsize::new(0);
    let verdicts: Mutex<Vec<Option<Verdict>>> = Mutex::new((0..jobs.len()).map(|_| None).collect());
    std::thread::scope(|scope| {
        for _ in 0..opt.threads {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= jobs.len() {
                    break;
                }
                let v = check(&jobs[i], &opt);
                let mut all = verdicts.lock().unwrap();
                all[i] = Some(v);
                let done = all.iter().filter(|v| v.is_some()).count();
                if done % 25 == 0 {
                    eprintln!("  {done} / {} moves checked, {:.0} s", jobs.len(), started.elapsed().as_secs_f64());
                }
            });
        }
    });
    let verdicts: Vec<Verdict> = verdicts.into_inner().unwrap().into_iter().map(Option::unwrap).collect();

    // ── Report ──────────────────────────────────────────────────────────────
    let mut too_big = 0;
    let mut checked = 0;
    let mut engine_mistakes = 0;
    let mut my_mistakes = 0;
    let mut unhinted_mistakes = 0;
    let (mut shown_sum, mut real_sum, mut calibrated) = (0.0, 0.0, 0u32);
    let mut last_game = usize::MAX;
    let mut games_with_engine_mistake = std::collections::BTreeSet::new();

    for (job, verdict) in jobs.iter().zip(&verdicts) {
        let Verdict::Checked { played, best, suggested, deals, exact } = verdict else {
            too_big += 1;
            continue;
        };
        checked += 1;
        if let Some(shown) = job.shown {
            shown_sum += shown;
            real_sum += played;
            calibrated += 1;
        }
        let Some((other, played_common, other_rate, common)) = best else { continue };
        if other_rate - played_common < opt.gap {
            continue;
        }
        if job.game != last_game {
            let g = games[job.game];
            println!("\nGame of {} - {}", date(g["startedAt"].as_f64().unwrap_or(0.0)), result(g));
            last_game = job.game;
        }
        let followed = job.suggested == Some(job.played);
        let who = match job.suggested {
            Some(_) if followed => {
                engine_mistakes += 1;
                games_with_engine_mistake.insert(job.game);
                "the engine's suggestion".to_string()
            }
            Some(s) => {
                my_mistakes += 1;
                let how = match suggested {
                    Some((mine, theirs, n)) => format!(
                        ", which wins {:.0}% against {:.0}% over {n} deals",
                        theirs * 100.0,
                        mine * 100.0
                    ),
                    None => ", which could not be compared".into(),
                };
                format!("my own choice; the engine suggested \"{}\"{how}", label(&job.legal[s]))
            }
            None => {
                unhinted_mistakes += 1;
                "no suggestion was recorded".to_string()
            }
        };
        let shown = job.shown.map_or(String::new(), |s| format!(" The engine showed {:.0}% for it.", s * 100.0));
        println!(
            "  move {:>3}, {} cards to draw: played \"{}\" ({who}).{shown}",
            job.step,
            job.state.stock.len(),
            label(&job.legal[job.played])
        );
        println!(
            "            it wins {:.0}% of deals; \"{}\" wins {:.0}% ({})",
            played_common * 100.0,
            label(&job.legal[*other]),
            other_rate * 100.0,
            if *exact { "nothing hidden: calculated".to_string() } else { format!("{common} of {deals} deals solved for both") }
        );
    }

    println!("\nSummary");
    println!("  moves checked: {checked}; too big to solve: {too_big}; unreadable: {unreadable}");
    println!(
        "  flagged (another move wins {:.0}+ points more of the deals): {} were the engine's suggestion (in {} games), {} were my own choice against it, {} had no suggestion recorded",
        opt.gap * 100.0,
        engine_mistakes,
        games_with_engine_mistake.len(),
        my_mistakes,
        unhinted_mistakes
    );
    if calibrated > 0 {
        println!(
            "  over {calibrated} checked moves with a recorded score: the engine showed {:.0}% on average, the moves won {:.0}% of the solved deals",
            100.0 * shown_sum / f64::from(calibrated),
            100.0 * real_sum / f64::from(calibrated)
        );
    }
    println!("  ({:.0} s)", started.elapsed().as_secs_f64());
}

fn value(v: Option<ExactValue>) -> Option<f64> {
    Some(match v? {
        ExactValue::Win => 1.0,
        ExactValue::Draw => 0.5,
        ExactValue::Loss => 0.0,
    })
}

/// Solve the position deal by deal and compare the played move with the rest.
fn check(job: &Job, opt: &Options) -> Verdict {
    // Per deal: the value of every legal move, where it was settled.
    let mut solved: Vec<Vec<Option<f64>>> = Vec::new();
    let exact = is_exactly_solvable(&job.state);
    let slots = |r: durak_core::ExactResult| -> Vec<Option<f64>> {
        job.legal
            .iter()
            .map(|a| r.actions.iter().find(|x| x.action.same_move(a)).and_then(|x| value(x.value)))
            .collect()
    };
    if exact {
        if let Ok(Some(r)) = solve_exact(&job.state, opt.nodes.saturating_mul(10)) {
            solved.push(slots(r));
        }
    } else {
        for d in 0..opt.deals {
            let seed = (job.game as u64) << 40 | (job.step as u64) << 20 | d;
            let Ok(det) = job.state.determinize(PlayerId::P0, &mut StdRng::seed_from_u64(seed)) else { break };
            if let Ok(Some(r)) = solve_exact_deal(&det, opt.nodes) {
                solved.push(slots(r));
            }
            // The first few deals all too big: so are the rest.
            if d == 3 && solved.iter().all(|deal| deal[job.played].is_none()) {
                return Verdict::TooBig;
            }
        }
    }

    let rates = |other: usize| -> Option<(f64, f64, u32)> {
        let both: Vec<(f64, f64)> = solved.iter().filter_map(|deal| Some((deal[job.played]?, deal[other]?))).collect();
        let n = both.len() as u32;
        // One calculated position is enough; sampled deals need a few.
        (n >= if exact { 1 } else { 10 }).then(|| {
            (both.iter().map(|b| b.0).sum::<f64>() / f64::from(n), both.iter().map(|b| b.1).sum::<f64>() / f64::from(n), n)
        })
    };
    let own: Vec<f64> = solved.iter().filter_map(|deal| deal[job.played]).collect();
    if own.len() < if exact { 1 } else { 10 } {
        return Verdict::TooBig;
    }
    let best = (0..job.legal.len())
        .filter(|&i| i != job.played)
        .filter_map(|i| rates(i).map(|(mine, theirs, n)| (i, mine, theirs, n)))
        .max_by(|a, b| (a.2 - a.1).partial_cmp(&(b.2 - b.1)).unwrap());
    Verdict::Checked {
        played: own.iter().sum::<f64>() / own.len() as f64,
        best,
        suggested: job.suggested.filter(|&s| s != job.played).and_then(rates),
        deals: solved.len() as u32,
        exact,
    }
}

/// The key the app gives a move (see `actionKey` in web/src/lib/aggregate.js).
fn action_key(action: &Value) -> String {
    let text = |v: &Value| match v {
        Value::String(s) => s.clone(),
        Value::Null => "-".into(),
        other => other.to_string(),
    };
    let card = &action["card"];
    let card = if card["suit"].is_null() { "-".to_string() } else { format!("{}:{}", text(&card["suit"]), text(&card["rank"])) };
    let pile = if action["type"] == "defend" { text(&action["pile_index"]) } else { "-".into() };
    format!("{}|{pile}|{card}", text(&action["type"]))
}

fn result(game: &Value) -> &str {
    game["userResult"].as_str().or(game["result"].as_str()).unwrap_or("unfinished")
}

/// A date and time (UTC) from milliseconds since 1970.
fn date(ms: f64) -> String {
    let secs = (ms / 1000.0) as i64;
    let (days, rest) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil date from a day count (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year}-{month:02}-{day:02} {:02}:{:02} UTC", rest / 3600, rest % 3600 / 60)
}

fn label(a: &Action) -> String {
    let c = |card: &Card| {
        let (s, r) = card.id().unwrap();
        let r = match r {
            Rank::Ten => "10".into(),
            Rank::Jack => "J".into(),
            Rank::Queen => "Q".into(),
            Rank::King => "K".into(),
            Rank::Ace => "A".into(),
            r => (r as u8).to_string(),
        };
        let s = match s {
            Suit::Clubs => "♣",
            Suit::Diamonds => "♦",
            Suit::Hearts => "♥",
            Suit::Spades => "♠",
        };
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
