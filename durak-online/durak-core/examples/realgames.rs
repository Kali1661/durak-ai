//! What do real opponents do? Measured on recorded games.
//!
//! Reads the file the app's "Export all (JSON)" button saves (Games panel)
//! and, for the two-player games played against real opponents, works out
//! with hindsight which hidden cards the opponent really held at each moment
//! (a card they play before drawing again was in their hand; a card I draw
//! later was in the stock). Then prints:
//!
//! 1. how good each rule in `belief` is against real play ("they took, so
//!    they probably have no higher card of that suit"): the weight that
//!    follows from where the cards really were;
//! 2. how often an opponent who could beat the cards picked them up anyway,
//!    by how many cards were left to draw.
//!
//! `cargo run --release --example realgames -- <export.json> [--list]`

use std::collections::HashMap;

use durak_core::{Action, Beliefs, Card, CardId, Evidence, GameState, Phase, PlayerId, Rank, Strength, Suit};
use serde_json::Value;

const KINDS: [Evidence; 6] = [
    Evidence::TakeSameSuit,
    Evidence::TakeTrump,
    Evidence::LeadLower,
    Evidence::LeadTrumpPlain,
    Evidence::StopLow,
    Evidence::StopHigh,
];

/// Cards pointed at, of them in the hand, expected in the hand if the rule meant nothing.
type Tally = [(f64, f64, f64); 6];

#[derive(Clone, Copy, PartialEq)]
enum Place {
    Hand,
    Stock,
}

fn ids(cards: &[Card]) -> impl Iterator<Item = CardId> + '_ {
    cards.iter().filter_map(|c| c.id())
}

/// Every card I can see somewhere in `s`.
fn visible(s: &GameState) -> Vec<CardId> {
    let mut v: Vec<CardId> = s.hands.iter().flat_map(|h| ids(h)).collect();
    v.extend(ids(&s.stock));
    v.extend(ids(&s.discard));
    for p in &s.table {
        v.extend(p.attack.id());
        v.extend(p.defense.and_then(|c| c.id()));
    }
    v
}

fn deck(size: usize) -> Vec<CardId> {
    let ranks = &Rank::ALL[Rank::ALL.len() - size / 4..];
    Suit::ALL.into_iter().flat_map(|s| ranks.iter().map(move |r| (s, *r))).collect()
}

fn unknowns(cards: &[Card]) -> usize {
    cards.iter().filter(|c| c.id().is_none()).count()
}

fn card_of(action: &Action) -> Option<CardId> {
    match action {
        Action::Attack { card } | Action::Defend { card, .. } | Action::Reflect { card } => card.id(),
        Action::Throw { card: Some(card) } => card.id(),
        _ => None,
    }
}

fn beats(a: CardId, b: CardId, trump: Suit) -> bool {
    (a.0 == b.0 && a.1 > b.1) || (a.0 == trump && b.0 != trump)
}

/// Can `hand` beat every card of `open`, one card each?
fn can_beat_all(hand: &[CardId], open: &[CardId], trump: Suit, used: &mut Vec<bool>) -> bool {
    let Some((first, rest)) = open.split_first() else { return true };
    for i in 0..hand.len() {
        if !used[i] && beats(hand[i], *first, trump) {
            used[i] = true;
            let ok = can_beat_all(hand, rest, trump, used);
            used[i] = false;
            if ok {
                return true;
            }
        }
    }
    false
}

