<script>
  import { onMount, onDestroy } from 'svelte'

  import Card from './lib/Card.svelte'
  import Seat from './components/Seat.svelte'
  import MoveRow from './components/MoveRow.svelte'
  import ThemeToggle from './components/ThemeToggle.svelte'
  import ModeTile from './components/ModeTile.svelte'
  import PillChoice from './components/PillChoice.svelte'
  import ThinkTime from './components/ThinkTime.svelte'
  import GamesPanel from './components/GamesPanel.svelte'
  import { cardLabel, suitSymbol } from './lib/cards.js'
  import {
    AI_TYPES, DECK_SIZES, LIMITS, PLAYER_COUNTS, PlayerType,
    loadSettings, maxThreads, sanitizeSettings, saveSettings, settingsToGameConfig,
  } from './lib/settings.js'
  import {
    ACTION_GROUPS, ME, actionCard, actionLogText, actionText, actorOf, cardKey, deckCards, handOf,
    isOut, isPublic, isUnknown, knownCards, parseCard, promptFor, ranksFor, roleOf, seatIndex,
    sortCards, unknownCount,
  } from './lib/game.js'
  import { actionKey, addResult, createTally, pickBest, summarize } from './lib/aggregate.js'
  import { SolverPool, friendlySolverError } from './lib/solver-pool.js'
  import { addStep, finalize, hintSnapshot, isRecording, newRecording, saveReplay, truncate } from './lib/replays.js'
  import initWasm, * as engine from './wasm/durak_wasm.js'

  // ───────────────────────────────────────────────────────────────────────
  // Constants
  // ───────────────────────────────────────────────────────────────────────

  const STORE = { play: 'durak_ai_play_v3', analyse: 'durak_ai_analyse_v3' }
  const LEGACY_STORE = { play: 'durak_computer_game', analyse: 'durak_manual_game' }
  const MAX_HISTORY = 60
  const MAX_LOG = 40
  const HAND_SIZE = 6
  const IN_PROGRESS = { status: 'in_progress' }

  // ───────────────────────────────────────────────────────────────────────
  // State
  // ───────────────────────────────────────────────────────────────────────

  let settings = loadSettings()
  let wasmReady = false
  let fatal = ''

  /** The engine's game state (plain JSON), or null on the setup screen. */
  let gs = null
  let legal = []
  let outcome = IN_PROGRESS
  /** Undo stack of { gs, log } snapshots. */
  let history = []
  let log = []
  /** { kind: 'error' | 'info', text } shown above the table. */
  let notice = null

  /** Bumped whenever the position changes; async work from an older epoch is dropped. */
  let epoch = 0
  let hintRun = 0
  let pendingTimer = null
  let pool = null

  const idleHint = () => ({ status: 'idle', summary: null, message: '' })
  const idleAi = () => ({ status: 'idle', player: null, message: '' })
  let hint = idleHint()
  let hintTally = null
  let ai = idleAi()

  let revealOpponents = false
  /** Real-game setup: trump card, my six cards, who attacks first. */
  let setup = { trump: null, hand: [], starter: ME }

  /** Recording of the game in progress (see lib/replays.js), or null. */
  let rec = null

  let settingsDialog
  let newGameDialog
  let gamesDialog
  let gamesPanel
  let confirmReset = false

  // ───────────────────────────────────────────────────────────────────────
  // Derived
  // ───────────────────────────────────────────────────────────────────────

  $: playMode = settings.computerShuffle
  $: seats = Array.from({ length: gs ? gs.hands.length : settings.numPlayers }, (_, i) => `P${i}`)
  $: opponents = seats.filter(p => p !== ME)
  $: actor = actorOf(gs)
  $: over = outcome.status !== 'in_progress'
  $: myTurn = !!gs && !over && actor === ME
  $: myCards = gs ? sortCards(handOf(gs, ME).map(parseCard).filter(Boolean), gs.trump) : []
  $: myUnknown = gs ? unknownCount(handOf(gs, ME)) : 0
  $: trumpCard = gs?.stock?.length ? parseCard(gs.stock[0]) : null
  $: stats = new Map((hint.summary?.actions || []).map(a => [a.key, a]))
  $: bestKey = (hint.summary && pickBest(hint.summary)?.key) || null
  $: actionsByCard = groupByCard(legal)
  $: nonCardActions = legal.filter(a => !actionCard(a))
  $: setupDeck = deckCards(settings.deckSize, setup.trump?.suit)
  $: setupReady = !!setup.trump && setup.hand.length === HAND_SIZE
  $: anySearchOpponent = opponents.some(p => settings.playerTypes[p] === PlayerType.MCTS)
  $: statusText = describeStatus(gs, outcome, actor, ai, settings)

  // ───────────────────────────────────────────────────────────────────────
  // Small helpers
  // ───────────────────────────────────────────────────────────────────────

  // Inside functions, read the mode and seat list from `settings` directly:
  // the `$:` values above are only recomputed after the current function ends.
  const isPlay = () => settings.computerShuffle
  const seatIds = () => Array.from({ length: settings.numPlayers }, (_, i) => `P${i}`)
  const wrap = c => ({ type: 'public', suit: c.suit, rank: c.rank })
  const pct = score => `${Math.round(score * 100)}%`
  const readable = e => friendlySolverError(e instanceof Error ? e.message : String(e))

  function nameOf(pid) {
    if (pid === ME) return 'You'
    return settings.playerNames[pid] || `Opponent ${seatIndex(pid)}`
  }

  function kindOf(pid) {
    if (pid === ME) return ''
    if (!isPlay()) return 'At the table'
    return AI_TYPES.find(t => t.value === settings.playerTypes[pid])?.label || ''
  }

  function groupByCard(actions) {
    const map = new Map()
    for (const a of actions) {
      const card = actionCard(a)
      if (!card) continue
      const key = cardKey(card)
      map.set(key, [...(map.get(key) || []), a])
    }
    return map
  }

  function describeStatus(state, result, who, aiState) {
    if (!state) return ''
    if (result.status === 'draw') return 'Game over: a draw. Everyone got rid of their cards.'
    if (result.status === 'durak') {
      return result.player === ME ? 'Game over: you are the durak.' : `Game over: ${nameOf(result.player)} is the durak. You are safe.`
    }
    const prompt = promptFor(state)
    if (who === ME) {
      if (unknownCount(handOf(state, ME)) > 0) return 'Tell the app which cards you drew.'
      return `Your turn: ${prompt.you}.`
    }
    if (aiState.status === 'thinking') return `${nameOf(who)} is thinking…`
    if (aiState.status === 'failed') return `${nameOf(who)} could not choose a move.`
    return `${nameOf(who)} ${prompt.other}.`
  }

  function randomSeed() {
    const buf = new BigUint64Array(1)
    crypto.getRandomValues(buf)
    return buf[0]
  }

  // ───────────────────────────────────────────────────────────────────────
  // Persistence
  // ───────────────────────────────────────────────────────────────────────

  const modeKey = () => (isPlay() ? 'play' : 'analyse')

  function cleanSetup(raw) {
    const ranks = new Set(ranksFor(settings.deckSize))
    const valid = c => c && typeof c.suit === 'string' && ranks.has(c.rank) && ['Clubs', 'Diamonds', 'Hearts', 'Spades'].includes(c.suit)
    const trump = valid(raw?.trump) ? { suit: raw.trump.suit, rank: raw.trump.rank } : null
    const seen = new Set(trump ? [cardKey(trump)] : [])
    const hand = []
    for (const c of Array.isArray(raw?.hand) ? raw.hand : []) {
      if (!valid(c) || seen.has(cardKey(c)) || hand.length >= HAND_SIZE) continue
      seen.add(cardKey(c))
      hand.push({ suit: c.suit, rank: c.rank })
    }
    const starter = seatIds().includes(raw?.starter) ? raw.starter : ME
    return { trump, hand, starter }
  }

  function saveGame() {
    try {
      const data = { v: 3, gs, history, log, rec }
      if (!isPlay()) data.setup = setup
      localStorage.setItem(STORE[modeKey()], JSON.stringify(data))
    } catch (e) {
      console.warn('Could not save the game.', e)
    }
  }

  /** Is this something the engine accepts as a game state? */
  function isValidState(candidate) {
    if (!candidate || typeof candidate !== 'object') return false
    try {
      engine.validate_state(candidate)
      return true
    } catch {
      return false
    }
  }

  /** Restore the saved game for the current mode, discarding anything invalid. */
  function loadGame() {
    const mode = modeKey()
    gs = null
    history = []
    log = []
    rec = null
    setup = cleanSetup(null)
    let discarded = false

    try {
      let raw = localStorage.getItem(STORE[mode])
      let data = raw ? JSON.parse(raw) : null
      if (!data) {
        // A game saved by an earlier version: keep the position if it is still valid.
        raw = localStorage.getItem(LEGACY_STORE[mode])
        const old = raw ? JSON.parse(raw) : null
        if (old) {
          data = { gs: old.state, history: [], log: [], setup: { trump: null, hand: old.manualHand } }
          localStorage.removeItem(LEGACY_STORE[mode])
        }
      }
      if (data && typeof data === 'object') {
        if (!isPlay()) setup = cleanSetup(data.setup)
        if (data.gs) {
          if (isValidState(data.gs)) {
            gs = data.gs
            const snaps = Array.isArray(data.history) ? data.history : []
            const ok = snaps.every(h => h && Array.isArray(h.log) && isValidState(h.gs))
            history = ok ? snaps.slice(-MAX_HISTORY) : []
            log = Array.isArray(data.log) ? data.log.filter(l => typeof l === 'string').slice(-MAX_LOG) : []
            if (!ok) discarded = true
            rec = isRecording(data.rec) ? data.rec : null
          } else {
            discarded = true
          }
        }
      }
    } catch (e) {
      console.warn('Saved game is unreadable.', e)
      discarded = true
    }

    if (discarded) {
      notice = {
        kind: 'info',
        text: gs
          ? 'Your saved game was restored, but its undo history could not be read and was cleared.'
          : 'Your saved game could not be restored - it uses a setting this version does not support (such as a removed deck size) or is damaged - so it was discarded. Start a new game below.',
      }
      saveGame()
    }
    refresh()
  }

  // ───────────────────────────────────────────────────────────────────────
  // Position bookkeeping
  // ───────────────────────────────────────────────────────────────────────

  /** Drop every piece of work that belongs to the position we are leaving. */
  function cancelPending() {
    epoch++
    hintRun++
    pool?.cancel()
    if (pendingTimer !== null) clearTimeout(pendingTimer)
    pendingTimer = null
    hint = idleHint()
    hintTally = null
    ai = idleAi()
  }

  /** Run `fn` later, unless the position changes first. */
  function schedule(fn, ms) {
    const at = epoch
    if (pendingTimer !== null) clearTimeout(pendingTimer)
    pendingTimer = setTimeout(() => {
      pendingTimer = null
      if (at === epoch) fn()
    }, ms)
  }

  function refresh() {
    try {
      outcome = gs ? engine.game_outcome(gs) : IN_PROGRESS
      const waitingForMyCards = gs && actorOf(gs) === ME && unknownCount(handOf(gs, ME)) > 0
      legal = gs && outcome.status === 'in_progress' && !waitingForMyCards ? engine.legal_actions(gs) : []
    } catch (e) {
      legal = []
      outcome = IN_PROGRESS
      notice = { kind: 'error', text: `This position cannot be continued: ${readable(e)}` }
    }
  }

  /**
   * Make `next` the current position. `step` describes the move that led to it
   * ({ actor, action, hint }) and adds it to the recording of the game.
   */
  function commit(next, { snapshot = false, line = '', step = null } = {}) {
    cancelPending()
    if (snapshot && gs) history = [...history.slice(-(MAX_HISTORY - 1)), { gs, log, rec: rec ? rec.steps.length : undefined }]
    gs = next
    if (line) log = [...log.slice(-(MAX_LOG - 1)), line]
    if (rec && step) rec = addStep(rec, { state: next, line, ...step })
    refresh()
    saveGame()
    if (outcome.status !== 'in_progress') storeRecording()
    schedule(advance, 0)
  }

  /** Put the recording of this game into the Games list (replacing an earlier save of it). */
  function storeRecording() {
    if (!rec || !rec.steps.length) return
    saveReplay(finalize(rec, outcome)).catch(e => console.warn('Could not save the game recording.', e))
  }

  function openGames() {
    gamesDialog?.showModal()
    gamesPanel?.refresh()
  }

  /** In a real game, fill in cards that can only be in one place. */
  function deduce(state) {
    try {
      return engine.all_cards_deducible(state) ? engine.deduce_cards(state) : state
    } catch (e) {
      console.warn('Card deduction skipped.', e)
      return state
    }
  }

  /** Apply a move for whoever is to move. Returns false (and explains) if the engine refuses it. */
  function perform(action, { snapshot = false, suffix = '' } = {}) {
    if (!gs) return false
    const who = nameOf(actorOf(gs))
    let next
    try {
      next = engine.apply_action(gs, action)
    } catch (e) {
      notice = { kind: 'error', text: `That move was not accepted: ${readable(e)}` }
      return false
    }
    if (!isPlay()) next = deduce(next)
    const actor = actorOf(gs)
    // For my own moves, remember what the search was saying when I chose.
    const advice = actor === ME && settings.showHints
      ? hintSnapshot(hint.summary, pickBest(hint.summary)?.key, actionKey(action), actionText)
      : null
    commit(next, { snapshot, line: actionLogText(action, who) + suffix, step: { actor, action, hint: advice } })
    return true
  }

  // ───────────────────────────────────────────────────────────────────────
  // Turn flow
  // ───────────────────────────────────────────────────────────────────────

  /** Decide what should happen now that the position is `gs`. */
  function advance() {
    if (!gs || outcome.status !== 'in_progress') return
    const who = actorOf(gs)

    if (who === ME) {
      if (unknownCount(handOf(gs, ME)) > 0) return
      if (isPlay() && legal.length === 1) {
        // Nothing to decide: play the forced move after a beat so it can be seen.
        const only = legal[0]
        schedule(() => perform(only, { suffix: ' (only legal move)' }), 700)
        return
      }
      startHints()
      return
    }

    if (!isPlay()) return // a real opponent: wait for their move to be entered
    if (settings.playerTypes[who] === PlayerType.Random) {
      schedule(playRandomMove, 600)
    } else {
      runAi(who)
    }
  }

  function playRandomMove() {
    let action
    try {
      action = engine.pick_random_action(gs)
    } catch (e) {
      notice = { kind: 'error', text: `Could not pick a move: ${readable(e)}` }
      return
    }
    if (action) perform(action)
  }

  function ensurePool() {
    if (!pool) {
      pool = new SolverPool(() => new Worker(new URL('./workers/solver.worker.js', import.meta.url), { type: 'module' }))
    }
    pool.resize(settings.numThreads)
  }

  /** Fold one batch into a tally. Returns a message if anything went wrong. */
  function absorb(tally, res) {
    let problem = null
    for (const r of res.results) {
      try {
        addResult(tally, r)
      } catch (e) {
        problem = `The solver returned an unreadable result (${e.message}).`
      }
    }
    if (res.errors.length) problem = friendlySolverError(res.errors[0])
    if (res.timedOut) problem = 'The search took too long and was stopped. Lower the search size in Settings.'
    return problem
  }

  /** Rank my legal moves. `more` continues the current search instead of restarting it. */
  async function startHints(more = false) {
    if (!settings.showHints || !gs || actorOf(gs) !== ME) return
    const at = epoch
    const run = ++hintRun
    const state = gs
    const moves = legal
    if (!more || !hintTally) hintTally = createTally()
    const tally = hintTally
    const stale = () => at !== epoch || run !== hintRun

    try {
      ensurePool()
    } catch (e) {
      hint = { status: 'error', summary: null, message: readable(e) }
      return
    }
    hint = { status: 'running', summary: summarize(tally, moves), message: '' }

    // Search in passes until the time budget runs out (0 = until stopped). Passes
    // start small so short budgets are kept, and grow while there is time.
    const limit = settings.hintThinkingMs
    const startedAt = performance.now()
    let rollouts = Math.min(500, settings.hintRollouts)
    for (;;) {
      const res = await pool.run({
        state,
        determinizations: settings.hintDeterminizations,
        rollouts,
        timeoutMs: 60000,
      })
      if (stale() || res.cancelled) return
      const problem = absorb(tally, res)
      const summary = tally.batches ? summarize(tally, moves) : null
      if (!res.results.length) {
        hint = { status: 'error', summary, message: problem || 'The search returned nothing.' }
        return
      }
      hint = { status: 'running', summary, message: problem || '' }
      if (limit > 0) {
        const remaining = limit - (performance.now() - startedAt)
        if (remaining < res.elapsedMs * 0.75) break
        if (res.elapsedMs < remaining / 3) rollouts = Math.min(settings.hintRollouts, rollouts * 2)
      } else {
        rollouts = Math.min(settings.hintRollouts, rollouts * 2)
      }
    }
    hint = { ...hint, status: 'done' }
  }

  /** Stop the suggestion search and keep what it found so far. */
  function stopHints() {
    if (hint.status !== 'running') return
    hintRun++
    pool?.cancel()
    hint = hint.summary?.totalVisits ? { ...hint, status: 'done' } : idleHint()
  }

  const fmtTime = ms => (ms === 0 ? 'no time limit' : `${ms / 1000} s`)

  /** Let a search opponent think for its time budget, then play its best move. */
  async function runAi(player) {
    const at = epoch
    const state = gs
    const moves = legal
    try {
      ensurePool()
    } catch (e) {
      ai = { status: 'failed', player, message: readable(e) }
      return
    }
    ai = { status: 'thinking', player, message: '' }

    const tally = createTally()
    const limit = settings.mctsThinkingTimeMs
    const startedAt = performance.now()
    let rollouts = 1000
    let problem = null

    for (;;) {
      const res = await pool.run({
        state,
        determinizations: settings.mctsDeterminizations,
        rollouts,
        timeoutMs: Math.max(15000, limit * 4),
      })
      if (at !== epoch || res.cancelled) return
      problem = absorb(tally, res) || problem
      if (!res.results.length) break
      const remaining = limit - (performance.now() - startedAt)
      if (remaining < res.elapsedMs * 0.75) break
      if (res.elapsedMs < remaining / 3) rollouts = Math.min(20000, rollouts * 2)
    }

    const best = pickBest(summarize(tally, moves))
    if (!best) {
      // Never substitute a random move silently: say what happened and let the player decide.
      ai = { status: 'failed', player, message: problem || 'The search did not evaluate any move.' }
      return
    }
    if (problem) console.warn(`Search for ${player} finished with a warning: ${problem}`)
    perform(best.action)
  }

  function retryAi() {
    if (ai.status === 'failed' && ai.player) runAi(ai.player)
  }

  function randomInsteadOfAi() {
    if (ai.status !== 'failed') return
    const who = nameOf(ai.player)
    let action
    try {
      action = engine.pick_random_action(gs)
    } catch (e) {
      notice = { kind: 'error', text: `Could not pick a move: ${readable(e)}` }
      return
    }
    if (action) perform(action, { suffix: ` (random move - ${who}'s search failed)` })
  }

  // ───────────────────────────────────────────────────────────────────────
  // Player actions
  // ───────────────────────────────────────────────────────────────────────

  /** I play a move. */
  function playMine(action) {
    if (!gs || over || actorOf(gs) !== ME || myUnknown > 0) return
    notice = null
    perform(action, { snapshot: true })
  }

  /** Clicking a card in my hand plays it when there is exactly one thing it can do. */
  function playCard(card) {
    const options = actionsByCard.get(cardKey(card)) || []
    if (options.length === 1) playMine(options[0])
  }

  /** Real game: record what an opponent did. */
  function enterOpponentMove(action) {
    if (!gs || over || isPlay() || actorOf(gs) === ME) return
    notice = null
    perform(action, { snapshot: true })
  }

  /** Real game: say which card I drew. */
  function revealDrawn(card) {
    if (!gs || isPlay()) return
    let next
    try {
      next = deduce(engine.reveal_card(gs, ME, wrap(card)))
    } catch (e) {
      notice = { kind: 'error', text: `That card was not accepted: ${readable(e)}` }
      return
    }
    notice = null
    commit(next, { snapshot: true, line: `You drew ${cardLabel(card)}`, step: { actor: ME } })
  }

  function undo() {
    if (!history.length) return
    const previous = history[history.length - 1]
    cancelPending()
    history = history.slice(0, -1)
    gs = previous.gs
    log = previous.log
    if (rec) rec = truncate(rec, previous.rec)
    notice = null
    refresh()
    saveGame()
    schedule(advance, 0)
  }

  function startGame() {
    if (!wasmReady) return
    const config = settingsToGameConfig(settings)
    let next
    try {
      next = isPlay()
        ? engine.new_computer_game({ seed: randomSeed() }, config)
        : engine.new_manual_game(
            {
              trump_card: wrap(setup.trump),
              player_hand: setup.hand.map(wrap),
              starting_player: seatIndex(setup.starter),
              opponent_trumps: [],
            },
            config,
          )
    } catch (e) {
      notice = { kind: 'error', text: `Could not start the game: ${readable(e)}` }
      return
    }
    notice = null
    history = []
    log = []
    rec = newRecording({
      mode: modeKey(),
      names: Object.fromEntries(seatIds().map(p => [p, nameOf(p)])),
      playerTypes: isPlay() ? Object.fromEntries(seatIds().filter(p => p !== ME).map(p => [p, settings.playerTypes[p]])) : null,
      initial: next,
    })
    const first = nameOf(actorOf(next))
    commit(next, { line: `New game. ${first === 'You' ? 'You attack' : `${first} attacks`} first.` })
  }

  /** Leave the current game and go back to the setup screen. */
  function abandonGame() {
    newGameDialog?.close()
    // A game left before the end is still kept, marked as unfinished.
    if (gs && outcome.status === 'in_progress') storeRecording()
    cancelPending()
    gs = null
    rec = null
    history = []
    log = []
    notice = null
    // A new game starts from a blank card selection.
    setup = cleanSetup({ starter: setup.starter })
    refresh()
    saveGame()
  }

  /** Unselect the trump and every chosen hand card. */
  function clearSetupCards() {
    setup = { ...setup, trump: null, hand: [] }
    saveGame()
  }

  function askNewGame() {
    if (gs && !over) newGameDialog?.showModal()
    else abandonGame()
  }

  function switchMode(toPlay) {
    if (settings.computerShuffle === toPlay) return
    saveGame()
    cancelPending()
    notice = null
    settings = sanitizeSettings({ ...settings, computerShuffle: toPlay })
    saveSettings(settings)
    loadGame()
    schedule(advance, 0)
  }

  /** Change settings; anything out of range is clamped by `sanitizeSettings`. */
  function update(patch, { restart = false } = {}) {
    settings = sanitizeSettings({ ...settings, ...patch })
    saveSettings(settings)
    if (!gs) {
      setup = cleanSetup(setup)
      saveGame()
    } else if (restart && !over) {
      // Search settings changed mid-turn: redo the current turn's search with them.
      cancelPending()
      schedule(advance, 0)
    }
  }

  const updateNumber = (key, restart = true) => e => update({ [key]: e.currentTarget.value }, { restart })

  function toggleHints() {
    update({ showHints: !settings.showHints })
    hintRun++
    hintTally = null
    hint = idleHint()
    if (settings.showHints) startHints()
    else if (!ai.player) pool?.cancel()
  }

  function toggleSetupCard(card) {
    const key = cardKey(card)
    if (setup.hand.some(c => cardKey(c) === key)) {
      setup = { ...setup, hand: setup.hand.filter(c => cardKey(c) !== key) }
    } else if (setup.hand.length < HAND_SIZE) {
      setup = { ...setup, hand: [...setup.hand, card] }
    }
    saveGame()
  }

  function chooseTrump(card) {
    setup = { ...setup, trump: card, hand: setup.hand.filter(c => cardKey(c) !== cardKey(card)) }
    saveGame()
  }

  function resetEverything() {
    cancelPending()
    try {
      for (const key of [...Object.values(STORE), ...Object.values(LEGACY_STORE), 'durak_game_settings']) {
        localStorage.removeItem(key)
      }
    } catch (e) {
      console.warn('Could not clear saved data.', e)
    }
    location.reload()
  }

  // ───────────────────────────────────────────────────────────────────────
  // Lifecycle
  // ───────────────────────────────────────────────────────────────────────

  onMount(async () => {
    try {
      await initWasm()
      wasmReady = true
    } catch (e) {
      fatal = `The game engine could not be loaded (${e?.message || e}). Reload the page; if that does not help, your browser may not support WebAssembly.`
      return
    }
    loadGame()
    schedule(advance, 0)
  })

  onDestroy(() => {
    if (pendingTimer !== null) clearTimeout(pendingTimer)
    epoch++
    pool?.dispose()
    pool = null
  })
