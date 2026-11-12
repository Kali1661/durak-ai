// User settings: defaults, limits and sanitising.
//
// Everything read from localStorage goes through `sanitizeSettings`, so the
// rest of the app can rely on every value being present and in range.

const SETTINGS_KEY = 'durak_game_settings'
const SETTINGS_VERSION = 3

export const PlayerType = {
  Human: 'Human',
  Random: 'Random',
  MCTS: 'MCTS',
  Real: 'Real', // a person at a real table (analyse mode)
}

/** Opponent kinds offered when playing against the computer. */
export const AI_TYPES = [
  { value: PlayerType.MCTS, label: 'Search (MCTS)', hint: 'Thinks before each move' },
  { value: PlayerType.Random, label: 'Random', hint: 'Plays any legal move' },
]

export const PLAYER_COUNTS = [2, 3]

export const DECK_SIZES = [
  { value: 24, label: '24 cards (9 to ace)' },
  { value: 36, label: '36 cards (6 to ace)' },
  { value: 52, label: '52 cards (2 to ace)' },
]

/** Inclusive [min, max] for every numeric setting. */
export const LIMITS = {
  numThreads: [1, 16],
  mctsThinkingTimeMs: [250, 30000],
  mctsDeterminizations: [1, 200],
  hintDeterminizations: [1, 500],
  hintRollouts: [100, 50000],
}

export function maxThreads() {
  const cores = (typeof navigator !== 'undefined' && navigator.hardwareConcurrency) || 4
  return Math.max(1, Math.min(LIMITS.numThreads[1], cores))
}

export const DEFAULT_SETTINGS = Object.freeze({
  _version: SETTINGS_VERSION,
  deckSize: 36,
  numPlayers: 2,
  trumpReflecting: false,
  reflecting: false,
  numThreads: Math.max(1, Math.min(8, Math.floor(maxThreads() / 2) || 1)),
  playerNames: Object.freeze({ P1: 'Opponent 1', P2: 'Opponent 2' }),
  playerTypes: Object.freeze({ P1: PlayerType.MCTS, P2: PlayerType.MCTS }),
  // true = play against the computer, false = analyse a real game
  computerShuffle: false,
  mctsThinkingTimeMs: 2000,
  mctsDeterminizations: 16,
  showHints: true,
  hintDeterminizations: 24,
  hintRollouts: 2000,
})

/** Round to an integer inside [min, max]; fall back when not a number. */
export function clampInt(value, [min, max], fallback) {
  const n = typeof value === 'string' ? Number.parseInt(value, 10) : value
  if (typeof n !== 'number' || !Number.isFinite(n)) return fallback
  return Math.min(max, Math.max(min, Math.round(n)))
}

function cleanName(value, fallback) {
  if (typeof value !== 'string') return fallback
  const trimmed = value.replace(/\s+/g, ' ').trim().slice(0, 20)
  return trimmed || fallback
}

/**
 * Turn anything (old saves, hand-edited storage, garbage) into valid settings.
 * Unknown keys are dropped; out-of-range values are clamped; unsupported
 * choices fall back to the default.
 */
export function sanitizeSettings(raw) {
  const src = raw && typeof raw === 'object' && !Array.isArray(raw) ? raw : {}
  const d = DEFAULT_SETTINGS
  const bool = (v, fallback) => (typeof v === 'boolean' ? v : fallback)
  const names = src.playerNames && typeof src.playerNames === 'object' ? src.playerNames : {}
  const types = src.playerTypes && typeof src.playerTypes === 'object' ? src.playerTypes : {}
  const aiType = (v, fallback) => (v === PlayerType.MCTS || v === PlayerType.Random ? v : fallback)
  // Names from before version 3 were "Player 1"-style defaults; replace those.
  const legacyName = v => (typeof v === 'string' && /^Player \d$/.test(v) ? undefined : v)

  return {
    _version: SETTINGS_VERSION,
    deckSize: DECK_SIZES.some(x => x.value === src.deckSize) ? src.deckSize : d.deckSize,
    numPlayers: PLAYER_COUNTS.includes(src.numPlayers) ? src.numPlayers : d.numPlayers,
    trumpReflecting: bool(src.trumpReflecting, d.trumpReflecting),
    reflecting: bool(src.reflecting, d.reflecting),
    numThreads: clampInt(src.numThreads, [1, maxThreads()], Math.min(d.numThreads, maxThreads())),
    playerNames: {
      P1: cleanName(legacyName(names.P1), d.playerNames.P1),
      P2: cleanName(legacyName(names.P2), d.playerNames.P2),
    },
    playerTypes: {
      P1: aiType(types.P1, d.playerTypes.P1),
      P2: aiType(types.P2, d.playerTypes.P2),
    },
    computerShuffle: bool(src.computerShuffle, d.computerShuffle),
    mctsThinkingTimeMs: clampInt(src.mctsThinkingTimeMs, LIMITS.mctsThinkingTimeMs, d.mctsThinkingTimeMs),
    mctsDeterminizations: clampInt(src.mctsDeterminizations, LIMITS.mctsDeterminizations, d.mctsDeterminizations),
    showHints: bool(src.showHints, d.showHints),
    hintDeterminizations: clampInt(src.hintDeterminizations, LIMITS.hintDeterminizations, d.hintDeterminizations),
    hintRollouts: clampInt(src.hintRollouts, LIMITS.hintRollouts, d.hintRollouts),
  }
}

export function loadSettings() {
  try {
    const stored = localStorage.getItem(SETTINGS_KEY)
    return sanitizeSettings(stored ? JSON.parse(stored) : null)
  } catch (e) {
    console.warn('Could not read saved settings; using defaults.', e)
    return sanitizeSettings(null)
  }
}

export function saveSettings(settings) {
  try {
    localStorage.setItem(SETTINGS_KEY, JSON.stringify(sanitizeSettings(settings)))
  } catch (e) {
    console.warn('Could not save settings.', e)
  }
}

/** Engine config for a new game. */
export function settingsToGameConfig(settings) {
  return {
    deck_size: settings.deckSize,
    num_players: settings.numPlayers,
    trump_reflecting: settings.trumpReflecting,
    reflecting: settings.reflecting,
  }
}
