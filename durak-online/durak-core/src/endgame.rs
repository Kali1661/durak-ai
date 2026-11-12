//! Exact endgame solver for two-player games.
//!
//! Once the stock is empty (or holds only the face-up trump card) a two-player
//! game has no hidden information left: every card the player to move cannot
//! see is in the opponent's hand. From there the result with best play by both
//! sides can be *calculated* instead of estimated, by searching every line of
//! play and remembering positions already solved.
//!
//! With passing ("reflecting") allowed, play can go round in circles: each
//! side takes a pair of cards and passes it back, and the same position comes
//! up again. So a move has one of three values. It is a *win* if the mover can
//! force the opponent to become the durak however they reply, a *loss* if the
//! opponent can force that on the mover, and otherwise a *draw*: both sides run
//! out together, or neither can make progress without losing and the play
//! repeats.
//!
//! The search works on a compact copy of the position ([`Pos`], a handful of
//! card sets) with the two-player rules written out again for speed. The
//! engine's own rules stay the authority: [`solve_exact_reference`] runs the
//! same proof on real [`GameState`]s, and the tests check the two agree.
//!
//! The search can be too large for big hands. It is given a limit of
//! `max_nodes` positions, shared out over the legal moves; a move it cannot
//! settle within its share is reported as unknown (`None`) while the others
//! keep their calculated values, and the caller falls back to the sampling
//! search for the rest.

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

use rand::{rngs::StdRng, SeedableRng};
use serde::{Deserialize, Serialize};

use crate::{
    Action, Card, EngineError, GameState, Outcome, Phase, PlayerId, FIRST_TRICK_PILES, HAND_SIZE, MAX_PILES,
};

/// The result of a move with best play by both sides, for the player making it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExactValue {
    /// That player ends up as the durak.
    Loss,
    /// Nobody can be forced to lose: both run out together, or the play repeats.
    Draw,
    /// The opponent ends up as the durak.
    Win,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExactAction {
    pub action: Action,
    /// `None` when the search for this move hit its limit.
    pub value: Option<ExactValue>,
}

/// Every legal move of the player to move with its calculated result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExactResult {
    pub perspective: PlayerId,
    /// The value of the position itself - the best result available - when it
    /// is known: a winning move was found, or every move was settled.
    pub value: Option<ExactValue>,
    /// One entry per legal action, in `legal_actions()` order.
    pub actions: Vec<ExactAction>,
    /// Positions searched.
    pub nodes: u64,
}

/// Can this position be solved exactly? Two players and nothing left to draw
/// except, at most, the face-up trump card.
pub fn is_exactly_solvable(state: &GameState) -> bool {
    state.num_players() == 2
        && state.stock.len() <= 1
        && state.stock.iter().all(|c| !c.is_unknown())
        && !state.hands[state.actor_to_move().index()].iter().any(Card::is_unknown)
}

/// Calculate the result of every legal move for the player to move.
///
/// Returns `Ok(None)` when the position is not exactly solvable (see
/// [`is_exactly_solvable`]) or the game is over. Moves the search could not
/// settle within `max_nodes` positions have the value `None`.
pub fn solve_exact(state: &GameState, max_nodes: u64) -> Result<Option<ExactResult>, EngineError> {
    solve_with(state, max_nodes, &mut FastSearch::new())
}

/// The same calculation done on the engine's own game states: much slower, and
/// limited to small positions, but it shares no rule code with [`solve_exact`].
/// It exists so the tests can check the fast solver against the real rules.
#[doc(hidden)]
pub fn solve_exact_reference(state: &GameState, max_nodes: u64) -> Result<Option<ExactResult>, EngineError> {
    solve_with(state, max_nodes, &mut ReferenceSearch::new())
}

trait PositionSearch {
    /// The value of a running game for P0 (0 = P0 is the durak, 1 = draw,
    /// 2 = P1 is), or `None` if it could not be settled within `budget` more
    /// positions.
    fn value(&mut self, s: &GameState, budget: u64) -> Result<Option<u8>, EngineError>;
    fn nodes(&self) -> u64;
}

