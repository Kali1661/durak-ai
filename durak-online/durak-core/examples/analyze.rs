//! Run the exact endgame solver over recorded positions and compare it with
//! the scores the sampling search showed at the time.
//!
//! `cargo run --release -p durak-core --example analyze -- <file> [max_nodes]`
//!
//! Each line of the file is one position, `|`-separated: game, step, trump
//! letter, classic ending (0/1), reflecting, five-card first trick, deck size,
//! attacker, phase letter, my hand, opponent hand, table (attack+defence pairs,
//! `--` = not beaten), stock, discard, the move played, the scores shown, the
//! game's result. Cards are rank + suit letter, e.g. `TS` = ten of spades.

use std::time::Instant;

use durak_core::{solve_exact, Action, ExactValue};

#[path = "../tests/common/recorded.rs"]
mod recorded;
use recorded::parse;

fn label(a: &Action) -> String {
    let c = |card: &durak_core::Card| {
        let (s, r) = card.id().unwrap();
        format!("{r:?} of {s:?}")
    };
    match a {
        Action::Attack { card } => format!("attack {}", c(card)),
        Action::Defend { card, .. } => format!("defend {}", c(card)),
        Action::Reflect { card } => format!("reflect {}", c(card)),
        Action::ReflectTrump { card } => format!("show {}", c(card)),
        Action::Throw { card: Some(card) } => format!("throw {}", c(card)),
        Action::Throw { card: None } => "done".into(),
        Action::PassAttack => "pass".into(),
        Action::Take => "take".into(),
        Action::LetDefend => "let defend".into(),
    }
}

fn main() {
    // Deep lines of play need more stack than the default main thread has.
    std::thread::Builder::new().stack_size(6usize << 30).spawn(run).unwrap().join().unwrap();
}

fn run() {
    let args: Vec<String> = std::env::args().collect();
    let text = std::fs::read_to_string(&args[1]).expect("readable file");
    let max_nodes: u64 = args.get(2).map(|s| s.parse().unwrap()).unwrap_or(5_000_000);

    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let (state, f) = parse(line);
        if let Err(e) = state.validate() {
            println!("game {} step {}: INVALID {e}", f[0], f[1]);
            continue;
        }
        let started = Instant::now();
        let result = solve_exact(&state, max_nodes).expect("solver runs");
        let ms = started.elapsed().as_millis();
        let cards = state.hands[0].len() + state.hands[1].len();
        match result {
            None => println!("game {} step {:>2} ({cards:>2} cards): not solvable ({ms} ms)   shown: {}", f[0], f[1], f[15]),
            Some(r) => {
                let word = |v: Option<ExactValue>| match v {
                    Some(ExactValue::Win) => "WIN",
                    Some(ExactValue::Draw) => "draw",
                    Some(ExactValue::Loss) => "LOSS",
                    None => "?",
                };
                let moves: Vec<String> = r.actions.iter().map(|a| format!("{}={}", label(&a.action), word(a.value))).collect();
                println!(
                    "game {} step {:>2} ({cards:>2} cards, {:>8} nodes, {ms:>5} ms): {:<4} | played {} | {}\n        shown: {}",
                    f[0], f[1], r.nodes, word(r.value), f[14], moves.join(", "), f[15]
                );
            }
        }
    }
}
