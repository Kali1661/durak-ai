# Inštalácia a spustenie Durak AI

[English](INSTALL.md) · **Slovensky** · [Späť na README](README.sk.md)

Návod nepredpokladá žiadne skúsenosti s programovaním. Je napísaný pre
**Windows**; macOS a Linux sú na konci. Prvá príprava trvá 15-30 minút, väčšinou
čakaním na sťahovanie. Potom sa aplikácia spúšťa jedným príkazom.

## Skôr než začneš: ako zadávať príkazy

Príkazy budeš písať do **Príkazového riadka (Command Prompt)**:

1. Stlač **kláves Windows**, napíš `cmd` a stlač **Enter**. Otvorí sa čierne okno.
2. Napíš (alebo vlož) **vždy len jeden príkaz**, stlač **Enter** a počkaj, kým
   skončí - blikajúci kurzor sa vráti za riadok ako `C:\Users\TvojeMeno>`.
   Až potom zadaj ďalší.
3. Pred pokračovaním si prečítaj, čo vypísal. Ak vložíš viac riadkov naraz,
   Príkazový riadok ich spustí jeden po druhom, aj keď niektorý zlyhal, a chyba,
   ktorú potrebuješ vidieť, sa stratí.

> **Najdôležitejšie pravidlo:** po inštalácii programu v krokoch 1-3 **zavri
> okno Príkazového riadka a otvor nové.** Okno pozná len programy nainštalované
> *predtým*, než bolo otvorené. Takmer každá chyba „is not recognized" vznikne
> preskočením tohto kroku.

## Krok 1 - Nainštaluj nástroje

Potrebuješ štyri programy. Spusti tieto tri riadky po jednom; každý stiahne a
nainštaluje program (Windows sa môže pýtať na povolenie - klikni **Áno**):

```bat
winget install --id Git.Git -e
winget install --id OpenJS.NodeJS.LTS -e
winget install --id Rustlang.Rustup -e
```

