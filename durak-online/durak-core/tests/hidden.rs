//! Determinization and deduction of hidden cards.

mod common;
use common::*;
use durak_core::*;
use rand::{rngs::StdRng, SeedableRng};
use std::collections::HashSet;

fn ids<'a>(cards: impl Iterator<Item = &'a Card>) -> Vec<CardId> {
    cards.filter_map(Card::id).collect()
}

fn all_ids(s: &GameState) -> Vec<CardId> {
    let mut v = ids(s.hands.iter().flatten());
    v.extend(ids(s.stock.iter()));
    v.extend(ids(s.discard.iter()));
    for p in &s.table {
        v.extend(p.attack.id());
        v.extend(p.defense.as_ref().and_then(Card::id));
    }
    v
}

fn manual_3p() -> GameState {
    let hand = vec![
        c(Clubs, Six), c(Clubs, Seven), c(Clubs, Eight),
        c(Clubs, Nine), c(Clubs, Ten), c(Clubs, Jack),
    ];
    let config = GameConfig { num_players: 3, ..GameConfig::default() };
    GameState::new_manual_game(c(Hearts, Six), hand, 0, vec![(P1, Some(Ace))], config).unwrap()
}

#[test]
fn determinize_fills_every_unknown_with_a_distinct_card() {
    let s = manual_3p();
    for seed in 0..50 {
        let d = s.determinize(P0, &mut StdRng::seed_from_u64(seed)).unwrap();
        d.validate().unwrap();
        assert!(d.is_fully_known());
        let all = all_ids(&d);
        assert_eq!(all.len(), 36);
        assert_eq!(all.iter().collect::<HashSet<_>>().len(), 36, "no duplicates");
    }
}

#[test]
fn determinize_keeps_everything_the_player_already_knows() {
    let s = manual_3p();
    for seed in 0..50 {
        let d = s.determinize(P0, &mut StdRng::seed_from_u64(seed)).unwrap();
        assert_eq!(ids(d.hands[0].iter()), ids(s.hands[0].iter()), "own hand untouched");
        assert!(d.stock[0].matches(&c(Hearts, Six)), "trump card stays at the bottom");
        assert!(d.hands[1].iter().any(|x| x.matches(&c(Hearts, Ace))), "shown trump stays put");
        assert_eq!(d.hands[1].len(), 6);
        assert_eq!(d.hands[2].len(), 6);
        assert_eq!(d.stock.len(), s.stock.len());
    }
}

#[test]
fn determinize_varies_with_the_seed_and_repeats_for_the_same_seed() {
    let s = manual_3p();
    let a = s.determinize(P0, &mut StdRng::seed_from_u64(1)).unwrap();
    let b = s.determinize(P0, &mut StdRng::seed_from_u64(1)).unwrap();
    assert_eq!(a, b);
    let distinct: HashSet<Vec<CardId>> = (0..30)
        .map(|seed| ids(s.determinize(P0, &mut StdRng::seed_from_u64(seed)).unwrap().hands[2].iter()))
        .collect();
    assert!(distinct.len() > 20, "different seeds should deal different hands");
}

#[test]
fn determinize_hides_other_players_private_cards() {
    // In a dealt game the engine knows every card, but P1 must not.
    let config = GameConfig { num_players: 3, ..GameConfig::default() };
    let s = GameState::new_computer_game(11, config).unwrap();
    let mut changed = false;
    for seed in 0..20 {
        let d = s.determinize(P1, &mut StdRng::seed_from_u64(seed)).unwrap();
        d.validate().unwrap();
        assert_eq!(ids(d.hands[1].iter()), ids(s.hands[1].iter()), "P1 keeps their own cards");
        for (i, hand) in s.hands.iter().enumerate() {
            for (j, card) in hand.iter().enumerate() {
                if card.is_public() {
                    assert!(d.hands[i][j].matches(card), "public cards stay where they are");
                }
            }
        }
        changed |= ids(d.hands[0].iter()) != ids(s.hands[0].iter());
    }
    assert!(changed, "P0's private cards must be re-dealt from P1's point of view");
}

#[test]
fn determinize_refuses_when_the_player_has_unidentified_cards() {
    let s = manual_3p();
    let e = s.determinize(P1, &mut StdRng::seed_from_u64(0)).unwrap_err();
    assert!(matches!(e, EngineError::InvalidState(_)), "{e}");
    assert!(s.determinize(PlayerId::P4, &mut StdRng::seed_from_u64(0)).is_err());
}

#[test]
fn determinize_reports_inconsistent_card_counts_instead_of_leaving_unknowns() {
    // One unknown card too many: more hidden slots than cards unaccounted for.
    let mut s = manual_3p();
    s.stock.push(Card::Unknown);
    assert!(s.validate().is_err());
    let e = s.determinize(P0, &mut StdRng::seed_from_u64(0)).unwrap_err();
    assert!(matches!(e, EngineError::InvalidState(_)), "{e}");

    // One too few.
    let mut s = manual_3p();
    s.stock.pop();
    assert!(s.determinize(P0, &mut StdRng::seed_from_u64(0)).is_err());
}

#[test]
fn remaining_cards_are_deduced_once_the_stock_is_empty() {
    // Two players, stock empty: whatever I cannot see must be in the opponent's hand.
    let mut s = Pos::new(vec![
        vec![Card::private(Clubs, Seven), Card::private(Diamonds, Eight)],
        vec![Card::Unknown, Card::Unknown, c(Spades, Ten)],
    ])
    .build_unchecked();
    // Take two cards back out of the discard pile to be the unknown ones.
    let hidden: Vec<Card> = vec![c(Spades, Ace), c(Spades, King)];
    s.discard.retain(|d| !hidden.iter().any(|h| h.matches(d)));
    s.validate().unwrap();

    assert!(s.all_cards_deducible(P0));
    let d = s.deduce_cards(P0).unwrap();
    d.validate().unwrap();
    assert!(d.is_fully_known());
    for h in &hidden {
        assert!(d.hands[1].iter().any(|x| x.matches(h)));
    }
}

#[test]
fn nothing_is_deduced_while_the_stock_or_two_opponents_hide_cards() {
    let s = manual_3p();
    assert!(!s.all_cards_deducible(P0), "stock is not empty");
    assert!(s.deduce_cards(P0).is_err());

    let mut s = Pos::new(vec![
        vec![Card::private(Clubs, Seven)],
        vec![Card::Unknown],
        vec![Card::Unknown],
    ])
    .build_unchecked();
    s.discard.retain(|d| !d.matches(&c(Spades, Ace)) && !d.matches(&c(Spades, King)));
    s.validate().unwrap();
    assert!(!s.all_cards_deducible(P0), "two opponents could hold either card");
    assert!(s.deduce_cards(P0).is_err());
}
