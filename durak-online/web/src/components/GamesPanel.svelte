<script>
  // My recorded games: statistics, the list, and a replay of any one of them.
  import ReplayViewer from './ReplayViewer.svelte'
  import {
    allReplays, clearReplays, computeStats, deleteReplay, effectiveResult, listReplays, loadReplay, setUserResult,
  } from '../lib/replays.js'

  let metas = []
  let loading = true
  let error = ''
  let viewing = null // a full recording
  let confirmClear = false

  $: stats = computeStats(metas)

  /** Called by the parent each time the panel is opened. */
  export async function refresh() {
    viewing = null
    confirmClear = false
    loading = true
    error = ''
    try {
      metas = await listReplays()
    } catch (e) {
      error = `Recorded games could not be read: ${e?.message || e}`
      metas = []
    }
    loading = false
  }

  async function attempt(fn) {
    error = ''
    try {
      await fn()
    } catch (e) {
      error = `That did not work: ${e?.message || e}`
    }
  }

  const open = id => attempt(async () => {
    const rec = await loadReplay(id)
    if (!rec) throw new Error('the recording is missing')
    viewing = rec
  })
  const remove = id => attempt(async () => { await deleteReplay(id); metas = await listReplays() })
  const correct = (id, value) => attempt(async () => { await setUserResult(id, value || null); metas = await listReplays() })
  const clearAll = () => attempt(async () => { await clearReplays(); confirmClear = false; metas = [] })

  const exportAll = () => attempt(async () => {
    const games = await allReplays()
    const blob = new Blob([JSON.stringify({ app: 'durak-ai', exportedAt: new Date().toISOString(), games })], { type: 'application/json' })
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url
    a.download = `durak-ai-games-${new Date().toISOString().slice(0, 10)}.json`
    a.click()
    setTimeout(() => URL.revokeObjectURL(url), 1000)
  })

  const pct = v => (v === null ? '-' : `${Math.round(v * 100)}%`)
  const when = ms => new Date(ms).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' })
  const RESULT = {
    win: { text: 'Won', cls: 'border-accent/60 bg-accent/10 text-accent' },
    loss: { text: 'Lost', cls: 'border-danger/60 bg-danger/10 text-danger' },
    draw: { text: 'Draw', cls: '' },
    unfinished: { text: 'Unfinished', cls: 'chip-out' },
  }
  const LETTER = { win: 'W', loss: 'L', draw: 'D' }
  const rules = m => `${m.players} players, ${m.deck} cards, ${m.reflecting ? 'passing' : 'throw-in'}, ${m.classic ? 'classic' : 'draw'}`
  const versus = m => Object.entries(m.names || {}).filter(([p]) => p !== 'P0').map(([, n]) => n).join(', ')
</script>