fn solve_with(
    state: &GameState,
    max_nodes: u64,
    search: &mut impl PositionSearch,
) -> Result<Option<ExactResult>, EngineError> {
    state.validate()?;
    if state.is_terminal() || !is_exactly_solvable(state) {
        return Ok(None);
    }
    let perspective = state.actor_to_move();
    // The opponent's unseen cards can only be the cards not seen anywhere else,
    // so "dealing" them is not a guess here: there is exactly one way.
    let full = state.determinize(perspective, &mut StdRng::seed_from_u64(0))?;
    if full.hands.iter().flatten().chain(&full.stock).any(Card::is_unknown) {
        return Ok(None);
    }

    let root_actions = state.legal_actions();
    let mut children = Vec::with_capacity(root_actions.len());
    for (shown, real) in root_actions.iter().zip(full.legal_actions()) {
        // Same move order in both: dealing changes visibility, never which moves exist.
        if !shown.same_move(&real) {
            return Err(EngineError::InvalidState("the solver's moves do not match the position".into()));
        }
        let mut child = full.clone();
        child.execute(&real)?;
        children.push(child);
    }

    // Two rounds. In the first every move gets an equal share of the limit,
    // so one hard move cannot starve the rest; whatever the easy moves did not
    // need is then shared out again among the moves still open. What a search
    // has settled is remembered, so the second round builds on the first.
    let mut values: Vec<Option<u8>> = children.iter().map(terminal_value).collect();
    for _round in 0..2 {
        let open: Vec<usize> = (0..children.len()).filter(|i| values[*i].is_none()).collect();
        for (n, i) in open.iter().enumerate() {
            let left = max_nodes.saturating_sub(search.nodes());
            if left == 0 {
                break;
            }
            values[*i] = search.value(&children[*i], left / (open.len() - n) as u64)?;
        }
    }
    let actions: Vec<ExactAction> = root_actions
        .into_iter()
        .zip(values)
        .map(|(action, v)| ExactAction { action, value: v.map(|v| for_player(v, perspective)) })
        .collect();
    let best = actions.iter().filter_map(|a| a.value).max();
    let complete = actions.iter().all(|a| a.value.is_some());
    let value = if complete || best == Some(ExactValue::Win) { best } else { None };
    Ok(Some(ExactResult { perspective, value, actions, nodes: search.nodes() }))
}

/// Value of a finished game for P0: 0 = P0 is the durak, 1 = draw, 2 = P1 is.
fn terminal_value(s: &GameState) -> Option<u8> {
    match s.outcome() {
        Outcome::InProgress => None,
        Outcome::Draw => Some(1),
        Outcome::Durak { player } => Some(if player == PlayerId::P0 { 0 } else { 2 }),
    }
}

fn for_player(p0_value: u8, player: PlayerId) -> ExactValue {
    let v = if player == PlayerId::P0 { p0_value } else { 2 - p0_value };
    match v {
        0 => ExactValue::Loss,
        1 => ExactValue::Draw,
        _ => ExactValue::Win,
    }
}

// ── Hashing ─────────────────────────────────────────────────────────────────

/// Positions are remembered by a 128-bit fingerprint, which is already
/// well-mixed; the table only needs to pass part of it through.
#[derive(Default)]
struct PassThrough(u64);

impl Hasher for PassThrough {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 = (self.0.rotate_left(8) ^ u64::from(*b)).wrapping_mul(0x517c_c1b7_2722_0a95);
        }
    }
    fn write_u128(&mut self, v: u128) {
        self.0 = (v as u64) ^ ((v >> 64) as u64).rotate_left(32);
    }
    fn write_u64(&mut self, v: u64) {
        self.0 = (self.0.rotate_left(5) ^ v).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
}

type Table<V> = HashMap<u128, V, BuildHasherDefault<PassThrough>>;

