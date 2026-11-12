//! Randomized state-transition tests with fixed seeds.
//!
//! Whole games are played with random legal moves across every supported
//! table. After each move the state must still satisfy every invariant.

mod common;
use common::*;
use durak_core::*;
use rand::{rngs::StdRng, Rng, SeedableRng};

const MOVE_LIMIT: usize = 5_000;

fn configs() -> Vec<GameConfig> {
    let mut out = vec![];
    for deck_size in SUPPORTED_DECK_SIZES {
        for num_players in [2, 3] {
            for (reflecting, trump_reflecting) in [(false, false), (true, false), (false, true), (true, true)] {
                for first_trick_limit in [false, true] {
                    // Alternate the ending so both are covered without doubling the run time.
                    let classic_ending = first_trick_limit ^ reflecting;
                    out.push(GameConfig {
                        deck_size, num_players, reflecting, trump_reflecting, first_trick_limit, classic_ending,
                    });
                }
            }
        }
    }
    out
}

fn check_finished(s: &GameState) {
    assert!(s.stock.is_empty());
    assert!(s.legal_actions().is_empty());
    let holders: Vec<usize> = (0..s.num_players()).filter(|i| !s.hands[*i].is_empty()).collect();
    match s.outcome() {
        Outcome::Durak { player } if holders.is_empty() => {
            assert!(s.config.classic_ending, "everyone out is a draw unless the classic ending is on");
            assert_eq!(player, s.last_played_attacker, "the classic ending blames the last attacker");
        }
        Outcome::Durak { player } => assert_eq!(holders, vec![player.index()]),
        Outcome::Draw => {
            assert!(holders.is_empty());
            assert!(!s.config.classic_ending, "the classic ending never produces a draw");
        }
        Outcome::InProgress => panic!("finished game reports in progress"),
    }
}

#[test]
fn random_games_stay_valid_and_finish_on_every_supported_table() {
    for config in configs() {
        for seed in 0..6u64 {
            let mut rng = StdRng::seed_from_u64(seed * 7919 + config.deck_size as u64);
            let mut s = GameState::new_computer_game(seed, config).unwrap();
            let mut moves = 0;
            while !s.is_terminal() {
                let legal = s.legal_actions();
                assert!(!legal.is_empty(), "{config:?} seed {seed}: stuck after {moves} moves");
                // Every listed action must apply cleanly and leave a valid state.
                for a in &legal {
                    let mut t = s.clone();
                    t.apply(a).unwrap_or_else(|e| panic!("{config:?} seed {seed}: legal {a:?} rejected: {e}"));
                    t.validate().unwrap_or_else(|e| panic!("{config:?} seed {seed}: {a:?} broke the state: {e}"));
                }
                let pick = legal[rng.gen_range(0..legal.len())].clone();
                s.apply(&pick).unwrap();
                moves += 1;
                assert!(moves < MOVE_LIMIT, "{config:?} seed {seed}: game did not finish");
            }
            s.validate().unwrap();
            check_finished(&s);
        }
    }
}

#[test]
fn apply_accepts_exactly_the_listed_actions_and_nothing_else() {
    for config in configs().into_iter().filter(|c| c.deck_size == 36) {
        for seed in 0..3u64 {
            let mut rng = StdRng::seed_from_u64(seed + 100);
            let mut s = GameState::new_computer_game(seed, config).unwrap();
            let candidates = every_action(config.deck_size);
            let mut moves = 0;
            while !s.is_terminal() && moves < MOVE_LIMIT {
                let legal = s.legal_actions();
                for a in &candidates {
                    let listed = legal.iter().any(|l| l.same_move(a));
                    let mut t = s.clone();
                    let accepted = t.apply(a).is_ok();
                    assert_eq!(accepted, listed, "{config:?} seed {seed} move {moves}: {a:?}");
                    if !accepted {
                        assert_eq!(t, s, "a rejected action changed the state: {a:?}");
                    }
                }
                let pick = legal[rng.gen_range(0..legal.len())].clone();
                s.apply(&pick).unwrap();
                moves += 1;
            }
            // Nothing is accepted once the game is over.
            for a in &candidates {
                let mut t = s.clone();
                assert!(t.apply(a).is_err());
                assert_eq!(t, s);
            }
        }
    }
}

