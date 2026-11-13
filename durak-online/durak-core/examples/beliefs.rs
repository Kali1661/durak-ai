//! How good is each kind of evidence about the opponent's hand?
//!
//! Plays two-player games and, every time the opponent's move triggers a rule
//! in `belief` ("they took, so they probably have no higher card of that
//! suit"), looks where the cards the rule points at really are: in the
//! opponent's hand, or still in the stock. Prints, per rule, how often such a
//! card was in the hand against how often a hidden card is there anyway, and
//! the weight that follows (the odds ratio; below 1 = the rule is right that
//! the card is less likely to be held).
//!
//! `cargo run --release --example beliefs -- [games] [rules|search]`

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use durak_core::{policy, solve, Beliefs, Evidence, GameConfig, GameState, PlayerId, SolveParams, Strength};

const KINDS: [Evidence; 6] = [
    Evidence::TakeSameSuit,
    Evidence::TakeTrump,
    Evidence::LeadLower,
    Evidence::LeadTrumpPlain,
    Evidence::StopLow,
    Evidence::StopHigh,
];

/// Per kind: cards pointed at, of them in the hand, and how many would be
/// expected in the hand if the rule meant nothing.
type Tally = [(f64, f64, f64); 6];

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let games: u64 = args.first().map_or(400, |s| s.parse().unwrap());
    let search = args.get(1).is_some_and(|s| s == "search");
    let config = GameConfig {
        deck_size: 36,
        num_players: 2,
        reflecting: true,
        classic_ending: true,
        ..GameConfig::default()
    };

    let total = Mutex::new([(0.0, 0.0, 0.0); 6]);
    let next = AtomicU64::new(0);
    std::thread::scope(|scope| {
        for _ in 0..std::thread::available_parallelism().map_or(4, |n| n.get().saturating_sub(1).max(1)) {
            scope.spawn(|| loop {
                let g = next.fetch_add(1, Ordering::Relaxed);
                if g >= games {
                    break;
                }
                let t = play(g, config, search);
                let mut all = total.lock().unwrap();
                for (a, b) in all.iter_mut().zip(t) {
                    a.0 += b.0;
                    a.1 += b.1;
                    a.2 += b.2;
                }
            });
        }
    });

    println!("{games} games, the opponent played by {}", if search { "the search" } else { "the rules player" });
    println!("{:<18} {:>8} {:>9} {:>9} {:>8}", "evidence", "cards", "in hand", "expected", "weight");
    for (kind, (n, held, expected)) in KINDS.iter().zip(total.into_inner().unwrap()) {
        if n == 0.0 {
            continue;
        }
        let (p, q) = (held / n, expected / n);
        let weight = (p / (1.0 - p)) / (q / (1.0 - q));
        println!("{:<18} {n:>8.0} {:>8.1}% {:>8.1}% {weight:>8.2}", format!("{kind:?}"), p * 100.0, q * 100.0);
    }
}

fn play(seed: u64, config: GameConfig, search: bool) -> Tally {
    let mut tally = [(0.0, 0.0, 0.0); 6];
    let mut s = GameState::new_computer_game(seed, config).unwrap();
    // Evidence switched off: only the reports matter here.
    let mut beliefs = Beliefs::new(Strength::none());
    let mut moves = 0u64;
    while !s.is_terminal() && moves < 2000 {
        let actor = s.actor_to_move();
        let action = if search && actor == PlayerId::P1 {
            let params = SolveParams { determinizations: 4, rollouts: 300, ..SolveParams::app() };
            solve(&s, params, seed ^ (moves << 20)).unwrap().actions[0].action.clone()
        } else {
            policy::heuristic_action(&s).unwrap()
        };
        let before = s.clone();
        s.apply(&action).unwrap();
        moves += 1;

        // Where the cards I (P0) cannot see really are.
        let hand: Vec<_> = before.hands[1].iter().filter(|c| c.is_private()).filter_map(|c| c.id()).collect();
        let stock: Vec<_> = before.stock.iter().filter(|c| !c.is_public()).filter_map(|c| c.id()).collect();
        let unseen = hand.len() + stock.len();
        if unseen == 0 || stock.is_empty() || hand.is_empty() {
            continue;
        }
        let base = hand.len() as f64 / unseen as f64;
        // Note what each rule points at with full-strength rules...
        let mut probe = Beliefs::new(Strength { take_same_suit: 0.5, take_trump: 0.5, lead_lower: 0.5, lead_trump_plain: 0.5, stop_low: 0.5, stop_high: 0.5 });
        probe.observe_with(PlayerId::P0, &before, &action, &s, &mut |kind, id| {
            let k = KINDS.iter().position(|x| *x == kind).unwrap();
            if hand.contains(&id) {
                tally[k].0 += 1.0;
                tally[k].1 += 1.0;
                tally[k].2 += base;
            } else if stock.contains(&id) {
                tally[k].0 += 1.0;
                tally[k].2 += base;
            }
        });
        beliefs.observe(PlayerId::P0, &before, &action, &s);
    }
    tally
}
