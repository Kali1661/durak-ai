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
use crate::{
    solve_exact_deal, Action, Beliefs, EngineError, ExactValue, GameState, Outcome, Phase, PlayerId, MAX_PLAYERS,
};

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

        let (value, cut_off) = play_out(&mut s, &mut rng, playout, cutoff, max_depth)?;

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

/// Play `s` on from here (random or heuristic players, see `policy`) and
/// value the result per player: 1 = not the durak, 0 = the durak, 1/2 = cut
/// off at `max_depth` (also returned as `true`). With `cutoff`, a two-player
/// playout stops once the stock is empty and the hands are scored instead
/// (see `eval`).
fn play_out(
    s: &mut GameState,
    rng: &mut StdRng,
    playout: Playout,
    cutoff: bool,
    max_depth: u32,
) -> Result<([f64; MAX_PLAYERS], bool), EngineError> {
    // Two players and the heuristic player: the same playout on a compact
    // copy of the position, many times faster (see `endgame`).
    if playout == Playout::Heuristic {
        if let Some((v, cut_off)) = crate::endgame::fast_playout(s, rng, cutoff, max_depth) {
            let mut value = [0.5; MAX_PLAYERS];
            value[..2].copy_from_slice(&v);
            return Ok((value, cut_off));
        }
    }
    let mut depth = 0u32;
    while !s.is_terminal() && depth < max_depth {
        if cutoff && s.stock.is_empty() && s.phase == Phase::Attacking && s.table.is_empty() {
            let p = eval::attacker_wins(s);
            let mut value = [0.5; MAX_PLAYERS];
            value[s.attacker.index()] = p;
            value[s.defender.index()] = 1.0 - p;
            return Ok((value, false));
        }
        let acts = s.legal_actions();
        if acts.is_empty() {
            return internal("a running game has no legal actions");
        }
        let i = policy::choose(s, &acts, playout, rng);
        s.execute(&acts[i])?;
        depth += 1;
    }
    Ok(match s.outcome() {
        Outcome::InProgress => ([0.5; MAX_PLAYERS], true),
        Outcome::Draw => ([1.0; MAX_PLAYERS], false),
        Outcome::Durak { player } => {
            let mut value = [1.0; MAX_PLAYERS];
            value[player.index()] = 0.0;
            (value, false)
        }
    })
}

// ── One tree over many deals ────────────────────────────────────────────────

/// A node of the shared tree: a sequence of moves, whatever the deal.
struct InfoNode {
    value: [f64; MAX_PLAYERS],
    visits: u32,
    cutoffs: u32,
    children: Vec<InfoChild>,
}

struct InfoChild {
    action: Action,
    node: usize,
    /// How often this move was available when its parent was visited (it
    /// depends on the deal); exploration is measured against this.
    available: u32,
}

impl InfoNode {
    fn new() -> Self {
        InfoNode { value: [0.0; MAX_PLAYERS], visits: 0, cutoffs: 0, children: Vec::new() }
    }
}

/// Most nodes one shared tree may hold (each is a few hundred bytes); past
/// this, playouts go on and the tree's statistics keep improving, but it
/// stops growing.
pub const MAX_TREE_NODES: usize = 400_000;

/// One tree for the player to move in the real `state`, hidden cards and
/// all: every iteration deals the hidden cards afresh (consistent with what
/// `perspective` knows), follows the tree using only moves legal in that
/// deal, and plays out. Statistics are pooled across deals at every node, so
/// a player's choices in the tree cannot depend on cards they cannot see.
/// (Single-observer information-set MCTS, with max^n values.)
///
/// The tree is kept between calls to [`SharedTree::run`], so a search can be
/// continued for as long as there is time.
pub struct SharedTree {
    state: GameState,
    perspective: PlayerId,
    params: SolveParams,
    rng: StdRng,
    root_actions: Vec<Action>,
    nodes: Vec<InfoNode>,
    iterations: u64,
    /// What the opponent's play suggests they hold, if anything.
    beliefs: Option<Beliefs>,
    /// Two players and heuristic playouts: the same search on compact
    /// positions, which does all the work instead (see `endgame`).
    fast: Option<crate::endgame::FastTree>,
}

