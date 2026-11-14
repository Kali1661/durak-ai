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
| `durak-native/` | The native program: serves the built app and answers its searches (`/api/solve`, `/api/exact`). |
| `web/` | The browser app. Tests in `web/tests/`. |

```bash
cargo test -p durak-core --release     # engine tests
cd web && npm install && npm test      # browser-side tests
npm run dev                            # http://localhost:5173
```

Measuring the engine (see the doc comments in each file):

```bash
cargo run --release -p durak-core --example arena -- match mcts-h mcts 200 16 1000
cargo run --release -p durak-core --example arena -- positions durak-core/examples/data/positions.txt
cargo run --release -p durak-core --example evalfit -- 4000 24
cargo run --release -p durak-core --example playfit -- 2000 24
```

Measuring it against your own games - export them from the Games panel ("Export all (JSON)") first:

```bash
cargo run --release -p durak-core --example calibrate -- durak-ai-games.json
cargo run --release -p durak-core --example realgames -- durak-ai-games.json
```

`calibrate` searches every position of your finished real games again and compares the win percentages with how the games ended; give it several engine variants to see which reads real opponents best. `realgames` shows what your opponents actually did: how good each rule for guessing their hand is, how often they picked up cards they could have beaten, and how often the imagined player of the search would have made the move that was really made.

Finding mistakes in your own games: export them from the Games panel ("Export all (JSON)"), then

```bash
cargo run --release -p durak-core --example review -- durak-ai-games.json --lost
```

checks your moves late in each two-player game against perfect play and lists the ones that gave the game away, saying whether each was the engine's suggestion or your own choice.