/// Where the cards hidden at step `i` really were, as far as the rest of the
/// game shows. `states[i]` is the position before `actions[i]`.
fn resolve(states: &[GameState], actions: &[Option<Action>], actors: &[Option<PlayerId>], i: usize) -> HashMap<CardId, Place> {
    let s = &states[i];
    let seen = visible(s);
    let hidden: Vec<CardId> = deck(s.config.deck_size).into_iter().filter(|c| !seen.contains(c)).collect();
    let mut place: HashMap<CardId, Place> = HashMap::new();
    let mut unsure: Vec<CardId> = Vec::new();
    let mut opponent_drew = false;
    for j in i..actions.len() {
        let (before, after) = (&states[j], &states[j + 1]);
        // A card the opponent plays from their hand.
        if actors[j] == Some(PlayerId::P1) {
            if let Some(card) = actions[j].as_ref().and_then(card_of) {
                if hidden.contains(&card) && !place.contains_key(&card) && !unsure.contains(&card) {
                    if opponent_drew {
                        unsure.push(card);
                    } else {
                        place.insert(card, Place::Hand);
                    }
                }
            }
        }
        // Did the opponent draw on the way to the next position?
        let off_stock = before.stock.len().saturating_sub(after.stock.len());
        let mine = unknowns(&after.hands[0]).saturating_sub(unknowns(&before.hands[0]));
        if off_stock > mine {
            opponent_drew = true;
        }
        // Cards that turn up in my hand were in the stock.
        for card in ids(&after.hands[0]) {
            if hidden.contains(&card) && !place.contains_key(&card) && !unsure.contains(&card) {
                place.insert(card, Place::Stock);
            }
        }
        // Cards the app has since worked out the opponent holds.
        for card in ids(&after.hands[1]) {
            if hidden.contains(&card) && !place.contains_key(&card) && !unsure.contains(&card) {
                if opponent_drew {
                    unsure.push(card);
                } else {
                    place.insert(card, Place::Hand);
                }
            }
        }
    }
    place
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.iter().find(|a| !a.starts_with("--")) else {
        eprintln!("usage: realgames <export.json> [--list]");
        std::process::exit(2);
    };
    let list = args.iter().any(|a| a == "--list");
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("could not read {path}: {e}"));
    let export: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path} is not JSON: {e}"));
    let games = export["games"].as_array().expect("an export of the Games panel");

    // Evidence: all games, then by how strong the opponent was rated.
    let groups = ["all", "strong", "average", "weak", "not rated"];
    let mut tallies: Vec<Tally> = vec![[(0.0, 0.0, 0.0); 6]; groups.len()];
    // Defences where the opponent's whole hand is known with hindsight:
    // [stock bucket][could beat everything] -> (decisions, takes).
    let mut defences = [[(0u32, 0u32); 2]; 4];
    // One card to beat and a hand that can: [stock bucket][cheapest card
    // that beats it: plain, trump up to ten, jack or queen, king or ace]
    // -> (decisions, takes); the same split by what the card to beat is
    // [plain below queen, plain queen or better, trump].
    let mut by_beater = [[(0u32, 0u32); 4]; 4];
    let mut by_card = [[(0u32, 0u32); 3]; 4];
    let mut used_games = 0;

    for g in games {
        if g["mode"] != "analyse" || g["initial"]["hands"].as_array().map_or(0, Vec::len) != 2 {
            continue;
        }
        let Ok(initial) = serde_json::from_value::<GameState>(g["initial"].clone()) else { continue };
        let mut states = vec![initial];
        let mut actions: Vec<Option<Action>> = Vec::new();
        let mut actors: Vec<Option<PlayerId>> = Vec::new();
        for step in g["steps"].as_array().map_or(&[][..], |v| &v[..]) {
            let Ok(state) = serde_json::from_value::<GameState>(step["state"].clone()) else { break };
            states.push(state);
            actions.push(serde_json::from_value(step["action"].clone()).ok().flatten());
            actors.push(serde_json::from_value(step["actor"].clone()).ok().flatten());
        }
        used_games += 1;
        let rating = g["opponent"].as_str().unwrap_or("not rated");
        let group = groups.iter().position(|x| *x == rating).unwrap_or(4);

        for i in 0..actions.len() {
            let (Some(action), Some(PlayerId::P1)) = (&actions[i], actors[i]) else { continue };
            let before = &states[i];
            if before.actor_to_move() != PlayerId::P1 {
                continue;
            }
            let place = resolve(&states, &actions, &actors, i);
            let hidden_in_hand = unknowns(&before.hands[1]);
            let hidden_in_stock = unknowns(&before.stock);
            let in_hand = place.values().filter(|p| **p == Place::Hand).count();

            // 1. The rules of `belief`.
            if hidden_in_hand > 0 && hidden_in_stock > 0 && !place.is_empty() {
                let base = in_hand as f64 / place.len() as f64;
                let mut probe = Beliefs::new(Strength {
                    take_same_suit: 0.5,
                    take_trump: 0.5,
                    lead_lower: 0.5,
                    lead_trump_plain: 0.5,
                    stop_low: 0.5,
                    stop_high: 0.5,
                    take_stacks: true,
                });
                probe.observe_with(PlayerId::P0, before, action, &states[i + 1], &mut |kind, id| {
                    let k = KINDS.iter().position(|x| *x == kind).unwrap();
                    if let Some(p) = place.get(&id) {
                        for t in [0, group] {
                            tallies[t][k].0 += 1.0;
                            tallies[t][k].1 += f64::from(u8::from(*p == Place::Hand));
                            tallies[t][k].2 += base;
                        }
                    }
                });
            }

            // 2. Taking when they could have beaten everything.
            let first_answer = before.phase == Phase::Defending && before.table.iter().all(|p| p.defense.is_none());
            if first_answer && in_hand == hidden_in_hand && matches!(action, Action::Take | Action::Defend { .. }) {
                let mut hand: Vec<CardId> = ids(&before.hands[1]).collect();
                hand.extend(place.iter().filter(|(_, p)| **p == Place::Hand).map(|(c, _)| *c));
                let open: Vec<CardId> = before.table.iter().filter_map(|p| p.attack.id()).collect();
                let could = can_beat_all(&hand, &open, before.trump, &mut vec![false; hand.len()]);
                let bucket = match before.stock.len() {
                    0 => 3,
                    1..=5 => 2,
                    6..=11 => 1,
                    _ => 0,
                };
                let took = matches!(action, Action::Take);
                let cell = &mut defences[bucket][usize::from(could)];
                cell.0 += 1;
                cell.1 += u32::from(took);
                if could && open.len() == 1 {
                    let trump = before.trump;
                    let worth = |c: &CardId| c.1 as i32 + if c.0 == trump { 20 } else { 0 };
                    let cheapest = hand.iter().filter(|c| beats(**c, open[0], trump)).min_by_key(|c| worth(c)).unwrap();
                    let kind = match cheapest {
                        (suit, _) if *suit != trump => 0,
                        (_, rank) if *rank <= Rank::Ten => 1,
                        (_, rank) if *rank <= Rank::Queen => 2,
                        _ => 3,
                    };
                    by_beater[bucket][kind].0 += 1;
                    by_beater[bucket][kind].1 += u32::from(took);
                    let card = match open[0] {
                        (suit, _) if suit == trump => 2,
                        (_, rank) if rank >= Rank::Queen => 1,
                        _ => 0,
                    };
                    by_card[bucket][card].0 += 1;
                    by_card[bucket][card].1 += u32::from(took);
                }
                if list && took && could {
                    println!(
                        "  took {open:?} holding {} cards ({} left to draw, trump {:?}, opponent {rating}): {hand:?}",
                        hand.len(),
                        before.stock.len(),
                        before.trump
                    );
                }
            }
        }
    }

    println!("{used_games} two-player games against real opponents\n");
    println!("1. The rules for guessing the opponent's hand (weight below 1 = the rule is right)");
    for (name, tally) in groups.iter().zip(&tallies) {
        println!("\n  opponents: {name}");
        println!("  {:<18} {:>8} {:>9} {:>9} {:>8}", "evidence", "cards", "in hand", "expected", "weight");
        for (kind, (n, held, expected)) in KINDS.iter().zip(tally) {
            if *n == 0.0 {
                continue;
            }
            let (p, q) = (held / n, expected / n);
            let weight = (p / (1.0 - p)) / (q / (1.0 - q));
            println!("  {:<18} {n:>8.0} {:>8.1}% {:>8.1}% {weight:>8.2}", format!("{kind:?}"), p * 100.0, q * 100.0);
        }
    }

    println!("\n2. First answer to an attack, opponent's hand known with hindsight");
    println!("  {:<22} {:>22} {:>26}", "cards left to draw", "could beat: took", "could not beat all: took");
    for (name, row) in ["12 or more", "6 to 11", "1 to 5", "none"].iter().zip(defences) {
        let show = |(n, took): (u32, u32)| {
            if n == 0 {
                "-".to_string()
            } else {
                format!("{took} of {n} ({:.0}%)", 100.0 * f64::from(took) / f64::from(n))
            }
        };
        println!("  {name:<22} {:>22} {:>26}", show(row[1]), show(row[0]));
    }

    let show = |(n, took): (u32, u32)| if n == 0 { "-".to_string() } else { format!("{took}/{n} ({:.0}%)", 100.0 * f64::from(took) / f64::from(n)) };
    println!("
3. One card to beat and a hand that can beat it: how often it was picked up anyway");
    println!("  by the cheapest card that beats it");
    println!("  {:<22} {:>14} {:>16} {:>16} {:>16}", "cards left to draw", "a plain card", "trump up to 10", "trump J or Q", "trump K or A");
    for (name, row) in ["12 or more", "6 to 11", "1 to 5", "none"].iter().zip(by_beater) {
        println!("  {name:<22} {:>14} {:>16} {:>16} {:>16}", show(row[0]), show(row[1]), show(row[2]), show(row[3]));
    }
    println!("  by the card to beat");
    println!("  {:<22} {:>14} {:>16} {:>16}", "cards left to draw", "plain to jack", "plain Q or more", "a trump");
    for (name, row) in ["12 or more", "6 to 11", "1 to 5", "none"].iter().zip(by_card) {
        println!("  {name:<22} {:>14} {:>16} {:>16}", show(row[0]), show(row[1]), show(row[2]));
    }
}
