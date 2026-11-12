//! How the simulated players move in a playout, beyond the search tree.
//!
//! Uniformly random players are a poor model of Durak: they waste trumps,
//! so keeping one is worth little in a random game, and they dislike taking,
//! because the player with the most cards usually loses a random game. A
//! search built on them likes beating a card with a trump over taking it.
//!
//! The heuristic player gives every legal move a cost and plays the cheapest,
//! the way a sensible beginner would: lead and throw in low cards, beat with
//! the lowest card that works, keep trumps while the stock is large and take
//! a small pile rather than spend one. Now and then it plays a random move
//! instead, so playouts still cover other lines.

use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::{Action, Card, GameState, Rank, Suit};

/// How playouts choose moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Playout {
    /// Every legal move equally likely.
    Random,
    /// Mostly the cheapest move by [`move_cost`], sometimes a random one.
    #[default]
    Heuristic,
}

/// Chance that the heuristic player makes a random move instead.
pub const RANDOM_MOVE_CHANCE: f64 = 0.1;

/// Trumps rank above every plain card by this much.
const TRUMP_BONUS: i32 = 20;

/// How much a card is worth keeping: its rank, plus a bonus for a trump.
fn worth(card: &Card, trump: Suit) -> i32 {
    card.rank() as i32 + if card.suit() == trump { TRUMP_BONUS } else { 0 }
}

/// Cards worth less than this are thrown in rather than kept.
fn throw_limit(state: &GameState) -> i32 {
    if state.stock.is_empty() {
        // Nothing left to draw: get rid of every plain card.
        Rank::Ace as i32 + 1
    } else {
        Rank::Queen as i32
    }
}

/// What taking the cards costs, on the same scale as [`worth`]: a defender
/// beats a card when the cheapest card that does so costs less than this.
fn take_cost(state: &GameState) -> i32 {
    let cards_on_table: i32 = state
        .table
        .iter()
        .map(|p| 1 + i32::from(p.defense.is_some()))
        .sum();
    let base = match state.stock.len() {
        0 => return i32::MAX / 2, // taking at the end nearly always loses
        n if n >= 12 => 24,       // early: only the lowest trumps beat a small pile
        n if n >= 6 => 28,
        _ => 31,
    };
    base + 3 * cards_on_table
}

/// The cost of a move for the heuristic player; lower is better.
pub fn move_cost(state: &GameState, action: &Action) -> i32 {
    let trump = state.trump;
    match action {
        // Leading a trick: the lowest card.
        Action::Attack { card } if state.table.is_empty() => worth(card, trump),
        // Adding to an attack or throwing in: only cards worth losing.
        Action::Attack { card } | Action::Throw { card: Some(card) } => {
            worth(card, trump) - throw_limit(state)
        }
        Action::PassAttack | Action::Throw { card: None } | Action::LetDefend => 0,
        Action::Defend { card, .. } => worth(card, trump),
        Action::Take => take_cost(state),
        // Passing the attack on costs the card but no answer.
        Action::Reflect { card } => worth(card, trump) - 8,
        // Showing a trump passes the attack on and keeps the card.
        Action::ReflectTrump { .. } => -10,
    }
}

/// Pick a move index from `actions` (the legal moves of `state`, not empty).
pub fn choose<R: Rng>(state: &GameState, actions: &[Action], playout: Playout, rng: &mut R) -> usize {
    if actions.len() == 1 {
        return 0;
    }
    if playout == Playout::Random || rng.gen_bool(RANDOM_MOVE_CHANCE) {
        return rng.gen_range(0..actions.len());
    }
    let mut best = 0;
    let mut best_cost = i32::MAX;
    for (i, a) in actions.iter().enumerate() {
        let c = move_cost(state, a);
        if c < best_cost {
            best_cost = c;
            best = i;
        }
    }
    best
}

/// The heuristic player's move in `state`, with no randomness; `None` when the game is over.
pub fn heuristic_action(state: &GameState) -> Option<Action> {
    state.legal_actions().into_iter().min_by_key(|a| move_cost(state, a))
}
