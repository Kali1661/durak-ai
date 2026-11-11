# durak-online (engine and web app)

This folder holds the Rust engine, its WebAssembly bindings and the Svelte web
app of **Durak AI**. The user-facing documentation - features, rules, build and
run instructions - is in the [root README](../README.md)
([Slovensky](../README.sk.md)).

It is based on [durak-online](https://github.com/jorisperrenet/durak-online) by
Joris Perrenet, used under the MIT licence (see [LICENSE](LICENSE)).

| Folder | Contents |
|---|---|
| `durak-core/` | Rules, state validation, hidden-card handling and the MCTS search. Tests in `durak-core/tests/`. |
| `durak-wasm/` | `wasm-bindgen` wrapper exposing the engine to JavaScript. |
| `web/` | The browser app. Tests in `web/tests/`. |

```bash
cargo test -p durak-core --release     # engine tests
cd web && npm install && npm test      # browser-side tests
npm run dev                            # http://localhost:5173
```