/// Strip visibility so an action from the dealt game can be replayed on the
/// observer's view of it.
fn as_seen(action: &Action) -> Action {
    match action {
        Action::Attack { card } => Action::Attack { card: card.as_public() },
        Action::Defend { pile_index, card } => Action::Defend { pile_index: *pile_index, card: card.as_public() },
        Action::Throw { card } => Action::Throw { card: card.map(|c| c.as_public()) },
        Action::Reflect { card } => Action::Reflect { card: card.as_public() },
        Action::ReflectTrump { card } => Action::ReflectTrump { card: card.as_public() },
        other => other.clone(),
    }
}

/// A dealt game and P0's real-game ("manual") view of it must never disagree:
/// every move played in the real game is legal in the view, and both tell the
/// same story about hand sizes, table, turn and result.
#[test]
fn the_observers_view_tracks_a_real_game_move_for_move() {
    for config in configs().into_iter().filter(|c| c.deck_size == 36 || c.deck_size == 52) {
        for seed in 0..5u64 {
            let mut rng = StdRng::seed_from_u64(seed + 500);
            let mut real = GameState::new_computer_game(seed, config).unwrap();
            let mut view = GameState::new_manual_game(
                real.stock[0],
                real.hands[0].clone(),
                real.attacker.index() as u8,
                vec![],
                config,
            )
            .unwrap();

            let mut moves = 0;
            let mut solved = 0;
            while !real.is_terminal() {
                // Occasionally check that the view is good enough to search from.
                if view.actor_to_move() == P0 && solved < 2 && rng.gen_bool(0.2) {
                    let p = SolveParams { determinizations: 2, rollouts: 60, ..SolveParams::default() };
                    let r = solve(&view, p, seed).unwrap_or_else(|e| panic!("{config:?} seed {seed}: {e}"));
                    assert_eq!(r.actions.len(), view.legal_actions().len());
                    solved += 1;
                }

                let legal = real.legal_actions();
                let action = legal[rng.gen_range(0..legal.len())].clone();
                let seen = as_seen(&action);
                assert!(view.is_legal(&seen), "{config:?} seed {seed} move {moves}: view rejects {seen:?}");
                real.apply(&action).unwrap();
                view.apply(&seen).unwrap();

                // P0 looks at any cards they just drew.
                for card in real.hands[0].clone() {
                    if !view.hands[0].iter().any(|v| v.matches(&card)) {
                        view.reveal_card(P0, card).unwrap();
                    }
                }
                assert!(view.hands[0].iter().all(|v| !v.is_unknown()));

                view.validate().unwrap_or_else(|e| panic!("{config:?} seed {seed} move {moves}: {e}"));
                assert_eq!(view.outcome(), real.outcome());
                assert_eq!(view.phase, real.phase);
                assert_eq!((view.attacker, view.defender), (real.attacker, real.defender));
                assert_eq!(view.attackers, real.attackers);
                assert_eq!(view.stock.len(), real.stock.len());
                assert_eq!(view.table.len(), real.table.len());
                for i in 0..config.num_players {
                    assert_eq!(view.hands[i].len(), real.hands[i].len());
                }
                if !real.is_terminal() {
                    assert_eq!(view.actor_to_move(), real.actor_to_move());
                }

                moves += 1;
                assert!(moves < MOVE_LIMIT);
            }
        }
    }
}

#[test]
fn searching_from_random_midgame_positions_never_fails() {
    for config in configs().into_iter().filter(|c| c.deck_size == 36) {
        let mut rng = StdRng::seed_from_u64(config.num_players as u64);
        let mut s = GameState::new_computer_game(9, config).unwrap();
        let mut moves = 0;
        while !s.is_terminal() && moves < MOVE_LIMIT {
            if moves % 7 == 0 {
                let p = SolveParams { determinizations: 2, rollouts: 80, ..SolveParams::default() };
                let r = solve(&s, p, moves as u64).unwrap_or_else(|e| panic!("{config:?} move {moves}: {e}"));
                assert_eq!(r.perspective, s.actor_to_move());
                assert_eq!(r.actions.iter().map(|e| e.visits).sum::<u32>(), r.total_visits);
            }
            let legal = s.legal_actions();
            let pick = legal[rng.gen_range(0..legal.len())].clone();
            s.apply(&pick).unwrap();
            moves += 1;
        }
    }
}
