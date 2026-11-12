// Game recordings ("replays"): building them, statistics, and storage.
//
// A recording keeps the full position after every move, plus - for my own
// moves - what the search suggested at the time. That makes a game replayable
// without the engine and usable later for checking the engine's advice.
//
// Recordings live in IndexedDB (they are too big for localStorage) in two
// stores: `meta` holds one small summary per game for the list and the
// statistics, `games` holds the full recordings.

import { ME } from './game.js'

export const REPLAY_VERSION = 1
export const MAX_REPLAYS = 500
export const RESULTS = ['win', 'loss', 'draw', 'unfinished']

const DB_NAME = 'durak_ai_replays'
const DB_VERSION = 1

// ── Building a recording (pure) ─────────────────────────────────────────────

export function newReplayId(now = Date.now(), random = Math.random()) {
  return `${now.toString(36)}-${Math.floor(random * 0xffffff).toString(36).padStart(4, '0')}`
}

/** Start recording a game from its first position. */
export function newRecording({ id, mode, names, playerTypes = null, initial, now = Date.now() }) {
  return {
    v: REPLAY_VERSION,
    id: id || newReplayId(now),
    mode, // 'play' | 'analyse'
    startedAt: now,
    config: initial?.config || null,
    names: { ...names },
    playerTypes: playerTypes ? { ...playerTypes } : null,
    initial,
    steps: [],
  }
}

/**
 * What the search said just before one of my moves. `summary` is the output of
 * `summarize`; returns null when nothing was evaluated.
 */
export function hintSnapshot(summary, bestKey, chosenKey, describe = () => '') {
  if (!summary || !summary.totalVisits) return null
  const moves = summary.actions
    .filter(a => (a.score !== null && a.visits > 0) || a.exact)
    .map(a => {
      const move = { key: a.key, text: describe(a.action), score: a.score, visits: a.visits }
      // 'win' | 'draw' | 'loss' when the exact endgame solver settled the move.
      if (a.exact) move.exact = a.exact
      return move
    })
  if (!moves.length) return null
  const snapshot = {
    best: bestKey || null,
    chosen: chosenKey,
    followed: !!bestKey && bestKey === chosenKey,
    playouts: summary.totalVisits,
    deals: summary.determinizations || 0,
    moves,
  }
  if (summary.exactValue) snapshot.exactValue = summary.exactValue
  return snapshot
}

/** Append the position reached by one move (or one revealed card). */
export function addStep(rec, { state, line = '', actor = null, action = null, hint = null }) {
  const step = { state, line, actor }
  if (action) step.action = action
  if (hint) step.hint = hint
  return { ...rec, steps: [...rec.steps, step] }
}

/** Drop everything after the first `length` steps (used by undo). */
export function truncate(rec, length) {
  if (!Number.isInteger(length) || length < 0 || length >= rec.steps.length) return rec
  return { ...rec, steps: rec.steps.slice(0, length) }
}

/** The result from my seat for an engine outcome ({ status, player }). */
export function resultOf(outcome, me = ME) {
  if (outcome?.status === 'draw') return 'draw'
  if (outcome?.status === 'durak') return outcome.player === me ? 'loss' : 'win'
  return 'unfinished'
}

/** Close a recording: stamp the end time and the result. */
export function finalize(rec, outcome, now = Date.now()) {
  return {
    ...rec,
    endedAt: now,
    result: resultOf(outcome),
    durak: outcome?.status === 'durak' ? outcome.player : null,
  }
}

/** Is this something `finalize` produced (or a recording in progress)? */
export function isRecording(x) {
  return !!x && typeof x === 'object' && typeof x.id === 'string' && Array.isArray(x.steps) && !!x.initial
    && (x.mode === 'play' || x.mode === 'analyse')
}

/** The small summary kept for the list and the statistics. */
export function metaOf(rec) {
  const mine = rec.steps.filter(s => s.actor === ME && s.action)
  const hinted = mine.filter(s => s.hint)
  return {
    v: REPLAY_VERSION,
    id: rec.id,
    mode: rec.mode,
    startedAt: rec.startedAt,
    endedAt: rec.endedAt ?? null,
    players: rec.initial?.hands?.length || rec.config?.num_players || 0,
    deck: rec.config?.deck_size || 0,
    reflecting: !!rec.config?.reflecting,
    classic: !!rec.config?.classic_ending,
    names: rec.names || {},
    playerTypes: rec.playerTypes || null,
    result: RESULTS.includes(rec.result) ? rec.result : 'unfinished',
    userResult: RESULTS.includes(rec.userResult) ? rec.userResult : null,
    durak: rec.durak ?? null,
    moves: rec.steps.filter(s => s.action).length,
    myMoves: mine.length,
    hinted: hinted.length,
    followed: hinted.filter(s => s.hint.followed).length,
  }
}

/** The result to show and count: my correction if there is one. */
export const effectiveResult = meta => meta?.userResult || meta?.result || 'unfinished'

// ── Statistics (pure) ───────────────────────────────────────────────────────

