//! Guessing the opponent's hand from how they play (two-player games).
//!
//! Without this, every card I cannot see is equally likely to be in the
//! opponent's hand or in the stock. But their moves say something: a player
//! who picks up a nine of hearts rather than beat it probably has no higher
//! heart; one who leads a jack probably has nothing lower; one who stops
//! adding cards probably has none of the ranks on the table.
//!
//! [`Beliefs`] keeps, for every card, a weight: how much more or less likely
//! than usual it is to be in the opponent's hand rather than in the stock
//! (1 = no idea, 0.2 = unlikely). [`GameState::determinize_with`] then deals
//! the hidden cards accordingly. The weights are evidence, not certainty -
//! players hold cards back and bluff - so they never go below [`FLOOR`], and
//! they fade as the opponent draws new cards.
//!
//! The strength of each kind of evidence ([`Strength`]) was measured on
//! games between the search engine and itself (the `beliefs` example) and on
//! recorded games against real opponents (the `realgames` example).

use serde::{Deserialize, Serialize};

use crate::{Action, Card, CardId, GameState, PlayerId, Rank, Suit};

/// No card is ever ruled out entirely.
pub const FLOOR: f32 = 0.05;

const CARDS: usize = 52;

fn slot((suit, rank): CardId) -> usize {
    suit as usize * 13 + (rank as usize - 2)
}

/// How strongly each kind of evidence counts: the factor a card's weight is
/// multiplied by (1 = ignore the evidence).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Strength {
    /// They took rather than beat a card: higher cards of its suit.
    pub take_same_suit: f32,
    /// ...and, when the card was not a trump, their trumps.
    pub take_trump: f32,
    /// They led a card: non-trump cards below it.
    pub lead_lower: f32,
    /// They led a trump: every non-trump card.
    pub lead_trump_plain: f32,
    /// They stopped adding cards: low non-trump cards of the ranks on the table.
    pub stop_low: f32,
    /// ...and high ones (jack and up).
    pub stop_high: f32,
    /// Every take counts again, so a card can be pushed down take after
    /// take (as before version 0.3.2). Normally taking only ever lowers a
    /// card to the take strength, however often it happens: an opponent who
    /// picks cards up on purpose, to keep their high cards, does it again
    /// and again, and each time says nothing new.
    #[serde(default)]
    pub take_stacks: bool,
}

impl Default for Strength {
    fn default() -> Self {
        Strength {
            // Measured weights (the `beliefs` example, 36 cards, passing):
            // against a player following simple rules / against the search.
            // The third number: in 193 recorded games against real
            // opponents (the `realgames` example, 24 cards).
            take_same_suit: 0.65,  // 0.25 / 0.87 / 0.63
            take_trump: 0.85,      // 0.72 / 1.05 / 0.82
            lead_lower: 0.5,       // 0.00 / 0.79 / 0.22
            lead_trump_plain: 0.5, // 0.00 / 0.86 / -
            stop_low: 0.2,         // 0.00 / 0.18 / 0.00
            stop_high: 0.6,        // 1.17 / 0.37 / 0.35
            take_stacks: false,
        }
    }
}

impl Strength {
    /// Evidence switched off: every weight stays 1.
    pub fn none() -> Self {
        Strength { take_same_suit: 1.0, take_trump: 1.0, lead_lower: 1.0, lead_trump_plain: 1.0, stop_low: 1.0, stop_high: 1.0, take_stacks: false }
    }
}

/// What kind of evidence touched a card (for measuring, see the `beliefs` example).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Evidence {
    TakeSameSuit,
    TakeTrump,
    LeadLower,
    LeadTrumpPlain,
    StopLow,
    StopHigh,
}

/// For every card: how likely it is to be in the opponent's hand rather than
/// the stock, relative to knowing nothing (1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Beliefs {
    weights: Vec<f32>,
    #[serde(default)]
    strength: Strength,
}

impl Default for Beliefs {
    fn default() -> Self {
        Self::new(Strength::default())
    }
}

impl Beliefs {
    pub fn new(strength: Strength) -> Self {
        Beliefs { weights: vec![1.0; CARDS], strength }
    }

    /// The weight of one card; 1 for anything out of range.
    pub fn weight(&self, id: CardId) -> f32 {
        self.weights.get(slot(id)).copied().filter(|w| w.is_finite() && *w > 0.0).unwrap_or(1.0)
    }

    /// True while nothing has been learned (dealing is then plain random).
    pub fn is_blank(&self) -> bool {
        self.weights.iter().all(|w| (*w - 1.0).abs() < 1e-6)
    }

    fn scale(&mut self, id: CardId, factor: f32) {
        if let Some(w) = self.weights.get_mut(slot(id)) {
            *w = (*w * factor).max(FLOOR);
        }
    }

    fn lower_to(&mut self, id: CardId, level: f32) {
        if let Some(w) = self.weights.get_mut(slot(id)) {
            *w = w.min(level.max(FLOOR));
        }
    }

    /// Learn from one move of a two-player game, seen by `me`: `before` is
    /// the position it was made in, `after` the position it led to. My own
    /// moves teach nothing about the opponent, but cards drawn afterwards
    /// still fade what was learned.
    pub fn observe(&mut self, me: PlayerId, before: &GameState, action: &Action, after: &GameState) {
        self.observe_with(me, before, action, after, &mut |_, _| {});
    }

