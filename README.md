# Durak AI

**English** · [Slovensky](README.sk.md)

A browser-based helper for the card game Durak. Enter the cards and moves from a
real game and it ranks your legal moves with Monte Carlo Tree Search, showing a
win probability for each.

## What's in here

| Path | What it is |
|---|---|
| `durak-online/` | The solver and web UI, based on [jorisperrenet/durak-online](https://github.com/jorisperrenet/durak-online) (MIT), with the changes below. |
| `.claude/launch.json` | Dev-server config for Claude Code's preview pane. |

## Changes to durak-online

**MCTS endgame fix (`durak-core/src/lib.rs`).** The original search stored wins
only from the advised player's point of view and maximised them at every node,
including opponents' turns. That modelled opponents as playing to help you, so
lost endgames evaluated as near-certain wins. Each node now tracks results for
every player and whoever is to move picks what is best for themselves (max^n),
which also handles 3+ player games correctly.

| Test position (`durak-core/tests/endgame.rs`) | Before | After |
|---|---|---|
| Forced loss | 99.5 % | 0.2 % |
| Forced win | 100 % | 100 % |

Expect win rates to look lower overall than before: the old numbers were inflated.

**Game setup (`web/src/App.svelte`).** The "opponent's lowest trump" step is
removed, since opponents' trumps aren't revealed in the variant this is built
for. Who starts is chosen manually.

## Running it

Requirements: [Rust](https://rustup.rs/) with the `wasm32-unknown-unknown`
target, [wasm-pack](https://rustwasm.github.io/wasm-pack/), Node.js 18+.

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-pack

cd durak-online/durak-wasm
wasm-pack build --target web
mkdir -p ../web/src/wasm
cp pkg/durak_wasm* ../web/src/wasm/

cd ../web
npm install
npm run dev        # http://localhost:5173
```

Use **Analyse a real game** mode: pick the trump card, your six cards and who
starts, then enter each move as it happens. Set the player count, deck size and
**Reflecting** to match your table.

Run the engine tests:

```bash
cd durak-online
cargo test -p durak-core --release
```

## Credits

Solver and original UI: [Joris Perrenet](https://github.com/jorisperrenet/durak-online),
MIT licence (see `durak-online/LICENSE`).
