<script>
  // A row of exclusive choices in one pill, like the player-count and deck
  // pickers in the Durak Online app: the chosen segment is light with a check.
  import { createEventDispatcher } from 'svelte'

  export let label
  /** [{ value, text, hint? }] */
  export let options
  export let value

  const dispatch = createEventDispatcher()

  function onKey(e, i) {
    const step = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 }[e.key]
    if (!step) return
    e.preventDefault()
    const j = (i + step + options.length) % options.length
    dispatch('change', options[j].value)
    e.currentTarget.parentElement.querySelectorAll('[role=radio]')[j]?.focus()
  }
</script>

<div class="pill" role="radiogroup" aria-label={label}>
  {#each options as opt, i (opt.value)}
    {@const on = opt.value === value}
    <button
      type="button"
      role="radio"
      aria-checked={on ? 'true' : 'false'}
      tabindex={on ? 0 : -1}
      title={opt.hint}
      class="pill-item"
      on:click={() => dispatch('change', opt.value)}
      on:keydown={e => onKey(e, i)}
    >
      <span class="font-display text-2xl font-bold sm:text-3xl">{opt.text}</span>
      {#if on}<span class="text-lg font-black" aria-hidden="true">✓</span>{/if}
    </button>
  {/each}
</div>
