//! Playouts per second on one core, from an opening position.
use durak_core::{solve, GameConfig, GameState, Playout, SolveParams};
use std::time::Instant;
fn main() {
    let config = GameConfig { deck_size: 36, num_players: 2, reflecting: true, trump_reflecting: false,
        first_trick_limit: true, classic_ending: true, cards_per_attack: 6 };
    for playout in [Playout::Random, Playout::Heuristic] {
        let s = GameState::new_computer_game(5, config).unwrap();
        let t = Instant::now();
        let p = SolveParams { determinizations: 10, rollouts: 2000, playout, ..SolveParams::default() };
        solve(&s, p, 1).unwrap();
        println!("{playout:?}: {:.0} playouts/s on one core", 20000.0 / t.elapsed().as_secs_f64());
    }
}
