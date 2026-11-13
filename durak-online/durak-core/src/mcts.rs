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

use crate::eval;
use crate::policy::{self, Playout};
use crate::{solve_exact_deal, Action, EngineError, ExactValue, GameState, Outcome, Phase, PlayerId, MAX_PLAYERS};

/// Upper bounds accepted by [`solve`]; larger requests are rejected.
pub const MAX_DETERMINIZATIONS: u32 = 2_000;
pub const MAX_ROLLOUTS: u32 = 200_000;
/// Largest per-deal limit for exact solving inside [`solve`].
pub const MAX_EXACT_NODES: u64 = 50_000_000;
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
    /// Sum of playout values per player (1 = not the durak, 0.5 = cut off).
    value: [f64; MAX_PLAYERS],
    visits: u32,
    cutoffs: u32,
    /// Explored moves: the move, the child node and the move's prior.
    children: Vec<(Action, usize, f32)>,
    /// Moves not tried yet, with their priors; popped from the back.
    unexplored: Vec<(Action, f32)>,
}

impl Node {
    /// A node for `state`. With priors, moves are tried best-looking first and
    /// carry a prior in (0, 1] from [`policy::move_cost`]; without, every
    /// prior is 0 and moves are tried in move order. With `candidates` > 0
    /// only that many moves - the cheapest by [`policy::move_cost`] - are
    /// searched at all.
    fn new(state: &GameState, use_priors: bool, candidates: usize) -> Self {
        let mut actions = state.legal_actions();
        let ranked = use_priors || (candidates > 0 && actions.len() > candidates);
        let mut unexplored: Vec<(Action, f32)> = if ranked {
            let mut costed: Vec<(i32, Action)> =
                actions.drain(..).map(|a| (policy::move_cost(state, &a), a)).collect();
            // Cheapest first; stable, so ties keep move order.
            costed.sort_by_key(|(c, _)| *c);
            if candidates > 0 {
                costed.truncate(candidates);
            }
            let best = costed.first().map_or(0, |(c, _)| *c);
            let mut v: Vec<(Action, f32)> = costed
                .into_iter()
                .map(|(c, a)| (a, if use_priors { 1.0 / (1.0 + (c - best) as f32 / 4.0) } else { 0.0 }))
                .collect();
            // Best first out: `unexplored` is popped from the back.
            v.reverse();
            v
        } else {
            actions.into_iter().rev().map(|a| (a, 0.0)).collect()
        };
        unexplored.shrink_to_fit();
        Self { value: [0.0; MAX_PLAYERS], visits: 0, cutoffs: 0, children: Vec::new(), unexplored }
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
    params: &SolveParams,
) -> Result<MctsEval, EngineError> {
    let SolveParams { rollouts, max_depth, exploration, playout, prior_weight, cutoff, .. } = *params;
    let cutoff = cutoff && state.num_players() == 2;
    let use_priors = prior_weight > 0.0;
    let candidates = usize::from(params.candidates);
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
    // My own decision is always searched in full; pruning applies to the
    // moves imagined after it.
    let mut nodes = vec![Node::new(state, use_priors, 0)];

    for _ in 0..rollouts {
        let mut s = state.clone();
        let mut path = vec![0usize];
        let mut cur = 0usize;

        // Selection, then expansion of one new node.
        while !s.is_terminal() {
            if let Some((action, prior)) = nodes[cur].unexplored.pop() {
                s.execute(&action)?;
                let child = nodes.len();
                let k = if s.stock.len() >= usize::from(params.candidates_min_stock) { candidates } else { 0 };
                nodes.push(Node::new(&s, use_priors, k));
                nodes[cur].children.push((action, child, prior));
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
            for (i, (_, child, prior)) in nodes[cur].children.iter().enumerate() {
                let c = &nodes[*child];
                let n = f64::from(c.visits.max(1));
                // Progressive bias: sensible-looking moves get a head start
                // that fades as real results come in.
                let uct = c.value[actor] / n
                    + exploration * (ln_n / n).sqrt()
                    + prior_weight * f64::from(*prior) / (n + 1.0);
                if uct > best_uct {
                    best_uct = uct;
                    best = i;
                }
            }
            let (action, child, _) = nodes[cur].children[best].clone();
            s.execute(&action)?;
            cur = child;
            path.push(child);
        }

        // Playout: random or heuristic players (see `policy`). With `cutoff`,
        // a two-player playout stops once the stock is empty and the hands
        // are scored instead (see `eval`).
        let mut depth = 0u32;
        let mut scored = None;
        while !s.is_terminal() && depth < max_depth {
            if cutoff && s.stock.is_empty() && s.phase == Phase::Attacking && s.table.is_empty() {
                scored = Some(eval::attacker_wins(&s));
                break;
            }
            let acts = s.legal_actions();
            if acts.is_empty() {
                return internal("a running game has no legal actions");
            }
            let i = policy::choose(&s, &acts, playout, &mut rng);
            s.execute(&acts[i])?;
            depth += 1;
        }

        let mut value = [0.5; MAX_PLAYERS];
        let mut cut_off = false;
        if let Some(p) = scored {
            value[s.attacker.index()] = p;
            value[s.defender.index()] = 1.0 - p;
        } else {
            match s.outcome() {
                Outcome::InProgress => cut_off = true,
                Outcome::Draw => value = [1.0; MAX_PLAYERS],
                Outcome::Durak { player } => {
                    value = [1.0; MAX_PLAYERS];
                    value[player.index()] = 0.0;
                }
            }
        }

        for idx in path {
            let node = &mut nodes[idx];
            node.visits += 1;
            node.cutoffs += u32::from(cut_off);
            for (total, v) in node.value.iter_mut().zip(value) {
                *total += v;
            }
        }
    }

    let p = perspective.index();
    let actions = root_actions
        .into_iter()
        .map(|action| {
            let child = nodes[0].children.iter().find(|(a, _, _)| *a == action).map(|(_, i, _)| &nodes[*i]);
            match child {
                Some(n) if n.visits > 0 => ActionEval {
                    action,
                    visits: n.visits,
                    score: (n.value[p] / f64::from(n.visits)) as f32,
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
    /// Two players with at most `exact_stock` cards left to draw: solve each
    /// deal exactly, searching up to this many positions, and fall back to
    /// playouts for a deal that is not settled in time. 0 = never.
    #[serde(default)]
    pub exact_nodes: u64,
    #[serde(default)]
    pub exact_stock: u8,
    /// Weight of the head start sensible-looking moves get in the tree
    /// (progressive bias from [`policy::move_cost`]); 0 = none.
    #[serde(default)]
    pub prior_weight: f64,
    /// Two players: end each playout once the stock is empty and score the
    /// hands with [`eval`] instead of playing on.
    #[serde(default)]
    pub cutoff: bool,
    /// Below the root, search only this many moves at each decision - the
    /// ones the heuristic player rates best (see [`policy::move_cost`]) - as
    /// long as at least `candidates_min_stock` cards are left to draw. The
    /// player's own moves at the root are always all searched. 0 = no pruning.
    #[serde(default)]
    pub candidates: u8,
    #[serde(default)]
    pub candidates_min_stock: u8,
}

impl Default for SolveParams {
    fn default() -> Self {
        Self {
            determinizations: 1,
            rollouts: 1000,
            max_depth: 500,
            exploration: 1.41,
            playout: Playout::default(),
            exact_nodes: 0,
            exact_stock: 0,
            prior_weight: 0.0,
            cutoff: false,
            candidates: 0,
            candidates_min_stock: 0,
        }
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
        if self.exact_nodes > MAX_EXACT_NODES {
            return Err(EngineError::InvalidInput(format!("exact_nodes must be at most {MAX_EXACT_NODES}")));
        }
        if !self.prior_weight.is_finite() || self.prior_weight < 0.0 {
            return Err(EngineError::InvalidInput("prior_weight must be a non-negative number".into()));
        }
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
    /// Deals solved exactly instead of by playouts (see [`SolveParams::exact_nodes`]).
    #[serde(default)]
    pub exact_deals: u32,
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
        exact_deals: 0,
        actions: vec![],
    };
    let exact = params.exact_nodes > 0
        && state.num_players() == 2
        && state.stock.len() <= usize::from(params.exact_stock);
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

        // Few cards left to draw: this deal may be small enough to solve.
        // Each solved move counts as many visits as a deal's playouts, valued
        // 1 for a win, 1/2 for a draw and 0 for a loss.
        if exact {
            if let Some(r) = solve_exact_deal(&det_state, params.exact_nodes)? {
                if r.actions.iter().all(|a| a.value.is_some()) {
                    for a in &r.actions {
                        let Some(slot) = root_actions.iter().position(|x| x.same_move(&a.action)) else {
                            return internal("the solver returned a move that is not legal in the real position");
                        };
                        let v = match a.value {
                            Some(ExactValue::Win) => 1.0,
                            Some(ExactValue::Draw) => 0.5,
                            _ => 0.0,
                        };
                        totals[slot].0 += u64::from(params.rollouts);
                        totals[slot].1 += f64::from(params.rollouts) * v;
                    }
                    result.exact_deals += 1;
                    continue;
                }
            }
        }

        let eval = mcts_evaluate_actions(&det_state, det_seed, perspective, &params)?;

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