fn mix(mut x: u64) -> u64 {
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

// ── The proof ───────────────────────────────────────────────────────────────

/// No dependency on any position still being searched.
const SETTLED: u32 = u32::MAX;

/// Bookkeeping for proving, for one player, the positions from which that
/// player can force the other to become the durak.
///
/// "Can force" is a statement about finite play: a position reached again
/// while it is still being examined proves nothing, so it counts as "no" for
/// now. A "no" that leaned on such a repeat is only *provisional* - the
/// repeated position may still turn out to be a "yes" by another route - and
/// is kept apart until the position it leaned on is settled. A "yes" never
/// needs revisiting.
#[derive(Default)]
struct Proof {
    memo: Table<bool>,
    /// Positions on the current line of play, with their depth.
    on_path: Table<u32>,
    /// Provisional "no" answers, and the shallowest depth each leans on.
    provisional: Vec<u128>,
    provisional_low: Table<u32>,
}

impl Proof {
    /// What is already known about a position, before searching it.
    fn lookup(&self, k: u128) -> Option<(bool, u32)> {
        if let Some(known) = self.memo.get(&k) {
            return Some((*known, SETTLED));
        }
        if let Some(d) = self.on_path.get(&k) {
            return Some((false, *d));
        }
        self.provisional_low.get(&k).map(|low| (false, *low))
    }

    /// Record the outcome for a position at `depth` whose search has finished.
    /// `first_provisional` is how many provisional answers existed when its
    /// search began. Returns what to tell the position above.
    fn close(&mut self, k: u128, depth: u32, first_provisional: usize, result: bool, low: u32) -> (bool, u32) {
        self.on_path.remove(&k);
        if result {
            // Proven for good. Answers below that assumed this position was a
            // "no" are void.
            for stale in self.provisional.drain(first_provisional..) {
                self.provisional_low.remove(&stale);
            }
            self.memo.insert(k, true);
            (true, SETTLED)
        } else if low >= depth {
            // Leans on nothing above: this "no", and every provisional "no"
            // found beneath it, is final.
            for settled in self.provisional.drain(first_provisional..) {
                self.provisional_low.remove(&settled);
                self.memo.insert(settled, false);
            }
            self.memo.insert(k, false);
            (false, SETTLED)
        } else {
            // Leans on a position further up the line; so does everything
            // beneath that was waiting on this one.
            for waiting in &self.provisional[first_provisional..] {
                if let Some(l) = self.provisional_low.get_mut(waiting) {
                    *l = (*l).min(low);
                }
            }
            self.provisional.push(k);
            self.provisional_low.insert(k, low);
            (false, low)
        }
    }
}

// ── Fast search on card sets ────────────────────────────────────────────────

const ATTACKING: u8 = 0;
const DEFENDING: u8 = 1;
const THROWING: u8 = 2;
const ADDING: u8 = 3;

/// A two-player position as sets of cards (bit `suit * 13 + rank - 2`).
/// The discard pile is whatever is in none of the sets.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Pos {
    hand: [u64; 2],
    /// Attack cards on the table.
    atk: u64,
    /// Cards that beat them.
    def: u64,
    /// Attack cards not beaten yet.
    open: u64,
    /// Trumps already shown to pass the attack on this trick.
    shown: u64,
    /// The trump card while it is still the last card of the stock.
    stock: u64,
    attacker: u8,
    phase: u8,
    /// Cards laid down since the defender last answered.
    batch: u8,
    /// Whether anything has been discarded yet (the first trick is shorter).
    discarded: bool,
}

#[derive(Clone, Copy)]
struct Rules {
    trump: u32,
    reflecting: bool,
    trump_reflecting: bool,
    first_trick_limit: bool,
    classic_ending: bool,
    cards_per_attack: u8,
}

fn bit(card: &Card) -> u64 {
    match card.id() {
        Some((suit, rank)) => 1u64 << (suit as u32 * 13 + (rank as u32 - 2)),
        None => 0,
    }
}

const RANKS: u64 = 0x1FFF;

/// Every card whose rank appears in `set`.
fn same_ranks(set: u64) -> u64 {
    let ranks = (set | set >> 13 | set >> 26 | set >> 39) & RANKS;
    ranks | ranks << 13 | ranks << 26 | ranks << 39
}

