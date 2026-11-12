//! Determinized UCT search.
//!
//! # What the score means
//!
//! Every playout ends in one of three ways, valued per player:
//!
//! | playout result                         | value |
//! |----------------------------------------|-------|
//! | game finished, player is not the durak | 1     |
//! | game finished, player is the durak     | 0     |
//! | cut off at the depth limit (unknown)   | 0.5   |
//!
//! An action's `score` is the mean value, for the player being advised, of the
//! playouts that went through it. It estimates "how often do I avoid being the
//! durak after this move" under the search's own model - hidden cards dealt at
//! random, random play beyond the tree. It is an estimate for *ranking* moves,
//! not a calibrated probability.
//!
//! Each tree node keeps the value for every player and the player to move
//! picks the child that is best for themselves (max^n), so opponents are
//! modelled as playing for their own result.

use rand::{rngs::StdRng, SeedableRng};
use serde::{Deserialize, Serialize};

use crate::policy::{self, Playout};
use crate::{Action, EngineError, GameState, Outcome, PlayerId, MAX_PLAYERS};

/// Upper bounds accepted by [`solve`]; larger requests are rejected.
pub const MAX_DETERMINIZATIONS: u32 = 2_000;
pub const MAX_ROLLOUTS: u32 = 200_000;
pub const MAX_SEARCH_DEPTH: u32 = 2_000;

/// Search statistics for one root action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionEval {
    pub action: Action,
    /// Playouts that went through this action. Zero means "not evaluated".
    pub visits: u32,
    /// Mean playout value in [0, 1] for the advised player; 0 when `visits` is 0.
    pub score: f32,
    /// How many of those playouts hit the depth limit before the game ended.
    pub cutoffs: u32,
}

/// Result of one tree search on a fully-known state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MctsEval {
    /// One entry per legal root action, in `legal_actions()` order.
    pub actions: Vec<ActionEval>,
    pub total_rollouts: u32,
}

struct Node {
    /// Sum of playout values per player, in half-points (2 = win, 1 = cut off).
    value2: [u32; MAX_PLAYERS],
    visits: u32,
    cutoffs: u32,
    children: Vec<(Action, usize)>,
    unexplored: Vec<Action>,
}

impl Node {
    fn new(unexplored: Vec<Action>) -> Self {
        Self { value2: [0; MAX_PLAYERS], visits: 0, cutoffs: 0, children: Vec::new(), unexplored }
    }
}

fn internal<T>(msg: impl Into<String>) -> Result<T, EngineError> {
    Err(EngineError::InvalidState(msg.into()))
}

/// UCT tree search from `state`, which must have no unknown cards
/// (see [`GameState::determinize`]). Deterministic for a given seed.
pub fn mcts_evaluate_actions(
    state: &GameState,
    seed: u64,
    perspective: PlayerId,
    rollouts: u32,
    max_depth: u32,
    exploration: f64,
    playout: Playout,
) -> Result<MctsEval, EngineError> {
    if !state.is_fully_known() {
        return internal("search needs a state without unknown cards; determinize it first");
    }
    if perspective.index() >= state.num_players() || state.num_players() > MAX_PLAYERS {
        return Err(EngineError::InvalidInput(format!(
            "{perspective:?} is not a player in this game"
        )));
    }

    let mut rng = StdRng::seed_from_u64(seed);
    let root_actions = state.legal_actions();
    // `unexplored` is popped from the back; reverse so expansion follows move order.
    let mut nodes = vec![Node::new(root_actions.iter().rev().cloned().collect())];

    for _ in 0..rollouts {
        let mut s = state.clone();
        let mut path = vec![0usize];
        let mut cur = 0usize;

        // Selection, then expansion of one new node.
        while !s.is_terminal() {
            if let Some(action) = nodes[cur].unexplored.pop() {
                s.execute(&action)?;
                let child = nodes.len();
                nodes.push(Node::new(s.legal_actions().into_iter().rev().collect()));
                nodes[cur].children.push((action, child));
                path.push(child);
                break;
            }
            if nodes[cur].children.is_empty() {
                return internal("a running game has no legal actions");
            }
            let actor = s.actor_to_move().index();
            let ln_n = f64::from(nodes[cur].visits.max(1)).ln();
            let mut best = 0usize;
            let mut best_uct = f64::NEG_INFINITY;
            for (i, (_, child)) in nodes[cur].children.iter().enumerate() {
                let c = &nodes[*child];
                let n = f64::from(c.visits.max(1));
                let uct = f64::from(c.value2[actor]) / (2.0 * n) + exploration * (ln_n / n).sqrt();
                if uct > best_uct {
                    best_uct = uct;
                    best = i;
                }
            }
            let (action, child) = nodes[cur].children[best].clone();
            s.execute(&action)?;
            cur = child;
            path.push(child);
        }

        // Playout: random or heuristic players (see `policy`).
        let mut depth = 0u32;
        while !s.is_terminal() && depth < max_depth {
            let acts = s.legal_actions();
            if acts.is_empty() {
                return internal("a running game has no legal actions");
            }
            let i = policy::choose(&s, &acts, playout, &mut rng);
            s.execute(&acts[i])?;
            depth += 1;
        }

        let mut value2 = [1u32; MAX_PLAYERS];
        let cut_off = match s.outcome() {
            Outcome::InProgress => true,
            Outcome::Draw => {
                value2 = [2; MAX_PLAYERS];
                false
            }
            Outcome::Durak { player } => {
                value2 = [2; MAX_PLAYERS];
                value2[player.index()] = 0;
                false
            }
        };

        for idx in path {
            let node = &mut nodes[idx];
            node.visits += 1;
            node.cutoffs += u32::from(cut_off);
            for (total, v) in node.value2.iter_mut().zip(value2) {
                *total += v;
            }
        }
    }

    let p = perspective.index();
    let actions = root_actions
        .into_iter()
        .map(|action| {
            let child = nodes[0].children.iter().find(|(a, _)| *a == action).map(|(_, i)| &nodes[*i]);
            match child {
                Some(n) if n.visits > 0 => ActionEval {
                    action,
                    visits: n.visits,
                    score: n.value2[p] as f32 / (2.0 * n.visits as f32),
                    cutoffs: n.cutoffs,
                },
                _ => ActionEval { action, visits: 0, score: 0.0, cutoffs: 0 },
            }
        })
        .collect();

    Ok(MctsEval { actions, total_rollouts: rollouts })
}

