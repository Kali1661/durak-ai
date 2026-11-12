//! Durak core engine: rules, state transitions and a determinized MCTS evaluator.
//!
//! Supported tables: 2 or 3 players, decks of 24, 36 or 52 cards, six-card hands.
//!
//! Rule summary (the variant this engine implements):
//! * One attack card is played at a time; the defender answers before the next.
//! * At most [`MAX_PILES`] attack cards per trick, and never more undefended
//!   cards than the defender holds.
//! * Throw-in: every attacker may add cards whose rank is already on the table.
//! * Optional *reflecting*: before defending anything, the defender may play a
//!   card of the same rank to pass the whole attack to the next player. With
//!   two players that next player is the original attacker.
//! * Optional *trump reflecting*: as above, but by showing (not playing) the
//!   trump of that rank, at most once per trump per trick.
//! * After a trick, attackers draw first (main attacker first), defender last.
//! * The game ends when the stock is empty and at most one player holds cards.
//!   That player is the durak; if nobody holds cards the game is a draw.
//!
//! Every public entry point validates its input and returns an error instead of
//! panicking. [`GameState::legal_actions`] and [`GameState::apply`] agree
//! exactly: `apply` accepts an action if and only if `legal_actions` lists it,
//! and a rejected action leaves the state untouched.

mod mcts;

pub use mcts::{
    mcts_evaluate_actions, solve, ActionEval, MctsEval, SolveParams, SolveResult,
    MAX_DETERMINIZATIONS, MAX_ROLLOUTS, MAX_SEARCH_DEPTH,
};

use rand::{rngs::StdRng, Rng, SeedableRng};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Cards dealt to each player, and the size hands are refilled to.
pub const HAND_SIZE: usize = 6;
/// Maximum number of attack cards (piles) in a single trick.
pub const MAX_PILES: usize = 6;
pub const MIN_PLAYERS: usize = 2;
pub const MAX_PLAYERS: usize = 3;
/// 24 = nine to ace, 36 = six to ace, 52 = two to ace.
pub const SUPPORTED_DECK_SIZES: [usize; 3] = [24, 36, 52];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Suit {
    Clubs,
    Diamonds,
    Hearts,
    Spades,
}

impl Suit {
    pub const ALL: [Suit; 4] = [Suit::Clubs, Suit::Diamonds, Suit::Hearts, Suit::Spades];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[repr(u8)]
pub enum Rank {
    Two = 2,
    Three = 3,
    Four = 4,
    Five = 5,
    Six = 6,
    Seven = 7,
    Eight = 8,
    Nine = 9,
    Ten = 10,
    Jack = 11,
    Queen = 12,
    King = 13,
    Ace = 14,
}

impl Rank {
    pub const ALL: [Rank; 13] = [
        Rank::Two, Rank::Three, Rank::Four, Rank::Five, Rank::Six, Rank::Seven, Rank::Eight,
        Rank::Nine, Rank::Ten, Rank::Jack, Rank::Queen, Rank::King, Rank::Ace,
    ];
}

/// Suit and rank of a card, ignoring who can see it.
pub type CardId = (Suit, Rank);

/// A card with visibility - who knows what this card is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Card {
    /// Everyone knows this card (played cards, the trump card, taken cards).
    Public { suit: Suit, rank: Rank },
    /// Only the holder knows this card.
    Private { suit: Suit, rank: Rank },
    /// Not known to the observer (opponent cards and the stock in a real game).
    Unknown,
}

impl Card {
    pub fn public(suit: Suit, rank: Rank) -> Self {
        Card::Public { suit, rank }
    }

    pub fn private(suit: Suit, rank: Rank) -> Self {
        Card::Private { suit, rank }
    }

    /// Suit and rank, or `None` for an unknown card.
    pub fn id(&self) -> Option<CardId> {
        match self {
            Card::Public { suit, rank } | Card::Private { suit, rank } => Some((*suit, *rank)),
            Card::Unknown => None,
        }
    }

    /// Panics on an unknown card; use [`Card::id`] when that is possible.
    pub fn suit(&self) -> Suit {
        self.id().expect("cannot get suit of unknown card").0
    }

    /// Panics on an unknown card; use [`Card::id`] when that is possible.
    pub fn rank(&self) -> Rank {
        self.id().expect("cannot get rank of unknown card").1
    }

    pub fn is_public(&self) -> bool {
        matches!(self, Card::Public { .. })
    }

    pub fn is_private(&self) -> bool {
        matches!(self, Card::Private { .. })
    }

    pub fn is_unknown(&self) -> bool {
        matches!(self, Card::Unknown)
    }

    /// True if this card beats `other` as a defence. Unknown cards beat nothing.
    pub fn beats(&self, other: &Card, trump: Suit) -> bool {
        match (self.id(), other.id()) {
            (Some((s, r)), Some((os, or))) => {
                if s == os { r > or } else { s == trump }
            }
            _ => false,
        }
    }

    /// Same suit and rank, ignoring visibility. Unknown cards match nothing.
    pub fn matches(&self, other: &Card) -> bool {
        match (self.id(), other.id()) {
            (Some(a), Some(b)) => a == b,
            _ => false,
        }
    }

    pub fn as_public(&self) -> Self {
        match self.id() {
            Some((suit, rank)) => Card::Public { suit, rank },
            None => Card::Unknown,
        }
    }

    pub fn as_private(&self) -> Self {
        match self.id() {
            Some((suit, rank)) => Card::Private { suit, rank },
            None => Card::Unknown,
        }
    }
}

/// Seat identifier. Only `P0`-`P2` are used by supported games; the remaining
/// variants exist so that data saved by older versions still parses (and is
/// then rejected by validation with a clear message).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum PlayerId {
    #[default]
    P0,
    P1,
    P2,
    P3,
    P4,
    P5,
}

