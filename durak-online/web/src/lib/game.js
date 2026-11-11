// Read-only helpers over the engine's game state (plain JSON from WASM).

import { cardLabel } from './cards.js'

export const ME = 'P0'
export const SUITS = ['Clubs', 'Diamonds', 'Hearts', 'Spades']
export const ALL_RANKS = ['Two', 'Three', 'Four', 'Five', 'Six', 'Seven', 'Eight', 'Nine', 'Ten', 'Jack', 'Queen', 'King', 'Ace']
const RED = ['Hearts', 'Diamonds']
const BLACK = ['Clubs', 'Spades']

export const cardKey = c => `${c?.suit || '?'}:${c?.rank || '?'}`
export const seatIndex = pid => Number.parseInt(String(pid).slice(1), 10)

/** { suit, rank } for a known card, null for an unknown one. */
export function parseCard(card) {
  if (!card || card.type === 'unknown') return null
  return card.suit && card.rank ? { suit: card.suit, rank: card.rank } : null
}

export const isUnknown = card => !card || card.type === 'unknown'
export const isPublic = card => card?.type === 'public'

export function ranksFor(deckSize) {
  return ALL_RANKS.slice(-(deckSize / 4))
}

/** Trump first, then suits alternating in colour, so a hand is easy to scan. */
export function suitOrder(trumpSuit) {
  if (!trumpSuit) return ['Hearts', 'Clubs', 'Diamonds', 'Spades']
  const same = RED.includes(trumpSuit) ? RED : BLACK
  const other = same === RED ? BLACK : RED
  return [trumpSuit, other[0], same.find(s => s !== trumpSuit), other[1]]
}

export function sortCards(cards, trumpSuit) {
  const order = suitOrder(trumpSuit)
  return [...cards].sort(
    (a, b) => order.indexOf(a.suit) - order.indexOf(b.suit) || ALL_RANKS.indexOf(b.rank) - ALL_RANKS.indexOf(a.rank),
  )
}

export function deckCards(deckSize, trumpSuit) {
  const ranks = ranksFor(deckSize)
  return sortCards(SUITS.flatMap(suit => ranks.map(rank => ({ suit, rank }))), trumpSuit)
}

/** Who is to move, or null without a state. Mirrors the engine's rule. */
export function actorOf(state) {
  if (!state) return null
  if (state.phase === 'Defending') return state.defender
  const list = state.attackers || []
  return list.length ? list[(state.current_attacker_idx || 0) % list.length] : state.attacker
}

export const handOf = (state, pid) => state?.hands?.[seatIndex(pid)] || []
export const unknownCount = hand => hand.filter(isUnknown).length

/** A player is out once they hold no cards and nothing is left to draw. */
export function isOut(state, pid) {
  return handOf(state, pid).length === 0 && (state?.stock?.length || 0) === 0
}

/**
 * A player's part in the current trick:
 * 'defender', 'attacker' (leads the attack), 'support' (may throw in), or null.
 */
export function roleOf(state, pid) {
  if (!state || isOut(state, pid)) return null
  if (state.defender === pid) return 'defender'
  const idx = (state.attackers || []).indexOf(pid)
  if (idx === 0 || (idx < 0 && state.attacker === pid)) return 'attacker'
  return idx > 0 ? 'support' : null
}

/** Every known card in play: hands, table, discard and the trump card. */
export function knownCards(state) {
  if (!state) return []
  const out = []
  const add = c => { const p = parseCard(c); if (p) out.push(p) }
  ;(state.hands || []).flat().forEach(add)
  ;(state.stock || []).forEach(add)
  ;(state.discard || []).forEach(add)
  for (const pile of state.table || []) { add(pile.attack); add(pile.defense) }
  return out
}

export function actionCard(action) {
  return action?.card ? parseCard(action.card) : null
}

const VERBS = {
  attack: 'Attack with',
  defend: 'Defend with',
  reflect: 'Reflect with',
  reflect_trump: 'Show',
  throw: 'Throw in',
}

/** Button text for an action, e.g. "Defend with K♣" or "Take the cards". */
export function actionText(action) {
  const card = actionCard(action)
  if (card) return `${VERBS[action.type] || action.type} ${cardLabel(card)}`
  if (action?.type === 'take') return 'Take the cards'
  if (action?.type === 'pass_attack') return 'Pass'
  if (action?.type === 'throw') return 'Done throwing in'
  return 'Unknown action'
}

/** Past-tense line for the move log, e.g. "Opponent 1 defended with K♣". */
export function actionLogText(action, who) {
  const card = actionCard(action)
  const label = card ? cardLabel(card) : ''
  switch (action?.type) {
    case 'attack': return `${who} attacked with ${label}`
    case 'defend': return `${who} defended with ${label}`
    case 'reflect': return `${who} reflected with ${label}`
    case 'reflect_trump': return `${who} showed ${label} to reflect`
    case 'take': return `${who} took the cards`
    case 'pass_attack': return `${who} passed`
    case 'throw': return card ? `${who} threw in ${label}` : `${who} finished throwing in`
    default: return `${who} moved`
  }
}

/** Short label for the group an action belongs to in a picker. */
export const ACTION_GROUPS = [
  { type: 'attack', label: 'Attack with' },
  { type: 'defend', label: 'Defend with' },
  { type: 'throw', label: 'Throw in' },
  { type: 'reflect', label: 'Reflect with (played)' },
  { type: 'reflect_trump', label: 'Reflect by showing trump' },
]

/**
 * What the player to move is being asked to do, in plain words:
 * `you` reads after "Your turn:", `other` after the player's name.
 */
export function promptFor(state) {
  if (!state) return { you: '', other: '' }
  if (state.phase === 'Defending') {
    return { you: 'beat the attacking card, or take the cards', other: 'must beat the attack or take the cards' }
  }
  if (state.phase === 'Throwing') {
    return { you: 'throw in more cards, or finish', other: 'may throw in more cards' }
  }
  return (state.table || []).length
    ? { you: 'add a card of a rank on the table, or pass', other: 'may add a card or pass' }
    : { you: 'lead the attack', other: 'leads the attack' }
}
