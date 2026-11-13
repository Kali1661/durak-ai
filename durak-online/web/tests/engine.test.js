// End-to-end: the real WASM engine -> result pooling -> move selection.
// This is the path a computer opponent's move takes in the app.

import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

import { addResult, createTally, pickBest, summarize, withExact } from '../src/lib/aggregate.js'
import { actorOf } from '../src/lib/game.js'

const wasm = await import('../src/wasm/durak_wasm.js')
wasm.initSync({ module: readFileSync(fileURLToPath(new URL('../src/wasm/durak_wasm_bg.wasm', import.meta.url))) })

const card = (suit, rank, type = 'public') => ({ type, suit, rank })
const config = (extra = {}) => ({ deck_size: 36, num_players: 2, trump_reflecting: false, reflecting: false, ...extra })
const newGame = (seed, extra) => wasm.new_computer_game({ seed: BigInt(seed) }, config(extra))

/** Run a few "workers" and pick a move exactly as the app does. */
function chooseMove(state, { batches = 3, determinizations = 4, rollouts = 400 } = {}) {
  const tally = createTally()
  for (let i = 0; i < batches; i++) {
    addResult(tally, wasm.solve({ state, determinizations, rollouts_per_determinization: rollouts, seed: BigInt(i + 1) }))
  }
  const summary = summarize(tally, wasm.legal_actions(state))
  return { summary, best: pickBest(summary) }
}

test('the solver returns visits, score and cutoffs for every legal action', () => {
  const state = newGame(1)
  const result = wasm.solve({ state, determinizations: 3, rollouts_per_determinization: 200, seed: 7n })
  const legal = wasm.legal_actions(state)
  assert.equal(result.actions.length, legal.length)
  assert.equal(result.total_visits, 600)
  assert.equal(result.perspective, actorOf(state))
  for (const a of result.actions) {
    assert.ok(Number.isInteger(a.visits) && a.visits > 0)
    assert.ok(a.score >= 0 && a.score <= 1)
    assert.ok(Number.isInteger(a.cutoffs))
    assert.equal(a.wins, undefined, 'there is no wins/losses/draws schema')
  }
})

test('a computer opponent plays the move the search prefers, not a random one', () => {
  // Three players, stock empty, hearts trump. P0 holds 6C + AH. Leading the six
  // keeps the trump ace for the attack that follows, which gets P0 out;
  // leading the ace gives that away.
  const all = []
  for (const suit of ['Clubs', 'Diamonds', 'Hearts', 'Spades']) {
    for (const rank of ['Six', 'Seven', 'Eight', 'Nine', 'Ten', 'Jack', 'Queen', 'King', 'Ace']) all.push(card(suit, rank))
  }
  const hands = [
    [card('Clubs', 'Six'), card('Hearts', 'Ace')],
    [card('Diamonds', 'Seven')],
    [card('Diamonds', 'Eight'), card('Diamonds', 'Nine'), card('Diamonds', 'Ten')],
  ]
  const inHand = new Set(hands.flat().map(c => `${c.suit}${c.rank}`))
  const state = {
    trump: 'Hearts', attacker: 'P0', defender: 'P1', phase: 'Attacking',
    attackers: ['P0', 'P2'], current_attacker_idx: 0, last_played_attacker: 'P0', throw_start_idx: 0,
    hands, stock: [], table: [], reflected_trumps: [],
    discard: all.filter(c => !inHand.has(`${c.suit}${c.rank}`)),
    config: config({ num_players: 3 }),
  }
  wasm.validate_state(state)

  const { summary, best } = chooseMove(state, { batches: 4, determinizations: 1, rollouts: 4000 })
  assert.ok(best, 'a move was chosen')
  assert.equal(best.action.card.rank, 'Six')
  const ace = summary.actions.find(a => a.action.card.rank === 'Ace')
  assert.ok(best.score > ace.score + 0.1, `six ${best.score} vs ace ${ace.score}`)
  // The chosen move is one of the legal actions and applies cleanly.
  wasm.apply_action(state, best.action)
})