impl Pos {
    fn from_state(s: &GameState) -> Pos {
        let set = |cards: &[Card]| cards.iter().fold(0u64, |m, c| m | bit(c));
        let (mut atk, mut def, mut open) = (0u64, 0u64, 0u64);
        for p in &s.table {
            atk |= bit(&p.attack);
            match &p.defense {
                Some(d) => def |= bit(d),
                None => open |= bit(&p.attack),
            }
        }
        Pos {
            hand: [set(&s.hands[0]), set(&s.hands[1])],
            atk,
            def,
            open,
            shown: set(&s.reflected_trumps),
            stock: set(&s.stock),
            attacker: s.attacker.index() as u8,
            phase: match s.phase {
                Phase::Attacking => ATTACKING,
                Phase::Defending => DEFENDING,
                Phase::Throwing => THROWING,
                Phase::Adding => ADDING,
            },
            batch: s.batch,
            discarded: !s.discard.is_empty(),
        }
    }

    fn fingerprint(&self) -> u128 {
        let flags = u64::from(self.attacker)
            | u64::from(self.phase) << 1
            | u64::from(self.discarded) << 3
            | u64::from(self.batch) << 4;
        let mut a = 0x243F_6A88_85A3_08D3u64;
        let mut b = 0x1319_8A2E_0370_7344u64;
        for v in [self.hand[0], self.hand[1], self.atk, self.def, self.open, self.shown, self.stock, flags] {
            a = mix(a ^ v);
            b = mix(b.rotate_left(23) ^ v ^ 0x9E37_79B9_7F4A_7C15);
        }
        u128::from(a) | u128::from(b) << 64
    }

    fn actor(&self) -> usize {
        if self.phase == DEFENDING { 1 - self.attacker as usize } else { self.attacker as usize }
    }

    /// Value for P0 if the game is over, as in [`GameState::outcome`].
    fn terminal(&self, rules: &Rules) -> Option<u8> {
        if self.stock != 0 {
            return None;
        }
        let defender = 1 - self.attacker as usize;
        match (self.hand[0] != 0, self.hand[1] != 0) {
            (true, true) => None,
            // Everyone ran out in the same trick.
            (false, false) => Some(if !rules.classic_ending {
                1
            } else if defender == 0 {
                0
            } else {
                2
            }),
            (p0_holds, _) => {
                let holder = if p0_holds { 0 } else { 1 };
                // The last defender may still beat the final attack.
                if holder == defender && (self.phase == DEFENDING || self.phase == ADDING) {
                    None
                } else {
                    Some(if holder == 0 { 0 } else { 2 })
                }
            }
        }
    }

    fn pile_limit(&self, rules: &Rules) -> usize {
        if rules.first_trick_limit && !self.discarded { FIRST_TRICK_PILES } else { MAX_PILES }
    }

    fn can_add_pile(&self, rules: &Rules) -> bool {
        let defender = 1 - self.attacker as usize;
        (self.atk.count_ones() as usize) < self.pile_limit(rules)
            && self.open.count_ones() < self.hand[defender].count_ones()
    }

    /// Draw up to six cards, the attacker first, then start the next trick
    /// with `main` attacking (or the other player, if `main` is out).
    fn next_trick(&mut self, main: usize) {
        for p in [self.attacker as usize, 1 - self.attacker as usize] {
            if self.stock != 0 && (self.hand[p].count_ones() as usize) < HAND_SIZE {
                self.hand[p] |= self.stock;
                self.stock = 0;
            }
        }
        let active = |pos: &Pos, p: usize| pos.hand[p] != 0 || pos.stock != 0;
        let main = if active(self, main) || !active(self, 1 - main) { main } else { 1 - main };
        self.attacker = main as u8;
        self.atk = 0;
        self.def = 0;
        self.open = 0;
        self.shown = 0;
        self.batch = 0;
        self.phase = ATTACKING;
    }

    /// Lay down `card` as the attacker; the defender answers next unless more
    /// may and can be added.
    fn lay(&self, card: u64, rules: &Rules) -> Pos {
        let a = self.attacker as usize;
        let mut p = *self;
        p.hand[a] ^= card;
        p.atk |= card;
        p.open |= card;
        p.batch = if self.phase == ADDING { self.batch + 1 } else { 1 };
        // As in the engine: decided by what everyone can see, not by the hand.
        let more = p.batch < rules.cards_per_attack && p.can_add_pile(rules) && p.hand[a] != 0;
        p.phase = if more { ADDING } else { DEFENDING };
        p
    }

