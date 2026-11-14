//! MCTS evaluation: forced outcomes, pooling, determinism and error handling.

mod common;
use common::*;
use durak_core::*;

fn params(determinizations: u32, rollouts: u32) -> SolveParams {
    SolveParams { determinizations, rollouts, ..SolveParams::default() }
}

fn score_of(result: &SolveResult, action: &Action) -> f32 {
    result.actions.iter().find(|e| e.action.same_move(action)).expect("action evaluated").score
}

/// P0 holds 6C 7C against 8C KH AH with hearts trump: whatever P0 leads is
/// beaten, then P0 cannot answer the trumps and ends up holding everything.
/// P0 only escapes if P1 chooses to pick up low clubs instead of beating them.
#[test]
fn a_forced_loss_scores_near_zero() {
    let s = Pos::new(vec![
        vec![c(Clubs, Six), c(Clubs, Seven)],
        vec![c(Clubs, Eight), c(Hearts, King), c(Hearts, Ace)],
    ])
    .build();
    let r = solve(&s, params(1, 20_000), 42).unwrap();
    assert_eq!(r.perspective, P0);
    let best = r.actions[0].score;
    assert!(best < 0.1, "lost position scored {best}");
}

#[test]
fn a_forced_win_scores_near_one() {
    let s = Pos::new(vec![
        vec![c(Clubs, Eight), c(Hearts, King), c(Hearts, Ace)],
        vec![c(Clubs, Six), c(Clubs, Seven)],
    ])
    .build();
    let r = solve(&s, params(1, 20_000), 42).unwrap();
    assert!(r.actions[0].score > 0.9, "won position scored {}", r.actions[0].score);
}

#[test]
fn the_search_tells_a_drawing_defence_from_a_losing_take() {
    let mut s = Pos::new(vec![vec![c(Clubs, King)], vec![c(Clubs, Ace)]]).build();
    s.apply(&attack(c(Clubs, King))).unwrap();
    let r = solve(&s, params(1, 2_000), 1).unwrap();
    assert_eq!(r.perspective, P1);
    assert_eq!(score_of(&r, &defend(c(Clubs, Ace))), 1.0, "a draw is not a loss");
    assert_eq!(score_of(&r, &Action::Take), 0.0);
    assert!(r.actions[0].action.same_move(&defend(c(Clubs, Ace))));
    assert_eq!(r.total_cutoffs, 0);
}

/// Three players, stock empty. Leading the 6C keeps the trump ace for the
/// attack that comes next, which gets P0 out. Leading the ace throws that away.
#[test]
fn three_player_position_prefers_keeping_the_trump() {
    let s = Pos::new(vec![
        vec![c(Clubs, Six), c(Hearts, Ace)],
        vec![c(Diamonds, Seven)],
        vec![c(Diamonds, Eight), c(Diamonds, Nine), c(Diamonds, Ten)],
    ])
    .build();
    let r = solve(&s, params(1, 20_000), 7).unwrap();
    let keep = score_of(&r, &attack(c(Clubs, Six)));
    let waste = score_of(&r, &attack(c(Hearts, Ace)));
    assert!(keep > 0.9, "safe line scored {keep}");
    assert!(keep > waste + 0.1, "6C {keep} should clearly beat AH {waste}");
    assert!(r.actions[0].action.same_move(&attack(c(Clubs, Six))));
}

#[test]
fn three_player_scores_stay_in_range_and_every_move_is_tried() {
    let s = Pos::new(vec![
        vec![c(Clubs, Six), c(Clubs, Seven)],
        vec![c(Clubs, Ace)],
        vec![c(Diamonds, Eight), c(Diamonds, Nine)],
    ])
    .build();
    let r = solve(&s, params(1, 10_000), 3).unwrap();
    assert_eq!(r.actions.len(), 2);
    for e in &r.actions {
        assert!((0.0..=1.0).contains(&e.score));
        assert!(e.visits > 0);
    }
}

