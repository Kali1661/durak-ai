//! The exact endgame solver.
//!
//! The solver has its own fast copy of the two-player rules, so most of these
//! tests are about trust: it must agree with a second solver that uses the
//! engine's real rules, and a move it calls winning must actually win when
//! played out against anything.

mod common;
use common::*;
use durak_core::*;
use rand::{rngs::StdRng, Rng, SeedableRng};

/// Generous limit for tests on small positions.
const NODES: u64 = 2_000_000;

/// Run on a thread with a large stack: the reference solver recurses deeply.
fn with_big_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new().stack_size(512 << 20).spawn(f).unwrap().join().unwrap()
}

fn two_player_configs() -> Vec<GameConfig> {
    let mut out = vec![];
    for (reflecting, trump_reflecting) in [(false, false), (true, false), (false, true), (true, true)] {
        for classic_ending in [false, true] {
            for first_trick_limit in [false, true] {
                // Cover each attack size without multiplying the run time.
                let cards_per_attack = [1, 2, 6][(out.len()) % 3];
                out.push(GameConfig {
                    deck_size: 24, num_players: 2, reflecting, trump_reflecting, first_trick_limit, classic_ending,
                    cards_per_attack,
                });
            }
        }
    }
    out
}

/// Positions from random real games, taken once the solver applies and at most
/// `max_cards` cards are still in play. They come up in every phase:
/// mid-trick, after a pass, while throwing in.
fn endgame_positions(config: GameConfig, seeds: std::ops::Range<u64>, max_cards: usize) -> Vec<GameState> {
    let mut out = vec![];
    for seed in seeds {
        let mut rng = StdRng::seed_from_u64(seed);
        let mut s = GameState::new_computer_game(seed, config).unwrap();
        for _ in 0..3_000 {
            if s.is_terminal() {
                break;
            }
            // Every card still in play, including the table the defender may pick up.
            let in_play = s.hands[0].len() + s.hands[1].len() + s.stock.len()
                + s.table.iter().map(|p| 1 + usize::from(p.defense.is_some())).sum::<usize>();
            if is_exactly_solvable(&s) && in_play <= max_cards {
                out.push(s.clone());
            }
            let acts = s.legal_actions();
            s.apply(&acts[rng.gen_range(0..acts.len())]).unwrap();
        }
    }
    out
}

