// End-to-end: the real WASM engine -> result pooling -> move selection.
// This is the path a computer opponent's move takes in the app.

import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

import { addResult, createTally, pickBest, summarize } from '../src/lib/aggregate.js'
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
  fails(() => wasm.new_computer_game({ seed: 1n }, config({ deck_size: 24 })), /not supported/)
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