impl SharedTree {
    pub fn new(state: &GameState, seed: u64, perspective: PlayerId, params: &SolveParams) -> Self {
        SharedTree {
            state: state.clone(),
            perspective,
            params: *params,
            rng: StdRng::seed_from_u64(seed),
            root_actions: state.legal_actions(),
            nodes: vec![InfoNode::new()],
            iterations: 0,
            beliefs: None,
            fast: crate::endgame::FastTree::new(state, seed, perspective, params),
        }
    }

    /// Deal the hidden cards by these beliefs from now on.
    pub fn set_beliefs(&mut self, beliefs: Option<Beliefs>) {
        if let Some(fast) = &mut self.fast {
            fast.set_beliefs(beliefs.clone());
        }
        self.beliefs = beliefs;
    }

    /// Playouts run so far.
    pub fn iterations(&self) -> u64 {
        self.fast.as_ref().map_or(self.iterations, |fast| fast.iterations())
    }

    /// Run `iterations` more playouts.
    pub fn run(&mut self, iterations: u32) -> Result<(), EngineError> {
        if let Some(fast) = &mut self.fast {
            return fast.run(iterations);
        }
        let SolveParams { max_depth, exploration, playout, cutoff, .. } = self.params;
        let cutoff = cutoff && self.state.num_players() == 2;
        let candidates = usize::from(self.params.candidates);
        let min_stock = usize::from(self.params.candidates_min_stock);
        let nodes = &mut self.nodes;
        let rng = &mut self.rng;

        for _ in 0..iterations {
            let mut s = self.state.determinize_with(self.perspective, rng, self.beliefs.as_ref())?;
            let mut path = vec![0usize];
            let mut cur = 0usize;

            while !s.is_terminal() {
                let mut legal = s.legal_actions();
                if legal.is_empty() {
                    return internal("a running game has no legal actions");
                }
                let prune = cur != 0 && candidates > 0 && legal.len() > candidates && s.stock.len() >= min_stock;
                if prune || playout == Playout::Heuristic {
                    // Untried moves are expanded best-looking first.
                    legal.sort_by_cached_key(|a| policy::move_cost(&s, a));
                    if prune {
                        legal.truncate(candidates);
                    }
                }
                // Which known moves are available in this deal, and the first untried one.
                let mut here: Vec<usize> = Vec::with_capacity(legal.len());
                let mut untried = None;
                for a in legal {
                    match nodes[cur].children.iter().position(|c| c.action.same_move(&a)) {
                        Some(ci) => here.push(ci),
                        None if untried.is_none() => untried = Some(a),
                        None => {}
                    }
                }
                for &ci in &here {
                    nodes[cur].children[ci].available += 1;
                }
                if let Some(action) = untried {
                    // A full tree stops growing; the playout starts here instead.
                    if nodes.len() < MAX_TREE_NODES {
                        s.execute(&action)?;
                        let node = nodes.len();
                        nodes.push(InfoNode::new());
                        nodes[cur].children.push(InfoChild { action, node, available: 1 });
                        path.push(node);
                        break;
                    }
                    if here.is_empty() {
                        break;
                    }
                }
                let actor = s.actor_to_move().index();
                let mut best = here[0];
                let mut best_uct = f64::NEG_INFINITY;
                for &ci in &here {
                    let c = &nodes[cur].children[ci];
                    let n = &nodes[c.node];
                    let visits = f64::from(n.visits.max(1));
                    let uct = n.value[actor] / visits + exploration * (f64::from(c.available).ln() / visits).sqrt();
                    if uct > best_uct {
                        best_uct = uct;
                        best = ci;
                    }
                }
                let child = &nodes[cur].children[best];
                let next = child.node;
                s.execute(&child.action.clone())?;
                cur = next;
                path.push(cur);
            }

            let (value, cut_off) = play_out(&mut s, rng, playout, cutoff, max_depth)?;
            for idx in path {
                let node = &mut nodes[idx];
                node.visits += 1;
                node.cutoffs += u32::from(cut_off);
                for (total, v) in node.value.iter_mut().zip(value) {
                    *total += v;
                }
            }
            self.iterations += 1;
        }
        Ok(())
    }

