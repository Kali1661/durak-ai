import test from 'node:test'
import assert from 'node:assert/strict'

import { actionKey, addResult, createTally, pickBest, scoreMargin, summarize } from '../src/lib/aggregate.js'

const card = (suit, rank, type = 'public') => ({ type, suit, rank })
const attack = (suit, rank, type) => ({ type: 'attack', card: card(suit, rank, type) })
const TAKE = { type: 'take' }

/** A solver result in the exact shape the engine returns. */
const result = (actions, extra = {}) => ({
  perspective: 'P0',
  determinizations: 1,
  total_visits: actions.reduce((n, a) => n + a.visits, 0),
  total_cutoffs: actions.reduce((n, a) => n + (a.cutoffs || 0), 0),
  actions: actions.map(a => ({ cutoffs: 0, ...a })),
  ...extra,
})

test('action identity ignores card visibility but not the move', () => {
  assert.equal(actionKey(attack('Clubs', 'Six', 'public')), actionKey(attack('Clubs', 'Six', 'private')))
  assert.notEqual(actionKey(attack('Clubs', 'Six')), actionKey(attack('Clubs', 'Seven')))
  assert.notEqual(actionKey(attack('Clubs', 'Six')), actionKey({ type: 'throw', card: card('Clubs', 'Six') }))
  assert.notEqual(
    actionKey({ type: 'defend', pile_index: 0, card: card('Clubs', 'Six') }),
    actionKey({ type: 'defend', pile_index: 1, card: card('Clubs', 'Six') }),
  )
  assert.notEqual(actionKey({ type: 'throw', card: null }), actionKey({ type: 'pass_attack' }))
})

test('pooled score is the visit-weighted mean of the engine scores', () => {
  const a = attack('Clubs', 'Six')
  const tally = createTally()
  addResult(tally, result([{ action: a, visits: 100, score: 0.2 }]))
  addResult(tally, result([{ action: a, visits: 300, score: 0.6 }]))
  const s = summarize(tally, [a])
  assert.equal(s.actions[0].visits, 400)
  // (100 * 0.2 + 300 * 0.6) / 400 = 0.5 - not the plain average 0.4
  assert.ok(Math.abs(s.actions[0].score - 0.5) < 1e-12)
  assert.equal(s.totalVisits, 400)
  assert.equal(s.determinizations, 2)
  assert.equal(s.batches, 2)
})

test('uses visits and score, the fields the engine actually returns', () => {
  const a = attack('Clubs', 'Six')
  const tally = createTally()
  // wins/losses/draws are not part of the engine's output and must not be needed.
  addResult(tally, result([{ action: a, visits: 50, score: 0.9, wins: undefined }]))
  const best = pickBest(summarize(tally, [a, TAKE]))
  assert.equal(best.key, actionKey(a))
  assert.ok(Math.abs(best.score - 0.9) < 1e-12)
})

test('results are matched to legal actions even when card visibility differs', () => {
  const tally = createTally()
  addResult(tally, result([{ action: attack('Hearts', 'Ace', 'public'), visits: 10, score: 1 }]))
  const s = summarize(tally, [attack('Hearts', 'Ace', 'private')])
  assert.equal(s.actions[0].score, 1)
  assert.equal(s.actions[0].action.card.type, 'private', 'the legal action object is kept')
})

test('unevaluated actions have a null score and sort last', () => {
  const a = attack('Clubs', 'Six')
  const b = attack('Clubs', 'Seven')
  const tally = createTally()
  addResult(tally, result([{ action: b, visits: 10, score: 0.1 }, { action: a, visits: 0, score: 0 }]))
  const s = summarize(tally, [a, b, TAKE])
  assert.deepEqual(s.actions.map(x => x.score), [0.1, null, null])
  assert.equal(s.actions[0].key, actionKey(b))
})