Ak samotný príkaz `winget` „is not recognized", aktualizuj **App Installer**
v Microsoft Store, alebo stiahni inštalátory ručne:
[Git](https://git-scm.com/download/win),
[Node.js (LTS)](https://nodejs.org/),
[Rust](https://rustup.rs/).

**Teraz zavri Príkazový riadok a otvor nový.**

Over, že je všetko na mieste - každý riadok má vypísať číslo verzie:

```bat
git --version
node --version
cargo --version
```

## Krok 2 - Pridaj do Rustu podporu WebAssembly

Spusti tieto dva riadky (druhý trvá **5-10 minút**; je normálne, že vypíše
množstvo riadkov „Compiling ..."):

```bat
rustup target add wasm32-unknown-unknown
cargo install wasm-pack
```

**Znova zavri Príkazový riadok a otvor nový**, potom over:

```bat
wasm-pack --version
```

## Krok 3 - Stiahni projekt

Vyber si miesto, napríklad priečinok Dokumenty, a stiahni ho tam:

```bat
cd %USERPROFILE%\Documents
git clone https://github.com/Kali1661/durak-ai.git
```

Vznikne priečinok `Documents\durak-ai`.

*Bez Gitu:* na [stránke projektu](https://github.com/Kali1661/durak-ai) klikni na
zelené tlačidlo **Code**, potom **Download ZIP** a rozbaľ ho do Dokumentov.
Priečinok sa bude volať `durak-ai-main` - v ďalšom kroku použi tento názov.

## Krok 4 - Nainštaluj aplikáciu a spusti ju

Prejdi do priečinka aplikácie. Príkazy fungujú len z **presne tohto priečinka**:

```bat
cd %USERPROFILE%\Documents\durak-ai\durak-online\web
```

Riadok pred kurzorom by mal teraz končiť na `durak-online\web>`. Potom:

```bat
npm install
npm run dev
```

`npm install` stiahne, čo aplikácia potrebuje (asi minútu, stačí raz).
`npm run dev` pri prvom spustení zostaví herný engine (minútu-dve) a potom
vypíše niečo ako:

```
  VITE ready in 900 ms
  ➜  Local:   http://localhost:5173/
```

Otvor **http://localhost:5173** vo webovom prehliadači. To je aplikácia.

Počas používania aplikácie **nechaj okno Príkazového riadka otvorené** - jeho
zatvorenie aplikáciu vypne. Ak ju chceš vypnúť zámerne, klikni do okna a stlač
**Ctrl + C**.

## Nabudúce

Otvor Príkazový riadok a spusti len tieto dva riadky:

```bat
cd %USERPROFILE%\Documents\durak-ai\durak-online\web
npm run dev
```

## Aktualizácia na najnovšiu verziu

```bat
cd %USERPROFILE%\Documents\durak-ai
git pull
cd durak-online\web
npm install
npm run dev
```

`npm run dev` sám zistí, že sa herný engine zmenil, a zostaví ho znova.

## Keď niečo nefunguje

| Čo vidíš | Čo to znamená a čo robiť |
|---|---|
| `'git'`, `'node'`, `'npm'`, `'cargo'` alebo `'wasm-pack'` **is not recognized as an internal or external command** | Program nie je nainštalovaný, alebo bolo okno otvorené pred jeho inštaláciou. Zavri Príkazový riadok, otvor nový a skús znova. Ak to pretrváva, zopakuj krok s daným programom. |
| `The system cannot find the path specified.` po príkaze `cd` | Priečinok nie je tam, kde ho príkaz čaká. Napíš `dir` a uvidíš obsah aktuálneho priečinka, alebo skontroluj, kam si projekt stiahol (krok 3). Pri ZIP-e je priečinok `durak-ai-main`. |
| `npm ERR! enoent ... package.json` | Si v nesprávnom priečinku. Najprv spusti riadok `cd ...\durak-online\web` z kroku 4. |
| `Could not build the game engine` | Chýba wasm-pack: urob krok 2 a otvor nové okno. |
| `linker 'link.exe' not found` (počas kroku 2) | Rust na Windows potrebuje C++ nástroje od Microsoftu. Spusti riadok nižšie, počkaj na koniec (je to veľké sťahovanie), otvor nové okno a zopakuj krok 2. |
| `Port 5173 is in use, trying another one...` | Aplikácia už pravdepodobne beží v inom okne. To nevadí - otvor adresu, ktorú vypíše nové okno (napríklad `http://localhost:5174`). |
| V PowerShelli: `running scripts is disabled on this system` | Použi Príkazový riadok (`cmd`) namiesto PowerShellu, tak ako v tomto návode. |
| Stránka je prázdna alebo ukazuje starú verziu | Stlač v prehliadači **Ctrl + F5** na úplné obnovenie. |

C++ nástroje (len ak sa objavila chyba s `link.exe`):

```bat
winget install --id Microsoft.VisualStudio.2022.BuildTools -e --override "--passive --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
```

Ak sa zasekneš, [založ issue](https://github.com/Kali1661/durak-ai/issues) a vlož
doň **celý** text z okna Príkazového riadka.

## macOS a Linux

Nainštaluj [Git](https://git-scm.com/), [Node.js 18 alebo novší](https://nodejs.org/)
a [Rust](https://rustup.rs/) (na macOS najprv `xcode-select --install`, potom Node
a Rust z ich stránok alebo cez Homebrew). Potom v termináli:

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-pack
git clone https://github.com/Kali1661/durak-ai.git
cd durak-ai/durak-online/web
npm install
npm run dev
```

a otvor <http://localhost:5173>.

## Pre vývojárov

| Príkaz (v `durak-online/web`) | Čo robí |
|---|---|
| `npm run dev` | Spustí vývojový server (ak sa zmenil kód v Ruste, engine zostaví znova) |
| `npm run build` | Zostaví engine a vytvorí statickú stránku v `dist/`, ktorú možno umiestniť kamkoľvek |
| `npm run wasm` | Zostaví iba engine |
| `npm test` | Testy webovej časti vrátane end-to-end testov so skutočným enginom |

Testy enginu: `cd durak-online` a potom `cargo test -p durak-core --release`.
