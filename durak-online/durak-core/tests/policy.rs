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
fn throws_in_plain_cards_and_keeps_trumps() {
    let taking = |hand: Vec<Card>| {
        let mut pos = Pos::new(vec![hand, vec![c(Diamonds, Ace), c(Diamonds, King), c(Diamonds, Queen)]]);
        pos.table = vec![pile(c(Spades, Ace), None)];
        pos.phase = Phase::Throwing;
        with_stock(pos, 10)
    };
    // Even an ace goes to a defender who is picking up; a trump does not.
    assert_eq!(chosen(&taking(vec![c(Clubs, Ace), c(Hearts, Ace)])), throw(c(Clubs, Ace)));
    assert_eq!(chosen(&taking(vec![c(Hearts, Ace)])), Action::Throw { card: None });
}

#[test]
fn adds_nothing_before_the_defender_answers() {
    for stock in [10, 0] {
        let mut pos = Pos::new(vec![
            vec![c(Spades, Seven), c(Clubs, Seven), c(Diamonds, Seven)],
            vec![c(Diamonds, Ace), c(Diamonds, King), c(Diamonds, Queen)],
        ]);
        pos.cards_per_attack = 6;
        let mut s = with_stock(pos, stock);
        s.apply(&attack(c(Spades, Seven))).unwrap();
        assert_eq!(s.phase, Phase::Adding);
        assert_eq!(chosen(&s), Action::LetDefend, "{stock} cards left to draw");
    }
}

#[test]
fn adds_plain_cards_of_any_rank_once_the_attack_is_beaten() {
    let beaten = |hand: Vec<Card>, stock: usize| {
        let mut pos = Pos::new(vec![hand, vec![c(Diamonds, Ace), c(Diamonds, King), c(Diamonds, Queen)]]);
        pos.table = vec![pile(c(Spades, King), Some(c(Spades, Ace)))];
        pos.phase = Phase::Attacking;
        with_stock(pos, stock)
    };
    assert_eq!(chosen(&beaten(vec![c(Clubs, Ace), c(Clubs, Six)], 10)), attack(c(Clubs, Ace)));
    // A high trump is kept while there are cards to draw, and goes at the end.
    assert_eq!(chosen(&beaten(vec![c(Hearts, King), c(Clubs, Six)], 10)), Action::PassAttack);
    assert_eq!(chosen(&beaten(vec![c(Hearts, King), c(Clubs, Six)], 0)), attack(c(Hearts, King)));
}

#[test]
fn random_playouts_stay_available() {
    let s = with_stock(defending(vec![c(Hearts, Ten), c(Clubs, Six)], c(Spades, Nine)), 20);
    let p = SolveParams { determinizations: 2, rollouts: 100, playout: Playout::Random, ..SolveParams::default() };
    let r = solve(&s, p, 1).unwrap();
    assert_eq!(r.total_visits, 200);
}
