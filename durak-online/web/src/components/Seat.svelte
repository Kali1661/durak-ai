<script>
  // One player's place at the table: who they are, what they are doing this
  // trick, and their cards. State is always given in words (and an icon), never
  // by colour alone.
  export let name = ''
  /** e.g. "Search (MCTS)", "At the table"; optional. */
  export let kind = ''
  /** 'attacker' | 'support' | 'defender' | null */
  export let role = null
  export let toMove = false
  export let out = false
  export let you = false
  export let cardCount = 0
  /** Extra line under the name, e.g. "2 cards known". */
  export let note = ''

  const ROLES = {
    attacker: { text: 'Attacking', icon: '⚔', cls: 'chip-attack' },
    support: { text: 'Can throw in', icon: '＋', cls: 'chip-attack' },
    defender: { text: 'Defending', icon: '⛨', cls: 'chip-defend' },
  }
  $: r = ROLES[role]
</script>

<section
  class="rounded-2xl border bg-surface p-3 transition-shadow sm:p-4
    {toMove ? 'border-gold shadow-[0_0_0_3px_rgb(var(--gold)/0.35)]' : 'border-line'}
    {out ? 'opacity-70' : ''}"
  aria-label="{you ? 'Your seat' : name}{toMove ? ', to move' : ''}{out ? ', out of the game' : ''}"
>
  <header class="flex flex-wrap items-center gap-x-2 gap-y-1">
    <h3 class="text-base font-bold">{name}{#if you}<span class="sr-only"> (you)</span>{/if}</h3>
    {#if kind}<span class="text-xs text-muted">{kind}</span>{/if}
    <span class="ml-auto text-sm tabular-nums text-muted">{cardCount} {cardCount === 1 ? 'card' : 'cards'}</span>
  </header>

  <div class="mt-1.5 flex min-h-[1.5rem] flex-wrap items-center gap-1.5">
    {#if out}
      <span class="chip chip-out">✓ Out - no cards left</span>
    {:else}
      {#if r}<span class="chip {r.cls}"><span aria-hidden="true">{r.icon}</span>{r.text}</span>{/if}
      {#if toMove}<span class="chip chip-turn"><span aria-hidden="true">▶</span>To move</span>{/if}
    {/if}
    {#if note}<span class="text-xs text-muted">{note}</span>{/if}
  </div>

  <div class="mt-3"><slot /></div>
</section>