#[test]
fn results_cover_every_legal_action_and_visits_add_up() {
    let config = GameConfig { num_players: 3, reflecting: true, ..GameConfig::default() };
    let s = GameState::new_computer_game(5, config).unwrap();
    let r = solve(&s, params(4, 300), 9).unwrap();

    let legal = s.legal_actions();
    assert_eq!(r.actions.len(), legal.len());
    for a in &legal {
        assert!(r.actions.iter().any(|e| e.action == *a), "{a:?} missing from the result");
    }
    assert_eq!(r.determinizations, 4);
    assert_eq!(r.total_visits, 4 * 300);
    assert_eq!(r.actions.iter().map(|e| e.visits).sum::<u32>(), r.total_visits);
    assert!(r.actions.iter().all(|e| (0.0..=1.0).contains(&e.score)));
    assert!(r.actions.iter().all(|e| e.cutoffs <= e.visits));
    assert!(r.actions.windows(2).all(|w| w[0].score >= w[1].score), "sorted best first");
}

#[test]
fn the_same_seed_gives_the_same_result() {
    let s = GameState::new_computer_game(5, GameConfig::default()).unwrap();
    let a = solve(&s, params(3, 400), 123).unwrap();
    let b = solve(&s, params(3, 400), 123).unwrap();
    assert_eq!(a, b);
    let other = solve(&s, params(3, 400), 124).unwrap();
    assert_ne!(a, other, "a different seed samples different deals");
}

#[test]
fn pooling_adds_visits_across_determinizations() {
    let s = Pos::new(vec![
        vec![c(Clubs, Six), c(Hearts, Ace)],
        vec![c(Diamonds, Seven), c(Spades, Nine)],
    ])
    .build();
    let one = solve(&s, params(1, 500), 77).unwrap();
    let many = solve(&s, params(6, 500), 77).unwrap();
    assert_eq!(many.total_visits, 6 * one.total_visits);
    for e in &many.actions {
        assert!(e.visits > 0);
        assert!((0.0..=1.0).contains(&e.score));
    }
}

#[test]
fn truncated_playouts_count_as_unknown_not_as_losses() {
    let s = GameState::new_computer_game(2, GameConfig::default()).unwrap();
    // A two-move limit can never finish a fresh game.
    let p = SolveParams { determinizations: 2, rollouts: 200, max_depth: 2, exploration: 1.41, ..SolveParams::default() };
    let r = solve(&s, p, 5).unwrap();
    assert_eq!(r.total_cutoffs, r.total_visits);
    for e in &r.actions {
        assert_eq!(e.cutoffs, e.visits);
        assert_eq!(e.score, 0.5, "an unfinished playout is worth one half");
    }
}

#[test]
fn a_finished_game_has_nothing_to_evaluate() {
    let mut s = Pos::new(vec![vec![c(Clubs, King)], vec![c(Clubs, Ace)]]).build();
    play(&mut s, &[attack(c(Clubs, King)), defend(c(Clubs, Ace))]);
    let r = solve(&s, params(2, 100), 1).unwrap();
    assert!(r.actions.is_empty());
    assert_eq!(r.total_visits, 0);
}

#[test]
fn solve_rejects_bad_requests() {
    let s = GameState::new_computer_game(2, GameConfig::default()).unwrap();
    for (d, r, depth) in [(0, 100, 100), (1, 0, 100), (1, 100, 0)] {
        let p = SolveParams { determinizations: d, rollouts: r, max_depth: depth, exploration: 1.41, ..SolveParams::default() };
        assert!(matches!(solve(&s, p, 1), Err(EngineError::InvalidInput(_))));
    }
    let huge = SolveParams { determinizations: MAX_DETERMINIZATIONS + 1, ..SolveParams::default() };
    assert!(solve(&s, huge, 1).is_err());
    let huge = SolveParams { rollouts: MAX_ROLLOUTS + 1, ..SolveParams::default() };
    assert!(solve(&s, huge, 1).is_err());
    let nan = SolveParams { exploration: f64::NAN, ..SolveParams::default() };
    assert!(solve(&s, nan, 1).is_err());

    let mut broken = s.clone();
    broken.hands[0].pop();
    assert!(matches!(solve(&broken, params(1, 10), 1), Err(EngineError::InvalidState(_))));
}

