//! How well do the engine's win percentages match what happened in real games?
//!
//! Reads the file the app's "Export all (JSON)" button saves (Games panel),
//! takes the two-player games against real opponents that have a result, and
//! searches every position I had to move in again - once per engine variant.
//! The score of the move I played is then compared with how the game ended.
//! A variant whose percentages are closer to the results (lower "loss") reads
//! real opponents better.
//!
//! ```text
//! cargo run --release --example calibrate -- <export.json> [--deals N] [--playouts N] [--every N] <variant>...
//! ```
//!
//! A variant is `name` or `name:key=value,...` with the keys
//! `inf=0` (do not guess the opponent's hand), `unequal=1` (score hands of
//! any size, as before 0.3.2), `hold=1` and `oldattack=1` (see `policy::Style`), and the
//! strengths of the evidence about the opponent's hand `take=`, `taketrump=`,
//! `lead=`, `leadtrump=`, `stoplow=`, `stophigh=` (1 = ignore), `stacks=1` (takes add up, as before 0.3.2).
//! `--every N` checks only every Nth position, for a quicker look.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use durak_core::{Action, Beliefs, GameState, PlayerId, Search, SolveParams, Strength};
use serde_json::Value;

#[derive(Clone)]
struct Variant {
    name: String,
    inf: bool,
    strength: Strength,
    params: SolveParams,
}

fn variant(spec: &str) -> Variant {
    let (name, opts) = spec.split_once(':').unwrap_or((spec, ""));
    let mut v = Variant { name: name.to_string(), inf: true, strength: Strength::default(), params: SolveParams::app() };
    for opt in opts.split(',').filter(|o| !o.is_empty()) {
        let (k, val) = opt.split_once('=').unwrap_or_else(|| panic!("option {opt} needs a value"));
        let f: f32 = val.parse().unwrap_or_else(|_| panic!("{opt}: not a number"));
        match k {
            "inf" => v.inf = f != 0.0,
            "unequal" => v.params.style.score_unequal = f != 0.0,
            "hold" => v.params.style.hold_trumps = f != 0.0,
            "oldattack" => v.params.style.old_attack = f != 0.0,
            "take" => v.strength.take_same_suit = f,
            "taketrump" => v.strength.take_trump = f,
            "lead" => v.strength.lead_lower = f,
            "leadtrump" => v.strength.lead_trump_plain = f,
            "stoplow" => v.strength.stop_low = f,
            "stophigh" => v.strength.stop_high = f,
            "stacks" => v.strength.take_stacks = f != 0.0,
            _ => panic!("unknown option {k}"),
        }
    }
    v
}