    /// [`Beliefs::observe`], reporting every card a piece of evidence touched.
    pub fn observe_with(
        &mut self,
        me: PlayerId,
        before: &GameState,
        action: &Action,
        after: &GameState,
        note: &mut dyn FnMut(Evidence, CardId),
    ) {
        if before.num_players() != 2 || me.index() >= 2 {
            return;
        }
        let actor = before.actor_to_move();
        let opponent = if me == PlayerId::P0 { PlayerId::P1 } else { PlayerId::P0 };
        let trump = before.trump;
        let s = self.strength;

        if actor == opponent {
            let on_table: Vec<CardId> = before
                .table
                .iter()
                .flat_map(|p| [Some(p.attack), p.defense].into_iter().flatten())
                .filter_map(|c| c.id())
                .collect();
            let mut touch = |beliefs: &mut Self, kind: Evidence, id: CardId, factor: f32| {
                if factor < 1.0 {
                    let taking = matches!(kind, Evidence::TakeSameSuit | Evidence::TakeTrump);
                    if taking && !s.take_stacks {
                        beliefs.lower_to(id, factor);
                    } else {
                        beliefs.scale(id, factor);
                    }
                    note(kind, id);
                }
            };
            match action {
                Action::Take => {
                    let open: Vec<CardId> =
                        before.table.iter().filter(|p| p.defense.is_none()).filter_map(|p| p.attack.id()).collect();
                    // With several cards to beat, one of them alone may have forced the take.
                    let soften = |f: f32| if open.len() > 1 { f.sqrt() } else { f };
                    for (suit, rank) in &open {
                        for higher in Rank::ALL.into_iter().filter(|r| r > rank) {
                            touch(self, Evidence::TakeSameSuit, (*suit, higher), soften(s.take_same_suit));
                        }
                    }
                    if !open.is_empty() && open.iter().all(|(suit, _)| *suit != trump) {
                        for rank in Rank::ALL {
                            touch(self, Evidence::TakeTrump, (trump, rank), soften(s.take_trump));
                        }
                    }
                }
                Action::Attack { card } if before.table.is_empty() => {
                    if let Some((suit, rank)) = card.id() {
                        for other in Suit::ALL.into_iter().filter(|x| *x != trump) {
                            for r in Rank::ALL {
                                if suit == trump {
                                    touch(self, Evidence::LeadTrumpPlain, (other, r), s.lead_trump_plain);
                                } else if r < rank {
                                    touch(self, Evidence::LeadLower, (other, r), s.lead_lower);
                                }
                            }
                        }
                    }
                }
                Action::PassAttack | Action::Throw { card: None } if !on_table.is_empty() => {
                    // Only when they could have added something: there was
                    // room on the table and I had cards left to face it.
                    let open = before.table.iter().filter(|p| p.defense.is_none()).count();
                    let room = before.table.len() < 6 && before.hands[me.index()].len() > open;
                    if room {
                        let mut ranks: Vec<Rank> = on_table.iter().map(|(_, r)| *r).collect();
                        ranks.sort();
                        ranks.dedup();
                        for rank in ranks {
                            for suit in Suit::ALL.into_iter().filter(|x| *x != trump) {
                                if rank < Rank::Jack {
                                    touch(self, Evidence::StopLow, (suit, rank), s.stop_low);
                                } else {
                                    touch(self, Evidence::StopHigh, (suit, rank), s.stop_high);
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        // Cards the opponent drew since: anything "not in their hand" may now
        // be. Their hand grew by what they drew and took, less what they played.
        let stock = before.stock.len();
        if stock > 0 && after.stock.len() < stock {
            let o = opponent.index();
            let played = usize::from(actor == opponent && plays_from_hand(action));
            let ended = after.table.is_empty();
            let taken = if ended && after.discard.len() == before.discard.len() && before.defender == opponent {
                before.table.iter().map(|p| 1 + usize::from(p.defense.is_some())).sum::<usize>()
                    + usize::from(actor != opponent && plays_from_hand(action))
            } else {
                0
            };
            let drawn = (after.hands[o].len() + played).saturating_sub(before.hands[o].len() + taken);
            let share = (drawn as f32 / stock as f32).min(1.0);
            for w in &mut self.weights {
                *w += (1.0 - *w) * share;
            }
        }
    }

    /// The beliefs after a whole game so far: `steps` are the positions
    /// reached and the moves that led to them, starting from `initial`.
    pub fn from_history<'a>(
        me: PlayerId,
        initial: &'a GameState,
        steps: impl IntoIterator<Item = (&'a Action, &'a GameState)>,
    ) -> Self {
        let mut beliefs = Beliefs::default();
        let mut before = initial;
        for (action, after) in steps {
            beliefs.observe(me, before, action, after);
            before = after;
        }
        beliefs
    }
}

/// Does this move take a card out of the mover's hand?
fn plays_from_hand(action: &Action) -> bool {
    match action {
        Action::Attack { .. } | Action::Defend { .. } | Action::Reflect { .. } => true,
        Action::Throw { card } => card.is_some(),
        _ => false,
    }
}

/// Pick which of `pool` go to the `hand` slots: each card in proportion to
/// its weight, without replacement. Leaves the chosen cards at the front.
pub(crate) fn deal_weighted(pool: &mut [Card], hand: usize, beliefs: &Beliefs, rng: &mut impl rand::Rng) {
    let mut weights: Vec<f32> = pool.iter().map(|c| c.id().map_or(1.0, |id| beliefs.weight(id))).collect();
    for k in 0..hand.min(pool.len()) {
        let total: f32 = weights[k..].iter().sum();
        let mut at = rng.gen_range(0.0..1.0f32) * total;
        let mut pick = pool.len() - 1;
        for (j, w) in weights.iter().enumerate().skip(k) {
            if at < *w {
                pick = j;
                break;
            }
            at -= *w;
        }
        pool.swap(k, pick);
        weights.swap(k, pick);
    }
}
