<script>
  // One legal move as a full-width button, with its search score when known.
  import Card from '../lib/Card.svelte'
  import { actionCard, actionText } from '../lib/game.js'

  export let action
  /** { score, visits, cutoffs } from the search, or null/undefined. */
  export let stat = null
  export let best = false
  /** 'win' | 'draw' | 'loss' when the exact endgame solver settled this move. */
  export let exact = null
  /** Below this many playouts the score is flagged as rough. */
  export let fewBelow = 200

  $: card = actionCard(action)
  $: scored = stat && stat.score !== null && stat.visits > 0
  $: pct = scored ? Math.round(stat.score * 100) : null
  $: playouts = scored ? stat.visits.toLocaleString('en') : ''
  $: rough = scored && stat.visits < fewBelow
  const EXACT = {
    win: { text: 'Wins', long: 'wins by force', cls: 'text-accent' },
    draw: { text: 'Draw', long: 'draws with best play', cls: 'text-ink' },
    loss: { text: 'Loses', long: 'loses against best play', cls: 'text-danger' },
  }
  $: verdict = EXACT[exact] || null
  $: label = `${actionText(action)}${best ? ', suggested' : ''}${verdict ? `, calculated: ${verdict.long}` : ''}${scored ? `, ${verdict ? 'estimate' : 'score'} ${pct} percent from ${playouts} playouts` : ''}`
</script>

<button
  type="button"
  class="flex min-h-[52px] w-full items-center gap-3 rounded-xl border px-3 py-2 text-left transition-colors hover:bg-sunken
    {best ? 'border-gold bg-gold/10' : 'border-line bg-surface'}"
  aria-label={label}
  on:click
>
  {#if card}
    <Card {card} size="sm" />
  {:else}
    <span class="flex h-[3.9rem] w-11 shrink-0 items-center justify-center rounded-lg border border-dashed border-line text-lg text-muted sm:h-[4.25rem] sm:w-12" aria-hidden="true">
      {action.type === 'take' ? '⤓' : '✓'}
    </span>
  {/if}
  <span class="min-w-0 flex-1">
    <span class="block text-sm font-semibold">{actionText(action)}</span>
    {#if best}<span class="chip chip-turn mt-1">★ Suggested</span>{/if}
  </span>
  {#if verdict}
    <span class="w-24 shrink-0 text-right" aria-hidden="true">
      <span class="block text-base font-bold {verdict.cls}">{verdict.text}</span>
      <span class="block text-[11px] text-muted">calculated</span>
      {#if scored}<span class="mt-0.5 block text-[11px] tabular-nums text-muted">estimate {pct}%</span>{/if}
    </span>
  {:else if scored}
    <span class="w-24 shrink-0 text-right" aria-hidden="true">
      <span class="block text-base font-bold tabular-nums">{pct}%</span>
      <span class="mt-0.5 block h-1.5 overflow-hidden rounded-full bg-sunken">
        <span class="block h-full rounded-full bg-accent" style="width: {pct}%"></span>
      </span>
      <span class="mt-0.5 block text-[11px] tabular-nums text-muted">{playouts}{rough ? ' · rough' : ''}</span>
    </span>
  {/if}
</button>
