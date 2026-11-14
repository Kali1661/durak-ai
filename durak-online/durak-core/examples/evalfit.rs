//! Fit the formula that scores a two-player position once the stock is empty
//! (see `durak_core::eval`), and show where it can be trusted.
//!
//! ```text
//! cargo run --release -p durak-core --example evalfit -- [games] [deck] [positions per solve] [take chance] [picky]
//! ```
//!
//! Games are played by the heuristic playout player; with a take chance, one
//! of the two also picks up that share of the attacks it faces late in the
//! game (with `picky`, only attacks with a queen or better or a trump in
//! them), which gives positions where one player holds many cards. At every
//! trick start with nothing left to draw the exact solver works out who wins
//! with perfect play; a logistic regression on the position's features is
//! then fitted to those answers. Prints the weights, ready to paste into
//! `src/eval.rs`, and how well the formula predicts positions it was not
//! fitted on - next to plain playouts of the same positions, by how many
//! cards are in play and how unequal the hands are.

use std::sync::Mutex;

use durak_core::{eval, playout_share, policy, solve_exact, Action, ExactValue, GameConfig, GameState, Phase, Playout, Style};
use rand::{rngs::StdRng, seq::SliceRandom, Rng, SeedableRng};

type X = [f64; eval::FEATURES];

/// `Style::hold_trumps` settings compared with the default.
const KEEPS: [bool; 1] = [true];

struct Sample {
    x: X,
    /// Value for the attacker with perfect play; `None` if too big to solve.
    y: Option<f64>,
    /// Share of playouts the attacker does not lose, per playing style tried.
    playouts: f64,
    styles: [f64; KEEPS.len()],
    in_range: bool,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let games: u64 = args.first().map_or(4000, |s| s.parse().unwrap());
    let deck: usize = args.get(1).map_or(36, |s| s.parse().unwrap());
    let nodes: u64 = args.get(2).map_or(300_000, |s| s.parse().unwrap());
    let take_chance: f64 = args.get(3).map_or(0.0, |s| s.parse().unwrap());
    let picky = args.get(4).is_some_and(|s| s == "picky");
    // Deck 0: half the games with 24 cards, half with 36.
    let config = |g: u64| GameConfig {
        deck_size: if deck == 0 { [24, 36][(g / 2 % 2) as usize] } else { deck },
        num_players: 2,
        reflecting: true,
        trump_reflecting: false,
        first_trick_limit: true,
        classic_ending: true,
        cards_per_attack: 6,
    };