</script>

<div class="flex min-h-screen flex-col">
  <a href="#main" class="sr-only focus:not-sr-only focus:absolute focus:left-2 focus:top-2 focus:z-50 focus:rounded-lg focus:bg-surface focus:px-3 focus:py-2">Skip to the game</a>

  <header class="border-b border-line bg-surface">
    <div class="mx-auto flex w-full max-w-6xl flex-wrap items-center gap-x-4 gap-y-2 px-3 py-3 sm:px-5">
      <div class="flex items-center gap-2">
        <span class="flex h-9 w-9 items-center justify-center rounded-xl bg-felt text-xl text-on-felt" aria-hidden="true">♠</span>
        <h1 class="font-display text-xl font-bold leading-none">Durak AI</h1>
      </div>

      <div class="seg" role="radiogroup" aria-label="What would you like to do?">
        <button type="button" class="seg-item" role="radio" aria-checked={playMode ? 'true' : 'false'} on:click={() => switchMode(true)}>Play the computer</button>
        <button type="button" class="seg-item" role="radio" aria-checked={playMode ? 'false' : 'true'} on:click={() => switchMode(false)}>Analyse a real game</button>
      </div>

      <div class="ml-auto flex items-center gap-2">
        <button type="button" class="btn btn-sm" on:click={openGames}>
          <span aria-hidden="true">▤</span> Games
        </button>
        <button type="button" class="btn btn-sm" on:click={() => { confirmReset = false; settingsDialog?.showModal() }}>
          <span aria-hidden="true">⚙</span> Settings
        </button>
        <ThemeToggle />
      </div>
    </div>
  </header>

  <main id="main" class="mx-auto w-full max-w-6xl flex-1 px-3 py-4 sm:px-5 sm:py-6">
    {#if fatal}
      <div class="notice notice-error" role="alert"><span aria-hidden="true">⚠</span><p class="m-0"><strong>Something went wrong.</strong> {fatal}</p></div>
    {:else if !wasmReady}
      <p class="flex items-center gap-2 text-muted" role="status"><span class="spinner" aria-hidden="true"></span> Loading the game engine…</p>
    {:else}
      {#if notice}
        <div class="notice mb-4 {notice.kind === 'error' ? 'notice-error' : 'notice-info'}" role={notice.kind === 'error' ? 'alert' : 'status'}>
          <span aria-hidden="true">{notice.kind === 'error' ? '⚠' : 'ℹ'}</span>
          <p class="m-0 flex-1"><strong>{notice.kind === 'error' ? 'Problem: ' : 'Note: '}</strong>{notice.text}</p>
          <button type="button" class="btn btn-sm" on:click={() => (notice = null)}>Dismiss</button>
        </div>
      {/if}

      {#if !gs}
        <!-- ═══════════════ Setup ═══════════════ -->
        <section class="panel mx-auto max-w-3xl" aria-labelledby="setup-title">
          <p class="eyebrow">{playMode ? 'Play the computer' : 'Analyse a real game'}</p>
          <h2 id="setup-title" class="panel-title mt-1 text-2xl">New game</h2>
          <p class="mt-2 text-sm text-muted">
            {#if playMode}
              The computer deals. You play against one or two computer opponents, with optional move suggestions.
            {:else}
              Mirror a game you are playing elsewhere. Enter your cards and each move as it happens; the app suggests your moves.
            {/if}
          </p>

          <div class="mt-5 flex flex-wrap gap-x-8 gap-y-5">
            <div>
              <h3 class="field-label">Players</h3>
              <PillChoice label="Players at the table" value={settings.numPlayers}
                options={PLAYER_COUNTS.map(n => ({ value: n, text: String(n), hint: n === 2 ? '1 v 1' : 'You and two opponents' }))}
                on:change={e => update({ numPlayers: e.detail })} />
            </div>
            <div>
              <h3 class="field-label">Card deck</h3>
              <PillChoice label="Card deck" value={settings.deckSize}
                options={DECK_SIZES.map(d => ({ value: d.value, text: String(d.value), hint: d.label }))}
                on:change={e => update({ deckSize: e.detail })} />
              <p class="field-help">{DECK_SIZES.find(d => d.value === settings.deckSize)?.label}</p>
            </div>
          </div>

          <div class="mt-5">
            <h3 class="field-label">Game modes</h3>
            <div class="flex flex-wrap gap-3">
              <ModeTile label="Passing the attack on" value={settings.reflecting}
                options={[
                  { value: false, title: 'throw-in', icon: 'throw-in', hint: 'No passing: the defender must beat the cards or take them' },
                  { value: true, title: 'passing', icon: 'passing', hint: 'The defender may pass the attack on with a card of the same rank' },
                ]}
                on:change={e => update({ reflecting: e.detail, trumpReflecting: e.detail && settings.trumpReflecting })} />
              <ModeTile label="End of the game" value={settings.classicEnding}
                options={[
                  { value: true, title: 'classic', icon: 'classic', hint: 'No draws: if the last two players run out together, the defender loses' },
                  { value: false, title: 'draw', icon: 'draw', hint: 'If the last two players run out together, the game is a draw' },
                ]}
                on:change={e => update({ classicEnding: e.detail })} />
            </div>
            <ul class="m-0 mt-3 list-none space-y-1 p-0 text-xs text-muted">
              <li><strong class="text-ink">{settings.reflecting ? 'passing' : 'throw-in'}:</strong>
                {settings.reflecting
                  ? `before beating anything, the defender may play a card of the same rank to pass the attack on${settings.numPlayers === 2 ? ' - with two players, back to the attacker' : ' to the next player'}.`
                  : 'the defender must beat the cards or take them; others may throw in cards of ranks already on the table.'}</li>
              <li><strong class="text-ink">{settings.classicEnding ? 'classic' : 'draw'}:</strong>
                {settings.classicEnding
                  ? 'no draws - if the defender beats the final attack with their last card, the defender still loses: the attacker got out first.'
                  : 'if the defender beats the final attack with their last card, the game is a draw.'}</li>
            </ul>
          </div>

          <fieldset class="mt-5 rounded-xl border border-line p-3">
            <legend class="px-1 text-sm font-semibold">More rules</legend>
            <label class="flex min-h-[44px] cursor-pointer items-start gap-3 py-1">
              <input type="checkbox" class="mt-1 h-5 w-5 accent-[rgb(var(--accent))]" checked={settings.firstTrickLimit} on:change={e => update({ firstTrickLimit: e.currentTarget.checked })} />
              <span><span class="block text-sm font-semibold">Five cards before the first discard</span>
                <span class="block text-xs text-muted">Until cards have been discarded once, a trick holds at most 5 attacking cards; after that, 6. Durak Online plays this way.</span></span>
            </label>
            {#if settings.reflecting}
              <label class="flex min-h-[44px] cursor-pointer items-start gap-3 py-1">
                <input type="checkbox" class="mt-1 h-5 w-5 accent-[rgb(var(--accent))]" checked={settings.trumpReflecting} on:change={e => update({ trumpReflecting: e.currentTarget.checked })} />
                <span><span class="block text-sm font-semibold">Also pass by showing a trump</span>
                  <span class="block text-xs text-muted">The defender may pass by showing the trump of that rank and keeping it, once per trump per trick. Not available in Durak Online - leave it off to match that app.</span></span>
              </label>
            {/if}
          </fieldset>

          <div class="mt-5">
            <span class="field-label">{opponents.length === 1 ? 'Your opponent' : 'Your opponents'}</span>
            <div class="grid gap-3 sm:grid-cols-2">
              {#each opponents as pid, i}
                <div class="rounded-xl border border-line p-3">
                  <label class="field-label" for="name-{pid}">Name{opponents.length > 1 ? ` (${i === 0 ? 'plays after you' : 'plays before you'})` : ''}</label>
                  <input id="name-{pid}" class="field" type="text" maxlength="20" value={settings.playerNames[pid]} on:change={e => update({ playerNames: { ...settings.playerNames, [pid]: e.currentTarget.value } })} />
                  {#if playMode}
                    <label class="field-label mt-3" for="type-{pid}">How it plays</label>
                    <select id="type-{pid}" class="field" value={settings.playerTypes[pid]} on:change={e => update({ playerTypes: { ...settings.playerTypes, [pid]: e.currentTarget.value } })}>
                      {#each AI_TYPES as t}<option value={t.value}>{t.label} - {t.hint.toLowerCase()}</option>{/each}
                    </select>
                  {/if}
                </div>
              {/each}
            </div>
            <div class="mt-3 flex flex-wrap gap-x-6 gap-y-3">
              <div class="w-full max-w-xs">
                <ThinkTime id="hint-think" label="Suggestion thinking time" allowUnlimited value={settings.hintThinkingMs}
                  max={LIMITS.hintThinkingMs[1]} on:change={e => update({ hintThinkingMs: e.detail })} />
                <p class="field-help">How long your move suggestions search. Unlimited keeps searching until you stop it or play.</p>
              </div>
              {#if playMode && anySearchOpponent}
                <div class="w-full max-w-xs">
                  <ThinkTime id="think" label="Computer thinking time" value={settings.mctsThinkingTimeMs}
                    min={LIMITS.mctsThinkingTimeMs[0]} max={LIMITS.mctsThinkingTimeMs[1]} on:change={e => update({ mctsThinkingTimeMs: e.detail })} />
                  <p class="field-help">Per computer move. Longer usually plays better.</p>
                </div>
              {/if}
            </div>
          </div>

          {#if !playMode}
            <ol class="m-0 mt-6 list-none space-y-5 p-0">
              <li>
                <h3 class="text-base font-bold"><span class="mr-2 inline-flex h-6 w-6 items-center justify-center rounded-full bg-accent text-xs text-on-accent">1</span>Trump card</h3>
                <p class="mt-1 text-sm text-muted">The face-up card under the stock.{#if setup.trump} Chosen: <strong class="text-ink">{cardLabel(setup.trump)}</strong> - trump suit {suitSymbol(setup.trump.suit)} {setup.trump.suit}.{/if}</p>
                <div class="mt-2 flex flex-wrap gap-1.5" role="group" aria-label="Choose the trump card">
                  {#each setupDeck as c (cardKey(c))}
                    <Card card={c} size="sm" selectable selected={!!setup.trump && cardKey(setup.trump) === cardKey(c)} on:click={() => chooseTrump(c)} />
                  {/each}
                </div>
              </li>
              <li>
                <h3 class="text-base font-bold"><span class="mr-2 inline-flex h-6 w-6 items-center justify-center rounded-full bg-accent text-xs text-on-accent">2</span>Your six cards</h3>
                <p class="mt-1 text-sm text-muted" role="status">{setup.hand.length} of {HAND_SIZE} chosen. Select a card again to remove it.</p>
                <div class="mt-2 flex flex-wrap gap-1.5" role="group" aria-label="Choose your six cards">
                  {#each setupDeck.filter(c => !setup.trump || cardKey(c) !== cardKey(setup.trump)) as c (cardKey(c))}
                    {@const chosen = setup.hand.some(h => cardKey(h) === cardKey(c))}
                    <Card card={c} size="sm" selectable selected={chosen} dim={!chosen && setup.hand.length >= HAND_SIZE} on:click={() => toggleSetupCard(c)} />
                  {/each}
                </div>
              </li>
              <li>
                <h3 class="text-base font-bold" id="starter-label"><span class="mr-2 inline-flex h-6 w-6 items-center justify-center rounded-full bg-accent text-xs text-on-accent">3</span>Who attacks first?</h3>
                <div class="seg mt-2" role="radiogroup" aria-labelledby="starter-label">
                  {#each seats as pid}
                    <button type="button" class="seg-item" role="radio" aria-checked={setup.starter === pid ? 'true' : 'false'} on:click={() => { setup = { ...setup, starter: pid }; saveGame() }}>{pid === ME ? 'Me' : nameOf(pid)}</button>
                  {/each}
                </div>
              </li>
            </ol>
          {/if}

          <div class="mt-6 flex flex-wrap items-center gap-3 border-t border-line pt-4">
            <button type="button" class="btn btn-primary px-6" disabled={!playMode && !setupReady} on:click={startGame}>{playMode ? 'Deal the cards' : 'Start tracking'}</button>
            {#if !playMode}
              <button type="button" class="btn" disabled={!setup.trump && setup.hand.length === 0} on:click={clearSetupCards}>Reset cards</button>
            {/if}
            {#if playMode}
              <p class="m-0 text-sm text-muted">Whoever holds the lowest trump attacks first.</p>
            {:else if !setupReady}
              <p class="m-0 text-sm text-muted">{!setup.trump ? 'Choose the trump card to continue.' : `Choose ${HAND_SIZE - setup.hand.length} more of your cards.`}</p>
            {/if}
          </div>
        </section>
      {:else}
        <!-- ═══════════════ Game ═══════════════ -->
        <div class="mb-4 flex flex-wrap items-center gap-3 rounded-2xl border px-4 py-3 {myTurn ? 'border-gold bg-gold/10' : 'border-line bg-surface'}">
          <p class="m-0 flex-1 text-base font-semibold" role="status" aria-live="polite">
            {#if ai.status === 'thinking'}<span class="spinner mr-2 align-[-2px]" aria-hidden="true"></span>{/if}{statusText}
          </p>
          <div class="flex gap-2">
            <button type="button" class="btn btn-sm" disabled={!history.length} on:click={undo}>↶ Undo{history.length ? ` (${history.length})` : ''}</button>
            <button type="button" class="btn btn-sm" on:click={askNewGame}>New game</button>
          </div>
        </div>

        <div class="grid gap-4 lg:grid-cols-[minmax(0,1fr)_22rem]">
          <!-- Table column -->
          <div class="min-w-0 space-y-4">
            <div class="grid gap-3 {opponents.length > 1 ? 'sm:grid-cols-2' : ''}">
              {#each opponents as pid}
                {@const hand = handOf(gs, pid)}
                {@const showAll = playMode && (revealOpponents || over)}
                {@const faceUp = sortCards(hand.filter(c => !isUnknown(c) && (showAll || !playMode || isPublic(c))).map(c => ({ ...parseCard(c), seen: isPublic(c) })), gs.trump)}
                {@const hidden = hand.length - faceUp.length}
                <Seat name={nameOf(pid)} kind={kindOf(pid)} role={roleOf(gs, pid)} toMove={!over && actor === pid} out={isOut(gs, pid)} cardCount={hand.length}
                  note={faceUp.length && hidden ? `${faceUp.length} known` : ''}>
                  <div class="flex min-h-[3.9rem] flex-wrap items-end gap-1">
                    {#each faceUp as c (cardKey(c))}<Card card={c} size="sm" tag={c.seen && !showAll ? 'seen' : ''} />{/each}
                    {#if hidden > 0}
                      <span class="flex" role="img" aria-label="{hidden} hidden {hidden === 1 ? 'card' : 'cards'}">
                        {#each Array(Math.min(hidden, 12)) as _, i}<span class={i ? '-ml-7 sm:-ml-8' : ''} aria-hidden="true"><Card faceDown size="sm" label="" /></span>{/each}
                      </span>
                      {#if hidden > 12}<span class="self-center text-sm text-muted">+{hidden - 12} more</span>{/if}
                    {/if}
                  </div>
                </Seat>
              {/each}
            </div>

            <section class="felt p-4" aria-label="Table">
              <div class="flex flex-wrap items-start gap-4">
                <div class="min-w-0 flex-1">
                  <h2 class="text-xs font-semibold uppercase tracking-wider text-on-felt/80">On the table</h2>
                  {#if gs.table.length}
                    <ul class="m-0 mt-3 flex list-none flex-wrap gap-x-9 gap-y-8 p-0 pb-6 pr-6">
                      {#each gs.table as p, i (i)}
                        {@const a = parseCard(p.attack)}
                        {@const d = parseCard(p.defense)}
                        <li class="relative" aria-label="{cardLabel(a)} {d ? `beaten by ${cardLabel(d)}` : 'not beaten yet'}">
                          <Card card={a} size="md" />
                          {#if d}<span class="absolute left-5 top-5 sm:left-6 sm:top-6"><Card card={d} size="md" /></span>{/if}
                        </li>
                      {/each}
                    </ul>
                  {:else}
                    <p class="m-0 mt-3 text-sm text-on-felt/80">No cards on the table.</p>
                  {/if}
                </div>
                <div class="flex shrink-0 items-center gap-3 rounded-xl bg-black/20 p-3">
                  {#if trumpCard}<Card card={trumpCard} size="sm" label="Trump card {cardLabel(trumpCard)}" />{/if}
                  <dl class="m-0 text-sm">
                    <dt class="text-xs uppercase tracking-wider text-on-felt/80">Trump</dt>
                    <dd class="m-0 text-lg font-bold">{suitSymbol(gs.trump)} {gs.trump}</dd>
                    <dt class="mt-1 text-xs uppercase tracking-wider text-on-felt/80">Stock</dt>
                    <dd class="m-0 font-semibold tabular-nums">{gs.stock.length ? `${gs.stock.length} left` : 'Empty'}</dd>
                  </dl>
                </div>
              </div>
            </section>

            <Seat name="You" you role={roleOf(gs, ME)} toMove={myTurn} out={isOut(gs, ME)} cardCount={handOf(gs, ME).length}>
              {#if myUnknown > 0}
                <div class="mb-3 rounded-xl border border-gold bg-gold/10 p-3">
                  <h3 class="text-sm font-bold">You drew {myUnknown} {myUnknown === 1 ? 'card' : 'cards'} - which {myUnknown === 1 ? 'one' : 'ones'}?</h3>
                  <p class="m-0 mt-1 text-xs text-muted">Pick {myUnknown === 1 ? 'it' : 'them one at a time'} from the cards not seen yet.</p>
                  <div class="mt-2 flex max-h-56 flex-wrap gap-1.5 overflow-y-auto p-1" role="group" aria-label="Cards you may have drawn">
                    {#each deckCards(gs.config.deck_size, gs.trump).filter(c => !knownCards(gs).some(k => cardKey(k) === cardKey(c))) as c (cardKey(c))}
                      <Card card={c} size="sm" selectable on:click={() => revealDrawn(c)} />
                    {/each}
                  </div>
                </div>
              {/if}
              <div class="flex flex-wrap gap-1.5 sm:gap-2">
                {#each myCards as c (cardKey(c))}
                  {@const options = myTurn && myUnknown === 0 ? actionsByCard.get(cardKey(c)) || [] : []}
                  {@const isBest = options.some(a => actionKey(a) === bestKey)}
                  <Card card={c} size="lg" selectable={options.length === 1} dim={myTurn && myUnknown === 0 && !options.length} best={isBest}
                    tag={options.length > 1 ? `${options.length} options` : ''}
                    label={options.length === 1 ? actionText(options[0]) : options.length > 1 ? `${cardLabel(c)}: choose how to play it in the move list` : cardLabel(c)}
                    on:click={() => playCard(c)} />
                {/each}
                {#each Array(myUnknown) as _}<Card faceDown size="lg" label="A card you drew, not identified yet" />{/each}
                {#if !myCards.length && !myUnknown}<p class="m-0 text-sm text-muted">No cards.</p>{/if}
              </div>
              {#if myTurn && myUnknown === 0 && nonCardActions.length}
                <div class="mt-3 flex flex-wrap gap-2">
                  {#each nonCardActions as a (actionKey(a))}
                    <button type="button" class="btn {actionKey(a) === bestKey ? 'border-gold bg-gold/10' : ''}" on:click={() => playMine(a)}>
                      {actionText(a)}{#if actionKey(a) === bestKey}<span class="chip chip-turn">★ Suggested</span>{/if}
                    </button>
                  {/each}
                </div>
              {/if}
            </Seat>
          </div>

          <!-- Side column -->
          <div class="min-w-0 space-y-4">
            {#if over}
              <section class="panel" aria-labelledby="over-title">
                <h2 id="over-title" class="panel-title">Game over</h2>
                <p class="mt-2 text-sm">{statusText.replace('Game over: ', '')}</p>
                <button type="button" class="btn btn-primary mt-4 w-full" on:click={abandonGame}>New game</button>
              </section>
            {:else if myTurn && myUnknown === 0}
              <section class="panel" aria-labelledby="moves-title">
                <div class="flex items-center justify-between gap-2">
                  <h2 id="moves-title" class="panel-title">Your moves</h2>
                  <button type="button" class="btn btn-sm" aria-pressed={settings.showHints ? 'true' : 'false'} on:click={toggleHints}>Suggestions: {settings.showHints ? 'on' : 'off'}</button>
                </div>

                {#if settings.showHints}
                  <p class="mt-2 flex items-center gap-2 text-xs text-muted" role="status">
                    {#if hint.status === 'running'}<span class="spinner" aria-hidden="true"></span> Searching ({fmtTime(settings.hintThinkingMs)})… {hint.summary?.totalVisits ? `${hint.summary.totalVisits.toLocaleString('en')} playouts so far` : ''}
                    {:else if hint.status === 'done'}Search finished: {hint.summary.totalVisits.toLocaleString('en')} playouts over {hint.summary.determinizations.toLocaleString('en')} possible deals.
                    {:else if hint.status === 'error'}Suggestions are unavailable for this move.
                    {:else}Waiting to search.{/if}
                  </p>
                  {#if hint.message}
                    <div class="notice notice-error mt-2" role="alert"><span aria-hidden="true">⚠</span><p class="m-0">{hint.message}</p></div>
                  {/if}
                {/if}

                <ul class="m-0 mt-3 list-none space-y-2 p-0">
                  {#each (hint.summary?.actions.map(a => a.action) || legal) as a (actionKey(a))}
                    <li><MoveRow action={a} stat={stats.get(actionKey(a))} best={actionKey(a) === bestKey} on:click={() => playMine(a)} /></li>
                  {/each}
                </ul>

                {#if settings.showHints}
                  {#if hint.status === 'running'}
                    <button type="button" class="btn btn-sm mt-3" on:click={stopHints}>Stop searching</button>
                  {:else if hint.status === 'done' || hint.status === 'error'}
                    <button type="button" class="btn btn-sm mt-3" on:click={() => startHints(hint.status === 'done')}>{hint.status === 'done' ? 'Search longer' : 'Try again'}</button>
                  {/if}
                  <details class="mt-3 text-xs text-muted">
                    <summary class="cursor-pointer font-semibold text-ink">What do the numbers mean?</summary>
                    <p class="mb-0 mt-2">The percentage is a <strong>score</strong>: across many simulated games from this position, how often you did not end up as the durak after that move. The hidden cards are dealt at random and the simulations play randomly past the first few moves, so treat it as a guide for comparing moves, not as an exact probability. The small number is how many simulations (playouts) back the score; "rough" marks scores built on few of them.</p>
                    {#if hint.summary?.totalCutoffs}<p class="mb-0 mt-2">{hint.summary.totalCutoffs.toLocaleString('en')} playouts were cut off before the game ended and count as half.</p>{/if}
                  </details>
                {/if}
              </section>
            {:else if myTurn}
              <section class="panel"><h2 class="panel-title">Your moves</h2><p class="mt-2 text-sm text-muted">Identify the cards you drew (above your hand) to see your moves.</p></section>
            {:else if !playMode}
              <section class="panel" aria-labelledby="enter-title">
                <h2 id="enter-title" class="panel-title">What did {nameOf(actor)} do?</h2>
                <p class="mt-1 text-xs text-muted">Only moves that are possible right now are offered.</p>
                {#if nonCardActions.length}
                  <div class="mt-3 flex flex-wrap gap-2">
                    {#each nonCardActions as a (actionKey(a))}<button type="button" class="btn" on:click={() => enterOpponentMove(a)}>{actionText(a).replace('Take the cards', 'Took the cards').replace('Pass', 'Passed').replace('Done throwing in', 'Finished throwing in')}</button>{/each}
                  </div>
                {/if}
                {#each ACTION_GROUPS as group}
                  {@const acts = legal.filter(a => a.type === group.type && actionCard(a))}
                  {#if acts.length}
                    <h3 class="mt-4 text-sm font-semibold">{group.label}</h3>
                    <div class="mt-2 flex max-h-64 flex-wrap gap-1.5 overflow-y-auto p-1" role="group" aria-label={group.label}>
                      {#each sortCards(acts.map(a => ({ ...actionCard(a), action: a })), gs.trump) as c (cardKey(c))}
                        <Card card={c} size="sm" selectable label="{nameOf(actor)}: {actionText(c.action)}" on:click={() => enterOpponentMove(c.action)} />
                      {/each}
                    </div>
                  {/if}
                {/each}
              </section>
            {:else if ai.status === 'failed'}
              <section class="panel" aria-labelledby="ai-fail-title">
                <h2 id="ai-fail-title" class="panel-title">{nameOf(ai.player)} could not choose a move</h2>
                <div class="notice notice-error mt-3" role="alert"><span aria-hidden="true">⚠</span><p class="m-0">{ai.message}</p></div>
                <p class="mt-3 text-sm text-muted">Nothing has been played. You can retry the search, or let a random legal move be played instead.</p>
                <div class="mt-3 flex flex-wrap gap-2">
                  <button type="button" class="btn btn-primary" on:click={retryAi}>Try again</button>
                  <button type="button" class="btn" on:click={randomInsteadOfAi}>Play a random move</button>
                </div>
              </section>
            {:else}
              <section class="panel">
                <h2 class="panel-title">{nameOf(actor)}'s turn</h2>
                <p class="mt-2 flex items-center gap-2 text-sm text-muted"><span class="spinner" aria-hidden="true"></span>{settings.playerTypes[actor] === PlayerType.MCTS ? `Searching for up to ${settings.mctsThinkingTimeMs / 1000} s…` : 'Choosing a move…'}</p>
              </section>
            {/if}

            {#if playMode && !over}
              <label class="flex min-h-[44px] cursor-pointer items-center gap-3 rounded-xl border border-line bg-surface px-3 text-sm">
                <input type="checkbox" class="h-5 w-5 accent-[rgb(var(--accent))]" bind:checked={revealOpponents} />
                Show opponents' cards (practice)
              </label>
            {/if}

            <section class="panel" aria-labelledby="log-title">
              <h2 id="log-title" class="panel-title">Moves so far</h2>
              {#if log.length}
                <ol class="m-0 mt-2 max-h-56 list-none space-y-1 overflow-y-auto p-0 text-sm" aria-live="polite" aria-relevant="additions">
                  {#each [...log].reverse() as line, i (log.length - i)}<li class={i === 0 ? 'font-semibold' : 'text-muted'}>{line}</li>{/each}
                </ol>
              {:else}
                <p class="mt-2 text-sm text-muted">Nothing yet.</p>
              {/if}
            </section>
          </div>
        </div>
      {/if}
    {/if}
  </main>

  <footer class="border-t border-line px-4 py-4 text-center text-xs text-muted">
    <p class="m-0">Durak AI · for 2 or 3 players · based on <a class="underline" href="https://github.com/jorisperrenet/durak-online">durak-online</a> by Joris Perrenet (MIT licence)</p>
  </footer>
</div>

<!-- ═══════════════ Dialogs ═══════════════ -->
<dialog class="sheet" bind:this={newGameDialog} aria-labelledby="ng-title">
  <div class="p-5">
    <h2 id="ng-title" class="panel-title">Leave this game?</h2>
    <p class="mt-2 text-sm text-muted">The game in progress will be discarded and cannot be continued.</p>
    <div class="mt-5 flex flex-wrap justify-end gap-2">
      <button type="button" class="btn" on:click={() => newGameDialog.close()}>Keep playing</button>
      <button type="button" class="btn btn-danger" on:click={abandonGame}>Discard and start new</button>
    </div>
  </div>
</dialog>

<dialog class="sheet sheet-wide" bind:this={gamesDialog} aria-labelledby="gm-title">
  <div class="p-5">
    <div class="flex items-center justify-between gap-3">
      <h2 id="gm-title" class="panel-title text-xl">My games</h2>
      <button type="button" class="btn btn-sm" on:click={() => gamesDialog.close()}>Close</button>
    </div>
    <p class="mt-1 text-xs text-muted">Every game is recorded automatically and kept in this browser only. If the app got a result wrong, set the real one - the statistics use it.</p>
    <GamesPanel bind:this={gamesPanel} />
  </div>
</dialog>

<dialog class="sheet" bind:this={settingsDialog} aria-labelledby="st-title">
  <div class="p-5">
    <div class="flex items-center justify-between gap-3">
      <h2 id="st-title" class="panel-title text-xl">Settings</h2>
      <button type="button" class="btn btn-sm" on:click={() => settingsDialog.close()}>Close</button>
    </div>
    <p class="mt-1 text-xs text-muted">Changes apply immediately and are remembered. Values outside the allowed range are adjusted.</p>

    <div class="mt-4 grid gap-4 sm:grid-cols-2">
      <div>
        <label class="field-label" for="s-threads">Search threads</label>
        <input id="s-threads" class="field" type="number" inputmode="numeric" min="1" max={maxThreads()} value={settings.numThreads} on:change={updateNumber('numThreads')} />
        <p class="field-help">1 to {maxThreads()} on this device. More is faster but uses more memory.</p>
      </div>
      <div>
        <ThinkTime id="s-hint-time" label="Suggestion thinking time" allowUnlimited value={settings.hintThinkingMs}
          max={LIMITS.hintThinkingMs[1]} on:change={e => update({ hintThinkingMs: e.detail })} />
        <p class="field-help">Used from the next search.</p>
      </div>
      <div>
        <ThinkTime id="s-time" label="Computer thinking time" value={settings.mctsThinkingTimeMs}
          min={LIMITS.mctsThinkingTimeMs[0]} max={LIMITS.mctsThinkingTimeMs[1]} on:change={e => update({ mctsThinkingTimeMs: e.detail })} />
        <p class="field-help">Used from the next computer move.</p>
      </div>
      <div>
        <label class="field-label" for="s-hd">Suggestions: deals per pass</label>
        <input id="s-hd" class="field" type="number" inputmode="numeric" min={LIMITS.hintDeterminizations[0]} max={LIMITS.hintDeterminizations[1]} value={settings.hintDeterminizations} on:change={updateNumber('hintDeterminizations')} />
        <p class="field-help">How many random deals of the hidden cards to try ({LIMITS.hintDeterminizations[0]} to {LIMITS.hintDeterminizations[1]}).</p>
      </div>
      <div>
        <label class="field-label" for="s-hr">Suggestions: max playouts per deal</label>
        <input id="s-hr" class="field" type="number" inputmode="numeric" min={LIMITS.hintRollouts[0]} max={LIMITS.hintRollouts[1]} step="100" value={settings.hintRollouts} on:change={updateNumber('hintRollouts')} />
        <p class="field-help">{LIMITS.hintRollouts[0]} to {LIMITS.hintRollouts[1].toLocaleString('en')}. Higher is steadier but slower.</p>
      </div>
      <div>
        <label class="field-label" for="s-md">Computer: deals per pass</label>
        <input id="s-md" class="field" type="number" inputmode="numeric" min={LIMITS.mctsDeterminizations[0]} max={LIMITS.mctsDeterminizations[1]} value={settings.mctsDeterminizations} on:change={updateNumber('mctsDeterminizations', false)} />
        <p class="field-help">{LIMITS.mctsDeterminizations[0]} to {LIMITS.mctsDeterminizations[1]}.</p>
      </div>
    </div>

    <div class="mt-5 border-t border-line pt-4">
      <h3 class="text-sm font-bold">Saved data</h3>
      <p class="mt-1 text-xs text-muted">Games and settings are stored only in this browser. Erasing here removes the game in progress and your settings; recorded games stay in Games, where they can be deleted separately.</p>
      {#if confirmReset}
        <div class="mt-2 flex flex-wrap items-center gap-2" role="alert">
          <span class="text-sm">Erase the games in progress and all settings?</span>
          <button type="button" class="btn btn-sm btn-danger" on:click={resetEverything}>Yes, erase everything</button>
          <button type="button" class="btn btn-sm" on:click={() => (confirmReset = false)}>Cancel</button>
        </div>
      {:else}
        <button type="button" class="btn btn-sm btn-danger mt-2" on:click={() => (confirmReset = true)}>Erase games in progress and settings…</button>
      {/if}
    </div>
  </div>
</dialog>
