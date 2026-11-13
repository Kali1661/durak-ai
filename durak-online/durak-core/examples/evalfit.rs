//! Fit the formula that scores a two-player position once the stock is empty
//! (see `durak_core::eval`).
//!
//! ```text
//! cargo run --release -p durak-core --example evalfit -- [games] [deck]
//! ```
//!
//! Games are played by the heuristic playout player. At every trick start
//! with nothing left to draw the exact solver works out who wins with perfect
//! play; a logistic regression on the position's features is then fitted to
//! those answers. Prints the weights, ready to paste into `src/eval.rs`, and
//! how well the formula predicts positions it was not fitted on.

use std::sync::Mutex;

use durak_core::{eval, policy, solve_exact, ExactValue, GameConfig, GameState, Phase, Playout};
use rand::{rngs::StdRng, SeedableRng};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let games: u64 = args.first().map_or(4000, |s| s.parse().unwrap());
    let deck: usize = args.get(1).map_or(36, |s| s.parse().unwrap());
    let config = GameConfig {
        deck_size: deck,
        num_players: 2,
        reflecting: true,
        trump_reflecting: false,
        first_trick_limit: true,
        classic_ending: true,
        cards_per_attack: 6,
    };

    // (features, value for the attacker in [0, 1])
    let data: Mutex<Vec<([f64; eval::FEATURES], f64)>> = Mutex::new(vec![]);
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()) as u64;
    std::thread::scope(|scope| {
        for t in 0..threads {
            let data = &data;
            scope.spawn(move || {
                let mut local = vec![];
                for g in (t..games).step_by(threads as usize) {
                    let mut rng = StdRng::seed_from_u64(g);
                    let mut s = GameState::new_computer_game(50_000 + g, config).unwrap();
                    for _ in 0..3000 {
                        if s.is_terminal() {
                            break;
                        }
                        if s.stock.is_empty() && s.phase == Phase::Attacking && s.table.is_empty() {
                            if let Some(v) = label(&s) {
                                local.push((eval::features(&s), v));
                            }
                        }
                        let acts = s.legal_actions();
                        let i = policy::choose(&s, &acts, Playout::Heuristic, &mut rng);
                        s.apply(&acts[i]).unwrap();
                    }
                }
                data.lock().unwrap().extend(local);
            });
        }
    });
    let data = data.into_inner().unwrap();
    let split = data.len() * 4 / 5;
    let (train, test) = data.split_at(split);
    println!("{} positions ({} to fit, {} to check)", data.len(), train.len(), test.len());

    let w = fit(train);
    println!("weights: {w:?}");
    println!("fitted:   log loss {:.4}, right {:.1}%", log_loss(test, &w), accuracy(test, &w) * 100.0);
    let current = eval::WEIGHTS;
    println!("current:  log loss {:.4}, right {:.1}%", log_loss(test, &current), accuracy(test, &current) * 100.0);
    let mean = train.iter().map(|d| d.1).sum::<f64>() / train.len() as f64;
    let base = test.iter().map(|(_, y)| -(y * mean.ln() + (1.0 - y) * (1.0 - mean).ln())).sum::<f64>() / test.len() as f64;
    println!("guessing: log loss {base:.4} (always {:.0}% for the attacker)", mean * 100.0);
}

/// Value for the attacker (player to move) with perfect play.
fn label(s: &GameState) -> Option<f64> {
    let r = solve_exact(s, 300_000).ok()??;
    Some(match r.value? {
        ExactValue::Win => 1.0,
        ExactValue::Draw => 0.5,
        ExactValue::Loss => 0.0,
    })
}

fn predict(x: &[f64; eval::FEATURES], w: &[f64; eval::FEATURES]) -> f64 {
    let z: f64 = x.iter().zip(w).map(|(a, b)| a * b).sum();
    1.0 / (1.0 + (-z).exp())
}

fn log_loss(d: &[([f64; eval::FEATURES], f64)], w: &[f64; eval::FEATURES]) -> f64 {
    d.iter()
        .map(|(x, y)| {
            let p = predict(x, w).clamp(1e-6, 1.0 - 1e-6);
            -(y * p.ln() + (1.0 - y) * (1.0 - p).ln())
        })
        .sum::<f64>()
        / d.len() as f64
}

fn accuracy(d: &[([f64; eval::FEATURES], f64)], w: &[f64; eval::FEATURES]) -> f64 {
    let decided: Vec<_> = d.iter().filter(|(_, y)| *y != 0.5).collect();
    decided.iter().filter(|(x, y)| (predict(x, w) > 0.5) == (*y > 0.5)).count() as f64 / decided.len() as f64
}

/// Logistic regression by plain gradient descent with a little L2.
fn fit(d: &[([f64; eval::FEATURES], f64)]) -> [f64; eval::FEATURES] {
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