#[test]
fn the_fast_solver_agrees_with_the_engine_rules() {
    let checked = with_big_stack(|| {
        let mut checked = 0;
        for config in two_player_configs() {
            for s in endgame_positions(config, 1..14, 8) {
                let fast = solve_exact(&s, NODES).unwrap().expect("solvable");
                let slow = solve_exact_reference(&s, NODES).unwrap().expect("solvable");
                assert!(fast.actions.iter().all(|a| a.value.is_some()), "fast solver ran out of nodes
{s:#?}");
                for (f, r) in fast.actions.iter().zip(&slow.actions) {
                    assert_eq!(f.action, r.action);
                    // The reference search is slow; where it gave up there is nothing to compare.
                    if r.value.is_some() {
                        assert_eq!(f.value, r.value, "{:?} in {config:?}\n{s:#?}", f.action);
                        checked += 1;
                    }
                }
            }
        }
        checked
    });
    assert!(checked > 500, "only {checked} moves were compared");
}

#[test]
fn a_move_called_winning_wins_against_any_replies() {
    // Follow the solver's winning moves; let the opponent play at random.
    // The opponent must end up as the durak every time.
    let mut played = 0;
    for config in two_player_configs() {
        for start in endgame_positions(config, 100..104, 8) {
            let me = start.actor_to_move();
            let first = solve_exact(&start, NODES).unwrap().unwrap();
            if first.value != Some(ExactValue::Win) {
                continue;
            }
            let mut rng = StdRng::seed_from_u64(played);
            let mut s = start.clone();
            for _ in 0..2_000 {
                if s.is_terminal() {
                    break;
                }
                let action = if s.actor_to_move() == me {
                    let r = solve_exact(&s, NODES).unwrap().unwrap();
                    assert_eq!(r.value, Some(ExactValue::Win), "a won position stopped being won\n{s:#?}");
                    r.actions.into_iter().find(|a| a.value == Some(ExactValue::Win)).unwrap().action
                } else {
                    let acts = s.legal_actions();
                    acts[rng.gen_range(0..acts.len())].clone()
                };
                s.apply(&action).unwrap();
            }
            let loser = if me == P0 { P1 } else { P0 };
            assert_eq!(s.outcome(), Outcome::Durak { player: loser }, "from {start:#?}");
            played += 1;
        }
    }
    assert!(played > 30, "only {played} won positions were played out");
}

#[test]
fn a_move_called_losing_loses_to_the_solver() {
    // The mirror image: when every move loses, the opponent - following the
    // solver - makes that true whatever the loser tries.
    let mut played = 0;
    for config in two_player_configs() {
        for start in endgame_positions(config, 200..204, 8) {
            let me = start.actor_to_move();
            let first = solve_exact(&start, NODES).unwrap().unwrap();
            if first.value != Some(ExactValue::Loss) {
                continue;
            }
            let mut rng = StdRng::seed_from_u64(played);
            let mut s = start.clone();
            for _ in 0..2_000 {
                if s.is_terminal() {
                    break;
                }
                let acts = s.legal_actions();
                let action = if s.actor_to_move() == me {
                    acts[rng.gen_range(0..acts.len())].clone()
                } else {
                    let r = solve_exact(&s, NODES).unwrap().unwrap();
                    r.actions.into_iter().find(|a| a.value == Some(ExactValue::Win)).expect("still winning").action
                };
                s.apply(&action).unwrap();
            }
            assert_eq!(s.outcome(), Outcome::Durak { player: me }, "from {start:#?}");
            played += 1;
        }
    }
    assert!(played > 30, "only {played} lost positions were played out");
}

#[test]
fn simple_positions_have_the_obvious_values() {
    let value_of = |r: &ExactResult, wanted: &Action| {
        r.actions.iter().find(|a| a.action.same_move(wanted)).expect("move is legal").value
    };

    // One card each, I attack with a king into an ace: with draws it is a draw
    // (the ace beats it and both are out); with the classic ending the defender loses.
    for (classic, expected) in [(false, ExactValue::Draw), (true, ExactValue::Win)] {
        let mut pos = Pos::new(vec![vec![c(Clubs, King)], vec![c(Clubs, Ace)]]);
        pos.classic_ending = classic;
        let r = solve_exact(&pos.build(), NODES).unwrap().unwrap();
        assert_eq!(r.value, Some(expected), "classic = {classic}");
    }

    // The defender's side of the same position under the classic ending:
    // beating the king and taking it both lose.
    let mut pos = Pos::new(vec![vec![c(Clubs, King)], vec![c(Clubs, Ace)]]);
    pos.classic_ending = true;
    let mut s = pos.build();
    play(&mut s, &[attack(c(Clubs, King))]);
    let r = solve_exact(&s, NODES).unwrap().unwrap();
    assert_eq!(r.perspective, P1);
    assert_eq!(r.value, Some(ExactValue::Loss));
    assert_eq!(value_of(&r, &defend(c(Clubs, Ace))), Some(ExactValue::Loss));
    assert_eq!(value_of(&r, &Action::Take), Some(ExactValue::Loss));

    // Two cards against one: leading the card the defender cannot beat wins,
    // leading the one they can beat lets them out first.
    let pos = Pos::new(vec![vec![c(Clubs, Ace), c(Diamonds, Six)], vec![c(Diamonds, King)]]);
    let r = solve_exact(&pos.build(), NODES).unwrap().unwrap();
    assert_eq!(value_of(&r, &attack(c(Clubs, Ace))), Some(ExactValue::Win));
    assert_eq!(value_of(&r, &attack(c(Diamonds, Six))), Some(ExactValue::Loss));
    assert_eq!(r.value, Some(ExactValue::Win));
}

#[test]
fn the_solver_only_applies_without_hidden_cards() {
    // Three players: no.
    let three = Pos::new(vec![vec![c(Clubs, King)], vec![c(Clubs, Ace)], vec![c(Clubs, Queen)]]).build();
    assert!(!is_exactly_solvable(&three));
    assert_eq!(solve_exact(&three, NODES).unwrap(), None);

    // A fresh game has a full stock: no.
    let fresh = GameState::new_computer_game(1, GameConfig { deck_size: 24, num_players: 2, ..GameConfig::default() }).unwrap();
    assert!(!is_exactly_solvable(&fresh));
    assert_eq!(solve_exact(&fresh, NODES).unwrap(), None);

    // A finished game: nothing to solve.
    let mut done = Pos::new(vec![vec![c(Clubs, King)], vec![c(Clubs, Ace)]]).build();
    play(&mut done, &[attack(c(Clubs, King)), defend(c(Clubs, Ace))]);
    assert_eq!(solve_exact(&done, NODES).unwrap(), None);
}

#[test]
fn the_opponents_unseen_cards_are_worked_out() {
    // A real game from my seat: the opponent's two cards are unknown to me,
    // but with the stock empty they can only be the two cards not seen.
    let known = Pos::new(vec![vec![c(Clubs, Ace), c(Diamonds, Six)], vec![c(Diamonds, King)]]);
    let mut s = known.build();
    s.hands[1] = vec![Card::Unknown];
    s.validate().unwrap();
    let r = solve_exact(&s, NODES).unwrap().expect("solvable from my seat");
    assert_eq!(r.value, Some(ExactValue::Win));
    // The moves are reported exactly as the position lists them.
    let listed = s.legal_actions();
    assert_eq!(r.actions.iter().map(|a| a.action.clone()).collect::<Vec<_>>(), listed);
}

#[test]
fn a_move_that_cannot_be_settled_is_reported_as_unknown() {
    // A big position with almost no budget: some answers must be missing, and
    // the ones given must match what a full search says.
    let config = GameConfig { deck_size: 36, num_players: 2, reflecting: true, ..GameConfig::default() };
    let s = endgame_positions(config, 1..30, 24).into_iter().max_by_key(|s| s.hands[0].len() + s.hands[1].len()).unwrap();
    let starved = solve_exact(&s, 30).unwrap().unwrap();
    assert!(starved.nodes <= 30 + starved.actions.len() as u64);
    assert!(starved.actions.iter().any(|a| a.value.is_none()), "30 positions were enough?");
    assert_eq!(starved.actions.len(), s.legal_actions().len());
}

#[test]
fn recorded_positions_from_real_games() {
    // Positions from games played with the app, where the sampling search was
    // badly wrong: it showed 65-77% for positions that were lost outright.
    let text = include_str!("data/recorded_endgames.txt");
    let find = |game: &str, step: &str| {
        let line = text.lines().find(|l| l.starts_with(&format!("{game}|{step}|"))).expect("position exists");
        recorded::parse(line).0
    };
    let values = |s: &GameState| {
        let r = solve_exact(s, 5_000_000).unwrap().unwrap();
        (r.value, r.actions.iter().map(|a| a.value).collect::<Vec<_>>())
    };

    // Two cards against fourteen, shown as 73% and 69%: both attacks lose.
    let (value, moves) = values(&find("9", "45"));
    assert_eq!(value, Some(ExactValue::Loss));
    assert_eq!(moves, vec![Some(ExactValue::Loss); 2]);

    // A won position where the search preferred a losing pass-on (48%) over
    // the only winning defence (40%).
    let s = find("6", "52");
    let r = solve_exact(&s, 4_000_000).unwrap().unwrap();
    assert_eq!(r.value, Some(ExactValue::Win));
    let value_of = |wanted: Action| r.actions.iter().find(|a| a.action.same_move(&wanted)).unwrap().value;
    // The move played, which the search had put first at 48%, loses by force.
    assert_eq!(value_of(Action::Reflect { card: c(Hearts, King) }), Some(ExactValue::Loss));
    assert_eq!(value_of(Action::Defend { pile_index: 0, card: c(Hearts, Jack) }), Some(ExactValue::Win));
    assert_eq!(value_of(Action::Take), Some(ExactValue::Win));

    // Every recorded position parses into a valid state.
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        recorded::parse(line).0.validate().unwrap();
    }
}
