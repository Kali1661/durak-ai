<script>
  // Game-mode icons, drawn after the ones in the Durak Online app.
  // Every shape uses currentColor, so the icon follows the tile's text colour.
  /** 'throw-in' | 'passing' | 'classic' | 'draw' */
  export let name
</script>

<svg viewBox="0 0 48 48" class="h-10 w-10 sm:h-12 sm:w-12" aria-hidden="true" focusable="false">
  {#if name === 'throw-in'}
    <!-- block arrow pointing right, as if pushing a card onto the table -->
    <path fill="currentColor" d="M5 17.5h18V9l20 15-20 15v-8.5H5z" />
  {:else if name === 'passing'}
    <!-- a card with a circular arrow: the attack is passed on -->
    <rect x="11" y="5" width="26" height="38" rx="4" fill="currentColor" />
    <path d="M31 24a7 7 0 1 1-2.6-5.4" fill="none" stroke="rgb(var(--tile))" stroke-width="3" stroke-linecap="round" />
    <path d="M30.5 13.5l-1.6 6.2-6-2.4z" fill="rgb(var(--tile))" />
  {:else if name === 'classic'}
    <!-- a card showing the fool's (durak's) cap -->
    <rect x="11" y="5" width="26" height="38" rx="4" fill="currentColor" />
    <path fill="rgb(var(--tile))" d="M16 28c2-6 5-9 8-9s6 3 8 9zM16 28l-3.5-5.5 4.5 1.5zM32 28l3.5-5.5-4.5 1.5zM24 19l-1-5 2 1.5 1-1z" />
    <rect x="15.5" y="28" width="17" height="3" rx="1.5" fill="rgb(var(--tile))" />
    <circle cx="12.5" cy="22.5" r="1.6" fill="rgb(var(--tile))" />
    <circle cx="35.5" cy="22.5" r="1.6" fill="rgb(var(--tile))" />
  {:else if name === 'draw'}
    <!-- a domino tile: two equal halves -->
    <rect x="15" y="4" width="18" height="40" rx="4" fill="currentColor" />
    <rect x="17" y="23" width="14" height="2" fill="rgb(var(--tile))" />
    <circle cx="24" cy="14" r="2.8" fill="rgb(var(--tile))" />
    <circle cx="24" cy="34" r="2.8" fill="rgb(var(--tile))" />
  {/if}
</svg>
