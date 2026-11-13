//! Where does a shared-tree search spend its time? (rough breakdown)
use std::time::Instant;
use durak_core::{policy, GameConfig, GameState, PlayerId, SharedTree, SolveParams};
use rand::{rngs::StdRng, SeedableRng};

fn main() {
    let config = GameConfig { deck_size: 36, num_players: 2, reflecting: true, classic_ending: true, ..GameConfig::default() };
    let s = GameState::new_computer_game(9, config).unwrap();
    let n = 60_000u32;

    let t = Instant::now();
    let mut tree = SharedTree::new(&s, 1, PlayerId::P0, &SolveParams::app());
    tree.run(n).unwrap();
    let total = t.elapsed().as_secs_f64();
    println!("search: {:.0} playouts/s", f64::from(n) / total);

    let mut rng = StdRng::seed_from_u64(1);
    let t = Instant::now();
    let mut deals = Vec::new();
    for _ in 0..n {
        deals.push(s.determinize(PlayerId::P0, &mut rng).unwrap());
    }
    let det = t.elapsed().as_secs_f64();
    println!("dealing the hidden cards: {:.0}% of the time", 100.0 * det / total);

    // A playout to the end of the stock, as the search does it, without a tree.
    let t = Instant::now();
    let mut moves = 0u64;
    for d in deals.iter().take(20_000) {
        let mut g = d.clone();
        while !g.is_terminal() && !(g.stock.is_empty() && g.table.is_empty()) {
            let acts = g.legal_actions();
            let i = policy::choose(&g, &acts, durak_core::Playout::Heuristic, &mut rng);
            g.apply(&acts[i]).unwrap();
            moves += 1;
        }
    }
    let per = t.elapsed().as_secs_f64() / 20_000.0;
    println!("a full playout from the start: {:.1} us, {:.0} moves, {:.2} us per move", per * 1e6, moves as f64 / 20_000.0, per * 1e6 / (moves as f64 / 20_000.0));
    println!("per search iteration: {:.1} us", total / f64::from(n) * 1e6);
}
