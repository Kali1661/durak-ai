//! The heuristic playout player makes the moves a sensible beginner would.

mod common;
use common::*;
use durak_core::*;

/// `pos` with `n` cards left to draw (the face-up one a trump).
fn with_stock(mut pos: Pos, n: usize) -> GameState {
    let mut used: Vec<CardId> = pos.hands.iter().flatten().filter_map(Card::id).collect();
    for p in &pos.table {
        used.extend(p.attack.id());
        used.extend(p.defense.as_ref().and_then(Card::id));
    }
    let mut rest: Vec<Card> =
        full_deck(36).unwrap().into_iter().filter(|c| !used.contains(&c.id().unwrap())).collect();
    // The bottom card shows the trump suit.
    let bottom = rest.iter().position(|c| c.suit() == pos.trump).unwrap();
    rest.swap(0, bottom);
    rest.truncate(n);
    pos.stock = rest;
    pos.build()
}

fn defending(hand: Vec<Card>, attack: Card) -> Pos {
    let mut pos = Pos::new(vec![vec![c(Clubs, Ace)], hand]);
    pos.table = vec![pile(attack, None)];
    pos.phase = Phase::Defending;
    pos
}

fn chosen(s: &GameState) -> Action {
    policy::heuristic_action(s).unwrap()
}

#[test]
fn leads_the_lowest_plain_card() {
    let s = with_stock(Pos::new(vec![vec![c(Hearts, Six), c(Clubs, Nine), c(Spades, Seven)], vec![c(Clubs, Ten)]]), 10);
    assert_eq!(chosen(&s), attack(c(Spades, Seven)));
}

#[test]
fn beats_with_a_plain_card_before_a_trump() {
    let s = with_stock(defending(vec![c(Hearts, Six), c(Spades, Jack)], c(Spades, Nine)), 10);
    assert_eq!(chosen(&s), defend(c(Spades, Jack)));
}

#[test]
fn takes_one_card_early_rather_than_spend_a_high_trump() {
    let s = with_stock(defending(vec![c(Hearts, Ten), c(Clubs, Six)], c(Spades, Nine)), 20);
    assert_eq!(chosen(&s), Action::Take);
}

#[test]
fn spends_the_trump_once_nothing_is_left_to_draw() {
    let s = with_stock(defending(vec![c(Hearts, Ten), c(Clubs, Six)], c(Spades, Nine)), 0);
    assert_eq!(chosen(&s), defend(c(Hearts, Ten)));
}

#[test]
fn throws_in_low_cards_and_keeps_trumps() {
    let mut pos = Pos::new(vec![
        vec![c(Clubs, Seven), c(Hearts, Seven)],
        vec![c(Diamonds, Ace), c(Diamonds, King), c(Diamonds, Queen)],
    ]);
    pos.table = vec![pile(c(Spades, Seven), None)];
    pos.phase = Phase::Throwing;
    let s = with_stock(pos, 10);
    assert_eq!(chosen(&s), throw(c(Clubs, Seven)));

    let mut pos = Pos::new(vec![vec![c(Hearts, Seven)], vec![c(Diamonds, Ace), c(Diamonds, King)]]);
    pos.table = vec![pile(c(Spades, Seven), None)];
    pos.phase = Phase::Throwing;
    let s = with_stock(pos, 10);
    assert_eq!(chosen(&s), Action::Throw { card: None });
}

#[test]
fn random_playouts_stay_available() {
    let s = with_stock(defending(vec![c(Hearts, Ten), c(Clubs, Six)], c(Spades, Nine)), 20);
    let p = SolveParams { determinizations: 2, rollouts: 100, playout: Playout::Random, ..SolveParams::default() };
    let r = solve(&s, p, 1).unwrap();
    assert_eq!(r.total_visits, 200);
}
