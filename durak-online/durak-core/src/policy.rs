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
//! a small pile rather than spend one. In a two-player game it adds nothing
//! before the defender answers, every plain card once they have beaten the
//! attack or are taking it, and - once nothing is left to draw - every card. Now and then it plays a random move
//! instead, so playouts still cover other lines.

use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::{Action, Card, GameState, Phase, Rank, Suit};

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

/// How the imagined players and the scoring of their games behave, beyond
/// [`Playout`]. The default is what the app uses; the other settings exist
/// so the arena can measure one against another.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Style {
    /// Score the hands when the stock runs out (see `eval`) whatever their
    /// sizes, as before version 0.3.2. Normally only hands of similar size
    /// are scored and the rest are played out (see `eval::in_range`).
    #[serde(default)]
    pub score_unequal: bool,
    /// Two players, nothing left to draw: the heuristic player adds and
    /// throws in plain cards only and holds its trumps back, as before
    /// version 0.3.2. Normally it then adds every card it can.
    #[serde(default)]
    pub hold_trumps: bool,
    /// Two players: the heuristic player adds cards below the queen at any
    /// moment of an attack, as before version
    /// 0.3.3 (every card with nothing left to draw). Normally it adds
    /// nothing before the defender answers and every plain card once they
    /// have beaten the attack or are taking it.
    #[serde(default)]
    pub old_attack: bool,
}

/// Chance that the heuristic player makes a random move instead.
pub const RANDOM_MOVE_CHANCE: f64 = 0.1;

/// Trumps rank above every plain card by this much.
const TRUMP_BONUS: i32 = 20;

/// How much a card is worth keeping: its rank, plus a bonus for a trump.
fn worth(card: &Card, trump: Suit) -> i32 {
    card.rank() as i32 + if card.suit() == trump { TRUMP_BONUS } else { 0 }
}

/// Above the worth of any card.
const ALL_CARDS: i32 = 99;

/// With cards left to draw, trumps worth less than this (up to the jack)
/// are added once the defender has beaten the attack.
pub(crate) const ADD_AFTER_BEAT: i32 = TRUMP_BONUS + Rank::Queen as i32;

/// With cards left to draw, every plain card is thrown in for a defender
/// who is taking, and no trump.
pub(crate) const THROW_IN: i32 = Rank::Ace as i32 + 1;

/// Cards worth less than this are added to an attack rather than kept.
///
/// Two players: nothing is added before the defender answers - whatever
/// could be added then can still follow once they have beaten the card or
/// taken it. Recorded real games (`examples/realgames.rs`) show the same:
/// a card is added before the answer about one time in eight, and after the
/// attack is beaten nine times in ten, whatever its rank. Against perfect
/// play (`examples/playfit.rs`) this player gives away a better result with
/// one or two cards left to draw in 2% of its moves; adding only cards
/// below the queen at any moment, as before version 0.3.3, in 7%.
fn add_limit(state: &GameState) -> i32 {
    let two = state.num_players() == 2;
    match (state.stock.is_empty(), state.phase) {
        (_, Phase::Adding) if two => 0,
        // Nothing left to draw: get rid of every card that can go.
        (true, _) if two => ALL_CARDS,
        (true, _) => Rank::Ace as i32 + 1,
        (false, _) if !two => Rank::Queen as i32,
        (false, Phase::Throwing) => THROW_IN,
        (false, _) => ADD_AFTER_BEAT,
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
            worth(card, trump) - add_limit(state)
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