#[test]
fn solve_refuses_to_guess_the_movers_own_cards() {
    let hand = vec![
        c(Clubs, Six), c(Clubs, Seven), c(Clubs, Eight),
        c(Clubs, Nine), c(Clubs, Ten), c(Clubs, Jack),
    ];
    // The opponent moves first and their hand is unknown to us.
    let s = GameState::new_manual_game(c(Hearts, Six), hand, 1, vec![], GameConfig::default()).unwrap();
    assert!(matches!(solve(&s, params(1, 10), 1), Err(EngineError::InvalidState(_))));
}

#[test]
fn tree_search_requires_a_fully_known_state() {
    let hand = vec![
        c(Clubs, Six), c(Clubs, Seven), c(Clubs, Eight),
        c(Clubs, Nine), c(Clubs, Ten), c(Clubs, Jack),
    ];
    let s = GameState::new_manual_game(c(Hearts, Six), hand, 0, vec![], GameConfig::default()).unwrap();
    assert!(mcts_evaluate_actions(&s, 1, P0, &SolveParams { rollouts: 10, max_depth: 100, ..SolveParams::default() }).is_err());
    assert!(solve(&s, params(2, 50), 1).is_ok(), "solve determinizes first");
}

#[test]
fn deals_with_few_cards_left_are_solved_exactly() {
    // Late in a game with two cards left to draw.
    let config = GameConfig { deck_size: 24, num_players: 2, ..GameConfig::default() };
    let mut s = GameState::new_computer_game(4, config).unwrap();
    while s.stock.len() > 2 {
        let a = policy::heuristic_action(&s).unwrap();
        s.apply(&a).unwrap();
    }
    assert!(!s.is_terminal());
    let p = SolveParams { determinizations: 4, rollouts: 100, exact_nodes: 2_000_000, exact_stock: 4, ..SolveParams::default() };
    let r = solve(&s, p, 9).unwrap();
    assert!(r.exact_deals > 0, "no deal was solved exactly");
    // Exact values are 0, 1/2 or 1 per deal, so pooled scores stay in range.
    assert!(r.actions.iter().all(|a| (0.0..=1.0).contains(&a.score)));
    // Also with one tree over all deals: near the end each deal is solved on its own.
    let r = solve(&s, SolveParams { shared_tree: true, cutoff: true, ..p }, 9).unwrap();
    assert!(r.exact_deals > 0, "no deal was solved exactly with the shared tree on");

    // Off by default.
    let r = solve(&s, SolveParams { determinizations: 4, rollouts: 100, ..SolveParams::default() }, 9).unwrap();
    assert_eq!(r.exact_deals, 0);
}

#[test]
fn pruning_never_drops_my_own_moves() {
    // A fresh game: the attacker may lead any of six cards. Pruning applies
    // below the root, so every one of them is still searched.
    let config = GameConfig { deck_size: 36, num_players: 2, ..GameConfig::default() };
    let s = GameState::new_computer_game(4, config).unwrap();
    assert_eq!(s.legal_actions().len(), 6);
    let p = SolveParams { determinizations: 2, rollouts: 300, candidates: 2, ..SolveParams::default() };
    let r = solve(&s, p, 1).unwrap();
    assert!(r.actions.iter().all(|a| a.visits > 0));
    // And the results differ from an unpruned search, so it did something below.
    let plain = solve(&s, SolveParams { determinizations: 2, rollouts: 300, ..SolveParams::default() }, 1).unwrap();
    assert_ne!(r.actions, plain.actions);
}