    /// Every position one move away, each with a rough cost: moves more likely
    /// to be best come first so the search can stop early.
    fn children(&self, rules: &Rules, out: &mut Vec<(u32, Pos)>) {
        let a = self.attacker as usize;
        let d = 1 - a;
        let trump_cards = RANKS << (rules.trump * 13);
        let cost = |card: u64| card.trailing_zeros() % 13 + if card & trump_cards != 0 { 20 } else { 0 };

        match self.phase {
            ATTACKING | ADDING => {
                let mut playable = if self.atk == 0 {
                    self.hand[a]
                } else if self.can_add_pile(rules) {
                    self.hand[a] & same_ranks(self.atk | self.def)
                } else {
                    0
                };
                while playable != 0 {
                    let card = playable & playable.wrapping_neg();
                    playable ^= card;
                    out.push((100 + cost(card), self.lay(card, rules)));
                }
                if self.phase == ADDING {
                    // Let the defender answer.
                    let mut p = *self;
                    p.phase = DEFENDING;
                    out.push((150, p));
                    out.sort_by_key(|(c, _)| *c);
                    return;
                }
                if self.atk != 0 {
                    // Pass: the defence succeeded, the table is discarded.
                    let mut p = *self;
                    p.discarded = true;
                    p.next_trick(d);
                    out.push((200, p));
                }
            }
            DEFENDING => {
                let target = self.open & self.open.wrapping_neg();
                let target_trump = target & trump_cards != 0;
                let target_suit = RANKS << (target.trailing_zeros() / 13 * 13);
                let same_rank = same_ranks(target);
                let untouched = self.def == 0;
                let piles = self.atk.count_ones();
                let room = (piles as usize) < self.pile_limit(rules);
                let attacker_cards = self.hand[a].count_ones();
                let mut hand = self.hand[d];
                while hand != 0 {
                    let card = hand & hand.wrapping_neg();
                    hand ^= card;
                    let beats = if card & target_suit != 0 {
                        card > target
                    } else {
                        card & trump_cards != 0 && !target_trump
                    };
                    if beats {
                        let mut p = *self;
                        p.hand[d] ^= card;
                        p.def |= card;
                        p.open ^= target;
                        if p.open == 0 {
                            p.phase = ATTACKING;
                        }
                        out.push((100 + cost(card), p));
                    }
                    if card & same_rank == 0 || !untouched {
                        continue;
                    }
                    if rules.reflecting && room && attacker_cards >= piles + 1 {
                        let mut p = *self;
                        p.hand[d] ^= card;
                        p.atk |= card;
                        p.open |= card;
                        p.attacker = d as u8;
                        p.batch = 0;
                        out.push((cost(card), p));
                    }
                    if rules.trump_reflecting
                        && card & trump_cards != 0
                        && self.shown & card == 0
                        && attacker_cards >= piles
                    {
                        let mut p = *self;
                        p.shown |= card;
                        p.attacker = d as u8;
                        p.batch = 0;
                        out.push((cost(card), p));
                    }
                }
                let mut p = *self;
                p.phase = THROWING;
                out.push((300, p));
            }
            _ => {
                if self.can_add_pile(rules) {
                    let mut playable = self.hand[a] & same_ranks(self.atk | self.def);
                    while playable != 0 {
                        let card = playable & playable.wrapping_neg();
                        playable ^= card;
                        let mut p = *self;
                        p.hand[a] ^= card;
                        p.atk |= card;
                        p.open |= card;
                        out.push((100 + cost(card), p));
                    }
                }
                // Done: the defender picks up the table and misses their attack.
                let mut p = *self;
                p.hand[d] |= self.atk | self.def;
                p.next_trick(a);
                out.push((200, p));
            }
        }
        out.sort_by_key(|(c, _)| *c);
    }
}

