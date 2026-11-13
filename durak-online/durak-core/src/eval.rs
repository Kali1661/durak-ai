//! A quick score for a two-player position once the stock is empty, used to
//! end playouts there instead of playing them out (see
//! [`crate::SolveParams::cutoff`]).
//!
//! The score is a logistic formula over a few features of the two hands, seen
//! from the attacker at the start of a trick. Its weights were fitted to the
//! exact solver's verdicts on positions from games of the heuristic player
//! (`examples/evalfit.rs`), so it estimates "the attacker wins with good play".

use crate::{Card, GameState};

pub const FEATURES: usize = 6;

/// Weights for [`features`], fitted by `examples/evalfit.rs` on 14,855
/// positions from 4,000 games (36 cards, reflecting, classic ending). On
/// positions left out of the fit it names the winner 85.6% of the time.
pub const WEIGHTS: [f64; FEATURES] = [2.304, 0.930, 0.474, 0.717, 0.487, -1.767];

/// Can `a` beat `b`?
fn beats(a: &Card, b: &Card, trump: crate::Suit) -> bool {
    a.beats(b, trump)
}

/// Features of a position from the attacker's side:
/// 1, the opponent's extra cards, extra trumps, extra trump strength,
/// extra cards the other cannot beat, and the cards in play.
pub fn features(s: &GameState) -> [f64; FEATURES] {
    let a = &s.hands[s.attacker.index()];
    let d = &s.hands[s.defender.index()];
    let trump = s.trump;
    let trumps = |h: &[Card]| h.iter().filter(|c| c.suit() == trump).count() as f64;
    let strength = |h: &[Card]| {
        h.iter().filter(|c| c.suit() == trump).map(|c| (c.rank() as i32 - 5) as f64 / 9.0).sum::<f64>()
    };
    let safe = |h: &[Card], other: &[Card]| h.iter().filter(|c| !other.iter().any(|o| beats(o, c, trump))).count() as f64;
    [
        1.0,
        d.len() as f64 - a.len() as f64,
        trumps(a) - trumps(d),
        strength(a) - strength(d),
        safe(a, d) - safe(d, a),
        (a.len() + d.len()) as f64 / 10.0,
    ]
}

/// Chance the attacker wins, by [`WEIGHTS`].
pub fn attacker_wins(s: &GameState) -> f64 {
    let z: f64 = features(s).iter().zip(WEIGHTS).map(|(x, w)| x * w).sum();
    1.0 / (1.0 + (-z).exp())
}
