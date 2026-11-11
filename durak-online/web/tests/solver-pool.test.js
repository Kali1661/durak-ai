import test from 'node:test'
import assert from 'node:assert/strict'

import { SolverPool, friendlySolverError, splitWork } from '../src/lib/solver-pool.js'

/** Stand-in for a Web Worker that the test drives by hand. */
class FakeWorker {
  static all = []
  constructor() {
    this.sent = []
    this.terminated = false
    this.onmessage = null
    this.onerror = null
    FakeWorker.all.push(this)
  }
  postMessage(msg) { this.sent.push(msg) }
  terminate() { this.terminated = true }
  reply(data) { this.onmessage?.({ data }) }
  crash(message) { this.onerror?.({ message, preventDefault() {} }) }
  get last() { return this.sent[this.sent.length - 1] }
}

function makePool(size) {
  FakeWorker.all = []
  const pool = new SolverPool(() => new FakeWorker())
  pool.resize(size)
  return pool
}

const live = () => FakeWorker.all.filter(w => !w.terminated)
const job = (extra = {}) => ({ state: { s: 1 }, determinizations: 8, rollouts: 100, timeoutMs: 5000, ...extra })
const ok = (worker, payload = {}) => worker.reply({ type: 'result', id: worker.last.id, result: payload })

test('splitWork never hands a worker zero work', () => {
  assert.deepEqual(splitWork(10, 4), [3, 3, 2, 2])
  assert.deepEqual(splitWork(2, 8), [1, 1])
  assert.deepEqual(splitWork(1, 4), [1])
  assert.deepEqual(splitWork(0, 4), [])
  assert.deepEqual(splitWork(5, 0), [5])
  assert.equal(splitWork(37, 6).reduce((a, b) => a + b), 37)
})

test('a job resolves once every worker has answered', async () => {
  const pool = makePool(2)
  const promise = pool.run(job())
  const [a, b] = live()
  assert.equal(a.last.req.determinizations + b.last.req.determinizations, 8)
  assert.equal(a.last.req.rollouts_per_determinization, 100)
  ok(a, { n: 1 })
  assert.equal(pool.busy, true, 'still waiting for the second worker')
  ok(b, { n: 2 })
  const out = await promise
  assert.deepEqual(out.results, [{ n: 1 }, { n: 2 }])
  assert.deepEqual(out.errors, [])
  assert.equal(out.timedOut, false)
  assert.equal(out.cancelled, false)
  assert.equal(pool.busy, false)
})

test('fewer determinizations than workers only uses the workers it needs', async () => {
  const pool = makePool(4)
  const promise = pool.run(job({ determinizations: 2 }))
  const used = live().filter(w => w.sent.length)
  assert.equal(used.length, 2)
  used.forEach(w => ok(w))
  assert.equal((await promise).results.length, 2)
})

test('a worker error is reported and the job still settles', async () => {
  const pool = makePool(2)
  const promise = pool.run(job())
  const [a, b] = live()
  ok(a, { n: 1 })
  b.reply({ type: 'error', id: b.last.id, message: 'invalid game state: boom' })
  const out = await promise
  assert.equal(out.results.length, 1)
  assert.deepEqual(out.errors, ['invalid game state: boom'])
})

test('a crashed worker is replaced and counted as an error', async () => {
  const pool = makePool(2)
  const promise = pool.run(job())
  const [a, b] = live()
  a.crash('out of memory')
  ok(b)
  const out = await promise
  assert.deepEqual(out.errors, ['out of memory'])
  assert.equal(a.terminated, true)
  assert.equal(live().length, 2, 'the pool is back to full size')
})

test('a timeout settles the job and replaces the stuck workers', async () => {
  const pool = makePool(2)
  const promise = pool.run(job({ timeoutMs: 20 }))
  const [a, b] = live()
  ok(a, { n: 1 })
  const out = await promise
  assert.equal(out.timedOut, true)
  assert.equal(out.results.length, 1, 'answers that arrived in time are kept')
  assert.equal(b.terminated, true, 'the worker that never answered is stopped')
  assert.equal(a.terminated, false)
  assert.equal(live().length, 2)
})