struct Frame {
    key: u128,
    /// This position's children still to be tried are `arena[next..end]`.
    first_child: usize,
    next: usize,
    end: usize,
    prover_to_move: bool,
    low: u32,
    first_provisional: usize,
}

/// What the depth-limited search knows about a position, for one player:
/// that player wins within `win_within` moves, and does not within `no_win_within`.
#[derive(Clone, Copy)]
struct Bounds {
    win_within: u16,
    no_win_within: u16,
}

const UNKNOWN: Bounds = Bounds { win_within: u16::MAX, no_win_within: 0 };

/// Deepest forced win the depth-limited search looks for.
const MAX_WIN_DEPTH: u16 = 400;

struct FastSearch {
    proofs: [Proof; 2],
    /// Depth-limited results per player (see [`FastSearch::wins_within`]).
    bounds: [Table<Bounds>; 2],
    nodes: u64,
    /// Searches stop when `nodes` passes this.
    max_nodes: u64,
    arena: Vec<Pos>,
    scratch: Vec<(u32, Pos)>,
}

impl FastSearch {
    fn new() -> Self {
        FastSearch {
            proofs: [Proof::default(), Proof::default()],
            bounds: [Table::default(), Table::default()],
            nodes: 0,
            max_nodes: 0,
            arena: Vec::new(),
            scratch: Vec::new(),
        }
    }

    /// Can player `who` force the other to become the durak within `depth`
    /// moves (by either side)? `None` if the node limit was reached.
    ///
    /// Forced wins are usually short, while aimless play can run for thousands
    /// of moves, so looking for a win a few moves deep and then a little
    /// deeper finds most of them quickly. The answer depends only on the
    /// position and the depth, never on how the position was reached, so
    /// repeated positions need no special care here - going round in a circle
    /// just uses up depth.
    fn wins_within(&mut self, who: usize, pos: &Pos, rules: &Rules, depth: u16) -> Option<bool> {
        if let Some(v) = pos.terminal(rules) {
            return Some(if who == 0 { v == 2 } else { v == 0 });
        }
        if depth == 0 {
            return Some(false);
        }
        let k = pos.fingerprint();
        let known = self.bounds[who].get(&k).copied().unwrap_or(UNKNOWN);
        if known.win_within <= depth {
            return Some(true);
        }
        if known.no_win_within >= depth {
            return Some(false);
        }
        if self.nodes >= self.max_nodes {
            return None;
        }
        self.nodes += 1;

        self.scratch.clear();
        pos.children(rules, &mut self.scratch);
        // Moves already known to win come first.
        for (cost, child) in self.scratch.iter_mut() {
            if let Some(b) = self.bounds[who].get(&child.fingerprint()) {
                if b.win_within < depth {
                    *cost = 0;
                }
            }
        }
        self.scratch.sort_by_key(|(c, _)| *c);
        let first = self.arena.len();
        self.arena.extend(self.scratch.iter().map(|(_, p)| *p));
        let end = self.arena.len();

        let prover_to_move = pos.actor() == who;
        let mut result = !prover_to_move;
        for i in first..end {
            let child = self.arena[i];
            let Some(wins) = self.wins_within(who, &child, rules, depth - 1) else {
                self.arena.truncate(first);
                return None;
            };
            if wins == prover_to_move {
                result = prover_to_move;
                break;
            }
        }
        self.arena.truncate(first);

        let entry = self.bounds[who].entry(k).or_insert(UNKNOWN);
        if result {
            entry.win_within = entry.win_within.min(depth);
        } else {
            entry.no_win_within = entry.no_win_within.max(depth);
        }
        Some(result)
    }

