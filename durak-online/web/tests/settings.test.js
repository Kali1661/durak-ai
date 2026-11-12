import test from 'node:test'
import assert from 'node:assert/strict'

import { DEFAULT_SETTINGS, LIMITS, PlayerType, clampInt, sanitizeSettings, settingsToGameConfig } from '../src/lib/settings.js'

test('garbage input yields complete default settings', () => {
  for (const junk of [null, undefined, 42, 'x', [], { numPlayers: 'many' }]) {
    const s = sanitizeSettings(junk)
    assert.equal(s.numPlayers, 2)
    assert.equal(s.deckSize, 36)
    assert.ok(s.numThreads >= 1)
    assert.deepEqual(Object.keys(s).sort(), Object.keys(DEFAULT_SETTINGS).sort())
  }
})

test('only two or three players survive, including old 4-6 player saves', () => {
  for (const n of [0, 1, 4, 5, 6, 2.5, '3', null]) {
    assert.equal(sanitizeSettings({ numPlayers: n }).numPlayers, 2, `numPlayers ${n}`)
  }
  assert.equal(sanitizeSettings({ numPlayers: 3 }).numPlayers, 3)
})

test('unsupported deck sizes fall back to 36', () => {
  for (const old of [32, 40, 44, 48, 20]) assert.equal(sanitizeSettings({ deckSize: old }).deckSize, 36, `deck ${old}`)
  assert.equal(sanitizeSettings({ deckSize: 24 }).deckSize, 24)
  assert.equal(sanitizeSettings({ deckSize: '52' }).deckSize, 36)
  assert.equal(sanitizeSettings({ deckSize: 52 }).deckSize, 52)
})

test('numeric settings are clamped so a solve can never be empty or enormous', () => {
  const low = sanitizeSettings({
    numThreads: 0, mctsThinkingTimeMs: 0, mctsDeterminizations: 0, hintDeterminizations: -5, hintRollouts: 0,
  })
  assert.equal(low.numThreads, 1)
  assert.equal(low.mctsThinkingTimeMs, LIMITS.mctsThinkingTimeMs[0])
  assert.equal(low.mctsDeterminizations, 1)
  assert.equal(low.hintDeterminizations, 1)
  assert.equal(low.hintRollouts, LIMITS.hintRollouts[0])

  const high = sanitizeSettings({
    numThreads: 9999, mctsThinkingTimeMs: 1e12, mctsDeterminizations: 1e9, hintDeterminizations: 1e9, hintRollouts: 1e9,
  })
  assert.ok(high.numThreads <= LIMITS.numThreads[1])
  assert.equal(high.mctsThinkingTimeMs, LIMITS.mctsThinkingTimeMs[1])
  assert.equal(high.mctsDeterminizations, LIMITS.mctsDeterminizations[1])
  assert.equal(high.hintDeterminizations, LIMITS.hintDeterminizations[1])
  assert.equal(high.hintRollouts, LIMITS.hintRollouts[1])

  const junk = sanitizeSettings({ numThreads: NaN, hintRollouts: 'lots', mctsThinkingTimeMs: Infinity })
  assert.equal(junk.hintRollouts, DEFAULT_SETTINGS.hintRollouts)
  assert.equal(junk.mctsThinkingTimeMs, DEFAULT_SETTINGS.mctsThinkingTimeMs)
  assert.ok(junk.numThreads >= 1)
})

test('clampInt rounds, clamps and falls back', () => {
  assert.equal(clampInt('12', [1, 20], 5), 12)
  assert.equal(clampInt(3.6, [1, 20], 5), 4)
  assert.equal(clampInt(99, [1, 20], 5), 20)
  assert.equal(clampInt(-1, [1, 20], 5), 1)
  assert.equal(clampInt('abc', [1, 20], 5), 5)
  assert.equal(clampInt(undefined, [1, 20], 5), 5)
})

test('opponents are always computer players; unknown kinds become the default', () => {
  const s = sanitizeSettings({ playerTypes: { P0: 'Random', P1: 'Human', P2: 'Random', P5: 'MCTS' } })
  assert.deepEqual(s.playerTypes, { P1: PlayerType.MCTS, P2: PlayerType.Random })
})

test('names are trimmed, bounded, and old defaults are replaced', () => {
  const s = sanitizeSettings({ playerNames: { P1: '   Anna   Maria  ', P2: 'x'.repeat(80), P3: 'gone' } })
  assert.equal(s.playerNames.P1, 'Anna Maria')
  assert.equal(s.playerNames.P2.length, 20)
  assert.equal(s.playerNames.P3, undefined)
  const old = sanitizeSettings({ playerNames: { P1: 'Player 1', P2: '' } })
  assert.deepEqual(old.playerNames, { P1: 'Opponent 1', P2: 'Opponent 2' })
})

test('game config uses the engine field names', () => {
  const config = settingsToGameConfig(sanitizeSettings({ numPlayers: 3, deckSize: 52, reflecting: true }))
  assert.deepEqual(config, { deck_size: 52, num_players: 3, trump_reflecting: false, reflecting: true })
})

test('sanitising is idempotent', () => {
  const once = sanitizeSettings({ numPlayers: 3, hintRollouts: 123456, playerNames: { P1: 'Bo' } })
  assert.deepEqual(sanitizeSettings(once), once)
})