/// Search effort for [`solve`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SolveParams {
    /// How many random deals of the hidden cards to search.
    pub determinizations: u32,
    /// Playouts per deal.
    pub rollouts: u32,
    /// Playout length limit in moves.
    pub max_depth: u32,
    /// UCT exploration constant.
    pub exploration: f64,
    /// How the players move in a playout.
    #[serde(default)]
    pub playout: Playout,
}

impl Default for SolveParams {
    fn default() -> Self {
        Self { determinizations: 1, rollouts: 1000, max_depth: 500, exploration: 1.41, playout: Playout::default() }
    }
}

impl SolveParams {
    pub fn validate(&self) -> Result<(), EngineError> {
        let check = |name: &str, value: u32, max: u32| {
            if value == 0 || value > max {
                Err(EngineError::InvalidInput(format!(
                    "{name} must be between 1 and {max}, got {value}"
                )))
            } else {
                Ok(())
            }
        };
        check("determinizations", self.determinizations, MAX_DETERMINIZATIONS)?;
        check("rollouts", self.rollouts, MAX_ROLLOUTS)?;
        check("max_depth", self.max_depth, MAX_SEARCH_DEPTH)?;
        if !self.exploration.is_finite() || self.exploration < 0.0 {
            return Err(EngineError::InvalidInput("exploration must be a non-negative number".into()));
        }
        Ok(())
    }
}

/// Move evaluation for the player to move, pooled over several deals.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SolveResult {
    /// The player the scores are for (the player to move).
    pub perspective: PlayerId,
    pub determinizations: u32,
    /// Total playouts across all deals.
    pub total_visits: u32,
    /// Playouts that hit the depth limit.
    pub total_cutoffs: u32,
    /// One entry per legal action, best score first; unevaluated actions last.
    pub actions: Vec<ActionEval>,
}

fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

/// Evaluate every legal action for the player to move.
///
/// For each determinization the hidden cards are dealt at random (consistent
/// with everything that player knows) and searched; results are pooled per
/// action as a visit-weighted mean: `score = sum(visits_i * score_i) / sum(visits_i)`.
/// Deterministic for a given seed. A finished game yields an empty action list.
pub fn solve(state: &GameState, params: SolveParams, seed: u64) -> Result<SolveResult, EngineError> {
    state.validate()?;
    params.validate()?;

    let perspective = state.actor_to_move();
    let root_actions = state.legal_actions();
    let mut result = SolveResult {
        perspective,
        determinizations: params.determinizations,
        total_visits: 0,
        total_cutoffs: 0,
        actions: vec![],
    };
    if root_actions.is_empty() {
        result.determinizations = 0;
        return Ok(result);
    }

    // (visits, value sum, cutoffs) per root action.
    let mut totals = vec![(0u64, 0f64, 0u64); root_actions.len()];

    for i in 0..params.determinizations {
        let det_seed = splitmix64(seed ^ splitmix64(u64::from(i)));
        let mut rng = StdRng::seed_from_u64(det_seed);
        let det_state = state.determinize(perspective, &mut rng)?;
        let eval = mcts_evaluate_actions(
            &det_state,
            det_seed,
            perspective,
            params.rollouts,
            params.max_depth,
            params.exploration,
            params.playout,
        )?;

        for e in eval.actions {
            // The deal changes card visibility, never which moves exist.
            let Some(slot) = root_actions.iter().position(|a| a.same_move(&e.action)) else {
                return internal("search returned a move that is not legal in the real position");
            };
            totals[slot].0 += u64::from(e.visits);
            totals[slot].1 += f64::from(e.visits) * f64::from(e.score);
            totals[slot].2 += u64::from(e.cutoffs);
        }
    }

    let clamp = |v: u64| u32::try_from(v).unwrap_or(u32::MAX);
    let mut actions: Vec<ActionEval> = root_actions
        .into_iter()
        .zip(&totals)
        .map(|(action, (visits, sum, cutoffs))| ActionEval {
            action,
            visits: clamp(*visits),
            score: if *visits > 0 { (*sum / *visits as f64) as f32 } else { 0.0 },
            cutoffs: clamp(*cutoffs),
        })
        .collect();
    // Best first; unevaluated actions (no visits) go last. Stable, so ties keep move order.
    actions.sort_by(|a, b| {
        (b.visits > 0)
            .cmp(&(a.visits > 0))
            .then(b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal))
    });

    result.total_visits = clamp(totals.iter().map(|t| t.0).sum());
    result.total_cutoffs = clamp(totals.iter().map(|t| t.2).sum());
    result.actions = actions;
    Ok(result)
}