test('a full game between search players finishes with a consistent result', () => {
  for (const extra of [{ num_players: 2 }, { num_players: 3, reflecting: true, trump_reflecting: true }]) {
    let state = newGame(42, extra)
    let moves = 0
    while (wasm.game_outcome(state).status === 'in_progress') {
      const { best } = chooseMove(state, { batches: 1, determinizations: 2, rollouts: 60 })
      assert.ok(best, `move ${moves}: the search always yields a move in a running game`)
      state = wasm.apply_action(state, best.action)
      assert.ok(++moves < 2000, 'the game must finish')
    }
    const outcome = wasm.game_outcome(state)
    assert.ok(['durak', 'draw'].includes(outcome.status))
    assert.deepEqual(wasm.legal_actions(state), [])
    const holders = state.hands.map((h, i) => (h.length ? `P${i}` : null)).filter(Boolean)
    if (outcome.status === 'durak') {
      assert.deepEqual(holders, [outcome.player])
      assert.equal(wasm.get_durak(state), outcome.player)
    } else {
      assert.deepEqual(holders, [])
    }
  }
})

test('a drawn game is reported as a draw, not as still running', () => {
  const rest = []
  for (const suit of ['Clubs', 'Diamonds', 'Hearts', 'Spades']) {
    for (const rank of ['Six', 'Seven', 'Eight', 'Nine', 'Ten', 'Jack', 'Queen', 'King', 'Ace']) {
      if (suit === 'Clubs' && (rank === 'King' || rank === 'Ace')) continue
      rest.push(card(suit, rank))
    }
  }
  let state = {
    trump: 'Hearts', attacker: 'P0', defender: 'P1', phase: 'Attacking',
    attackers: ['P0'], current_attacker_idx: 0, last_played_attacker: 'P0', throw_start_idx: 0,
    hands: [[card('Clubs', 'King')], [card('Clubs', 'Ace')]],
    stock: [], table: [], reflected_trumps: [], discard: rest, config: config(),
  }
  state = wasm.apply_action(state, { type: 'attack', card: card('Clubs', 'King') })
  assert.equal(wasm.game_outcome(state).status, 'in_progress')
  state = wasm.apply_action(state, { type: 'defend', pile_index: 0, card: card('Clubs', 'Ace') })
  assert.deepEqual(wasm.game_outcome(state), { status: 'draw' })
  assert.equal(wasm.get_durak(state) ?? null, null)
})

test('the engine rejects bad input with a readable message instead of crashing', () => {
  const state = newGame(3)
  const fails = (fn, pattern) => assert.throws(fn, e => pattern.test(String(e)), String(pattern))

  fails(() => wasm.new_computer_game({ seed: 1n }, config({ num_players: 4 })), /2 or 3/)
  fails(() => wasm.new_computer_game({ seed: 1n }, config({ num_players: 6 })), /2 or 3/)
  fails(() => wasm.new_computer_game({ seed: 1n }, config({ deck_size: 32 })), /not supported/)
  fails(() => wasm.new_computer_game({ seed: 1n }, { deck_size: 'big' }), /could not read/)
  fails(() => wasm.legal_actions({ nonsense: true }), /could not read/)
  fails(() => wasm.legal_actions({ ...state, hands: [state.hands[0]] }), /invalid game state/)
  fails(() => wasm.apply_action(state, { type: 'take' }), /illegal move/)
  fails(() => wasm.apply_action(state, { type: 'fly' }), /could not read/)
  fails(() => wasm.solve({ state, determinizations: 0 }), /between 1 and/)
  fails(() => wasm.solve({ state, rollouts_per_determinization: 10_000_000 }), /between 1 and/)

  const hand = ['Six', 'Seven', 'Eight', 'Nine', 'Ten', 'Jack'].map(r => card('Clubs', r))
  const setup = { trump_card: card('Hearts', 'Six'), player_hand: hand, starting_player: 0, opponent_trumps: [] }
  fails(() => wasm.new_manual_game({ ...setup, starting_player: 2 }, config()), /only has players/)
  fails(() => wasm.new_manual_game({ ...setup, player_hand: hand.slice(1) }, config()), /exactly 6/)
  fails(() => wasm.new_manual_game({ ...setup, trump_card: hand[0] }, config()), /cannot also be in your hand/)
  fails(() => wasm.new_manual_game({ ...setup, player_hand: [...hand.slice(1), hand[1]] }, config()), /twice/)

  // A rejected action leaves the caller's state object untouched.
  const before = JSON.stringify(state)
  assert.throws(() => wasm.apply_action(state, { type: 'pass_attack' }))
  assert.equal(JSON.stringify(state), before)
})