impl PlayerId {
    pub fn index(self) -> usize {
        self as usize
    }

    /// Seat for an index that is known to be in range (0-5).
    pub fn from_index(i: usize) -> Self {
        match i {
            0 => PlayerId::P0,
            1 => PlayerId::P1,
            2 => PlayerId::P2,
            3 => PlayerId::P3,
            4 => PlayerId::P4,
            _ => PlayerId::P5,
        }
    }

    pub fn next(self, num_players: usize) -> PlayerId {
        PlayerId::from_index((self.index() + 1) % num_players.max(1))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Pile {
    pub attack: Card,
    pub defense: Option<Card>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Phase {
    /// Current attacker may add a card or pass.
    Attacking,
    /// Defender must beat the open pile, reflect, or take.
    Defending,
    /// Defender is taking; attackers may throw in extra cards.
    Throwing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GameConfig {
    /// Number of cards in the deck: one of [`SUPPORTED_DECK_SIZES`].
    pub deck_size: usize,
    /// Number of players: 2 or 3.
    pub num_players: usize,
    /// Defender may redirect an attack by showing the trump of the same rank.
    pub trump_reflecting: bool,
    /// Defender may redirect an attack by playing a card of the same rank.
    pub reflecting: bool,
}

impl Default for GameConfig {
    fn default() -> Self {
        Self { deck_size: 36, num_players: 2, trump_reflecting: false, reflecting: false }
    }
}

impl GameConfig {
    pub fn validate(&self) -> Result<(), EngineError> {
        if !(MIN_PLAYERS..=MAX_PLAYERS).contains(&self.num_players) {
            return Err(EngineError::InvalidConfig(format!(
                "{} players is not supported; choose 2 or 3",
                self.num_players
            )));
        }
        if !SUPPORTED_DECK_SIZES.contains(&self.deck_size) {
            return Err(EngineError::InvalidConfig(format!(
                "a {}-card deck is not supported; choose one of {:?}",
                self.deck_size, SUPPORTED_DECK_SIZES
            )));
        }
        // Every player gets a full hand and at least the trump card stays in the stock.
        if self.num_players * HAND_SIZE >= self.deck_size {
            return Err(EngineError::InvalidConfig(format!(
                "a {}-card deck is too small to deal {} cards to {} players",
                self.deck_size, HAND_SIZE, self.num_players
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EngineError {
    #[error("illegal move: {0}")]
    IllegalMove(String),
    #[error("invalid game settings: {0}")]
    InvalidConfig(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("invalid game state: {0}")]
    InvalidState(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    /// Attacker plays an attacking card.
    Attack { card: Card },
    /// Attacker stops adding cards.
    PassAttack,
    /// Defender beats the open pile.
    Defend { pile_index: usize, card: Card },
    /// Defender picks up the table.
    Take,
    /// Attacker throws in a card for a defender who is taking; `None` = done.
    Throw { card: Option<Card> },
    /// Defender plays a card of the same rank; the attack moves to the next player.
    Reflect { card: Card },
    /// Defender shows the trump of the same rank; the attack moves on, the card stays in hand.
    ReflectTrump { card: Card },
}

impl Action {
    /// The card this action plays or shows, if any.
    pub fn card(&self) -> Option<&Card> {
        match self {
            Action::Attack { card }
            | Action::Defend { card, .. }
            | Action::Reflect { card }
            | Action::ReflectTrump { card } => Some(card),
            Action::Throw { card } => card.as_ref(),
            Action::PassAttack | Action::Take => None,
        }
    }

    /// True if both actions are the same move, ignoring card visibility.
    pub fn same_move(&self, other: &Action) -> bool {
        use Action::*;
        match (self, other) {
            (Attack { card: a }, Attack { card: b })
            | (Reflect { card: a }, Reflect { card: b })
            | (ReflectTrump { card: a }, ReflectTrump { card: b }) => a.matches(b),
            (Defend { pile_index: i, card: a }, Defend { pile_index: j, card: b }) => {
                i == j && a.matches(b)
            }
            (Throw { card: Some(a) }, Throw { card: Some(b) }) => a.matches(b),
            (Throw { card: None }, Throw { card: None }) | (PassAttack, PassAttack) | (Take, Take) => true,
            _ => false,
        }
    }
}

/// How a game stands: still running, or finished with a durak or a draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Outcome {
    InProgress,
    /// The named player is left holding cards.
    Durak { player: PlayerId },
    /// Everybody got rid of their cards.
    Draw,
}

/// Game state with full card tracking.
///
/// Cards carry visibility (Public/Private/Unknown):
/// - Computer-dealt games: every card is known to the engine, private to its holder until played.
/// - Real ("manual") games: only the observer's cards are known; the rest are `Unknown`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameState {
    pub trump: Suit,

    /// Main attacker (first of `attackers`).
    pub attacker: PlayerId,
    pub defender: PlayerId,
    pub phase: Phase,

    /// Players who may attack this trick, main attacker first.
    #[serde(default)]
    pub attackers: Vec<PlayerId>,

    /// Index into `attackers` of the player to attack/throw.
    #[serde(default)]
    pub current_attacker_idx: usize,

    /// The attacker who last played a card (used to detect that everyone passed).
    #[serde(default)]
    pub last_played_attacker: PlayerId,

    /// Attacker index at which throwing started (used to detect completion).
    #[serde(default)]
    pub throw_start_idx: usize,

    pub hands: Vec<Vec<Card>>,

    /// Draw pile; cards are drawn from the end. `stock[0]` is the face-up trump card.
    pub stock: Vec<Card>,

    pub table: Vec<Pile>,
    pub discard: Vec<Card>,

    /// Trumps already shown via `ReflectTrump` this trick.
    #[serde(default)]
    pub reflected_trumps: Vec<Card>,

    pub config: GameConfig,
}

fn invalid_state<T>(msg: impl Into<String>) -> Result<T, EngineError> {
    Err(EngineError::InvalidState(msg.into()))
}

fn invalid_input<T>(msg: impl Into<String>) -> Result<T, EngineError> {
    Err(EngineError::InvalidInput(msg.into()))
}

fn illegal<T>(msg: impl Into<String>) -> Result<T, EngineError> {
    Err(EngineError::IllegalMove(msg.into()))
}

/// Ranks used by a deck of the given size, lowest first.
pub fn deck_ranks(deck_size: usize) -> Result<&'static [Rank], EngineError> {
    if !SUPPORTED_DECK_SIZES.contains(&deck_size) {
        return Err(EngineError::InvalidConfig(format!(
            "a {deck_size}-card deck is not supported; choose one of {SUPPORTED_DECK_SIZES:?}"
        )));
    }
    Ok(&Rank::ALL[Rank::ALL.len() - deck_size / 4..])
}

/// Every card of a deck, as private cards in a fixed order.
pub fn full_deck(deck_size: usize) -> Result<Vec<Card>, EngineError> {
    let ranks = deck_ranks(deck_size)?;
    let mut out = Vec::with_capacity(deck_size);
    for s in Suit::ALL {
        for r in ranks {
            out.push(Card::private(s, *r));
        }
    }
    Ok(out)
}

fn deck_ids(deck_size: usize) -> Result<HashSet<CardId>, EngineError> {
    Ok(full_deck(deck_size)?.iter().filter_map(Card::id).collect())
}

fn seat(index: usize, num_players: usize, what: &str) -> Result<PlayerId, EngineError> {
    if index >= num_players {
        return invalid_input(format!(
            "{what} is player {index}, but this game only has players 0-{}",
            num_players - 1
        ));
    }
    Ok(PlayerId::from_index(index))
}

fn initial_attackers(attacker: PlayerId, defender: PlayerId, num_players: usize) -> Vec<PlayerId> {
    let mut attackers = Vec::with_capacity(num_players - 1);
    let mut pid = attacker;
    for _ in 0..num_players {
        if pid != defender {
            attackers.push(pid);
        }
        pid = pid.next(num_players);
    }
    attackers
}

impl GameState {
    // ─────────────────────────────────────────────────────────────────────
    // Construction
    // ─────────────────────────────────────────────────────────────────────

    /// Deal a new game from a seeded shuffle. Every card is known to the
    /// engine; each player's lowest trump is public and the holder of the
    /// lowest trump overall attacks first (P0 if nobody holds a trump).
    pub fn new_computer_game(seed: u64, config: GameConfig) -> Result<Self, EngineError> {
        config.validate()?;
        let mut rng = StdRng::seed_from_u64(seed);

        let mut deck = full_deck(config.deck_size)?;
        for i in (1..deck.len()).rev() {
            let j = rng.gen_range(0..=i);
            deck.swap(i, j);
        }

        // The trump card sits face-up at the bottom of the stock.
        deck[0] = deck[0].as_public();
        let trump = deck[0].suit();

        let mut hands: Vec<Vec<Card>> = Vec::with_capacity(config.num_players);
        let mut lowest_trump: Option<(usize, Rank)> = None;

        for player_idx in 0..config.num_players {
            let mut hand: Vec<Card> = deck.drain(deck.len() - HAND_SIZE..).collect();
            let lowest = hand
                .iter()
                .enumerate()
                .filter(|(_, c)| c.suit() == trump)
                .min_by_key(|(_, c)| c.rank())
                .map(|(i, c)| (i, c.rank()));
            if let Some((i, rank)) = lowest {
                hand[i] = hand[i].as_public();
                if lowest_trump.map_or(true, |(_, r)| rank < r) {
                    lowest_trump = Some((player_idx, rank));
                }
            }
            hands.push(hand);
        }

        let attacker = PlayerId::from_index(lowest_trump.map_or(0, |(idx, _)| idx));
        let defender = attacker.next(config.num_players);

        let state = Self {
            trump,
            attacker,
            defender,
            phase: Phase::Attacking,
            attackers: initial_attackers(attacker, defender, config.num_players),
            current_attacker_idx: 0,
            last_played_attacker: attacker,
            throw_start_idx: 0,
            hands,
            stock: deck,
            table: vec![],
            discard: vec![],
            reflected_trumps: vec![],
            config,
        };
        state.validate()?;
        Ok(state)
    }

    /// Start tracking a real game in which only the observer's (P0's) hand is known.
    ///
    /// `opponent_trumps` optionally records a trump an opponent has shown
    /// (`None` = nothing shown); those cards become public knowledge.
    pub fn new_manual_game(
        trump_card: Card,
        player_hand: Vec<Card>,
        starting_player: u8,
        opponent_trumps: Vec<(PlayerId, Option<Rank>)>,
        config: GameConfig,
    ) -> Result<Self, EngineError> {
        config.validate()?;
        let n = config.num_players;
        let deck = deck_ids(config.deck_size)?;

        let Some(trump_id) = trump_card.id() else {
            return invalid_input("the trump card must be a known card");
        };
        if !deck.contains(&trump_id) {
            return invalid_input(format!(
                "the trump card is not part of a {}-card deck",
                config.deck_size
            ));
        }
        let trump = trump_id.0;

        if player_hand.len() != HAND_SIZE {
            return invalid_input(format!(
                "your starting hand must have exactly {HAND_SIZE} cards, got {}",
                player_hand.len()
            ));
        }
        let mut seen: HashSet<CardId> = HashSet::new();
        seen.insert(trump_id);
        for card in &player_hand {
            let Some(id) = card.id() else {
                return invalid_input("your starting hand cannot contain unknown cards");
            };
            if !deck.contains(&id) {
                return invalid_input(format!(
                    "{id:?} is not part of a {}-card deck",
                    config.deck_size
                ));
            }
            if id == trump_id {
                return invalid_input("the trump card cannot also be in your hand");
            }
            if !seen.insert(id) {
                return invalid_input(format!("{id:?} appears twice in your hand"));
            }
        }

        let attacker = seat(starting_player as usize, n, "the starting player")?;

        let mut hands: Vec<Vec<Card>> = Vec::with_capacity(n);
        hands.push(player_hand.iter().map(Card::as_private).collect());
        for _ in 1..n {
            hands.push(vec![Card::Unknown; HAND_SIZE]);
        }

        let mut declared: HashSet<usize> = HashSet::new();
        for (pid, rank) in &opponent_trumps {
            let idx = pid.index();
            if idx == 0 || idx >= n {
                return invalid_input(format!("{pid:?} is not an opponent in this game"));
            }
            if !declared.insert(idx) {
                return invalid_input(format!("{pid:?} has more than one shown trump"));
            }
            let Some(rank) = rank else { continue };
            let id = (trump, *rank);
            if !deck.contains(&id) {
                return invalid_input(format!(
                    "{id:?} is not part of a {}-card deck",
                    config.deck_size
                ));
            }
            if !seen.insert(id) {
                return invalid_input(format!(
                    "{id:?} cannot be an opponent's trump: it is already accounted for"
                ));
            }
            hands[idx][0] = Card::public(id.0, id.1);
        }

        let stock_size = config.deck_size - n * HAND_SIZE;
        let mut stock = vec![Card::Unknown; stock_size];
        stock[0] = trump_card.as_public();

        let defender = attacker.next(n);
        let state = Self {
            trump,
            attacker,
            defender,
            phase: Phase::Attacking,
            attackers: initial_attackers(attacker, defender, n),
            current_attacker_idx: 0,
            last_played_attacker: attacker,
            throw_start_idx: 0,
            hands,
            stock,
            table: vec![],
            discard: vec![],
            reflected_trumps: vec![],
            config,
        };
        state.validate()?;
        Ok(state)
    }

    // ─────────────────────────────────────────────────────────────────────
    // Validation
    // ─────────────────────────────────────────────────────────────────────

    /// Check every structural invariant. States built by this crate always
    /// pass; this exists for states that arrive from outside (saved games).
    pub fn validate(&self) -> Result<(), EngineError> {
        self.config.validate()?;
        let n = self.config.num_players;

        if self.hands.len() != n {
            return invalid_state(format!(
                "expected {n} hands, found {}",
                self.hands.len()
            ));
        }
        for (name, pid) in [
            ("attacker", self.attacker),
            ("defender", self.defender),
            ("last attacker", self.last_played_attacker),
        ] {
            if pid.index() >= n {
                return invalid_state(format!("{name} {pid:?} is not a player in this game"));
            }
        }
        if let Some(pid) = self.attackers.iter().find(|p| p.index() >= n) {
            return invalid_state(format!("attacker {pid:?} is not a player in this game"));
        }

        // Card accounting: every card of the deck exactly once.
        let deck = deck_ids(self.config.deck_size)?;
        let mut seen: HashSet<CardId> = HashSet::new();
        let mut total = 0usize;
        let mut account = |card: &Card, place: &str, must_be_known: bool| -> Result<(), EngineError> {
            total += 1;
            match card.id() {
                None if must_be_known => invalid_state(format!("unknown card {place}")),
                None => Ok(()),
                Some(id) if !deck.contains(&id) => invalid_state(format!(
                    "{id:?} {place} is not part of a {}-card deck",
                    self.config.deck_size
                )),
                Some(id) if !seen.insert(id) => {
                    invalid_state(format!("{id:?} appears more than once ({place})"))
                }
                Some(_) => Ok(()),
            }
        };
        for hand in &self.hands {
            for card in hand {
                account(card, "in a hand", false)?;
            }
        }
        for card in &self.stock {
            account(card, "in the stock", false)?;
        }
        for pile in &self.table {
            account(&pile.attack, "on the table", true)?;
            if let Some(d) = &pile.defense {
                account(d, "on the table", true)?;
            }
        }
        for card in &self.discard {
            account(card, "in the discard pile", true)?;
        }
        if total != self.config.deck_size {
            return invalid_state(format!(
                "{total} cards in play, but the deck has {}",
                self.config.deck_size
            ));
        }

        if let Some(bottom) = self.stock.first() {
            match bottom.id() {
                Some((suit, _)) if suit == self.trump => {}
                _ => return invalid_state("the bottom stock card must be the known trump card"),
            }
        }
        if self.table.len() > MAX_PILES {
            return invalid_state(format!("more than {MAX_PILES} piles on the table"));
        }
        for card in &self.reflected_trumps {
            match card.id() {
                Some((suit, _)) if suit == self.trump => {}
                _ => return invalid_state("a shown trump is not a known trump card"),
            }
        }
        for pile in &self.table {
            if let Some(d) = &pile.defense {
                if !d.beats(&pile.attack, self.trump) {
                    return invalid_state("a defended pile holds a card that does not beat the attack");
                }
            }
        }

        if self.is_terminal() {
            return Ok(());
        }

        // Turn structure only matters while the game is running.
        if self.attacker == self.defender {
            return invalid_state("attacker and defender are the same player");
        }
        if self.attackers.is_empty() || self.attackers[0] != self.attacker {
            return invalid_state("the attackers list must start with the main attacker");
        }
        let unique: HashSet<PlayerId> = self.attackers.iter().copied().collect();
        if unique.len() != self.attackers.len() || unique.contains(&self.defender) {
            return invalid_state("the attackers list is inconsistent");
        }
        if self.current_attacker_idx >= self.attackers.len()
            || self.throw_start_idx >= self.attackers.len()
        {
            return invalid_state("attacker index out of range");
        }

        let open = self.undefended_pile_count();
        match self.phase {
            Phase::Attacking if open > 0 => {
                return invalid_state("attacking phase with an undefended pile")
            }
            Phase::Defending if open == 0 => {
                return invalid_state("defending phase without an undefended pile")
            }
            Phase::Throwing if self.table.is_empty() => {
                return invalid_state("throwing phase with an empty table")
            }
            _ => {}
        }
        if open > self.hand_size(self.defender) {
            return invalid_state("more undefended cards than the defender can answer");
        }
        if self.table.is_empty()
            && (self.hand_size(self.attacker) == 0 || self.hand_size(self.defender) == 0)
        {
            return invalid_state("a new trick needs an attacker and a defender with cards");
        }
        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────
    // Queries
    // ─────────────────────────────────────────────────────────────────────

    pub fn hand_size(&self, pid: PlayerId) -> usize {
        self.hands.get(pid.index()).map_or(0, Vec::len)
    }

    pub fn num_players(&self) -> usize {
        self.hands.len()
    }

    /// A player is in the game while they hold cards or can still draw.
    pub fn is_player_active(&self, pid: PlayerId) -> bool {
        self.hand_size(pid) > 0 || !self.stock.is_empty()
    }

    fn players_with_cards(&self) -> Vec<PlayerId> {
        (0..self.num_players())
            .map(PlayerId::from_index)
            .filter(|p| self.hand_size(*p) > 0)
            .collect()
    }

    /// The game is over when the stock is empty and nobody can still get out.
    ///
    /// One subtle case: when a single player holds cards but is the defender
    /// of an unanswered attack, the game is *not* over yet - beating that
    /// attack with their last card makes the game a draw.
    pub fn is_terminal(&self) -> bool {
        if !self.stock.is_empty() {
            return false;
        }
        let holders = self.players_with_cards();
        match holders.as_slice() {
            [] => true,
            [last] => !(*last == self.defender && self.phase == Phase::Defending),
            _ => false,
        }
    }

    pub fn outcome(&self) -> Outcome {
        if !self.is_terminal() {
            return Outcome::InProgress;
        }
        match self.players_with_cards().as_slice() {
            [player] => Outcome::Durak { player: *player },
            _ => Outcome::Draw,
        }
    }

    /// The loser, if the game has ended with one. `None` both while the game
    /// is running and after a draw - use [`GameState::outcome`] to tell them apart.
    pub fn durak(&self) -> Option<PlayerId> {
        match self.outcome() {
            Outcome::Durak { player } => Some(player),
            _ => None,
        }
    }

    pub fn actor_to_move(&self) -> PlayerId {
        match self.phase {
            Phase::Attacking | Phase::Throwing => {
                if self.attackers.is_empty() {
                    self.attacker
                } else {
                    self.attackers[self.current_attacker_idx % self.attackers.len()]
                }
            }
            Phase::Defending => self.defender,
        }
    }

    /// True if no card anywhere is `Unknown`.
    pub fn is_fully_known(&self) -> bool {
        self.hands.iter().flatten().chain(self.stock.iter()).all(|c| !c.is_unknown())
    }

    fn ranks_on_table(&self) -> HashSet<Rank> {
        let mut set = HashSet::new();
        for p in &self.table {
            set.insert(p.attack.rank());
            if let Some(d) = p.defense {
                set.insert(d.rank());
            }
        }
        set
    }

    fn open_pile_index(&self) -> Option<usize> {
        self.table.iter().position(|p| p.defense.is_none())
    }

    fn undefended_pile_count(&self) -> usize {
        self.table.iter().filter(|p| p.defense.is_none()).count()
    }

    /// Room for one more attack card: under the pile limit and within what the
    /// defender could still answer.
    fn can_add_pile(&self) -> bool {
        self.table.len() < MAX_PILES
            && self.undefended_pile_count() < self.hand_size(self.defender)
    }

    fn next_active_player(&self, from: PlayerId) -> PlayerId {
        let n = self.num_players();
        let mut pid = from.next(n);
        for _ in 0..n {
            if pid != from && self.is_player_active(pid) {
                return pid;
            }
            pid = pid.next(n);
        }
        from
    }

    /// Who would have to defend if the current defender reflected: the next
    /// active player in seat order. With two players this is the attacker.
    fn reflect_target(&self) -> Option<PlayerId> {
        let target = self.next_active_player(self.defender);
        (target != self.defender).then_some(target)
    }

    /// Every card id visible to somebody: table, discard, known hand cards and
    /// the trump card.
    fn known_ids(&self) -> HashSet<CardId> {
        let mut known = HashSet::new();
        for pile in &self.table {
            known.extend(pile.attack.id());
            known.extend(pile.defense.as_ref().and_then(Card::id));
        }
        known.extend(self.discard.iter().filter_map(Card::id));
        known.extend(self.hands.iter().flatten().filter_map(Card::id));
        known.extend(self.stock.iter().filter_map(Card::id));
        known
    }

    /// Cards a player could play. A hand with unknown cards could hold any
    /// card that is not accounted for elsewhere.
    fn usable_cards(&self, pid: PlayerId) -> Vec<Card> {
        let hand = &self.hands[pid.index()];
        let mut usable: Vec<Card> = hand.iter().filter(|c| !c.is_unknown()).copied().collect();
        if usable.len() == hand.len() {
            return usable;
        }
        let known = self.known_ids();
        if let Ok(deck) = full_deck(self.config.deck_size) {
            usable.extend(
                deck.into_iter()
                    .filter(|c| c.id().is_some_and(|id| !known.contains(&id)))
                    .map(|c| c.as_public()),
            );
        }
        usable
    }

    fn can_reflect(&self, card: &Card, attack: &Card) -> bool {
        self.config.reflecting
            && card.rank() == attack.rank()
            && self.table.iter().all(|p| p.defense.is_none())
            && self.table.len() < MAX_PILES
            && self
                .reflect_target()
                .is_some_and(|t| self.hand_size(t) >= self.table.len() + 1)
    }

    fn can_reflect_trump(&self, card: &Card, attack: &Card) -> bool {
        self.config.trump_reflecting
            && card.suit() == self.trump
            && card.rank() == attack.rank()
            && self.table.iter().all(|p| p.defense.is_none())
            && !self.reflected_trumps.iter().any(|r| r.matches(card))
            && self
                .reflect_target()
                .is_some_and(|t| self.hand_size(t) >= self.table.len())
    }

    /// Every action the player to move may take. Empty only when the game is over.
    pub fn legal_actions(&self) -> Vec<Action> {
        if self.is_terminal() {
            return vec![];
        }
        let actor = self.actor_to_move();

        match self.phase {
            Phase::Attacking => {
                let usable = self.usable_cards(actor);
                if self.table.is_empty() {
                    return usable.into_iter().map(|card| Action::Attack { card }).collect();
                }
                let mut acts = vec![Action::PassAttack];
                if self.can_add_pile() {
                    let ranks = self.ranks_on_table();
                    acts.extend(
                        usable
                            .into_iter()
                            .filter(|c| ranks.contains(&c.rank()))
                            .map(|card| Action::Attack { card }),
                    );
                }
                acts
            }
            Phase::Defending => {
                let Some(pile_index) = self.open_pile_index() else {
                    return vec![];
                };
                let attack = self.table[pile_index].attack;
                let mut acts = Vec::new();
                for card in self.usable_cards(self.defender) {
                    if card.beats(&attack, self.trump) {
                        acts.push(Action::Defend { pile_index, card });
                    }
                    if self.can_reflect(&card, &attack) {
                        acts.push(Action::Reflect { card });
                    }
                    if self.can_reflect_trump(&card, &attack) {
                        acts.push(Action::ReflectTrump { card });
                    }
                }
                acts.push(Action::Take);
                acts
            }
            Phase::Throwing => {
                let mut acts = vec![Action::Throw { card: None }];
                if self.can_add_pile() {
                    let ranks = self.ranks_on_table();
                    acts.extend(
                        self.usable_cards(actor)
                            .into_iter()
                            .filter(|c| ranks.contains(&c.rank()))
                            .map(|card| Action::Throw { card: Some(card) }),
                    );
                }
                acts
            }
        }
    }

    pub fn is_legal(&self, action: &Action) -> bool {
        self.legal_actions().iter().any(|a| a.same_move(action))
    }

    // ─────────────────────────────────────────────────────────────────────
    // Transitions
    // ─────────────────────────────────────────────────────────────────────

    /// Apply an action. Accepts exactly the actions [`GameState::legal_actions`]
    /// lists; anything else is rejected and the state is left unchanged.
    pub fn apply(&mut self, action: &Action) -> Result<(), EngineError> {
        if self.is_terminal() {
            return illegal("the game is over");
        }
        if !self.is_legal(action) {
            return illegal(format!(
                "{action:?} is not allowed for {:?} right now",
                self.actor_to_move()
            ));
        }
        let mut next = self.clone();
        next.execute(action)?;
        *self = next;
        Ok(())
    }

    /// Perform an action without the legality check. Only for actions taken
    /// straight from `legal_actions()` of this state (the search does this to
    /// avoid generating the move list twice per step). On error the state may
    /// be partially updated and must be discarded.
    pub(crate) fn execute(&mut self, action: &Action) -> Result<(), EngineError> {
        let actor = self.actor_to_move();

        match (self.phase, action) {
            (Phase::Attacking, Action::Attack { card }) => {
                let card = *card;
                self.remove_from_hand(actor, &card)?;
                self.table.push(Pile { attack: card.as_public(), defense: None });
                self.last_played_attacker = actor;
                self.phase = Phase::Defending;
                Ok(())
            }
            (Phase::Attacking, Action::PassAttack) => {
                if self.table.is_empty() || self.open_pile_index().is_some() {
                    return illegal("cannot pass now");
                }
                if !self.attackers.is_empty() {
                    self.current_attacker_idx =
                        (self.current_attacker_idx + 1) % self.attackers.len();
                    // Not back at the player who attacked last: someone else may still add cards.
                    if self.attackers[self.current_attacker_idx] != self.last_played_attacker {
                        return Ok(());
                    }
                }

                // Everyone passed: the defence succeeded.
                for p in self.table.drain(..) {
                    self.discard.push(p.attack);
                    self.discard.extend(p.defense);
                }
                self.refill_hands();
                self.new_trick(self.defender);
                Ok(())
            }
            (Phase::Defending, Action::Defend { pile_index, card }) => {
                let (pile_index, card) = (*pile_index, *card);
                match self.table.get(pile_index) {
                    Some(p) if p.defense.is_none() && card.beats(&p.attack, self.trump) => {}
                    _ => return illegal("that card cannot defend that pile"),
                }
                self.remove_from_hand(self.defender, &card)?;
                self.table[pile_index].defense = Some(card.as_public());
                if self.open_pile_index().is_none() {
                    self.phase = Phase::Attacking;
                }
                Ok(())
            }
            (Phase::Defending, Action::Take) => {
                self.throw_start_idx = self.current_attacker_idx;
                self.phase = Phase::Throwing;
                Ok(())
            }
            (Phase::Defending, Action::Reflect { card }) => {
                let card = *card;
                let Some(new_defender) = self.reflect_target() else {
                    return illegal("nobody to pass the attack to");
                };
                self.remove_from_hand(self.defender, &card)?;
                self.table.push(Pile { attack: card.as_public(), defense: None });
                self.redirect_attack(new_defender);
                Ok(())
            }
            (Phase::Defending, Action::ReflectTrump { card }) => {
                let card = *card;
                let Some(new_defender) = self.reflect_target() else {
                    return illegal("nobody to pass the attack to");
                };
                // The card stays in hand but is now known to everyone.
                let hand = &mut self.hands[self.defender.index()];
                let slot = hand
                    .iter()
                    .position(|c| c.matches(&card))
                    .or_else(|| hand.iter().position(Card::is_unknown));
                match slot {
                    Some(i) => hand[i] = card.as_public(),
                    None => return illegal("that trump is not in the defender's hand"),
                }
                self.reflected_trumps.push(card.as_public());
                self.redirect_attack(new_defender);
                Ok(())
            }
            (Phase::Throwing, Action::Throw { card: Some(card) }) => {
                let card = *card;
                self.remove_from_hand(actor, &card)?;
                self.table.push(Pile { attack: card.as_public(), defense: None });
                Ok(())
            }
            (Phase::Throwing, Action::Throw { card: None }) => {
                if !self.attackers.is_empty() {
                    self.current_attacker_idx =
                        (self.current_attacker_idx + 1) % self.attackers.len();
                    if self.current_attacker_idx != self.throw_start_idx {
                        return Ok(());
                    }
                }

                // Everyone is done throwing: the defender picks up the table.
                let taken: Vec<Card> = self
                    .table
                    .drain(..)
                    .flat_map(|p| std::iter::once(p.attack).chain(p.defense))
                    .collect();
                let defender = self.defender;
                self.hands[defender.index()].extend(taken);
                self.refill_hands();
                // The defender loses their turn to attack.
                self.new_trick(defender.next(self.num_players()));
                Ok(())
            }
            (_, a) => illegal(format!("{a:?} is not allowed in the {:?} phase", self.phase)),
        }
    }

    /// The reflecting defender becomes the main attacker; `new_defender` must answer.
    fn redirect_attack(&mut self, new_defender: PlayerId) {
        self.attacker = self.defender;
        self.defender = new_defender;
        self.attackers = self.build_attackers(self.attacker);
        self.current_attacker_idx = 0;
        self.last_played_attacker = self.attacker;
        self.phase = Phase::Defending;
    }

    /// Active players other than the defender, starting from `main_attacker`.
    fn build_attackers(&self, main_attacker: PlayerId) -> Vec<PlayerId> {
        let n = self.num_players();
        let mut attackers = Vec::with_capacity(n.saturating_sub(1));
        let mut pid = main_attacker;
        for _ in 0..n {
            if pid != self.defender && (pid == main_attacker || self.is_player_active(pid)) {
                attackers.push(pid);
            }
            pid = pid.next(n);
        }
        attackers
    }

    fn new_trick(&mut self, main_attacker: PlayerId) {
        let main_attacker = if self.is_player_active(main_attacker) {
            main_attacker
        } else {
            self.next_active_player(main_attacker)
        };
        self.attacker = main_attacker;
        self.defender = self.next_active_player(main_attacker);
        self.attackers = self.build_attackers(main_attacker);
        self.current_attacker_idx = 0;
        self.last_played_attacker = main_attacker;
        self.throw_start_idx = 0;
        self.phase = Phase::Attacking;
        self.reflected_trumps.clear();
        self.table.clear();
    }

    /// Remove a card from a hand. If the hand does not show the card but has
    /// unknown cards, one of those turns out to have been it.
    fn remove_from_hand(&mut self, pid: PlayerId, card: &Card) -> Result<(), EngineError> {
        let hand = &mut self.hands[pid.index()];
        let slot = hand
            .iter()
            .position(|c| c.matches(card))
            .or_else(|| hand.iter().position(Card::is_unknown));
        match slot {
            Some(i) => {
                hand.swap_remove(i);
                Ok(())
            }
            None => illegal(format!("{pid:?} does not hold that card")),
        }
    }

    /// Draw back up to six cards: attackers in order, the defender last.
    fn refill_hands(&mut self) {
        let mut order = self.attackers.clone();
        order.push(self.defender);
        for pid in order {
            while self.hand_size(pid) < HAND_SIZE {
                let Some(card) = self.stock.pop() else { return };
                // The face-up trump stays public; everything else is private to the drawer.
                let card = if card.is_public() || card.is_unknown() { card } else { card.as_private() };
                self.hands[pid.index()].push(card);
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────────
    // Hidden information
    // ─────────────────────────────────────────────────────────────────────

    /// Tell the engine what an unknown card in `pid`'s hand turned out to be
    /// (used when the observer draws from the stock in a real game).
    pub fn reveal_card(&mut self, pid: PlayerId, card: Card) -> Result<(), EngineError> {
        if pid.index() >= self.num_players() {
            return invalid_input(format!("{pid:?} is not a player in this game"));
        }
        let Some(id) = card.id() else {
            return invalid_input("cannot reveal an unknown card");
        };
        if !deck_ids(self.config.deck_size)?.contains(&id) {
            return invalid_input(format!(
                "{id:?} is not part of a {}-card deck",
                self.config.deck_size
            ));
        }
        if self.known_ids().contains(&id) {
            return invalid_input(format!("{id:?} is already accounted for"));
        }
        let hand = &mut self.hands[pid.index()];
        match hand.iter().position(Card::is_unknown) {
            Some(i) => {
                hand[i] = card.as_private();
                Ok(())
            }
            None => invalid_input(format!("{pid:?} has no unknown cards")),
        }
    }

    /// Card ids `perspective` can see: public cards plus their own hand.
    fn visible_ids(&self, perspective: PlayerId) -> HashSet<CardId> {
        let mut known = HashSet::new();
        for (idx, hand) in self.hands.iter().enumerate() {
            for card in hand {
                if card.is_public() || (card.is_private() && idx == perspective.index()) {
                    known.extend(card.id());
                }
            }
        }
        known.extend(self.stock.iter().filter(|c| c.is_public()).filter_map(Card::id));
        known.extend(self.discard.iter().filter_map(Card::id));
        for pile in &self.table {
            known.extend(pile.attack.id());
            known.extend(pile.defense.as_ref().and_then(Card::id));
        }
        known
    }

    /// Positions (hand, index) and stock indices whose card `perspective` cannot see.
    fn hidden_slots(&self, perspective: PlayerId) -> (Vec<(usize, usize)>, Vec<usize>) {
        let mut hand_slots = Vec::new();
        for (h, hand) in self.hands.iter().enumerate() {
            for (i, card) in hand.iter().enumerate() {
                if card.is_unknown() || (card.is_private() && h != perspective.index()) {
                    hand_slots.push((h, i));
                }
            }
        }
        let stock_slots = self
            .stock
            .iter()
            .enumerate()
            .filter(|(_, c)| !c.is_public())
            .map(|(i, _)| i)
            .collect();
        (hand_slots, stock_slots)
    }

    /// The cards `perspective` cannot see, in a fixed order.
    fn hidden_pool(&self, perspective: PlayerId) -> Result<Vec<Card>, EngineError> {
        let visible = self.visible_ids(perspective);
        Ok(full_deck(self.config.deck_size)?
            .into_iter()
            .filter(|c| c.id().is_some_and(|id| !visible.contains(&id)))
            .map(|c| c.as_public())
            .collect())
    }

    /// Sample one concrete world consistent with what `perspective` knows:
    /// every card they can see stays put, every hidden card is dealt at random.
    /// The result has no unknown cards. Fails if the state is inconsistent or
    /// if `perspective` does not know their own hand.
    pub fn determinize(
        &self,
        perspective: PlayerId,
        rng: &mut impl Rng,
    ) -> Result<Self, EngineError> {
        if perspective.index() >= self.num_players() {
            return invalid_input(format!("{perspective:?} is not a player in this game"));
        }
        if self.hands[perspective.index()].iter().any(Card::is_unknown) {
            return invalid_state(format!(
                "{perspective:?} has cards that have not been identified yet"
            ));
        }

        let mut pool = self.hidden_pool(perspective)?;
        let (hand_slots, stock_slots) = self.hidden_slots(perspective);
        if pool.len() != hand_slots.len() + stock_slots.len() {
            return invalid_state(format!(
                "{} hidden cards but {} cards unaccounted for",
                hand_slots.len() + stock_slots.len(),
                pool.len()
            ));
        }
        for i in (1..pool.len()).rev() {
            let j = rng.gen_range(0..=i);
            pool.swap(i, j);
        }

        let mut state = self.clone();
        let mut deal = pool.into_iter();
        for (h, i) in hand_slots {
            state.hands[h][i] = deal.next().expect("pool size checked");
        }
        for i in stock_slots {
            state.stock[i] = deal.next().expect("pool size checked");
        }
        for card in state.hands.iter_mut().flatten() {
            *card = card.as_public();
        }
        Ok(state)
    }

    /// True if `perspective` can work out every remaining card: the stock is
    /// empty, they know their own hand, and at most one other player holds
    /// cards they cannot see.
    pub fn all_cards_deducible(&self, perspective: PlayerId) -> bool {
        if !self.stock.is_empty() || perspective.index() >= self.num_players() {
            return false;
        }
        if self.hands[perspective.index()].iter().any(Card::is_unknown) {
            return false;
        }
        let hidden_hands = self
            .hands
            .iter()
            .enumerate()
            .filter(|(i, hand)| *i != perspective.index() && hand.iter().any(|c| !c.is_public()))
            .count();
        hidden_hands <= 1
    }

    /// Replace unknown cards with the cards they must be. Only valid when
    /// [`GameState::all_cards_deducible`] holds.
    pub fn deduce_cards(&self, perspective: PlayerId) -> Result<Self, EngineError> {
        if !self.all_cards_deducible(perspective) {
            return invalid_state("the remaining cards cannot be deduced yet");
        }
        let known = self.known_ids();
        let mut remaining: Vec<Card> = full_deck(self.config.deck_size)?
            .into_iter()
            .filter(|c| c.id().is_some_and(|id| !known.contains(&id)))
            .map(|c| c.as_public())
            .collect();
        let unknown_slots = self.hands.iter().flatten().filter(|c| c.is_unknown()).count();
        if remaining.len() != unknown_slots {
            return invalid_state(format!(
                "{unknown_slots} unknown cards but {} cards unaccounted for",
                remaining.len()
            ));
        }
        let mut state = self.clone();
        for card in state.hands.iter_mut().flatten() {
            if card.is_unknown() {
                *card = remaining.pop().expect("count checked");
            }
        }
        Ok(state)
    }
}

/// Pick a uniformly random legal action; `None` when the game is over.
pub fn pick_random_action(state: &GameState, seed: u64) -> Option<Action> {
    let actions = state.legal_actions();
    if actions.is_empty() {
        return None;
    }
    let mut rng = StdRng::seed_from_u64(seed);
    Some(actions[rng.gen_range(0..actions.len())].clone())
}
