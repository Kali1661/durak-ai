// Dedicated worker: loads the WASM engine once (or uses the native one, when
// the app is served by it) and answers solve requests
// ('solve' = sampling search, 'exact' = exact endgame solver). Every request
// gets exactly one reply - { type: 'result' | 'exact-result' } or
// { type: 'error' } - carrying the id it was sent with.

import init, { solve, solve_exact } from '../wasm/durak_wasm.js'

let ready = null

// Served by the native program (durak-native)? Then searches go to it instead
// of the WebAssembly engine: same requests, same answers, more speed.
let native = null
function nativeEngine() {
  native ??= fetch('/api/info')
    .then(r => (r.ok ? r.json() : null))
    .then(info => info?.engine === 'native')
    .catch(() => false)
  return native
}

async function post(path, body) {
  const r = await fetch(path, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) })
  if (!r.ok) throw new Error(await r.text())
  return r.json()
}

function describe(err) {
  if (err instanceof Error) return err.message
  if (typeof err === 'string') return err
  try { return JSON.stringify(err) } catch { return String(err) }
}

self.onmessage = async e => {
  const msg = e.data
  if (msg?.type !== 'solve' && msg?.type !== 'exact') return
  try {
    const useNative = await nativeEngine()
    if (!useNative) {
      ready ??= init()
      await ready
    }
    const startedAt = performance.now()
    if (msg.type === 'exact') {
      const result = useNative ? await post('/api/exact', msg.req) : solve_exact(msg.req)
      self.postMessage({ type: 'exact-result', id: msg.id, result, elapsedMs: performance.now() - startedAt })
      return
    }
    const result = useNative ? await post('/api/solve', msg.req) : solve(msg.req)
    self.postMessage({ type: 'result', id: msg.id, result, elapsedMs: performance.now() - startedAt })
  } catch (err) {
    // A failed init must be retried by the next request, not cached.
    if (ready) ready.catch(() => { ready = null })
    self.postMessage({ type: 'error', id: msg.id, message: describe(err) })
  }
}