test('a real-game position can be entered, searched and updated with drawn cards', () => {
  const hand = ['Six', 'Seven', 'Eight', 'Nine', 'Ten', 'Jack'].map(r => card('Clubs', r))
  let state = wasm.new_manual_game(
    { trump_card: card('Hearts', 'Six'), player_hand: hand, starting_player: 0, opponent_trumps: [] },
    config({ num_players: 3 }),
  )
  assert.equal(state.hands.length, 3)
  assert.ok(state.hands[1].every(c => c.type === 'unknown'))

  const { best } = chooseMove(state, { batches: 1, determinizations: 4, rollouts: 100 })
  assert.equal(best.action.type, 'attack')

  // I attack; P1 beats it with a card I had not seen; everyone passes.
  state = wasm.apply_action(state, { type: 'attack', card: card('Clubs', 'Six') })
  state = wasm.apply_action(state, { type: 'defend', pile_index: 0, card: card('Clubs', 'Ace') })
  assert.equal(state.hands[1].length, 5)
  state = wasm.apply_action(state, { type: 'pass_attack' })
  state = wasm.apply_action(state, { type: 'pass_attack' })

  // I drew one card and must say which before the engine will search for me.
  assert.equal(state.hands[0].filter(c => c.type === 'unknown').length, 1)
  assert.throws(() => wasm.reveal_card(state, 'P0', card('Clubs', 'Ace')), /already accounted for/)
  state = wasm.reveal_card(state, 'P0', card('Spades', 'King'))
  assert.ok(state.hands[0].every(c => c.type !== 'unknown'))
  wasm.validate_state(state)
})

test('classic ending: the defender who beats the last card with their last card is the durak', () => {
  // Both players down to one card, stock empty: the attacker leads the king, the defender holds the ace.
  const lastCards = classic => {
    const s = newGame(1, { deck_size: 24, classic_ending: classic })
    const pub = c => ({ ...c, type: 'public' })
    const all = [...s.hands.flat(), ...s.stock].map(pub)
    const king = all.find(c => c.suit === 'Clubs' && c.rank === 'King')
    const ace = all.find(c => c.suit === 'Clubs' && c.rank === 'Ace')
    const position = {
      ...s,
      attacker: 'P1', defender: 'P0', attackers: ['P1'], current_attacker_idx: 0, last_played_attacker: 'P1',
      phase: 'Attacking', table: [], stock: [],
      hands: [[ace], [king]],
      discard: all.filter(c => c !== king && c !== ace),
    }
    wasm.validate_state(position)
    return wasm.apply_action(position, { type: 'attack', card: king })
  }

  const classic = lastCards(true)
  assert.equal(wasm.game_outcome(classic).status, 'in_progress')
  const beat = wasm.legal_actions(classic).find(a => a.type === 'defend')
  assert.deepEqual(wasm.game_outcome(wasm.apply_action(classic, beat)), { status: 'durak', player: 'P0' })
  // The search must not call that a win for the defender.
  const { summary } = chooseMove(classic)
  for (const a of summary.actions) assert.equal(a.score, 0, `${a.key} loses under the classic ending`)

  const draw = lastCards(false)
  const beatDraw = wasm.legal_actions(draw).find(a => a.type === 'defend')
  assert.equal(wasm.game_outcome(wasm.apply_action(draw, beatDraw)).status, 'draw')
})

