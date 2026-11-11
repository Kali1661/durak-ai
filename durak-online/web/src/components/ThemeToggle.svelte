<script>
  import { onMount } from 'svelte'

  let dark = false
  onMount(() => { dark = document.documentElement.classList.contains('dark') })

  function toggle() {
    dark = !dark
    document.documentElement.classList.toggle('dark', dark)
    try { localStorage.setItem('theme', dark ? 'dark' : 'light') } catch { /* private mode: theme just won't persist */ }
  }
</script>

<button type="button" class="btn btn-sm" on:click={toggle} aria-pressed={dark ? 'true' : 'false'} title="Switch between light and dark">
  <span aria-hidden="true">{dark ? '☾' : '☀'}</span>
  <span>{dark ? 'Dark' : 'Light'}<span class="sr-only"> theme (press to switch)</span></span>
</button>