    /// (playouts, value sum for the advised player, cut-off playouts) for
    /// every root action so far, in `legal_actions()` order.
    fn totals(&self) -> Vec<(u64, f64, u64)> {
        if let Some(fast) = &self.fast {
            return fast.totals();
        }
        let p = self.perspective.index();
        self.root_actions
            .iter()
            .map(|action| match self.nodes[0].children.iter().find(|c| c.action.same_move(action)) {
                Some(c) => {
                    let n = &self.nodes[c.node];
                    (u64::from(n.visits), n.value[p], u64::from(n.cutoffs))
                }
                None => (0, 0.0, 0),
            })
            .collect()
    }

    /// The results so far.
    pub fn evaluate(&self) -> MctsEval {
        let actions = self
            .root_actions
            .iter()
            .zip(self.totals())
            .map(|(action, (visits, sum, cutoffs))| ActionEval {
                action: action.clone(),
                visits: u32::try_from(visits).unwrap_or(u32::MAX),
                score: if visits > 0 { (sum / visits as f64) as f32 } else { 0.0 },
                cutoffs: u32::try_from(cutoffs).unwrap_or(u32::MAX),
            })
            .collect();
        MctsEval { actions, total_rollouts: u32::try_from(self.iterations()).unwrap_or(u32::MAX) }
    }
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
    /// Search one tree over all the deals instead of one tree per deal (see
    /// [`SharedTree`]). Where exact solving of deals applies (see
    /// `exact_stock`) the deals are solved first, and the tree takes over as
    /// soon as one turns out too big to solve.
    #[serde(default)]
    pub shared_tree: bool,
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
            shared_tree: false,
        }
    }
}

impl SolveParams {
    /// The settings the app searches with (its browser and native engines
    /// alike). The options beyond the defaults only act in two-player games.
    pub fn app() -> Self {
        Self {
            // One tree over all deals: in 300 arena games it was the durak
            // 41% of the time against the separate trees, at the same number
            // of playouts.
            shared_tree: true,
            // Four or fewer cards left to draw: solve each deal exactly where
            // it fits in a million positions; the first deal that does not
            // sends the rest of the search to the tree.
            exact_nodes: 1_000_000,
            exact_stock: 4,
            // Once the stock is empty, score the hands (trumps, cards that
            // cannot be beaten) instead of playing on.
            cutoff: true,
            ..Self::default()
        }
    }

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
    Search::new(state, params, seed)?.run(params.determinizations, params.rollouts)
}

/// A search of one position that can be continued: every [`Search::run`]
/// adds work and returns what *that* work found, so the results of several
/// runs pool like the results of separate searches - while a shared tree
/// (see [`SolveParams::shared_tree`]) is kept and grown from run to run
/// instead of being started again.
pub struct Search {
    state: GameState,
    params: SolveParams,
    seed: u64,
    perspective: PlayerId,
    root_actions: Vec<Action>,
    /// Deals dealt so far (each gets its own seed).
    deals: u64,
    /// Still solving deals exactly (see [`SolveParams::exact_nodes`]).
    exact: bool,
    tree: Option<SharedTree>,
    /// What the tree had found when it was last reported.
    reported: Vec<(u64, f64, u64)>,
    beliefs: Option<Beliefs>,
}

impl Search {
    /// `params.determinizations` and `params.rollouts` are not used here:
    /// each [`Search::run`] is given its own.
    pub fn new(state: &GameState, params: SolveParams, seed: u64) -> Result<Self, EngineError> {
        state.validate()?;
        params.validate()?;
        // The newer options are tuned for, and only used in, two-player games;
        // three-player games search as before.
        let mut params = params;
        let two = state.num_players() == 2;
        if !two {
            params.shared_tree = false;
            params.candidates = 0;
            params.cutoff = false;
        }
        let root_actions = state.legal_actions();
        Ok(Search {
            state: state.clone(),
            params,
            seed,
            perspective: state.actor_to_move(),
            exact: two && params.exact_nodes > 0 && state.stock.len() <= usize::from(params.exact_stock),
            reported: vec![(0, 0.0, 0); root_actions.len()],
            root_actions,
            deals: 0,
            tree: None,
            beliefs: None,
        })
    }

    /// Deal the opponent's hidden cards by what their play suggests they
    /// hold (see [`Beliefs`]). Set before the first [`Search::run`].
    pub fn with_beliefs(mut self, beliefs: Option<Beliefs>) -> Self {
        self.beliefs = beliefs;
        self
    }

    /// The position being searched.
    pub fn state(&self) -> &GameState {
        &self.state
    }

