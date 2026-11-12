<script>
  // Two exclusive options stacked in one tile, like the game-mode tiles in the
  // Durak Online app: the chosen half turns light with a check mark.
  import { createEventDispatcher } from 'svelte'
  import ModeIcon from './ModeIcon.svelte'

  /** Accessible name of the whole choice, e.g. "Passing the attack on". */
  export let label
  /** [{ value, title, icon, hint }, { ... }] - top then bottom. */
  export let options
  export let value

  const dispatch = createEventDispatcher()

  function onKey(e, i) {
    // Arrow keys move between the two halves, as in any radio group.
    if (!['ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight'].includes(e.key)) return
    e.preventDefault()
    const next = options[1 - i]
    dispatch('change', next.value)
    e.currentTarget.parentElement.querySelectorAll('[role=radio]')[1 - i]?.focus()
  }
</script>

<div class="mode-tile" role="radiogroup" aria-label={label}>
  {#each options as opt, i (opt.value)}
    {@const on = opt.value === value}
    <button
      type="button"
      role="radio"
      aria-checked={on ? 'true' : 'false'}
      tabindex={on ? 0 : -1}
      title={opt.hint}
      class="mode-half {i === 0 ? 'rounded-t-2xl' : 'rounded-b-2xl'}"
      on:click={() => dispatch('change', opt.value)}
      on:keydown={e => onKey(e, i)}
    >
      <ModeIcon name={opt.icon} />
      <span class="text-sm font-semibold leading-tight">{opt.title}</span>
      {#if on}<span class="mode-check" aria-hidden="true">✓</span>{/if}
    </button>
  {/each}
</div>