    /// Carry on `run`: can player `who` force the other to become the durak
    /// from its starting position? `None` if the node limit was reached first -
    /// the run is left as it stands and the next call picks it up. Uses its own
    /// stack, not the call stack: lines of play can be many thousands of moves long.
    fn forces(&mut self, who: usize, run: &mut Run, rules: &Rules) -> Option<bool> {
        let wins = |v: u8| if who == 0 { v == 2 } else { v == 0 };
        let proof = &mut self.proofs[who];
        loop {
            if let Some(pos) = run.pending.take() {
                // Look at a position: answer it at once if possible, else open it.
                if let Some(v) = pos.terminal(rules) {
                    run.answer = Some((wins(v), SETTLED));
                } else {
                    let k = pos.fingerprint();
                    if let Some(known) = proof.lookup(k) {
                        run.answer = Some(known);
                    } else {
                        if self.nodes >= self.max_nodes {
                            run.pending = Some(pos);
                            return None;
                        }
                        self.nodes += 1;
                        self.scratch.clear();
                        pos.children(rules, &mut self.scratch);
                        let first_child = run.arena.len();
                        run.arena.extend(self.scratch.iter().map(|(_, p)| *p));
                        proof.on_path.insert(k, run.stack.len() as u32);
                        run.stack.push(Frame {
                            key: k,
                            first_child,
                            next: first_child,
                            end: run.arena.len(),
                            prover_to_move: pos.actor() == who,
                            low: SETTLED,
                            first_provisional: proof.provisional.len(),
                        });
                    }
                }
            }

            let Some(frame) = run.stack.last_mut() else {
                return run.answer.map(|(forced, _)| forced);
            };
            // The prover needs one move that works; the other side one that refutes.
            let mut verdict = None;
            if let Some((forced, child_low)) = run.answer.take() {
                if forced == frame.prover_to_move {
                    verdict = Some((forced, if forced { SETTLED } else { child_low }));
                } else if !forced {
                    frame.low = frame.low.min(child_low);
                }
            }
            if verdict.is_none() {
                if frame.next < frame.end {
                    run.pending = Some(run.arena[frame.next]);
                    frame.next += 1;
                    continue;
                }
                // Nothing decisive: no winning move for the prover, or no refutation.
                verdict = Some((!frame.prover_to_move, frame.low));
            }
            let (result, low) = verdict.unwrap_or((false, SETTLED));
            let Some(frame) = run.stack.pop() else { return None };
            run.arena.truncate(frame.first_child);
            run.answer = Some(proof.close(frame.key, run.stack.len() as u32, frame.first_provisional, result, low));
        }
    }
}

/// One player's search for a forced win from one position, which can be
/// stopped at the node limit and carried on later.
struct Run {
    stack: Vec<Frame>,
    /// The children of every position on the stack, back to back.
    arena: Vec<Pos>,
    /// The answer for the position just examined, on its way to the one above.
    answer: Option<(bool, u32)>,
    /// The position to look at next.
    pending: Option<Pos>,
}

impl Run {
    fn new(root: Pos) -> Run {
        Run { stack: Vec::new(), arena: Vec::new(), answer: None, pending: Some(root) }
    }
}

/// Positions each side's search gets before the other side's has a turn.
const TURN: u64 = 20_000;

impl PositionSearch for FastSearch {
    fn value(&mut self, s: &GameState, budget: u64) -> Result<Option<u8>, EngineError> {
        let rules = Rules {
            trump: s.trump as u32,
            reflecting: s.config.reflecting,
            trump_reflecting: s.config.trump_reflecting,
            first_trick_limit: s.config.first_trick_limit,
            classic_ending: s.config.classic_ending,
            cards_per_attack: s.config.cards_per_attack,
        };
        let pos = Pos::from_state(s);
        let end = self.nodes + budget;

        // Two ways of searching take turns, because neither is better
        // everywhere. The complete search follows one line of play to its end
        // before trying the next: quick when the obvious moves win, and the
        // only one that can settle draws and positions where play repeats. The
        // deepening search looks for a forced win a few moves deep, then
        // deeper: quick when the win is short but hidden among long, aimless
        // lines. Each asks "can P0 force a win?" and "can P1?" separately, and
        // those take turns too, so the easy answer never waits for the hard one.
        for proof in &mut self.proofs {
            // Nothing may be left over from a search that was abandoned.
            proof.on_path.clear();
            for k in proof.provisional.drain(..) {
                proof.provisional_low.remove(&k);
            }
        }
        let mut runs = [Run::new(pos), Run::new(pos)];
        let mut refuted = [false; 2];
        let mut depth = 4u16;
        loop {
            for (who, value) in [(1usize, 0u8), (0, 2)] {
                if refuted[who] {
                    continue;
                }
                self.max_nodes = (self.nodes + TURN).min(end);
                match self.forces(who, &mut runs[who], &rules) {
                    Some(true) => return Ok(Some(value)),
                    Some(false) => refuted[who] = true,
                    None => {}
                }
            }
            if refuted == [true, true] {
                return Ok(Some(1));
            }

            // The deepening search starts over each turn, but what it has
            // settled is remembered, so it loses very little.
            self.max_nodes = (self.nodes + 2 * TURN).min(end);
            'deepening: while depth <= MAX_WIN_DEPTH {
                for (who, value) in [(1usize, 0u8), (0, 2)] {
                    if refuted[who] {
                        continue;
                    }
                    match self.wins_within(who, &pos, &rules, depth) {
                        Some(true) => return Ok(Some(value)),
                        Some(false) => {}
                        None => break 'deepening,
                    }
                }
                // Each pass repeats the shallower ones, so grow the depth in bigger steps.
                depth += (depth / 4).max(4);
            }

            if self.nodes >= end {
                return Ok(None);
            }
        }
    }

    fn nodes(&self) -> u64 {
        self.nodes
    }
}