test('cancel settles the running job as cancelled', async () => {
  const pool = makePool(2)
  const promise = pool.run(job())
  pool.cancel()
  const out = await promise
  assert.equal(out.cancelled, true)
  assert.equal(pool.busy, false)
  pool.cancel() // nothing running: must be harmless
})

test('starting a new job cancels the old one, whose late replies are ignored', async () => {
  const pool = makePool(1)
  const first = pool.run(job())
  const oldWorker = live()[0]
  const oldId = oldWorker.last.id
  const second = pool.run(job())
  assert.equal((await first).cancelled, true)

  // The old worker was mid-search, so it was replaced.
  assert.equal(oldWorker.terminated, true)
  const fresh = live()[0]
  // A stale reply carrying the old id must not end up in the new job.
  fresh.reply({ type: 'result', id: oldId, result: { stale: true } })
  assert.equal(pool.busy, true)
  ok(fresh, { fresh: true })
  assert.deepEqual((await second).results, [{ fresh: true }])
})

test('a late timer from a finished job cannot disturb the next job', async () => {
  const pool = makePool(1)
  const first = pool.run(job({ timeoutMs: 15 }))
  ok(live()[0], { n: 1 })
  await first
  const second = pool.run(job({ timeoutMs: 5000 }))
  await new Promise(r => setTimeout(r, 40)) // well past the first job's deadline
  assert.equal(pool.busy, true, 'the second job is still running')
  ok(live()[0], { n: 2 })
  const out = await second
  assert.equal(out.timedOut, false)
  assert.deepEqual(out.results, [{ n: 2 }])
})

test('duplicate replies from one worker are counted once', async () => {
  const pool = makePool(2)
  const promise = pool.run(job())
  const [a, b] = live()
  ok(a, { n: 1 })
  ok(a, { n: 'again' })
  ok(b, { n: 2 })
  assert.deepEqual((await promise).results, [{ n: 1 }, { n: 2 }])
})

test('zero determinizations fails fast instead of hanging', async () => {
  const pool = makePool(2)
  const out = await pool.run(job({ determinizations: 0 }))
  assert.equal(out.results.length, 0)
  assert.equal(out.errors.length, 1)
  assert.equal(pool.busy, false)
})

test('a pool with no workers creates one on demand', async () => {
  FakeWorker.all = []
  const pool = new SolverPool(() => new FakeWorker())
  const promise = pool.run(job({ determinizations: 3 }))
  assert.equal(live().length, 1)
  ok(live()[0], { n: 1 })
  assert.deepEqual((await promise).results, [{ n: 1 }])
})

test('failing to create a worker is an error, not a hang', async () => {
  const pool = new SolverPool(() => { throw new Error('workers are blocked') })
  const out = await pool.run(job())
  assert.equal(out.errors.length, 1)
  assert.match(out.errors[0], /workers are blocked/)
})

test('resize and dispose terminate workers and settle any job', async () => {
  const pool = makePool(4)
  const promise = pool.run(job())
  pool.resize(2)
  assert.equal((await promise).cancelled, true)
  assert.equal(pool.size, 2)
  assert.equal(live().length, 2)

  const last = pool.run(job())
  pool.dispose()
  assert.equal((await last).cancelled, true)
  assert.equal(live().length, 0)
  const after = await pool.run(job())
  assert.equal(after.errors.length, 1, 'a disposed pool refuses new work')
})

test('one message handler per worker, however many jobs run', async () => {
  const pool = makePool(1)
  for (let i = 0; i < 25; i++) {
    const promise = pool.run(job())
    ok(live()[0], { i })
    assert.deepEqual((await promise).results, [{ i }])
  }
  assert.equal(FakeWorker.all.length, 1, 'no worker was replaced or leaked')
})

test('engine errors are turned into advice', () => {
  assert.match(friendlySolverError('RangeError: WebAssembly.Memory(): out of memory'), /Lower the thread count/)
  assert.match(friendlySolverError('RuntimeError: unreachable'), /stopped unexpectedly/)
  assert.equal(friendlySolverError('invalid game state: x'), 'invalid game state: x')
  assert.ok(friendlySolverError('').length > 0)
})
