import test from 'node:test'
import assert from 'node:assert/strict'

import { ExactSolver, exactNodeBudget } from '../src/lib/exact-solver.js'

class FakeWorker {
  constructor() {
    this.sent = []
    this.terminated = false
  }
  postMessage(msg) { this.sent.push(msg) }
  terminate() { this.terminated = true }
  reply(msg) { this.onmessage({ data: msg }) }
}

function setup() {
  const workers = []
  const solver = new ExactSolver(() => { const w = new FakeWorker(); workers.push(w); return w })
  return { solver, workers }
}

test('a request resolves with the worker result', async () => {
  const { solver, workers } = setup()
  const p = solver.solve({ fake: 'state' }, 1000)
  assert.deepEqual(workers[0].sent[0].req, { state: { fake: 'state' }, max_nodes: 1000 })
  workers[0].reply({ type: 'exact-result', id: workers[0].sent[0].id, result: { value: 'win', actions: [] } })
  assert.deepEqual(await p, { value: 'win', actions: [] })
  // The worker is kept for the next request.
  solver.solve({}, 1)
  assert.equal(workers.length, 1)
})

test('"not solvable" (no result) and errors both resolve with null', async () => {
  const { solver, workers } = setup()
  let p = solver.solve({}, 1)
  workers[0].reply({ type: 'exact-result', id: workers[0].sent[0].id, result: undefined })
  assert.equal(await p, null)
  p = solver.solve({}, 1)
  workers[0].reply({ type: 'error', id: workers[0].sent[1].id, message: 'boom' })
  assert.equal(await p, null)
})

test('a new request cancels the old one and replaces the busy worker', async () => {
  const { solver, workers } = setup()
  const first = solver.solve({ n: 1 }, 1)
  const second = solver.solve({ n: 2 }, 1)
  assert.equal(await first, null)
  assert.equal(workers[0].terminated, true)
  assert.equal(workers.length, 2)
  // A late reply for the cancelled request is ignored.
  workers[0].reply({ type: 'exact-result', id: workers[0].sent[0].id, result: { value: 'loss' } })
  workers[1].reply({ type: 'exact-result', id: workers[1].sent[0].id, result: { value: 'draw' } })
  assert.deepEqual(await second, { value: 'draw' })
})

test('a crashed worker and a worker that cannot start settle the request', async () => {
  const { solver, workers } = setup()
  const p = solver.solve({}, 1)
  workers[0].onerror({ message: 'out of memory' })
  assert.equal(await p, null)

  const broken = new ExactSolver(() => { throw new Error('no workers here') })
  assert.equal(await broken.solve({}, 1), null)

  solver.terminate()
  assert.equal(await solver.solve({}, 1), null)
})

test('the node budget follows the thinking time within limits', () => {
  assert.equal(exactNodeBudget(0), 6_000_000, 'unlimited time gets the cap')
  assert.equal(exactNodeBudget(100), 400_000)
  assert.equal(exactNodeBudget(2000), 3_000_000)
  assert.equal(exactNodeBudget(600_000), 6_000_000)
})
