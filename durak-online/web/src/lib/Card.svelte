<script>
  // A playing card. Renders a real <button> only when it does something, so
  // screen readers and keyboards are not offered dead controls.
  import { cardImageUrl, cardBackUrl, cardLabel } from './cards.js'

  export let card = null
  export let faceDown = false
  /** Clickable when true. */
  export let selectable = false
  /** Greyed out: present but not playable right now. */
  export let dim = false
  /** Chosen in a picker (reported to assistive tech as pressed). */
  export let selected = false
  /** Highlighted as the suggested move. */
  export let best = false
  /** 'sm' | 'md' | 'lg' */
  export let size = 'md'
  /** Small caption under the card, e.g. "seen" for a card everyone knows. */
  export let tag = ''
  /** Accessible name override (defaults to the card's name). */
  export let label = ''

  const sizes = {
    sm: 'w-11 h-[3.9rem] sm:w-12 sm:h-[4.25rem]',
    md: 'w-14 h-[5rem] sm:w-16 sm:h-[5.7rem]',
    lg: 'w-16 h-[5.7rem] sm:w-20 sm:h-[7.1rem]',
  }

  $: name = label || (faceDown ? 'Face-down card' : cardLabel(card))
  $: frame = `relative block shrink-0 ${sizes[size] || sizes.md} rounded-lg border bg-white p-0.5 shadow-md shadow-black/30 transition-transform
    ${best ? 'border-gold ring-2 ring-gold' : selected ? 'border-accent ring-[3px] ring-accent' : 'border-black/25'}
    ${dim ? 'opacity-45 saturate-50' : ''}`
</script>

{#if selectable}
  <button type="button" class="{frame} cursor-pointer hover:-translate-y-1 focus-visible:-translate-y-1" aria-label={name} aria-pressed={selected ? 'true' : undefined} on:click>
    <img class="h-full w-full rounded-[5px] object-contain" src={faceDown ? cardBackUrl() : cardImageUrl(card)} alt="" draggable="false" />
    {#if best}<span class="absolute -top-2 left-1/2 -translate-x-1/2 rounded-full bg-gold px-1.5 text-[10px] font-bold leading-4 text-page">Best</span>{/if}
    {#if tag}<span class="absolute inset-x-0 bottom-0 rounded-b-[5px] bg-black/70 text-center text-[10px] font-semibold leading-4 text-white">{tag}</span>{/if}
  </button>
{:else}
  <span class={frame} role="img" aria-label={name}>
    <img class="h-full w-full rounded-[5px] object-contain" src={faceDown ? cardBackUrl() : cardImageUrl(card)} alt="" draggable="false" />
    {#if best}<span class="absolute -top-2 left-1/2 -translate-x-1/2 rounded-full bg-gold px-1.5 text-[10px] font-bold leading-4 text-page">Best</span>{/if}
    {#if tag}<span class="absolute inset-x-0 bottom-0 rounded-b-[5px] bg-black/70 text-center text-[10px] font-semibold leading-4 text-white">{tag}</span>{/if}
  </span>
{/if}