    let data: Mutex<Vec<Sample>> = Mutex::new(vec![]);
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()) as u64;
    std::thread::scope(|scope| {
        for t in 0..threads {
            let data = &data;
            scope.spawn(move || {
                let mut local = vec![];
                for g in (t..games).step_by(threads as usize) {
                    let mut rng = StdRng::seed_from_u64(g);
                    let mut s = GameState::new_computer_game(50_000 + g, config(g)).unwrap();
                    for _ in 0..3000 {
                        if s.is_terminal() {
                            break;
                        }
                        if s.stock.is_empty() && s.phase == Phase::Attacking && s.table.is_empty() {
                            local.push(Sample {
                                x: eval::features(&s),
                                y: label(&s, nodes),
                                playouts: playout_share(&s, 200, &mut rng, Style::default()).unwrap_or(0.5),
                                styles: KEEPS.map(|keep| {
                                    playout_share(&s, 200, &mut rng, Style { hold_trumps: keep, ..Style::default() }).unwrap_or(0.5)
                                }),
                                in_range: eval::in_range(&s),
                            });
                        }
                        let acts = s.legal_actions();
                        let mut i = policy::choose(&s, &acts, Playout::Heuristic, &mut rng);
                        // The hoarder: one seat, every other game.
                        if take_chance > 0.0
                            && s.phase == Phase::Defending
                            && s.defender.index() == (g % 2) as usize
                            && s.stock.len() <= 8
                            && (!picky
                                || s.table.iter().any(|p| {
                                    p.attack.id().is_some_and(|(suit, rank)| suit == s.trump || rank >= durak_core::Rank::Queen)
                                }))
                            && rng.gen_bool(take_chance)
                        {
                            if let Some(t) = acts.iter().position(|a| matches!(a, Action::Take)) {
                                i = t;
                            }
                        }
                        s.apply(&acts[i]).unwrap();
                    }
                }
                data.lock().unwrap().extend(local);
            });
        }
    });
    let mut all = data.into_inner().unwrap();
    // Threads finish in any order: shuffle so the check set is a fair sample.
    all.shuffle(&mut StdRng::seed_from_u64(1));
    let (solved, unsolved): (Vec<Sample>, Vec<Sample>) = all.into_iter().partition(|s| s.y.is_some());
    let split = solved.len() * 4 / 5;
    let (train, test) = solved.split_at(split);
    println!(
        "{} positions solved ({} to fit, {} to check), {} too big to solve",
        solved.len(),
        train.len(),
        test.len(),
        unsolved.len()
    );

    let fit_on: Vec<(X, f64)> = train.iter().filter(|s| s.in_range).map(|s| (s.x, s.y.unwrap())).collect();
    let w = fit(&fit_on);
    println!("weights (fitted on the {} positions the formula is used for): {w:?}", fit_on.len());
    let current = eval::WEIGHTS;

    let report = |name: &str, pick: &dyn Fn(&Sample) -> bool| {
        let part: Vec<&Sample> = test.iter().filter(|s| pick(s)).collect();
        let big: Vec<&Sample> = unsolved.iter().filter(|s| pick(s)).collect();
        // Among the unsolved: how far the formula is from the playouts.
        let apart = big.iter().map(|s| (predict(&s.x, &current) - s.playouts).abs()).sum::<f64>() / big.len().max(1) as f64;
        if part.is_empty() {
            println!("  {name:<10} none solved; {} unsolved (formula and playouts {:.2} apart)", big.len(), apart);
            return;
        }
        let ll = |f: &dyn Fn(&Sample) -> f64| {
            part.iter()
                .map(|s| {
                    let (p, y) = (f(s).clamp(0.02, 0.98), s.y.unwrap());
                    -(y * p.ln() + (1.0 - y) * (1.0 - p).ln())
                })
                .sum::<f64>()
                / part.len() as f64
        };
        let right = |f: &dyn Fn(&Sample) -> f64| {
            let decided: Vec<_> = part.iter().filter(|s| s.y != Some(0.5)).collect();
            100.0 * decided.iter().filter(|s| (f(s) > 0.5) == (s.y.unwrap() > 0.5)).count() as f64 / decided.len().max(1) as f64
        };
        println!(
            "  {name:<10} {:>6}: current {:.3} {:>5.1}% | fitted {:.3} {:>5.1}% | playouts {:.3} {:>5.1}% | holding trumps back {:.3} | {} unsolved ({:.2} apart)",
            part.len(),
            ll(&|s| predict(&s.x, &current)),
            right(&|s| predict(&s.x, &current)),
            ll(&|s| predict(&s.x, &w)),
            right(&|s| predict(&s.x, &w)),
            ll(&|s| s.playouts),
            right(&|s| s.playouts),
            ll(&|s| s.styles[0]),
            big.len(),
            apart
        );
    };
    println!("\nlog loss (lower is better) and how often the winner is named, on the positions kept for checking");
    report("all", &|_| true);
    report("in range", &|s| s.in_range);
    report("out of it", &|s| !s.in_range);
    println!("by difference in cards");
    for (name, lo, hi) in [("0-1", 0.0, 1.0), ("2-3", 2.0, 3.0), ("4-6", 4.0, 6.0), ("7-10", 7.0, 10.0), ("11+", 11.0, 99.0)] {
        report(name, &|s| s.x[1].abs() >= lo && s.x[1].abs() <= hi);
    }
    println!("by cards in play");
    for (name, lo, hi) in [("2-6", 0.0, 0.6), ("7-10", 0.7, 1.0), ("11-14", 1.1, 1.4), ("15-18", 1.5, 1.8), ("19+", 1.9, 9.9)] {
        report(name, &|s| s.x[5] >= lo - 0.01 && s.x[5] <= hi + 0.01);
    }
}

/// Value for the attacker (player to move) with perfect play.
fn label(s: &GameState, nodes: u64) -> Option<f64> {
    let r = solve_exact(s, nodes).ok()??;
    Some(match r.value? {
        ExactValue::Win => 1.0,
        ExactValue::Draw => 0.5,
        ExactValue::Loss => 0.0,
    })
}

fn predict(x: &X, w: &X) -> f64 {
    let z: f64 = x.iter().zip(w).map(|(a, b)| a * b).sum();
    1.0 / (1.0 + (-z).exp())
}

/// Logistic regression by plain gradient descent with a little L2.
fn fit(d: &[(X, f64)]) -> X {
    let mut w = [0.0; eval::FEATURES];
    let n = d.len() as f64;
    for step in 0..4000 {
        let mut grad = [0.0; eval::FEATURES];
        for (x, y) in d {
            let e = predict(x, &w) - y;
            for (g, xi) in grad.iter_mut().zip(x) {
                *g += e * xi;
            }
        }
        let rate = if step < 2000 { 0.5 } else { 0.1 };
        for (wi, g) in w.iter_mut().zip(grad) {
            *wi -= rate * (g / n + 1e-4 * *wi);
        }
    }
    w
}