// ── Reference search on engine states ───────────────────────────────────────

/// Lines of play longer than this are not followed by the reference search
/// (it recurses, and would run out of stack).
const REFERENCE_MAX_DEPTH: u32 = 100_000;

struct ReferenceSearch {
    proofs: [Proof; 2],
    nodes: u64,
    max_nodes: u64,
    gave_up: bool,
}

impl ReferenceSearch {
    fn new() -> Self {
        ReferenceSearch { proofs: [Proof::default(), Proof::default()], nodes: 0, max_nodes: 0, gave_up: false }
    }

    fn forces(&mut self, who: usize, s: &GameState, depth: u32) -> Result<(bool, u32), EngineError> {
        if let Some(v) = terminal_value(s) {
            return Ok((if who == 0 { v == 2 } else { v == 0 }, SETTLED));
        }
        let k = Pos::from_state(s).fingerprint();
        if let Some(known) = self.proofs[who].lookup(k) {
            return Ok(known);
        }
        self.nodes += 1;
        if self.nodes > self.max_nodes || depth >= REFERENCE_MAX_DEPTH {
            self.gave_up = true;
            return Ok((false, SETTLED));
        }
        let actions = s.legal_actions();
        if actions.is_empty() {
            return Err(EngineError::InvalidState("a running game has no legal actions".into()));
        }

        self.proofs[who].on_path.insert(k, depth);
        let first_provisional = self.proofs[who].provisional.len();
        let prover_to_move = s.actor_to_move().index() == who;
        let mut result = !prover_to_move;
        let mut low = SETTLED;
        for action in &actions {
            let mut child = s.clone();
            child.execute(action)?;
            let (forced, child_low) = self.forces(who, &child, depth + 1)?;
            if self.gave_up {
                return Ok((false, SETTLED));
            }
            if forced == prover_to_move {
                result = prover_to_move;
                low = if forced { SETTLED } else { child_low };
                break;
            }
            if !forced {
                low = low.min(child_low);
            }
        }
        Ok(self.proofs[who].close(k, depth, first_provisional, result, low))
    }
}

impl PositionSearch for ReferenceSearch {
    fn value(&mut self, s: &GameState, budget: u64) -> Result<Option<u8>, EngineError> {
        self.max_nodes = self.nodes + budget;
        for (who, value) in [(0usize, 2u8), (1, 0)] {
            let (forced, _) = self.forces(who, s, 0)?;
            if self.gave_up {
                // Leave nothing half-finished behind for the next move.
                self.gave_up = false;
                for proof in &mut self.proofs {
                    proof.on_path.clear();
                    for k in proof.provisional.drain(..) {
                        proof.provisional_low.remove(&k);
                    }
                }
                return Ok(None);
            }
            if forced {
                return Ok(Some(value));
            }
        }
        Ok(Some(1))
    }

    fn nodes(&self) -> u64 {
        self.nodes
    }
}
