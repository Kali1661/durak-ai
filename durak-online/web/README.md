# Durak AI web app

Svelte + Vite front end. See the [root README](../../README.md) for what the
app does and how to set up Rust and wasm-pack.

```bash
npm install
npm run dev      # development server (builds the WASM engine if it is missing)
npm run wasm     # rebuild the WASM engine after changing Rust code
npm test         # unit tests + end-to-end tests against the real engine
npm run build    # rebuild the engine and write a static site to dist/
```

- `src/App.svelte` - game flow and screens
- `src/components/`, `src/lib/Card.svelte` - UI pieces
- `src/lib/aggregate.js` - pooling search results and choosing a move
- `src/lib/solver-pool.js` - worker pool (timeouts, cancellation, stale results)
- `src/lib/settings.js` - settings, limits and sanitising
- `src/app.css`, `tailwind.config.js` - design tokens and shared component styles