    /// Search `determinizations` more deals with `rollouts` playouts each (a
    /// shared tree gets that many playouts in total) and return the results
    /// of this run alone.
    pub fn run(&mut self, determinizations: u32, rollouts: u32) -> Result<SolveResult, EngineError> {
        let params = SolveParams { determinizations, rollouts, ..self.params };
        params.validate()?;
        let mut result = SolveResult {
            perspective: self.perspective,
            determinizations,
            total_visits: 0,
            total_cutoffs: 0,
            exact_deals: 0,
            actions: vec![],
        };
        if self.root_actions.is_empty() {
            result.determinizations = 0;
            return Ok(result);
        }

        // (visits, value sum, cutoffs) per root action.
        let mut totals = vec![(0u64, 0f64, 0u64); self.root_actions.len()];
        let mut deals_left = determinizations;

        while deals_left > 0 && (self.exact || !params.shared_tree) {
            let det_seed = splitmix64(self.seed ^ splitmix64(self.deals));
            let mut rng = StdRng::seed_from_u64(det_seed);
            let det_state = self.state.determinize_with(self.perspective, &mut rng, self.beliefs.as_ref())?;

            // Few cards left to draw: this deal may be small enough to solve.
            // Each solved move counts as many visits as a deal's playouts, valued
            // 1 for a win, 1/2 for a draw and 0 for a loss.
            if self.exact {
                let solved = solve_exact_deal(&det_state, params.exact_nodes)?
                    .filter(|r| r.actions.iter().all(|a| a.value.is_some()));
                if let Some(r) = solved {
                    for a in &r.actions {
                        let Some(slot) = self.root_actions.iter().position(|x| x.same_move(&a.action)) else {
                            return internal("the solver returned a move that is not legal in the real position");
                        };
                        let v = match a.value {
                            Some(ExactValue::Win) => 1.0,
                            Some(ExactValue::Draw) => 0.5,
                            _ => 0.0,
                        };
                        totals[slot].0 += u64::from(rollouts);
                        totals[slot].1 += f64::from(rollouts) * v;
                    }
                    result.exact_deals += 1;
                    self.deals += 1;
                    deals_left -= 1;
                    continue;
                }
                if params.shared_tree {
                    // Too big to solve: the next deals of this position will
                    // be too, so stop trying and give the shared tree the
                    // rest of the work, this deal's share included.
                    self.exact = false;
                    break;
                }
            }
            self.deals += 1;
            deals_left -= 1;

            let eval = mcts_evaluate_actions(&det_state, det_seed, self.perspective, &params)?;
            for e in eval.actions {
                // The deal changes card visibility, never which moves exist.
                let Some(slot) = self.root_actions.iter().position(|a| a.same_move(&e.action)) else {
                    return internal("search returned a move that is not legal in the real position");
                };
                totals[slot].0 += u64::from(e.visits);
                totals[slot].1 += f64::from(e.visits) * f64::from(e.score);
                totals[slot].2 += u64::from(e.cutoffs);
            }
        }

        if deals_left > 0 {
            // One tree, a fresh deal for every playout: as many playouts as the
            // separate trees would have had together.
            let (state, perspective, seed) = (&self.state, self.perspective, self.seed);
            let beliefs = &self.beliefs;
            let tree = self.tree.get_or_insert_with(|| {
                let mut tree = SharedTree::new(state, splitmix64(seed), perspective, &params);
                tree.set_beliefs(beliefs.clone());
                tree
            });
            tree.run(deals_left.saturating_mul(rollouts))?;
            let now = tree.totals();
            for ((total, new), old) in totals.iter_mut().zip(&now).zip(&self.reported) {
                total.0 += new.0 - old.0;
                total.1 += new.1 - old.1;
                total.2 += new.2 - old.2;
            }
            self.reported = now;
        }

        let clamp = |v: u64| u32::try_from(v).unwrap_or(u32::MAX);
        let mut actions: Vec<ActionEval> = self
            .root_actions
            .iter()
            .zip(&totals)
            .map(|(action, (visits, sum, cutoffs))| ActionEval {
                action: action.clone(),
                visits: clamp(*visits),
                score: if *visits > 0 { (*sum / *visits as f64).clamp(0.0, 1.0) as f32 } else { 0.0 },
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
}
