# Durak AI

**English** · [Slovensky](README.sk.md)

A browser app for the card game [Durak](https://en.wikipedia.org/wiki/Durak), for
**two or three players**. Play against computer opponents, or follow along with a
game you are playing elsewhere and get move suggestions.

Everything runs in your browser: the game engine is Rust compiled to
WebAssembly, and nothing is sent to a server.

## What you can do

**Play the computer.** The app deals, and you play one or two computer
opponents. Each opponent either searches for a good move (you choose how long
it may think) or plays at random. Suggestions for your own moves can be turned
on or off at any time.

**Analyse a real game.** Enter the trump card, your six cards and who attacks
first, then record each move as it happens at the table. Whenever it is your
turn the app ranks your legal moves. When you draw cards you tell the app which
ones; once the stock is empty and only one opponent's cards are unknown, the
app works them out for you.

In both modes you can undo, games are saved in the browser and restored when
you come back, and the app works on phones, tablets and desktops in light or
dark theme.

**Games** (top right) keeps a recording of every game you play or track:

- statistics - win rate, wins, losses and draws, how often you followed the
  suggestions, and breakdowns by mode and by table;
- a replay of any game, one move at a time, showing what the search suggested
  before each of your moves and whether you played it;
- a way to set the real result when the app got it wrong, which the statistics
  then use;
- an export of all recordings as one JSON file.

Recordings stay in this browser only. You choose how long suggestions search
for - 1, 2, 5 or 10 seconds, a custom time, or unlimited until you stop it.

## Rules implemented

- 2 or 3 players; decks of 24 (nine to ace), 36 (six to ace) or 52 cards;
  six-card hands.
- **Cards per attack** (chosen when you start; 1, 2 or any): how many cards an
  attacker may lay down - the first one, then more of a rank already on the
  table, such as a pair of aces - before the defender answers. With *any*, all
  four nines can go down at once. When entering an opponent's attack in a real
  game, enter each card: the app assumes they added nothing more, and if they
  did, press *They added more cards*.
- At most six attack cards per trick, and never more unanswered cards than the
  defender holds.
- Throw-in: every attacker may add cards whose rank is already on the table.
- After a trick, attackers draw first (the main attacker first), the defender
  last. A defender who takes the cards misses their turn to attack.
- In dealt games, whoever holds the lowest trump attacks first.
- The game ends when the stock is empty and at most one player still holds
  cards. That player is the durak.

Chosen when you start a game, with buttons modelled on the Durak Online app:

- **throw-in / passing** - with *passing*, before beating anything the defender
  may play a card of the same rank to pass the whole attack to the next player,
  provided that player holds enough cards to answer it. With two players the
  attack goes back to the original attacker.
- **classic / draw** - what happens when the last defender beats the final
  attack with their last card, so the last two players run out together.
  *Draw*: the game is a draw. *Classic*: there are no draws, and that defender
  is the durak, because the attacker ran out of cards first.
- **Five cards before the first discard** (on by default) - until cards have
  been discarded for the first time, a trick holds at most five attack cards.
- **Pass by showing a trump** (off by default) - with passing on, the defender
  may also pass by showing the trump of that rank and keeping it, once per trump
  per trick. Durak Online does not have this rule.

To mirror a Durak Online game, choose the same players, deck, *throw-in* or
*passing* and *classic* or *draw* as in the app, keep the five-card rule on and
the trump rule off. In that app pick *fair play*; *neighbors* and *all* behave
the same with two or three players. Set *cards per attack* to *any*: that app
lets an attacker lay down several cards at once.

## About the suggestions

Suggestions come from a Monte Carlo tree search. The cards you cannot see are
dealt at random many times; each deal is searched and played out to the end.
The percentage shown next to a move is its **score**: how often, across those
simulated games, you did not end up as the durak (a draw counts as not losing;
a simulation cut off before the end counts as half).

Treat the score as a way to compare moves, not as an exact probability. It
depends on random deals and largely random play-outs, it varies from run to
run, and the search can be wrong. The number of simulations behind each score
is shown so you can judge how settled it is, and "Search longer" adds more.
**Calculated results.** In a two-player game, once no cards are hidden - the
stock is empty or down to the trump card - the app stops estimating and works
the result out by checking every line of play. Moves are then marked *Wins*,
*Draw* or *Loses*: a move that wins does so against any defence. A draw means
both players run out together, or neither can make progress without losing
(with passing, play can go round in circles). The estimate stays next to it,
which helps choose between moves with the same calculated result - in a lost
position, the move that gives the opponent the most chances to go wrong.

Positions with many cards can be too large to finish in the time available;
the app then shows the result for the moves it settled and estimates for the
rest. In games with three players, and while cards are still hidden, every
score is an estimate.

## Running it

**New to this? Follow the [step-by-step installation guide](INSTALL.md).** It
covers installing the tools, downloading the project and fixing common errors.

In short, with [Git](https://git-scm.com/), [Node.js](https://nodejs.org/) 18+,
[Rust](https://rustup.rs/) and [wasm-pack](https://rustwasm.github.io/wasm-pack/installer/)
installed:

```bash
git clone https://github.com/Kali1661/durak-ai.git
cd durak-ai/durak-online/web
npm install
npm run dev
```

then open <http://localhost:5173>. Run each line on its own and check it
worked before the next. After installing a tool, open a new terminal window.

## Project layout

```
durak-online/
├── durak-core/   Rust engine: rules, validation, search, solver  (tests in tests/)
├── durak-wasm/   WebAssembly bindings
└── web/          Svelte app                                      (tests in tests/)
```

## Relationship to the original

This project started from
[durak-online](https://github.com/jorisperrenet/durak-online) by Joris Perrenet
and keeps its overall design: the game model, the determinized Monte Carlo
search running in WebAssembly workers, and the card artwork.

Most of the code has since been rewritten and improved. The engine was reworked
to validate every input and saved game, to check its own rules with an
automated test suite, and to score positions for each player separately. The
search, the browser app and the interface were rebuilt from scratch.

Compared with the original, this version:

- supports two- and three-player tables only;
- lets you choose who attacks first in a real game;
- offers the classic or draw ending and the five-card first trick, as in Durak Online;
- shows how many simulations are behind each suggestion;
- has a new interface with a phone layout and keyboard and screen-reader support.

Any mistakes in this version are its own, not the original author's.

## Credits and licence

The game engine and the original web app are by
[Joris Perrenet](https://github.com/jorisperrenet/durak-online) and are used
under the MIT licence; see [`durak-online/LICENSE`](durak-online/LICENSE). The
changes in this repository are released under the same licence.

Card images come from the original project.
