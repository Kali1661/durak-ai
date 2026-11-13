<script>
  // My own rating of how strong the opponent in a real game seemed. Clicking
  // the chosen rating again removes it.
  import { createEventDispatcher } from 'svelte'
  import { OPPONENTS } from '../lib/replays.js'

  /** 'weak' | 'average' | 'strong' | null */
  export let value = null

  const dispatch = createEventDispatcher()
  const TEXT = { weak: 'Weak', average: 'Average', strong: 'Strong' }
</script>

<div>
  <p class="m-0 text-xs font-semibold text-muted">How strong was the opponent?</p>
  <div class="mt-1 flex flex-wrap gap-2" role="group" aria-label="How strong was the opponent?">
    {#each OPPONENTS as level (level)}
      <button
        type="button"
        class="btn btn-sm {value === level ? 'border-gold bg-gold/10' : ''}"
        aria-pressed={value === level ? 'true' : 'false'}
        on:click={() => dispatch('change', value === level ? null : level)}
      >{TEXT[level]}{#if value === level}<span aria-hidden="true"> ✓</span>{/if}</button>
    {/each}
  </div>
</div>
