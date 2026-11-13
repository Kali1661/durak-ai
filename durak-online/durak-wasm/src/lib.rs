//! WebAssembly bindings for `durak-core`.
//!
//! Every function validates what it is given and reports problems as a thrown
//! string; nothing here substitutes defaults for malformed input or panics on
//! bad data.

use durak_core::{
    pick_random_action as core_pick_random_action, Action, Card, EngineError, GameConfig,
    GameState, Outcome, PlayerId, Rank, SolveParams,
};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

const DEFAULT_ROLLOUTS: u32 = 1000;
const DEFAULT_MAX_DEPTH: u32 = 500;
const EXPLORATION: f64 = 1.41;

fn err(e: EngineError) -> JsValue {
    JsValue::from_str(&e.to_string())
}

fn parse<T: for<'de> Deserialize<'de>>(value: JsValue, what: &str) -> Result<T, JsValue> {
    serde_wasm_bindgen::from_value(value)
        .map_err(|e| JsValue::from_str(&format!("could not read {what}: {e}")))
}

fn out<T: Serialize>(value: &T) -> Result<JsValue, JsValue> {
    serde_wasm_bindgen::to_value(value).map_err(|e| JsValue::from_str(&e.to_string()))
}

/// Parse and validate a game state.
fn state_in(value: JsValue) -> Result<GameState, JsValue> {
    let state: GameState = parse(value, "the game state")?;
    state.validate().map_err(err)?;
    Ok(state)
}

/// A missing config means "defaults"; a malformed one is an error.
fn config_in(value: JsValue) -> Result<GameConfig, JsValue> {
    let config = if value.is_undefined() || value.is_null() {
        GameConfig::default()
    } else {
        parse(value, "the game settings")?
    };
    config.validate().map_err(err)?;
    Ok(config)
}

