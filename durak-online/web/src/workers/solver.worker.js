// Dedicated worker: loads the WASM engine once and answers solve requests.
// Every request gets exactly one reply - { type: 'result' } or { type: 'error' } -
// carrying the id it was sent with.

import init, { solve } from '../wasm/durak_wasm.js'

let ready = null

function describe(err) {
  if (err instanceof Error) return err.message
  if (typeof err === 'string') return err
  try { return JSON.stringify(err) } catch { return String(err) }
}

self.onmessage = async e => {
  const msg = e.data
  if (msg?.type !== 'solve') return
  try {
    ready ??= init()
    await ready
    const startedAt = performance.now()
    const result = solve(msg.req)
    self.postMessage({ type: 'result', id: msg.id, result, elapsedMs: performance.now() - startedAt })
  } catch (err) {
    // A failed init must be retried by the next request, not cached.
    if (ready) ready.catch(() => { ready = null })
    self.postMessage({ type: 'error', id: msg.id, message: describe(err) })
  }
}
