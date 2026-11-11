//! Endgame sanity checks for the MCTS evaluator.
//!
//! These use fully-known positions with an empty stock, where the correct
//! result can be worked out by hand.

use durak_core::{
    mcts_evaluate_actions, Card, GameConfig, GameState, Phase, Pile, PlayerId, Rank, Suit,
};

fn c(suit: Suit, rank: Rank) -> Card {
    Card::public(suit, rank)
}

fn two_player(p0: Vec<Card>, p1: Vec<Card>, attacker: PlayerId, table: Vec<Pile>, phase: Phase) -> GameState {
    let defender = attacker.other();
    GameState {
        trump: Suit::Hearts,
        attacker,
        defender,
        phase,
        attackers: vec![attacker],
        current_attacker_idx: 0,
        last_played_attacker: attacker,
        throw_start_idx: 0,
        hands: vec![p0, p1],
        stock: vec![],
        table,
        discard: vec![],
        reflected_trumps: vec![],
        config: GameConfig::default(),
    }
}

fn best_score(state: &GameState, perspective: PlayerId) -> f32 {
    let eval = mcts_evaluate_actions(state, 42, perspective, 20_000, 100, 1.41);
    eval.actions.iter().map(|a| a.score).fold(0.0, f32::max)
}

/// P0 holds 6C 7C, P1 holds 8C KH AH, hearts trump, P0 to attack.
///
/// Whatever P0 leads, P1 beats it, then leads trumps P0 cannot beat; P0 ends
/// up holding everything. P0 only escapes if P1 voluntarily TAKES low clubs
/// instead of beating them - a blunder no opponent makes.
#[test]
fn forced_loss_is_not_reported_as_a_win() {
    let s = two_player(
        vec![c(Suit::Clubs, Rank::Six), c(Suit::Clubs, Rank::Seven)],
        vec![c(Suit::Clubs, Rank::Eight), c(Suit::Hearts, Rank::King), c(Suit::Hearts, Rank::Ace)],
        PlayerId::P0,
        vec![],
        Phase::Attacking,
    );
    let score = best_score(&s, PlayerId::P0);
    println!("forced loss: best score for P0 = {score:.3}");
    assert!(score < 0.2, "lost position evaluated at {score:.3}");
}

/// Mirror image: P0 holds the trumps and wins by force.
#[test]
fn forced_win_is_reported_as_a_win() {
    let s = two_player(
        vec![c(Suit::Clubs, Rank::Eight), c(Suit::Hearts, Rank::King), c(Suit::Hearts, Rank::Ace)],
        vec![c(Suit::Clubs, Rank::Six), c(Suit::Clubs, Rank::Seven)],
        PlayerId::P0,
        vec![],
        Phase::Attacking,
    );
    let score = best_score(&s, PlayerId::P0);
    println!("forced win: best score for P0 = {score:.3}");
    assert!(score > 0.8, "won position evaluated at {score:.3}");
}
