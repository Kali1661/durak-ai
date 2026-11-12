// Pooling solver results and choosing a move.
//
// The engine returns, per legal action, `visits` (playouts through that
// action) and `score` (mean playout value in [0, 1]: 1 = not the durak,
// 0 = durak, 0.5 = playout cut off). Results from several workers and batches
// are pooled as a visit-weighted mean:
//
//     score = sum(visits_i * score_i) / sum(visits_i)
//
// which is exactly the mean over all pooled playouts.

/** Identity of a move, ignoring whether its card is public or private. */
export function actionKey(action) {
  if (!action || typeof action !== 'object') return 'invalid'
  const card = action.card && action.card.suit ? `${action.card.suit}:${action.card.rank}` : '-'
  const pile = action.type === 'defend' ? action.pile_index : '-'
  return `${action.type}|${pile}|${card}`
}

export function createTally() {
  return { byKey: new Map(), determinizations: 0, totalVisits: 0, totalCutoffs: 0, batches: 0 }
}

const isCount = n => Number.isInteger(n) && n >= 0

/**
 * Add one solver result to a tally. Throws on a malformed result rather than
 * folding garbage into the statistics.
 */
export function addResult(tally, result) {
  if (!result || !Array.isArray(result.actions)) throw new Error('solver result has no action list')
  for (const entry of result.actions) {
    if (!entry || !entry.action || !isCount(entry.visits)) throw new Error('solver result has a malformed action')
    if (entry.visits === 0) continue
    if (typeof entry.score !== 'number' || !(entry.score >= 0 && entry.score <= 1)) {
      throw new Error('solver result has a score outside 0-1')
    }
    const key = actionKey(entry.action)
    const cur = tally.byKey.get(key) || { visits: 0, weighted: 0, cutoffs: 0 }
    cur.visits += entry.visits
    cur.weighted += entry.visits * entry.score
    cur.cutoffs += isCount(entry.cutoffs) ? entry.cutoffs : 0
    tally.byKey.set(key, cur)
  }
  tally.determinizations += isCount(result.determinizations) ? result.determinizations : 0
  tally.totalVisits += isCount(result.total_visits) ? result.total_visits : 0
  tally.totalCutoffs += isCount(result.total_cutoffs) ? result.total_cutoffs : 0
  tally.batches += 1
  return tally
}

/**
 * Per-action statistics for the current legal actions, best first.
 * Actions the solver has not evaluated have `score: null` and come last.
 * Results for actions that are not (or no longer) legal are ignored.
 */
export function summarize(tally, legal) {
  const actions = (legal || []).map(action => {
    const key = actionKey(action)
    const t = tally?.byKey.get(key)
    const visits = t?.visits || 0
    return {
      action,
      key,
      visits,
      score: visits > 0 ? t.weighted / visits : null,
      cutoffs: t?.cutoffs || 0,
    }
  })
  actions.sort((a, b) => {
    if ((a.score === null) !== (b.score === null)) return a.score === null ? 1 : -1
    if (a.score === null) return 0
    return b.score - a.score || b.visits - a.visits
  })
  return {
    actions,
    determinizations: tally?.determinizations || 0,
    totalVisits: tally?.totalVisits || 0,
    totalCutoffs: tally?.totalCutoffs || 0,
    batches: tally?.batches || 0,
  }
}

/**
 * How good a calculated result is. A move the exact solver could not settle
 * ranks below a certain draw and above a certain loss.
 */
const EXACT_RANK = { win: 3, draw: 2, loss: 0 }
const exactRank = a => (a.exact ? EXACT_RANK[a.exact] ?? 1 : 1)

/**
 * Attach the exact endgame solver's results to a summary: every action gains
 * `exact` ('win' | 'draw' | 'loss' | null) and the list is re-sorted so that
 * calculated results come before estimates - wins first, losses last, the
 * search score deciding within each group. `exact` is the engine's
 * `solve_exact` result; with none, the summary is returned unchanged.
 */
export function withExact(summary, exact) {
  if (!summary || !exact || !Array.isArray(exact.actions)) return summary
  const values = new Map(exact.actions.map(a => [actionKey(a.action), a.value || null]))
  const actions = summary.actions.map(a => ({ ...a, exact: values.get(a.key) ?? null }))
  if (!actions.some(a => a.exact)) return summary
  actions.sort((a, b) => {
    if (exactRank(a) !== exactRank(b)) return exactRank(b) - exactRank(a)
    if ((a.score === null) !== (b.score === null)) return a.score === null ? 1 : -1
    if (a.score === null) return 0
    return b.score - a.score || b.visits - a.visits
  })
  return { ...summary, actions, exactValue: exact.value || null, exactNodes: exact.nodes || 0 }
}

/**
 * The move to play: the highest score among sufficiently sampled actions.
 *
 * An action explored only a handful of times has a meaningless score, so
 * actions with fewer than `minVisitShare` of the most-visited action's
 * playouts are not candidates. Ties go to the action with more playouts.
 * Returns null when nothing has been evaluated - the caller must decide what
 * to do then; this function never guesses.
 *
 * When the exact solver has settled some moves (see `withExact`), only the
 * moves with the best calculated result are candidates, and the search score
 * just breaks ties among them: a certain win beats any estimate.
 */
export function pickBest(summary, { minVisitShare = 0.05 } = {}) {
  let pool = summary?.actions || []
  if (pool.some(a => a.exact)) {
    const top = Math.max(...pool.map(exactRank))
    pool = pool.filter(a => exactRank(a) === top)
    // A calculated result needs no playouts behind it.
    if (!pool.some(a => a.score !== null && a.visits > 0)) return pool[0] || null
  }
  const evaluated = pool.filter(a => a.score !== null && a.visits > 0)
  if (!evaluated.length) return null
  const mostVisits = Math.max(...evaluated.map(a => a.visits))
  const threshold = Math.max(1, Math.ceil(mostVisits * minVisitShare))
  const candidates = evaluated.filter(a => a.visits >= threshold)
  return candidates.reduce((best, a) =>
    a.score > best.score || (a.score === best.score && a.visits > best.visits) ? a : best,
  )
}

/** Rough half-width of a 95% interval, treating playouts as independent. */
export function scoreMargin(score, visits) {
  if (score === null || !(visits > 0)) return null
  return 1.96 * Math.sqrt(Math.max(score * (1 - score), 0.25 / visits) / visits)
}
