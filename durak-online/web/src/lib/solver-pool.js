// A small pool of solver workers that runs one job at a time.
//
// Guarantees:
//  * `run()` always settles - with results, errors, a timeout or a cancel.
//  * A new `run()` or `cancel()` settles the previous job as cancelled.
//  * Replies are matched to the current job by id, so a late reply from an
//    earlier job can never be mistaken for a current result.
//  * A worker that is still computing when its job ends is terminated and
//    replaced, so abandoned searches do not keep burning CPU.
//  * One message handler per worker for its whole life: nothing accumulates.

/** Split `total` units of work over at most `parts` workers, never assigning zero. */
export function splitWork(total, parts) {
  const n = Math.max(0, Math.floor(total))
  const p = Math.max(1, Math.min(Math.floor(parts) || 1, n || 1))
  if (n === 0) return []
  const base = Math.floor(n / p)
  const extra = n % p
  return Array.from({ length: p }, (_, i) => base + (i < extra ? 1 : 0))
}

export class SolverPool {
  /** @param {() => Worker} createWorker */
  constructor(createWorker) {
    this.createWorker = createWorker
    this.slots = []
    this.job = null
    this.nextId = 1
    this.disposed = false
  }

  get size() {
    return this.slots.length
  }

  get busy() {
    return this.job !== null
  }

  _spawn(index) {
    const slot = { worker: null, busy: false }
    const worker = this.createWorker()
    worker.onmessage = e => this._onMessage(index, e?.data)
    worker.onerror = e => {
      e?.preventDefault?.()
      this._onFailure(index, e?.message || 'the solver worker crashed')
    }
    slot.worker = worker
    this.slots[index] = slot
    return slot
  }

  _retire(index) {
    const slot = this.slots[index]
    if (!slot?.worker) return
    slot.worker.onmessage = null
    slot.worker.onerror = null
    try { slot.worker.terminate() } catch { /* already gone */ }
    slot.worker = null
    slot.busy = false
  }

  /** Make the pool exactly `count` workers. Cancels any running job. */
  resize(count) {
    const target = Math.max(1, Math.floor(count) || 1)
    if (target === this.slots.length && this.slots.every(s => s?.worker)) return
    this.cancel()
    while (this.slots.length > target) {
      this._retire(this.slots.length - 1)
      this.slots.pop()
    }
    for (let i = 0; i < target; i++) {
      if (!this.slots[i]?.worker) this._spawn(i)
    }
  }

  /**
   * Run one search batch spread over the pool.
   * Resolves with { id, results, errors, timedOut, cancelled, elapsedMs }.
   * Never rejects.
   */
  run({ state, determinizations, rollouts, maxDepth = 500, timeoutMs = 30000 }) {
    this.cancel()
    if (this.disposed) {
      return Promise.resolve(this._outcome({ id: 0, errors: ['the solver has been shut down'] }))
    }
    const id = this.nextId++
    const startedAt = Date.now()

    return new Promise(resolve => {
      const job = { id, pending: new Set(), results: [], errors: [], timer: null, resolve, startedAt }
      // Make sure there is at least one worker before the job becomes current
      // (resizing cancels whatever job is current).
      let spawnError = null
      try {
        if (!this.slots.length) this.resize(1)
      } catch (e) {
        spawnError = e
      }
      this.job = job

      let chunks
      try {
        if (spawnError) throw spawnError
        chunks = splitWork(determinizations, this.slots.length)
        if (!chunks.length) throw new Error('nothing to search: determinizations must be at least 1')
        chunks.forEach((count, i) => {
          const slot = this.slots[i]?.worker ? this.slots[i] : this._spawn(i)
          slot.busy = true
          job.pending.add(i)
          slot.worker.postMessage({
            type: 'solve',
            id,
            req: { state, determinizations: count, rollouts_per_determinization: rollouts, max_depth: maxDepth },
          })
        })
      } catch (e) {
        job.errors.push(`could not start the solver: ${e?.message || e}`)
        this._settle(job, {})
        return
      }

      job.timer = setTimeout(() => this._settle(job, { timedOut: true }), Math.max(1, timeoutMs))
    })
  }

  /** Stop the running job, if any. Its promise resolves with `cancelled: true`. */
  cancel() {
    if (this.job) this._settle(this.job, { cancelled: true })
  }

  dispose() {
    this.cancel()
    this.disposed = true
    for (let i = 0; i < this.slots.length; i++) this._retire(i)
    this.slots = []
  }

  _outcome({ id, results = [], errors = [], timedOut = false, cancelled = false, startedAt = Date.now() }) {
    return { id, results, errors, timedOut, cancelled, elapsedMs: Date.now() - startedAt }
  }

  _settle(job, { timedOut = false, cancelled = false }) {
    if (this.job !== job) return
    this.job = null
    if (job.timer !== null) clearTimeout(job.timer)
    job.timer = null
    // Workers that have not answered are still computing a result nobody wants.
    for (const index of job.pending) {
      this._retire(index)
      if (!this.disposed) {
        try { this._spawn(index) } catch { /* reported on the next run */ }
      }
    }
    job.pending.clear()
    job.resolve(this._outcome({ ...job, timedOut, cancelled }))
  }

  _onMessage(index, msg) {
    const slot = this.slots[index]
    const job = this.job
    if (!msg || !job || msg.id !== job.id || !job.pending.has(index)) return
    if (msg.type !== 'result' && msg.type !== 'error') return

    if (slot) slot.busy = false
    job.pending.delete(index)
    if (msg.type === 'result') job.results.push(msg.result)
    else job.errors.push(String(msg.message || 'unknown solver error'))
    if (job.pending.size === 0) this._settle(job, {})
  }

  _onFailure(index, message) {
    const job = this.job
    this._retire(index)
    if (!this.disposed) {
      try { this._spawn(index) } catch { /* reported on the next run */ }
    }
    if (!job || !job.pending.has(index)) return
    job.pending.delete(index)
    job.errors.push(String(message))
    if (job.pending.size === 0) this._settle(job, {})
  }
}

/** Turn a raw engine/worker message into something a person can act on. */
export function friendlySolverError(message) {
  const text = String(message || '')
  if (/memory|allocation/i.test(text)) {
    return 'The solver ran out of memory. Lower the thread count or the search size in Settings.'
  }
  if (/unreachable|panicked|RuntimeError/i.test(text)) {
    return 'The solver stopped unexpectedly. Please start a new game; if it keeps happening, report it.'
  }
  return text || 'The solver failed for an unknown reason.'
}
