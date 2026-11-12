//! Shared helpers for building hand-made positions.
#![allow(dead_code, unused_imports)]

use durak_core::*;

pub use durak_core::PlayerId::{P0, P1, P2};
pub use durak_core::Rank::*;
pub use durak_core::Suit::{Clubs, Diamonds, Hearts, Spades};

pub fn c(suit: Suit, rank: Rank) -> Card {
    Card::public(suit, rank)
}

pub fn pile(attack: Card, defense: Option<Card>) -> Pile {
    Pile { attack, defense }
}

/// A position described by what matters; everything else is filled in.
pub struct Pos {
    pub hands: Vec<Vec<Card>>,
    pub stock: Vec<Card>,
    pub table: Vec<Pile>,
    pub attacker: PlayerId,
    pub defender: PlayerId,
    pub phase: Phase,
    pub trump: Suit,
    pub reflecting: bool,
    pub trump_reflecting: bool,
    pub deck_size: usize,
    pub first_trick_limit: bool,
    pub classic_ending: bool,
}

impl Pos {
    pub fn new(hands: Vec<Vec<Card>>) -> Self {
        Self {
            hands,
            stock: vec![],
            table: vec![],
            attacker: P0,
            defender: P1,
            phase: Phase::Attacking,
            trump: Hearts,
            reflecting: false,
            trump_reflecting: false,
            deck_size: 36,
            first_trick_limit: false,
            classic_ending: false,
        }
    }

    /// Build the state. Cards not mentioned anywhere go to the discard pile so
    /// that the deck is complete. Does not validate - call `.validate()` or use `build`.
    pub fn build_unchecked(self) -> GameState {
        let n = self.hands.len();
        let mut used: Vec<CardId> = vec![];
        used.extend(self.hands.iter().flatten().filter_map(Card::id));
        used.extend(self.stock.iter().filter_map(Card::id));
        for p in &self.table {
            used.extend(p.attack.id());
            used.extend(p.defense.as_ref().and_then(Card::id));
        }
        let discard = full_deck(self.deck_size)
            .unwrap()
            .into_iter()
            .filter(|c| !used.contains(&c.id().unwrap()))
            .map(|c| c.as_public())
            .collect();

        let mut attackers = vec![];
        let mut pid = self.attacker;
        for _ in 0..n {
            if pid != self.defender {
                attackers.push(pid);
            }
            pid = pid.next(n);
        }

        GameState {
            trump: self.trump,
            attacker: self.attacker,
            defender: self.defender,
            phase: self.phase,
            attackers,
            current_attacker_idx: 0,
            last_played_attacker: self.attacker,
            throw_start_idx: 0,
            hands: self.hands,
            stock: self.stock,
            table: self.table,
            discard,
            reflected_trumps: vec![],
            config: GameConfig {
                deck_size: self.deck_size,
                num_players: n,
                trump_reflecting: self.trump_reflecting,
                reflecting: self.reflecting,
                first_trick_limit: self.first_trick_limit,
                classic_ending: self.classic_ending,
            },
        }
    }

    pub fn build(self) -> GameState {
        let s = self.build_unchecked();
        s.validate().expect("test position must be valid");
        s
    }
}

pub fn attack(card: Card) -> Action {
    Action::Attack { card }
}
pub fn defend(card: Card) -> Action {
    Action::Defend { pile_index: 0, card }
}
pub fn defend_at(pile_index: usize, card: Card) -> Action {
    Action::Defend { pile_index, card }
}
pub fn throw(card: Card) -> Action {
    Action::Throw { card: Some(card) }
}
pub const DONE: Action = Action::Throw { card: None };

/// Apply a sequence of actions, panicking with context on the first failure.
pub fn play(state: &mut GameState, actions: &[Action]) {
    for (i, a) in actions.iter().enumerate() {
        state.apply(a).unwrap_or_else(|e| panic!("step {i} {a:?} failed: {e}"));
        state.validate().unwrap_or_else(|e| panic!("state invalid after step {i} {a:?}: {e}"));
    }
}

pub fn has(state: &GameState, action: &Action) -> bool {
    state.legal_actions().iter().any(|a| a.same_move(action))
}

/// Every action that could conceivably be named, legal or not.
pub fn every_action(deck_size: usize) -> Vec<Action> {
    let mut all = vec![Action::PassAttack, Action::Take, DONE];
    for card in full_deck(deck_size).unwrap() {
        all.push(Action::Attack { card });
        all.push(Action::Throw { card: Some(card) });
        all.push(Action::Reflect { card });
        all.push(Action::ReflectTrump { card });
        for pile_index in 0..MAX_PILES {
            all.push(Action::Defend { pile_index, card });
        }
    }
    all
}