#[test]
fn one_tree_over_all_deals() {
    for players in [2, 3] {
        let config = GameConfig { deck_size: 36, num_players: players, ..GameConfig::default() };
        let mut s = GameState::new_computer_game(11, config).unwrap();
        // A few moves in, so there is a table and a defender to think about.
        for _ in 0..3 {
            let a = policy::heuristic_action(&s).unwrap();
            s.apply(&a).unwrap();
        }
        let p = SolveParams { determinizations: 4, rollouts: 250, shared_tree: true, ..SolveParams::default() };
        let r = solve(&s, p, 5).unwrap();
        // Every playout went through exactly one of my moves.
        assert_eq!(r.actions.iter().map(|a| a.visits).sum::<u32>(), 1000);
        assert_eq!(r.total_visits, 1000);
        assert!(r.actions.iter().all(|a| (0.0..=1.0).contains(&a.score)));
        // Same seed, same answer.
        assert_eq!(solve(&s, p, 5).unwrap().actions, r.actions);
        // Two players only: with three the option is ignored.
        let separate = solve(&s, SolveParams { shared_tree: false, ..p }, 5).unwrap();
        assert_eq!(separate.actions == r.actions, players == 3);
    }
}

#[test]
fn a_continued_search_keeps_its_tree() {
    let config = GameConfig { deck_size: 36, num_players: 2, ..GameConfig::default() };
    let s = GameState::new_computer_game(21, config).unwrap();
    let p = SolveParams { shared_tree: true, ..SolveParams::default() };

    // Two runs of a kept search find together exactly what one run of twice
    // the size finds: the second run goes on from the first one's tree.
    let whole = solve(&s, SolveParams { determinizations: 2, rollouts: 400, ..p }, 5).unwrap();
    let mut search = Search::new(&s, p, 5).unwrap();
    let first = search.run(1, 400).unwrap();
    let second = search.run(1, 400).unwrap();
    assert_eq!(first.total_visits, 400);
    assert_eq!(second.total_visits, 400);
    for a in &whole.actions {
        let part = |r: &durak_core::SolveResult| {
            let e = r.actions.iter().find(|e| e.action == a.action).unwrap();
            (e.visits, f64::from(e.visits) * f64::from(e.score))
        };
        let (v1, s1) = part(&first);
        let (v2, s2) = part(&second);
        assert_eq!(v1 + v2, a.visits);
        assert!((s1 + s2 - f64::from(a.visits) * f64::from(a.score)).abs() < 0.05);
    }
}

#[test]
fn deals_too_big_to_solve_go_to_the_shared_tree() {
    let config = GameConfig { deck_size: 36, num_players: 2, ..GameConfig::default() };
    let mut s = GameState::new_computer_game(3, config).unwrap();
    while s.stock.len() > 4 {
        let a = policy::heuristic_action(&s).unwrap();
        s.apply(&a).unwrap();
    }
    assert!(!s.is_terminal());
    // One position is never enough to solve a deal.
    let p = SolveParams { shared_tree: true, cutoff: true, exact_nodes: 1, exact_stock: 4, ..SolveParams::default() };
    let mut search = Search::new(&s, p, 2).unwrap();
    for _ in 0..2 {
        let r = search.run(3, 200).unwrap();
        assert_eq!(r.exact_deals, 0);
        // Nothing is lost: the tree does the playouts of every deal asked for.
        assert_eq!(r.total_visits, 600);
    }
}

#[test]
fn few_cards_against_a_hoard_of_high_cards_is_not_a_sure_win() {
    // From a real game (24 cards, nothing left to draw): I hold five small
    // cards, the opponent nineteen with the four top trumps. Scoring the
    // hands by counting cards called this 99% for me; the game was lost, and
    // a few moves later the position was a proven loss. Hands this unequal
    // are played out instead of scored.
    let s: GameState = serde_json::from_str(include_str!("data/few-cards-against-a-hoard.json")).unwrap();
    assert!(!eval::in_range(&s));
    let p = SolveParams { determinizations: 4, rollouts: 2000, ..SolveParams::app() };
    let now = solve(&s, p, 3).unwrap().actions[0].score;
    let mut old = p;
    old.style.score_unequal = true;
    let before = solve(&s, old, 3).unwrap().actions[0].score;
    assert!(before > 0.95, "the old scoring showed {before}");
    // A short search is still too hopeful; a long one settles near 30%.
    assert!(now < before - 0.1, "shown {now}, before {before}");
}
