<script>
  // Step through a recorded game: the position after each move, and - for my
  // moves - what the search suggested at the time.
  import Card from '../lib/Card.svelte'
  import Seat from './Seat.svelte'
  import { cardLabel, suitSymbol } from '../lib/cards.js'
  import { ME, actorOf, cardKey, handOf, isOut, isUnknown, parseCard, roleOf, sortCards } from '../lib/game.js'

  export let record

  let index = 0
  $: last = record.steps.length
  $: step = index > 0 ? record.steps[index - 1] : null
  $: gs = step ? step.state : record.initial
  $: seats = gs.hands.map((_, i) => `P${i}`)
  $: opponents = seats.filter(p => p !== ME)
  $: finished = index === last && record.result && record.result !== 'unfinished'
  $: actor = finished ? null : actorOf(gs)
  $: trumpCard = gs.stock?.length ? parseCard(gs.stock[0]) : null
  $: caption = step ? step.line || 'Move' : 'Start of the game'

  const nameOf = pid => (pid === ME ? 'You' : record.names?.[pid] || `Opponent ${pid.slice(1)}`)
  const pct = score => `${Math.round(score * 100)}%`
  const go = i => { index = Math.max(0, Math.min(last, i)) }

  function cardsOf(pid) {
    const hand = handOf(gs, pid)
    const known = sortCards(hand.filter(c => !isUnknown(c)).map(parseCard), gs.trump)
    return { known, hidden: hand.length - known.length, count: hand.length }
  }

  function onKey(e) {
    if (e.key === 'ArrowLeft') { go(index - 1); e.preventDefault() }
    else if (e.key === 'ArrowRight') { go(index + 1); e.preventDefault() }
    else if (e.key === 'Home') { go(0); e.preventDefault() }
    else if (e.key === 'End') { go(last); e.preventDefault() }
  }

  const RESULT_TEXT = { win: 'You were not the durak', loss: 'You were the durak', draw: 'A draw', unfinished: 'Not finished' }
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<section aria-label="Replay" tabindex="0" on:keydown={onKey} class="rounded-xl">
  <div class="flex flex-wrap items-center gap-2">
    <button type="button" class="btn btn-sm" disabled={index === 0} on:click={() => go(0)} aria-label="First position">« Start</button>
    <button type="button" class="btn btn-sm" disabled={index === 0} on:click={() => go(index - 1)}>◀ Back</button>
    <button type="button" class="btn btn-sm btn-primary" disabled={index === last} on:click={() => go(index + 1)}>Next ▶</button>
    <button type="button" class="btn btn-sm" disabled={index === last} on:click={() => go(last)} aria-label="Last position">End »</button>
    <input class="min-w-[8rem] flex-1 accent-[rgb(var(--accent))]" type="range" min="0" max={last} bind:value={index} aria-label="Position in the game" />
    <span class="text-sm tabular-nums text-muted">{index} / {last}</span>
  </div>

  <p class="mt-3 rounded-xl border border-line bg-sunken px-3 py-2 text-sm font-semibold" role="status">
    {caption}{#if finished} - game over: {RESULT_TEXT[record.userResult || record.result].toLowerCase()}.{/if}
  </p>

  {#if step?.hint}
    <div class="mt-2 rounded-xl border px-3 py-2 text-sm {step.hint.followed ? 'border-line' : 'border-gold bg-gold/10'}">
      <p class="m-0 font-semibold">
        {step.hint.followed ? 'You played the suggested move.' : 'You played a different move from the suggestion.'}
        <span class="font-normal text-muted">Suggestions before this move ({step.hint.playouts.toLocaleString('en')} playouts):</span>
      </p>
      <ul class="m-0 mt-1 flex list-none flex-wrap gap-x-4 gap-y-1 p-0 text-xs">
        {#each step.hint.moves as m (m.key)}
          <li class={m.key === step.hint.chosen ? 'font-bold text-ink' : 'text-muted'}>
            {m.text || m.key}: {pct(m.score)}{#if m.key === step.hint.best} ★{/if}{#if m.key === step.hint.chosen} (played){/if}
          </li>
        {/each}
      </ul>
    </div>
  {/if}

  <div class="mt-3 grid gap-3 {opponents.length > 1 ? 'sm:grid-cols-2' : ''}">
    {#each opponents as pid}
      {@const h = cardsOf(pid)}
      <Seat name={nameOf(pid)} role={roleOf(gs, pid)} toMove={actor === pid} out={isOut(gs, pid)} cardCount={h.count}>
        <div class="flex min-h-[3.9rem] flex-wrap items-end gap-1">
          {#each h.known as c (cardKey(c))}<Card card={c} size="sm" />{/each}
          {#if h.hidden > 0}
            <span class="flex" role="img" aria-label="{h.hidden} hidden {h.hidden === 1 ? 'card' : 'cards'}">
              {#each Array(Math.min(h.hidden, 12)) as _, i}<span class={i ? '-ml-7 sm:-ml-8' : ''} aria-hidden="true"><Card faceDown size="sm" label="" /></span>{/each}
            </span>
          {/if}
        </div>
      </Seat>
    {/each}
  </div>

  <section class="felt mt-3 p-4" aria-label="Table">
    <div class="flex flex-wrap items-start gap-4">
      <div class="min-w-0 flex-1">
        <h3 class="text-xs font-semibold uppercase tracking-wider text-on-felt/80">On the table</h3>
        {#if gs.table.length}
          <ul class="m-0 mt-3 flex list-none flex-wrap gap-x-9 gap-y-8 p-0 pb-6 pr-6">
            {#each gs.table as p, i (i)}
              {@const a = parseCard(p.attack)}
              {@const d = parseCard(p.defense)}
              <li class="relative" aria-label="{cardLabel(a)} {d ? `beaten by ${cardLabel(d)}` : 'not beaten yet'}">
                <Card card={a} size="md" />
                {#if d}<span class="absolute left-5 top-5 sm:left-6 sm:top-6"><Card card={d} size="md" /></span>{/if}
              </li>
            {/each}
          </ul>
        {:else}
          <p class="m-0 mt-3 text-sm text-on-felt/80">No cards on the table.</p>
        {/if}
      </div>
      <div class="flex shrink-0 items-center gap-3 rounded-xl bg-black/20 p-3">
        {#if trumpCard}<Card card={trumpCard} size="sm" label="Trump card {cardLabel(trumpCard)}" />{/if}
        <dl class="m-0 text-sm">
          <dt class="text-xs uppercase tracking-wider text-on-felt/80">Trump</dt>
          <dd class="m-0 text-lg font-bold">{suitSymbol(gs.trump)} {gs.trump}</dd>
          <dt class="mt-1 text-xs uppercase tracking-wider text-on-felt/80">Stock</dt>
          <dd class="m-0 font-semibold tabular-nums">{gs.stock.length ? `${gs.stock.length} left` : 'Empty'}</dd>
          <dt class="mt-1 text-xs uppercase tracking-wider text-on-felt/80">Discarded</dt>
          <dd class="m-0 font-semibold tabular-nums">{gs.discard?.length || 0}</dd>
        </dl>
      </div>
    </div>
  </section>

  <div class="mt-3">
    <Seat name="You" you role={roleOf(gs, ME)} toMove={actor === ME} out={isOut(gs, ME)} cardCount={cardsOf(ME).count}>
      {@const h = cardsOf(ME)}
      <div class="flex min-h-[3.9rem] flex-wrap items-end gap-1.5">
        {#each h.known as c (cardKey(c))}<Card card={c} size="sm" />{/each}
        {#each Array(h.hidden) as _}<Card faceDown size="sm" label="Card not entered yet" />{/each}
      </div>
    </Seat>
  </div>
  <p class="mt-2 text-xs text-muted">Arrow keys step through the game; Home and End jump to the start and the end.</p>
</section>