fn random_seed() -> u64 {
    let hi = (js_sys::Math::random() * 4_294_967_296.0) as u64;
    let lo = (js_sys::Math::random() * 4_294_967_296.0) as u64;
    (hi << 32) | lo
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewComputerGameRequest {
    /// Seed for the shuffle.
    pub seed: u64,
}

/// Deal a new game.
#[wasm_bindgen]
pub fn new_computer_game(req: JsValue, config: JsValue) -> Result<JsValue, JsValue> {
    let req: NewComputerGameRequest = parse(req, "the new-game request")?;
    let config = config_in(config)?;
    out(&GameState::new_computer_game(req.seed, config).map_err(err)?)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManualStartRequest {
    pub trump_card: Card,
    pub player_hand: Vec<Card>,
    pub starting_player: u8,
    /// Trumps opponents have shown, if any.
    #[serde(default)]
    pub opponent_trumps: Vec<(PlayerId, Option<Rank>)>,
}

/// Start tracking a real game from the observer's starting hand.
#[wasm_bindgen]
pub fn new_manual_game(req: JsValue, config: JsValue) -> Result<JsValue, JsValue> {
    let req: ManualStartRequest = parse(req, "the game setup")?;
    let config = config_in(config)?;
    let state = GameState::new_manual_game(
        req.trump_card,
        req.player_hand,
        req.starting_player,
        req.opponent_trumps,
        config,
    )
    .map_err(err)?;
    out(&state)
}

/// Throws if the state is malformed or inconsistent; returns nothing otherwise.
#[wasm_bindgen]
pub fn validate_state(state: JsValue) -> Result<(), JsValue> {
    state_in(state).map(|_| ())
}

#[wasm_bindgen]
pub fn legal_actions(state: JsValue) -> Result<JsValue, JsValue> {
    out(&state_in(state)?.legal_actions())
}

/// Apply an action; throws (leaving the caller's state as it was) if it is not legal.
#[wasm_bindgen]
pub fn apply_action(state: JsValue, action: JsValue) -> Result<JsValue, JsValue> {
    let mut state = state_in(state)?;
    let action: Action = parse(action, "the action")?;
    state.apply(&action).map_err(err)?;
    out(&state)
}

#[wasm_bindgen]
pub fn actor_to_move(state: JsValue) -> Result<JsValue, JsValue> {
    out(&state_in(state)?.actor_to_move())
}

/// `{status: "in_progress"}`, `{status: "durak", player}` or `{status: "draw"}`.
#[wasm_bindgen]
pub fn game_outcome(state: JsValue) -> Result<JsValue, JsValue> {
    let outcome: Outcome = state_in(state)?.outcome();
    out(&outcome)
}

/// The loser, or null while the game runs *and* after a draw (see `game_outcome`).
#[wasm_bindgen]
pub fn get_durak(state: JsValue) -> Result<JsValue, JsValue> {
    out(&state_in(state)?.durak())
}

/// Can the observer (P0) work out every remaining card?
#[wasm_bindgen]
pub fn all_cards_deducible(state: JsValue) -> Result<bool, JsValue> {
    Ok(state_in(state)?.all_cards_deducible(PlayerId::P0))
}

/// Replace unknown cards with the cards they must be (observer = P0).
#[wasm_bindgen]
pub fn deduce_cards(state: JsValue) -> Result<JsValue, JsValue> {
    out(&state_in(state)?.deduce_cards(PlayerId::P0).map_err(err)?)
}

/// Record what an unknown card in `player`'s hand turned out to be.
#[wasm_bindgen]
pub fn reveal_card(state: JsValue, player: JsValue, card: JsValue) -> Result<JsValue, JsValue> {
    let mut state = state_in(state)?;
    let player: PlayerId = parse(player, "the player")?;
    let card: Card = parse(card, "the card")?;
    state.reveal_card(player, card).map_err(err)?;
    out(&state)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolveRequest {
    pub state: GameState,
    /// Random deals of the hidden cards to search (default 1).
    #[serde(default)]
    pub determinizations: Option<u32>,
    /// Playouts per deal (default 1000).
    #[serde(default)]
    pub rollouts_per_determinization: Option<u32>,
    /// Playout length limit in moves (default 500).
    #[serde(default)]
    pub max_depth: Option<u32>,
    /// Fixed seed for reproducible results; random when omitted.
    #[serde(default)]
    pub seed: Option<u64>,
}

/// Evaluate every legal action for the player to move.
///
/// Returns `{perspective, determinizations, total_visits, total_cutoffs, actions}`
/// where each action is `{action, visits, score, cutoffs}`. `score` is the
/// mean playout value in [0, 1] (1 = not the durak, 0 = durak, 0.5 = playout
/// cut off) - a ranking estimate, not a calibrated probability.
#[wasm_bindgen]
pub fn solve(req: JsValue) -> Result<JsValue, JsValue> {
    let req: SolveRequest = parse(req, "the solve request")?;
    let params = SolveParams {
        determinizations: req.determinizations.unwrap_or(1),
        rollouts: req.rollouts_per_determinization.unwrap_or(DEFAULT_ROLLOUTS),
        max_depth: req.max_depth.unwrap_or(DEFAULT_MAX_DEPTH),
        exploration: EXPLORATION,
        // Playout style, exact solving of small deals, priors and the
        // stock-empty cutoff: the engine's tuned defaults.
        ..SolveParams::default()
    };
    let seed = req.seed.unwrap_or_else(random_seed);
    out(&durak_core::solve(&req.state, params, seed).map_err(err)?)
}

/// Default limit on positions searched by [`solve_exact`].
const DEFAULT_EXACT_NODES: u32 = 2_000_000;
/// Hard limit: each position remembered costs memory.
const MAX_EXACT_NODES: u32 = 6_000_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExactRequest {
    pub state: GameState,
    /// Give up after searching this many positions (default 2 million, at most 6 million).
    #[serde(default)]
    pub max_nodes: Option<u32>,
}

/// Calculate - not estimate - the result of every legal move, when the game
/// has no hidden cards left (two players, stock empty or down to the trump card).
///
/// Returns `{perspective, value, actions, nodes}` where each action is
/// `{action, value}` and a value is `"win"`, `"draw"` or `"loss"` for the
/// player to move with best play by both sides. Returns null when the position
/// cannot be solved exactly or is too large for the node limit.
#[wasm_bindgen]
pub fn solve_exact(req: JsValue) -> Result<JsValue, JsValue> {
    let req: ExactRequest = parse(req, "the exact-solve request")?;
    let max_nodes = req.max_nodes.unwrap_or(DEFAULT_EXACT_NODES).min(MAX_EXACT_NODES);
    out(&durak_core::solve_exact(&req.state, u64::from(max_nodes)).map_err(err)?)
}

/// Whether [`solve_exact`] applies to this position at all.
#[wasm_bindgen]
pub fn is_exactly_solvable(state: JsValue) -> Result<bool, JsValue> {
    Ok(durak_core::is_exactly_solvable(&state_in(state)?))
}

/// A uniformly random legal action, or null when the game is over.
#[wasm_bindgen]
pub fn pick_random_action(state: JsValue) -> Result<JsValue, JsValue> {
    out(&core_pick_random_action(&state_in(state)?, random_seed()))
}
