import test from 'node:test'
import assert from 'node:assert/strict'

import {
  addStep, computeStats, effectiveResult, finalize, hintSnapshot, isRecording, metaOf, newRecording, resultOf, truncate,
} from '../src/lib/replays.js'

const state = (n = 2) => ({ config: { deck_size: 36, num_players: n, reflecting: true, classic_ending: true }, hands: Array.from({ length: n }, () => []) })
const fresh = (over = {}) => newRecording({ id: 'g1', mode: 'analyse', names: { P0: 'You', P1: 'Ann' }, initial: state(), now: 1000, ...over })
const TAKE = { type: 'take' }
const summary = {
  totalVisits: 300,
  determinizations: 3,
  actions: [
    { action: { type: 'pass_attack' }, key: 'pass', score: 0.8, visits: 200 },
    { action: TAKE, key: 'take', score: 0.4, visits: 100 },
    { action: { type: 'x' }, key: 'never-tried', score: null, visits: 0 },
  ],
}

test('a recording grows by steps and undo cuts it back', () => {
  let rec = fresh()
  assert.equal(isRecording(rec), true)
  rec = addStep(rec, { state: state(), line: 'Ann attacked', actor: 'P1', action: { type: 'attack' } })
  rec = addStep(rec, { state: state(), line: 'You took', actor: 'P0', action: TAKE })
  rec = addStep(rec, { state: state(), line: 'You drew 7', actor: 'P0' })
  assert.equal(rec.steps.length, 3)
  assert.equal('action' in rec.steps[2], false, 'a revealed card is a step without an action')

  assert.equal(truncate(rec, 1).steps.length, 1)
  assert.equal(truncate(rec, undefined), rec, 'snapshots from before recording existed change nothing')
  assert.equal(truncate(rec, 99), rec)
  assert.equal(rec.steps.length, 3, 'recordings are never changed in place')
})

test('the hint snapshot keeps evaluated moves and whether I followed the best one', () => {
  const followed = hintSnapshot(summary, 'pass', 'pass', a => a.type)
  assert.equal(followed.followed, true)
  assert.deepEqual(followed.moves.map(m => m.key), ['pass', 'take'])
  assert.equal(followed.moves[0].text, 'pass_attack')
  assert.equal(followed.playouts, 300)

  assert.equal(hintSnapshot(summary, 'pass', 'take').followed, false)
  assert.equal(hintSnapshot(null, null, 'take'), null)
  assert.equal(hintSnapshot({ totalVisits: 0, actions: [] }, null, 'take'), null)
})

test('results are from my seat', () => {
  assert.equal(resultOf({ status: 'durak', player: 'P0' }), 'loss')
  assert.equal(resultOf({ status: 'durak', player: 'P1' }), 'win')
  assert.equal(resultOf({ status: 'draw' }), 'draw')
  assert.equal(resultOf({ status: 'in_progress' }), 'unfinished')
})

test('the summary counts moves and followed suggestions', () => {
  let rec = fresh()
  rec = addStep(rec, { state: state(), actor: 'P1', action: { type: 'attack' } })
  rec = addStep(rec, { state: state(), actor: 'P0', action: TAKE, hint: hintSnapshot(summary, 'pass', 'take') })
  rec = addStep(rec, { state: state(), actor: 'P0' })
  rec = addStep(rec, { state: state(), actor: 'P0', action: TAKE, hint: hintSnapshot(summary, 'take', 'take') })
  rec = addStep(rec, { state: state(), actor: 'P0', action: TAKE })
  const meta = metaOf(finalize(rec, { status: 'durak', player: 'P1' }, 5000))

  assert.deepEqual(
    { ...meta, names: undefined, playerTypes: undefined },
    {
      v: 1, id: 'g1', mode: 'analyse', startedAt: 1000, endedAt: 5000, players: 2, deck: 36, reflecting: true,
      classic: true, names: undefined, playerTypes: undefined, result: 'win', userResult: null, durak: 'P1',
      moves: 4, myMoves: 3, hinted: 2, followed: 1,
    },
  )
})

test('my correction of a result wins over the recorded one', () => {
  assert.equal(effectiveResult({ result: 'win' }), 'win')
  assert.equal(effectiveResult({ result: 'win', userResult: 'loss' }), 'loss')
  assert.equal(effectiveResult({}), 'unfinished')
})

test('statistics: win rate over finished games, with breakdowns', () => {
  const m = (over = {}) => ({ mode: 'analyse', players: 2, deck: 36, result: 'win', startedAt: 1, hinted: 0, followed: 0, ...over })
  const stats = computeStats([
    m({ startedAt: 4, hinted: 10, followed: 9 }),
    m({ startedAt: 3, result: 'loss', hinted: 10, followed: 5 }),
    m({ startedAt: 2, result: 'draw' }),
    m({ startedAt: 1, result: 'unfinished' }),
    m({ startedAt: 5, mode: 'play', players: 3, deck: 52, result: 'win', userResult: 'loss' }),
  ])

  assert.equal(stats.games, 5)
  assert.equal(stats.finished, 4)
  assert.deepEqual([stats.wins, stats.losses, stats.draws, stats.unfinished], [1, 2, 1, 1])
  assert.equal(stats.winRate, 0.25)
  assert.equal(stats.followRate, 0.7)
  assert.deepEqual(stats.recent, ['loss', 'win', 'loss', 'draw'], 'newest first, corrected results, no unfinished')

  const by = Object.fromEntries(stats.groups.map(g => [g.label, g]))
  assert.equal(by['Real games'].finished, 3)
  assert.equal(by['Against the computer'].losses, 1)
  assert.equal(by['3 players, 52 cards'].games, 1)
  assert.equal(by['Followed 80%+ of suggestions'].winRate, 1)
  assert.equal(by['Followed under 80%'].winRate, 0)
})

test('statistics cope with no games', () => {
  const stats = computeStats([])
  assert.equal(stats.winRate, null)
  assert.equal(stats.followRate, null)
  assert.deepEqual(stats.groups, [])
  assert.deepEqual(computeStats(null).recent, [])
})
