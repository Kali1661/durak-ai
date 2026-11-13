//! The fast two-player playout against the real rules, move by move: the
//! same legal moves, the same costs for the heuristic player, the same
//! resulting positions and the same hand score.

use durak_core::*;
use rand::{rngs::StdRng, Rng, SeedableRng};

fn configs() -> Vec<GameConfig> {
    let mut all = Vec::new();
    for deck_size in [24, 36, 52] {
        for (reflecting, trump_reflecting) in [(false, false), (true, false), (true, true)] {
            for cards_per_attack in [1, 6] {
                for classic_ending in [false, true] {
                    all.push(GameConfig {
                        deck_size,
                        num_players: 2,
                        reflecting,
                        trump_reflecting,
                        cards_per_attack,
                        classic_ending,
                        ..GameConfig::default()
                    });
                }
            }
        }
    }
    all
}

#[test]
fn the_fast_playout_follows_the_rules_and_the_heuristic_player() {
    let mut positions = 0u32;
    let mut scored = 0u32;
    for (c, config) in configs().into_iter().enumerate() {
        for seed in 0..12u64 {
            let mut rng = StdRng::seed_from_u64(seed * 1000 + c as u64);
            let dealt = GameState::new_computer_game(seed + 100 * c as u64, config).unwrap();
            // Every card known, as in a playout.
            let mut s = dealt.determinize(PlayerId::P0, &mut rng).unwrap();
            for _ in 0..600 {
                let (fast, score, over) = fast_playout_probe(&s, None);
                assert_eq!(over, s.is_terminal(), "is the game over? {config:?} seed {seed}");
                if over {
                    break;
                }
                let legal = s.legal_actions();
                // With several cards to beat, the real rules answer the first
                // one laid and the fast code the lowest: only comparable when
                // those are the same card.
                let open: Vec<CardId> = s.table.iter().filter(|p| p.defense.is_none()).filter_map(|p| p.attack.id()).collect();
                let lowest = open.iter().min_by_key(|(suit, rank)| (*suit as usize, *rank));
                let comparable = s.phase != Phase::Defending || open.first() == lowest;
                if !comparable {
                    let i = rng.gen_range(0..legal.len());
                    s.apply(&legal[i].clone()).unwrap();
                    continue;
                }
                let mut slow: Vec<i32> = legal.iter().map(|a| policy::move_cost(&s, a)).collect();
                let mut quick: Vec<i32> = fast.iter().map(|m| m.0).collect();
                slow.sort();
                quick.sort();
                assert_eq!(slow, quick, "move costs differ in {s:?}");

                if s.stock.is_empty() && s.phase == Phase::Attacking && s.table.is_empty() {
                    assert!((score - eval::attacker_wins(&s)).abs() < 1e-9, "hand score differs in {s:?}");
                    scored += 1;
                }

                // Mostly the heuristic move, sometimes any other, as playouts do.
                let i = if rng.gen_bool(0.3) {
                    rng.gen_range(0..legal.len())
                } else {
                    policy::choose(&s, &legal, Playout::Heuristic, &mut rng)
                };
                let action = legal[i].clone();
                let cost = policy::move_cost(&s, &action);
                let mut next = s.clone();
                next.apply(&action).unwrap();
                let (fast, _, _) = fast_playout_probe(&s, Some(&next));
                assert!(
                    fast.iter().any(|(c, same)| *same && *c == cost),
                    "no fast move matches {action:?} (cost {cost}) in {s:?}"
                );
                s = next;
                positions += 1;
            }
            assert!(s.is_terminal(), "the game ended");
        }
    }
    assert!(positions > 10_000 && scored > 500, "{positions} positions, {scored} scored");
}

#[test]
fn searching_with_fast_playouts_gives_sane_scores() {
    let config = GameConfig { deck_size: 36, num_players: 2, reflecting: true, classic_ending: true, ..GameConfig::default() };
    let s = GameState::new_computer_game(8, config).unwrap();
    let r = solve(&s, SolveParams { determinizations: 4, rollouts: 500, ..SolveParams::app() }, 3).unwrap();
    assert_eq!(r.total_visits, 2000);
    assert!(r.actions.iter().all(|a| a.visits > 0 && (0.0..=1.0).contains(&a.score)));
    // A fresh game is close to even: nothing should look won or lost.
    assert!(r.actions.iter().all(|a| (0.15..=0.85).contains(&a.score)), "{:?}", r.actions);
}