test('exact solver: an endgame with every card known is calculated, a fresh game is not', () => {
  const fresh = newGame(3, { deck_size: 24 })
  assert.equal(wasm.is_exactly_solvable(fresh), false)
  assert.equal(wasm.solve_exact({ state: fresh }) ?? null, null)

  // Play on (the same way every run) until the stock is gone and few cards are left.
  let state = newGame(7, { deck_size: 24, reflecting: true, classic_ending: true })
  for (let i = 0; i < 3000; i++) {
    if (wasm.game_outcome(state).status !== 'in_progress') break
    const cards = state.hands[0].length + state.hands[1].length
    if (wasm.is_exactly_solvable(state) && cards <= 9) break
    const moves = wasm.legal_actions(state)
    state = wasm.apply_action(state, moves[(i * 7 + 3) % moves.length])
  }
  assert.equal(wasm.game_outcome(state).status, 'in_progress', 'the test game ended before an endgame was reached')
  assert.equal(wasm.is_exactly_solvable(state), true)

  const exact = wasm.solve_exact({ state, max_nodes: 2_000_000 })
  const legal = wasm.legal_actions(state)
  assert.equal(exact.perspective, actorOf(state))
  assert.equal(exact.actions.length, legal.length)
  for (const a of exact.actions) assert.ok(['win', 'draw', 'loss'].includes(a.value), `unsettled move: ${JSON.stringify(a)}`)
  assert.ok(['win', 'draw', 'loss'].includes(exact.value))

  // The app's choice follows the calculation, whatever the estimates say.
  const { summary } = chooseMove(state)
  const best = pickBest(withExact(summary, exact))
  assert.equal(best.exact, exact.value)
})

test('several cards per attack: the attacker is offered to add more or let the defender answer', () => {
  for (const [size, expectAdding] of [[1, false], [2, true], [6, true]]) {
    const state = newGame(11, { cards_per_attack: size })
    const first = wasm.legal_actions(state).find(a => a.type === 'attack')
    const after = wasm.apply_action(state, first)
    assert.equal(after.phase === 'Adding', expectAdding, `size ${size}`)
    if (expectAdding) {
      assert.equal(actorOf(after), actorOf(state), 'the same attacker is still to move')
      assert.ok(wasm.legal_actions(after).some(a => a.type === 'let_defend'))
      const answered = wasm.apply_action(after, { type: 'let_defend' })
      assert.equal(answered.phase, 'Defending')
    }
  }
})

test('a kept search goes on from run to run and its results pool', () => {
  const state = newGame(5, { reflecting: true })
  const search = new wasm.KeptSearch({ state, seed: 3n })
  const tally = createTally()
  for (let i = 0; i < 3; i++) {
    const result = search.run(2, 300)
    assert.equal(result.total_visits, 600, 'each run reports its own playouts only')
    addResult(tally, result)
  }
  search.free()
  const summary = summarize(tally, wasm.legal_actions(state))
  assert.equal(summary.totalVisits, 1800)
  assert.ok(pickBest(summary))
  assert.throws(() => new wasm.KeptSearch({ state: { nonsense: true } }), /could not read/)
})

test('the opponent\'s play says something about their hand, and the search takes it', () => {
  // Play a computer-dealt game on with the first legal move until P1 has moved a few times.
  const initial = newGame(12, { reflecting: true })
  let state = initial
  const steps = []
  for (let i = 0; i < 40 && wasm.game_outcome(state).status === 'in_progress'; i++) {
    const action = wasm.legal_actions(state)[0]
    state = wasm.apply_action(state, action)
    steps.push({ state, action })
  }
  const beliefs = wasm.opponent_beliefs(initial, steps)
  assert.equal(beliefs.weights.length, 52)
  assert.ok(beliefs.weights.every(w => w > 0 && w <= 1))
  assert.ok(beliefs.weights.some(w => w < 1), 'forty moves taught something')
  assert.deepEqual(wasm.opponent_beliefs(initial, []).weights, Array(52).fill(1))

  if (wasm.game_outcome(state).status === 'in_progress') {
    const search = new wasm.KeptSearch({ state, seed: 1n, beliefs })
    assert.ok(search.run(2, 200).total_visits >= 400)
    search.free()
  }
})
