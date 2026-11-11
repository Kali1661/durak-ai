import { defineConfig } from 'vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'

// A relative base lets the production build be served from any path
// (a domain root, a sub-folder, GitHub Pages) without rebuilding.
export default defineConfig({
  plugins: [svelte()],
  base: './',
  worker: { format: 'es' },
})
