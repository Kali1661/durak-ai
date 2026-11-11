# Durak AI

[English](README.md) · **Slovensky**

Webový pomocník pre kartovú hru Durak. Zadáš karty a ťahy z reálnej hry
a pomocou Monte Carlo Tree Search zoradí tvoje možné ťahy, pri každom
s pravdepodobnosťou výhry.

## Obsah

| Cesta | Čo to je |
|---|---|
| `durak-online/` | Solver a webové rozhranie, založené na [jorisperrenet/durak-online](https://github.com/jorisperrenet/durak-online) (MIT), so zmenami uvedenými nižšie. |
| `.claude/launch.json` | Konfigurácia dev servera pre náhľad v Claude Code. |

## Zmeny v durak-online

**Oprava MCTS v koncovke (`durak-core/src/lib.rs`).** Pôvodné vyhľadávanie
ukladalo výhry iba z pohľadu radeného hráča a maximalizovalo ich v každom uzle,
aj keď bol na ťahu súper. Tým modelovalo súperov tak, akoby ti pomáhali vyhrať,
takže prehraté koncovky vychádzali ako takmer istá výhra. Každý uzol teraz
sleduje výsledky všetkých hráčov a hráč na ťahu volí to, čo je najlepšie preňho
(max^n), čo zároveň správne rieši hry s 3 a viac hráčmi.

| Testovacia pozícia (`durak-core/tests/endgame.rs`) | Predtým | Teraz |
|---|---|---|
| Vynútená prehra | 99,5 % | 0,2 % |
| Vynútená výhra | 100 % | 100 % |

Celkovo budú pravdepodobnosti výhry nižšie ako predtým: staré čísla boli
nadhodnotené.

**Nastavenie hry (`web/src/App.svelte`).** Krok „najnižší tromf súpera" je
odstránený, pretože vo variante, pre ktorý je nástroj určený, sa tromfy súperov
neukazujú. Kto začína, sa volí ručne.

## Spustenie

Požiadavky: [Rust](https://rustup.rs/) s cieľom `wasm32-unknown-unknown`,
[wasm-pack](https://rustwasm.github.io/wasm-pack/), Node.js 18+.

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

Použi režim **Analyse a real game**: vyber tromfovú kartu, svojich šesť kariet
a kto začína, potom zadávaj každý ťah tak, ako sa odohrá. Počet hráčov, veľkosť
balíčka a **Reflecting** (prekladanie) nastav podľa svojho stola.

Spustenie testov enginu:

```bash
cd durak-online
cargo test -p durak-core --release
```

## Poďakovanie

Solver a pôvodné rozhranie: [Joris Perrenet](https://github.com/jorisperrenet/durak-online),
licencia MIT (pozri `durak-online/LICENSE`).