{#if error}
  <div class="notice notice-error mt-3" role="alert"><span aria-hidden="true">⚠</span><p class="m-0">{error}</p></div>
{/if}

{#if viewing}
  <div class="mt-3 flex flex-wrap items-center gap-2">
    <button type="button" class="btn btn-sm" on:click={() => (viewing = null)}>← All games</button>
    <p class="m-0 text-sm text-muted">{when(viewing.startedAt)} - {viewing.mode === 'play' ? 'against the computer' : 'real game'}</p>
  </div>
  <div class="mt-3">
    {#key viewing.id}<ReplayViewer record={viewing} />{/key}
  </div>
{:else if loading}
  <p class="mt-4 flex items-center gap-2 text-sm text-muted"><span class="spinner" aria-hidden="true"></span> Loading…</p>
{:else if !metas.length}
  <p class="mt-4 text-sm text-muted">No games recorded yet. Every game you play or track from now on is saved here automatically, in this browser only.</p>
{:else}
  <section class="mt-4" aria-labelledby="stats-title">
    <h3 id="stats-title" class="text-sm font-bold">Statistics</h3>
    <dl class="m-0 mt-2 grid grid-cols-2 gap-2 sm:grid-cols-4">
      <div class="rounded-xl border border-line p-3"><dt class="eyebrow">Win rate</dt><dd class="m-0 text-2xl font-bold tabular-nums">{pct(stats.winRate)}</dd></div>
      <div class="rounded-xl border border-line p-3"><dt class="eyebrow">Finished games</dt><dd class="m-0 text-2xl font-bold tabular-nums">{stats.finished}</dd></div>
      <div class="rounded-xl border border-line p-3"><dt class="eyebrow">Won / lost / draw</dt><dd class="m-0 text-2xl font-bold tabular-nums">{stats.wins} / {stats.losses} / {stats.draws}</dd></div>
      <div class="rounded-xl border border-line p-3"><dt class="eyebrow">Suggestions followed</dt><dd class="m-0 text-2xl font-bold tabular-nums">{pct(stats.followRate)}</dd></div>
    </dl>
    <p class="mt-2 text-xs text-muted">
      Winning means not ending up as the durak. Win rate counts finished games only{stats.unfinished ? ` (${stats.unfinished} unfinished left out)` : ''}.
      {#if stats.recent.length}Last {stats.recent.length}, newest first: <span class="font-mono font-semibold text-ink">{stats.recent.map(r => LETTER[r]).join(' ')}</span>{/if}
    </p>

    {#if stats.groups.length}
      <div class="mt-3 overflow-x-auto">
        <table class="w-full border-collapse text-left text-sm">
          <thead>
            <tr class="border-b border-line text-xs uppercase tracking-wider text-muted">
              <th class="py-1.5 pr-3 font-semibold" scope="col">Group</th>
              <th class="px-2 py-1.5 text-right font-semibold" scope="col">Games</th>
              <th class="px-2 py-1.5 text-right font-semibold" scope="col">Won</th>
              <th class="px-2 py-1.5 text-right font-semibold" scope="col">Lost</th>
              <th class="px-2 py-1.5 text-right font-semibold" scope="col">Draw</th>
              <th class="py-1.5 pl-2 text-right font-semibold" scope="col">Win rate</th>
            </tr>
          </thead>
          <tbody>
            {#each stats.groups as g (g.label)}
              <tr class="border-b border-line/60">
                <th class="py-1.5 pr-3 font-semibold" scope="row">{g.label}</th>
                <td class="px-2 py-1.5 text-right tabular-nums">{g.finished}</td>
                <td class="px-2 py-1.5 text-right tabular-nums">{g.wins}</td>
                <td class="px-2 py-1.5 text-right tabular-nums">{g.losses}</td>
                <td class="px-2 py-1.5 text-right tabular-nums">{g.draws}</td>
                <td class="py-1.5 pl-2 text-right font-semibold tabular-nums">{pct(g.winRate)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  </section>

  <section class="mt-5" aria-labelledby="list-title">
    <div class="flex flex-wrap items-center gap-2">
      <h3 id="list-title" class="text-sm font-bold">Games ({metas.length})</h3>
      <div class="ml-auto flex flex-wrap items-center gap-2">
        <button type="button" class="btn btn-sm" on:click={exportAll}>Export all (JSON)</button>
        {#if confirmClear}
          <span class="text-xs text-danger" role="alert">Delete every recorded game?</span>
          <button type="button" class="btn btn-sm btn-danger" on:click={clearAll}>Yes, delete all</button>
          <button type="button" class="btn btn-sm" on:click={() => (confirmClear = false)}>Cancel</button>
        {:else}
          <button type="button" class="btn btn-sm" on:click={() => (confirmClear = true)}>Delete all…</button>
        {/if}
      </div>
    </div>

    <ul class="m-0 mt-2 list-none space-y-2 p-0">
      {#each metas as m (m.id)}
        {@const r = effectiveResult(m)}
        <li class="rounded-xl border border-line p-3">
          <div class="flex flex-wrap items-center gap-x-3 gap-y-1">
            <span class="chip {RESULT[r].cls}">{RESULT[r].text}{m.userResult ? ' (corrected)' : ''}</span>
            <span class="text-sm font-semibold">{when(m.startedAt)}</span>
            <span class="text-xs text-muted">{m.mode === 'play' ? 'Against the computer' : 'Real game'}{versus(m) ? ` - vs ${versus(m)}` : ''}</span>
          </div>
          <p class="m-0 mt-1 text-xs text-muted">
            {rules(m)} - {m.moves} moves{#if m.hinted}, followed {m.followed} of {m.hinted} suggestions{/if}
          </p>
          <div class="mt-2 flex flex-wrap items-center gap-2">
            <button type="button" class="btn btn-sm btn-primary" on:click={() => open(m.id)}>Replay</button>
            <label class="flex items-center gap-1.5 text-xs text-muted">
              Real result
              <select class="field min-h-[36px] w-auto py-1 text-xs" value={m.userResult || ''} on:change={e => correct(m.id, e.currentTarget.value)}>
                <option value="">As recorded ({RESULT[m.result].text.toLowerCase()})</option>
                <option value="win">I won</option>
                <option value="loss">I lost</option>
                <option value="draw">Draw</option>
              </select>
            </label>
            <button type="button" class="btn btn-sm ml-auto" on:click={() => remove(m.id)} aria-label="Delete the game from {when(m.startedAt)}">Delete</button>
          </div>
        </li>
      {/each}
    </ul>
  </section>
{/if}