struct Job {
    game: usize,
    /// Index into the game's steps of my move.
    step: usize,
    state: GameState,
    played: Action,
    won: bool,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let number = |name: &str, default: u32| -> u32 {
        args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).map_or(default, |v| v.parse().unwrap())
    };
    let deals = number("--deals", 8);
    let playouts = number("--playouts", 2500);
    let every = number("--every", 1) as usize;
    let mut rest = args.iter().enumerate().filter(|(i, a)| {
        !a.starts_with("--") && !(*i > 0 && ["--deals", "--playouts", "--every"].contains(&args[i - 1].as_str()))
    });
    let Some((_, path)) = rest.next() else {
        eprintln!("usage: calibrate <export.json> [--deals N] [--playouts N] [--every N] <variant>...");
        std::process::exit(2);
    };
    let mut variants: Vec<Variant> = rest.map(|(_, s)| variant(s)).collect();
    if variants.is_empty() {
        variants.push(variant("app"));
    }

    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("could not read {path}: {e}"));
    let export: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path} is not JSON: {e}"));
    let games: Vec<&Value> = export["games"]
        .as_array()
        .expect("an export of the Games panel")
        .iter()
        .filter(|g| g["mode"] == "analyse" && g["initial"]["hands"].as_array().map_or(0, Vec::len) == 2)
        .filter(|g| ["win", "loss"].contains(&result(g)))
        .collect();

    // Every game's positions and moves, and my moves in them.
    let mut histories: Vec<(GameState, Vec<(Option<Action>, GameState)>)> = Vec::new();
    let mut jobs = Vec::new();
    let mut count = 0usize;
    for (gi, g) in games.iter().enumerate() {
        let Ok(initial) = serde_json::from_value::<GameState>(g["initial"].clone()) else { continue };
        let mut steps = Vec::new();
        for step in g["steps"].as_array().map_or(&[][..], |v| &v[..]) {
            let Ok(state) = serde_json::from_value::<GameState>(step["state"].clone()) else { break };
            let action: Option<Action> = serde_json::from_value(step["action"].clone()).ok().flatten();
            let mine = step["actor"] == "P0";
            let before = steps.last().map_or(&initial, |(_, s): &(Option<Action>, GameState)| s);
            if let (true, Some(action)) = (mine, &action) {
                let legal = before.legal_actions();
                if before.actor_to_move() == PlayerId::P0 && legal.len() >= 2 && legal.iter().any(|a| a.same_move(action)) {
                    count += 1;
                    if count % every == 0 {
                        jobs.push(Job { game: gi, step: steps.len(), state: before.clone(), played: action.clone(), won: result(g) == "win" });
                    }
                }
            }
            steps.push((action, state));
        }
        histories.push((initial, steps));
    }
    let wins = games.iter().filter(|g| result(g) == "win").count();
    println!(
        "{} real games with a result ({} won, {} lost), {} of my moves checked with {} deals x {} playouts",
        games.len(),
        wins,
        games.len() - wins,
        jobs.len(),
        deals,
        playouts
    );

    for v in &variants {
        let started = Instant::now();
        let next = AtomicUsize::new(0);
        let scores: Mutex<Vec<Option<f64>>> = Mutex::new(vec![None; jobs.len()]);
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get().saturating_sub(1).max(1));
        std::thread::scope(|scope| {
            for _ in 0..threads {
                scope.spawn(|| loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= jobs.len() {
                        break;
                    }
                    let job = &jobs[i];
                    let beliefs = v.inf.then(|| {
                        let (initial, steps) = &histories[job.game];
                        let mut b = Beliefs::new(v.strength);
                        let mut before = initial;
                        for (action, after) in &steps[..job.step] {
                            if let Some(action) = action {
                                b.observe(PlayerId::P0, before, action, after);
                            }
                            before = after;
                        }
                        b
                    });
                    let score = Search::new(&job.state, v.params, 1000 + i as u64).ok().and_then(|s| {
                        let r = s.with_beliefs(beliefs).run(deals, playouts).ok()?;
                        let played = r.actions.iter().find(|a| a.action.same_move(&job.played))?;
                        // A move the search never tried: go by the best one.
                        Some(f64::from(if played.visits > 0 { played.score } else { r.actions[0].score }))
                    });
                    scores.lock().unwrap()[i] = score;
                });
            }
        });
        let scores = scores.into_inner().unwrap();

        println!("\n{} ({:.0} s)", v.name, started.elapsed().as_secs_f64());
        println!("  {:<26} {:>6} {:>8} {:>8} {:>8} {:>8}", "cards left to draw", "moves", "shown", "won", "loss", "brier");
        let buckets: [(&str, &dyn Fn(usize) -> bool); 5] = [
            ("all", &|_| true),
            ("9 or more", &|n| n >= 9),
            ("5 to 8", &|n| (5..=8).contains(&n)),
            ("1 to 4", &|n| (1..=4).contains(&n)),
            ("none", &|n| n == 0),
        ];
        for (name, pick) in buckets {
            let part: Vec<(f64, f64)> = jobs
                .iter()
                .zip(&scores)
                .filter(|(j, _)| pick(j.state.stock.len()))
                .filter_map(|(j, s)| Some((((*s)?), f64::from(u8::from(j.won)))))
                .collect();
            if part.is_empty() {
                continue;
            }
            let n = part.len() as f64;
            let shown = part.iter().map(|p| p.0).sum::<f64>() / n;
            let won = part.iter().map(|p| p.1).sum::<f64>() / n;
            let loss = part
                .iter()
                .map(|(p, y)| {
                    let p = p.clamp(0.02, 0.98);
                    -(y * p.ln() + (1.0 - y) * (1.0 - p).ln())
                })
                .sum::<f64>()
                / n;
            let brier = part.iter().map(|(p, y)| (p - y) * (p - y)).sum::<f64>() / n;
            println!("  {name:<26} {:>6} {:>7.1}% {:>7.1}% {loss:>8.3} {brier:>8.3}", part.len(), shown * 100.0, won * 100.0);
        }
        // The confident calls: how often they were wrong.
        let sure: Vec<bool> = jobs.iter().zip(&scores).filter_map(|(j, s)| ((*s)? >= 0.9).then_some(j.won)).collect();
        let games_sure: std::collections::BTreeSet<usize> =
            jobs.iter().zip(&scores).filter(|(j, s)| s.is_some_and(|s| s >= 0.9) && !j.won).map(|(j, _)| j.game).collect();
        println!(
            "  shown 90% or more: {} moves, {} of them in games I lost ({} games)",
            sure.len(),
            sure.iter().filter(|w| !**w).count(),
            games_sure.len()
        );
    }
}

fn result<'a>(g: &'a Value) -> &'a str {
    g["userResult"].as_str().or(g["result"].as_str()).unwrap_or("unfinished")
}
