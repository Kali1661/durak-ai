// Runs the exact endgame solver in its own worker, one request at a time.
//
// The calculation can take a few seconds and cannot be interrupted from the
// outside, so cancelling means replacing the worker. Every request settles:
// with the engine's result, or with null when the position is not exactly
// solvable, the request was cancelled, or anything went wrong (the caller then
// simply carries on with the sampling search).

/** Positions the solver may search, for a thinking time in ms (0 = unlimited). */
export function exactNodeBudget(thinkingMs) {
  const MAX = 6_000_000
  if (!(thinkingMs > 0)) return MAX
  // Roughly what the solver gets through in that time; never tiny, never over the cap.
  return Math.max(400_000, Math.min(MAX, Math.round(thinkingMs * 1500)))
}

export class ExactSolver {
  /** @param {() => Worker} createWorker */
  constructor(createWorker) {
    this._create = createWorker
    this._worker = null
    this._nextId = 1
    this._pending = null // { id, resolve }
    this._closed = false
  }

  /** Resolves with `{ perspective, value, actions, nodes }` or null. */
  solve(state, maxNodes) {
    this.cancel()
    if (this._closed) return Promise.resolve(null)
    return new Promise(resolve => {
      const id = this._nextId++
      this._pending = { id, resolve }
      try {
        this._worker ??= this._spawn()
        this._worker.postMessage({ type: 'exact', id, req: { state, max_nodes: maxNodes } })
      } catch (e) {
        console.warn('The exact solver could not be started.', e)
        this._settle(id, null)
      }
    })
  }

  /** Abandon the running request, if any; its promise resolves with null. */
  cancel() {
    if (!this._pending) return
    // The worker is busy and cannot be told to stop: drop it.
    this._discardWorker()
    this._settle(this._pending.id, null)
  }

  terminate() {
    this.cancel()
    this._discardWorker()
    this._closed = true
  }

  _spawn() {
    const worker = this._create()
    worker.onmessage = e => {
      const msg = e.data
      if (msg?.type === 'exact-result') this._settle(msg.id, msg.result ?? null)
      else if (msg?.type === 'error') {
        console.warn(`The exact solver failed: ${msg.message}`)
        this._settle(msg.id, null)
      }
    }
    worker.onerror = e => {
      console.warn('The exact solver worker crashed.', e?.message || e)
      this._discardWorker()
      if (this._pending) this._settle(this._pending.id, null)
    }
    return worker
  }

  _discardWorker() {
    try {
      this._worker?.terminate()
    } catch {
      // already gone
    }
    this._worker = null
  }

  _settle(id, result) {
    if (!this._pending || this._pending.id !== id) return
    const { resolve } = this._pending
    this._pending = null
    resolve(result)
  }
}