test('results for moves that are not legal are ignored', () => {
  const tally = createTally()
  addResult(tally, result([{ action: attack('Spades', 'Ace'), visits: 999, score: 1 }]))
  const s = summarize(tally, [TAKE])
  assert.deepEqual(s.actions.map(x => x.key), [actionKey(TAKE)])
  assert.equal(pickBest(s), null)
})

test('pickBest takes the highest score, not the most visits', () => {
  const a = attack('Clubs', 'Six')
  const b = attack('Clubs', 'Seven')
  const tally = createTally()
  addResult(tally, result([{ action: a, visits: 900, score: 0.4 }, { action: b, visits: 300, score: 0.7 }]))
  assert.equal(pickBest(summarize(tally, [a, b])).key, actionKey(b))
})

test('pickBest ignores a barely sampled action with a lucky score', () => {
  const solid = attack('Clubs', 'Six')
  const fluke = attack('Clubs', 'Seven')
  const tally = createTally()
  addResult(tally, result([{ action: solid, visits: 1000, score: 0.6 }, { action: fluke, visits: 3, score: 1 }]))
  const s = summarize(tally, [solid, fluke])
  assert.equal(pickBest(s).key, actionKey(solid))
  // With the guard off the fluke would win - which is why the guard exists.
  assert.equal(pickBest(s, { minVisitShare: 0 }).key, actionKey(fluke))
})

test('pickBest breaks score ties by sample count', () => {
  const a = attack('Clubs', 'Six')
  const b = attack('Clubs', 'Seven')
  const tally = createTally()
  addResult(tally, result([{ action: a, visits: 100, score: 0.5 }, { action: b, visits: 400, score: 0.5 }]))
  assert.equal(pickBest(summarize(tally, [a, b])).key, actionKey(b))
})

test('pickBest returns null instead of guessing when nothing was evaluated', () => {
  assert.equal(pickBest(summarize(createTally(), [TAKE])), null)
  assert.equal(pickBest(summarize(null, [TAKE])), null)
  assert.equal(pickBest(null), null)
  const tally = createTally()
  addResult(tally, result([{ action: TAKE, visits: 0, score: 0 }]))
  assert.equal(pickBest(summarize(tally, [TAKE])), null)
})

test('malformed solver results are rejected, not folded in', () => {
  const a = attack('Clubs', 'Six')
  assert.throws(() => addResult(createTally(), null))
  assert.throws(() => addResult(createTally(), { actions: 'nope' }))
  assert.throws(() => addResult(createTally(), result([{ action: a, visits: 10, score: 1.5 }])))
  assert.throws(() => addResult(createTally(), result([{ action: a, visits: 10, score: Number.NaN }])))
  assert.throws(() => addResult(createTally(), result([{ action: a, visits: -1, score: 0.5 }])))
  assert.throws(() => addResult(createTally(), result([{ visits: 10, score: 0.5 }])))
  // The old, wrong schema carries no visits at all.
  assert.throws(() => addResult(createTally(), { actions: [{ action: a, wins: 3, losses: 1, draws: 0 }] }))
})

test('cutoffs are pooled alongside visits', () => {
  const a = attack('Clubs', 'Six')
  const tally = createTally()
  addResult(tally, result([{ action: a, visits: 10, score: 0.5, cutoffs: 4 }]))
  addResult(tally, result([{ action: a, visits: 10, score: 0.5, cutoffs: 1 }]))
  const s = summarize(tally, [a])
  assert.equal(s.actions[0].cutoffs, 5)
  assert.equal(s.totalCutoffs, 5)
})

test('score margin shrinks with more samples and is absent without data', () => {
  assert.equal(scoreMargin(null, 0), null)
  assert.equal(scoreMargin(0.5, 0), null)
  assert.ok(scoreMargin(0.5, 100) > scoreMargin(0.5, 10000))
  assert.ok(scoreMargin(1, 50) > 0, 'a perfect score from few samples is still uncertain')
})