function tally(metas) {
  const t = { games: metas.length, finished: 0, wins: 0, losses: 0, draws: 0, unfinished: 0, hinted: 0, followed: 0 }
  for (const m of metas) {
    const r = effectiveResult(m)
    if (r === 'win') t.wins++
    else if (r === 'loss') t.losses++
    else if (r === 'draw') t.draws++
    else t.unfinished++
    t.hinted += m.hinted || 0
    t.followed += m.followed || 0
  }
  t.finished = t.wins + t.losses + t.draws
  // "Win" in Durak means not being the durak; a draw is neither.
  t.winRate = t.finished ? t.wins / t.finished : null
  t.lossRate = t.finished ? t.losses / t.finished : null
  t.followRate = t.hinted ? t.followed / t.hinted : null
  return t
}

const MODE_LABEL = { play: 'Against the computer', analyse: 'Real games' }

/** Overall numbers plus breakdowns by mode, table and how closely I followed the advice. */
export function computeStats(metas) {
  const list = Array.isArray(metas) ? metas : []
  const group = (label, items) => ({ label, ...tally(items) })
  const groups = []

  for (const mode of ['analyse', 'play']) {
    const items = list.filter(m => m.mode === mode)
    if (items.length) groups.push(group(MODE_LABEL[mode], items))
  }
  const tables = new Map()
  for (const m of list) {
    const label = `${m.players} players, ${m.deck} cards`
    tables.set(label, [...(tables.get(label) || []), m])
  }
  for (const [label, items] of [...tables].sort((a, b) => a[0].localeCompare(b[0]))) groups.push(group(label, items))

  // Did following the suggestions go with better results?
  const rated = list.filter(m => m.hinted >= 5 && effectiveResult(m) !== 'unfinished')
  const close = rated.filter(m => m.followed / m.hinted >= 0.8)
  const loose = rated.filter(m => m.followed / m.hinted < 0.8)
  if (close.length) groups.push(group('Followed 80%+ of suggestions', close))
  if (loose.length) groups.push(group('Followed under 80%', loose))

  const recent = [...list]
    .filter(m => effectiveResult(m) !== 'unfinished')
    .sort((a, b) => b.startedAt - a.startedAt)
    .slice(0, 10)
    .map(effectiveResult)

  return { ...tally(list), groups, recent }
}

// ── Storage (IndexedDB) ─────────────────────────────────────────────────────

let dbPromise = null

function openDb() {
  if (typeof indexedDB === 'undefined') return Promise.reject(new Error('This browser cannot store game recordings.'))
  dbPromise ??= new Promise((resolve, reject) => {
    const req = indexedDB.open(DB_NAME, DB_VERSION)
    req.onupgradeneeded = () => {
      const db = req.result
      if (!db.objectStoreNames.contains('meta')) db.createObjectStore('meta', { keyPath: 'id' })
      if (!db.objectStoreNames.contains('games')) db.createObjectStore('games', { keyPath: 'id' })
    }
    req.onsuccess = () => resolve(req.result)
    req.onerror = () => reject(req.error || new Error('Could not open the recordings database.'))
    req.onblocked = () => reject(new Error('The recordings database is in use by another tab.'))
  }).catch(e => {
    dbPromise = null // let the next call try again
    throw e
  })
  return dbPromise
}

const done = tx => new Promise((resolve, reject) => {
  tx.oncomplete = () => resolve()
  tx.onerror = () => reject(tx.error)
  tx.onabort = () => reject(tx.error || new Error('The database transaction was aborted.'))
})
const ask = req => new Promise((resolve, reject) => {
  req.onsuccess = () => resolve(req.result)
  req.onerror = () => reject(req.error)
})

/** Save (or replace) a finished recording. Oldest games beyond the cap are dropped. */
export async function saveReplay(rec) {
  const db = await openDb()
  // Plain JSON only: Svelte proxies and the like cannot be cloned into the database.
  const clean = JSON.parse(JSON.stringify(rec))
  const tx = db.transaction(['meta', 'games'], 'readwrite')
  tx.objectStore('games').put(clean)
  tx.objectStore('meta').put(metaOf(clean))
  await done(tx)

  const metas = await listReplays()
  if (metas.length > MAX_REPLAYS) {
    for (const old of metas.slice(MAX_REPLAYS)) await deleteReplay(old.id)
  }
}

/** Every game's summary, newest first. */
export async function listReplays() {
  const db = await openDb()
  const metas = await ask(db.transaction('meta').objectStore('meta').getAll())
  return metas.sort((a, b) => b.startedAt - a.startedAt)
}

export async function loadReplay(id) {
  const db = await openDb()
  return (await ask(db.transaction('games').objectStore('games').get(id))) || null
}

export async function deleteReplay(id) {
  const db = await openDb()
  const tx = db.transaction(['meta', 'games'], 'readwrite')
  tx.objectStore('games').delete(id)
  tx.objectStore('meta').delete(id)
  await done(tx)
}

export async function clearReplays() {
  const db = await openDb()
  const tx = db.transaction(['meta', 'games'], 'readwrite')
  tx.objectStore('games').clear()
  tx.objectStore('meta').clear()
  await done(tx)
}

/** Record what really happened when the app's result is wrong (null removes the correction). */
export async function setUserResult(id, userResult) {
  const rec = await loadReplay(id)
  if (!rec) return
  if (RESULTS.includes(userResult)) rec.userResult = userResult
  else delete rec.userResult
  await saveReplay(rec)
}

/** Every full recording, newest first - for exporting. */
export async function allReplays() {
  const db = await openDb()
  const games = await ask(db.transaction('games').objectStore('games').getAll())
  return games.sort((a, b) => b.startedAt - a.startedAt)
}
