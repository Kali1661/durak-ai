<script>
  // Thinking-time picker: preset seconds, optionally "unlimited" (0), or a custom value.
  import { createEventDispatcher } from 'svelte'

  export let id
  export let label
  export let value // milliseconds; 0 = unlimited
  export let allowUnlimited = false
  export let min = 500 // ms
  export let max = 600000 // ms

  const PRESETS = [1000, 2000, 5000, 10000]
  const dispatch = createEventDispatcher()

  let custom = false // "Custom…" picked by hand, even if the number matches a preset
  let seconds = 30
  $: isPreset = PRESETS.includes(value) || (allowUnlimited && value === 0)
  $: choice = custom || !isPreset ? 'custom' : String(value)
  // Follow changes made elsewhere (the same setting has a picker in two places).
  $: if (value > 0) seconds = value / 1000

  function pick(e) {
    const v = e.currentTarget.value
    if (v === 'custom') {
      custom = true
      seconds = value > 0 ? value / 1000 : 30
      emitSeconds()
    } else {
      custom = false
      dispatch('change', Number(v))
    }
  }

  function emitSeconds() {
    const s = Number(seconds)
    if (!Number.isFinite(s)) return
    const ms = Math.min(max, Math.max(min, Math.round(s * 1000)))
    seconds = ms / 1000
    dispatch('change', ms)
  }
</script>

<label class="field-label" for={id}>{label}</label>
<div class="flex flex-wrap items-center gap-2">
  <select {id} class="field w-auto min-w-[9rem] flex-1" value={choice} on:change={pick}>
    {#each PRESETS as ms}<option value={String(ms)}>{ms / 1000} s</option>{/each}
    {#if allowUnlimited}<option value="0">Unlimited</option>{/if}
    <option value="custom">Custom…</option>
  </select>
  {#if choice === 'custom'}
    <span class="flex items-center gap-1">
      <input class="field w-24" type="number" inputmode="decimal" step="0.5" min={min / 1000} max={max / 1000}
        aria-label="{label} in seconds" bind:value={seconds} on:change={emitSeconds} />
      <span class="text-sm text-muted">s</span>
    </span>
  {/if}
</div>
