//! Config, manual-game input and saved-state validation.

mod common;
use common::*;
use durak_core::*;

fn cfg(deck_size: usize, num_players: usize) -> GameConfig {
    GameConfig { deck_size, num_players, reflecting: false, trump_reflecting: false }
}

fn my_hand() -> Vec<Card> {
    vec![
        c(Clubs, Six), c(Clubs, Seven), c(Clubs, Eight),
        c(Clubs, Nine), c(Clubs, Ten), c(Clubs, Jack),
    ]
}

fn manual(trump: Card, hand: Vec<Card>, starter: u8, config: GameConfig) -> Result<GameState, EngineError> {
    GameState::new_manual_game(trump, hand, starter, vec![], config)
}

#[test]
fn only_two_or_three_players_are_supported() {
    for n in [0, 1, 4, 5, 6, 100] {
        let e = cfg(36, n).validate().unwrap_err();
        assert!(matches!(e, EngineError::InvalidConfig(_)), "{n} players: {e}");
        assert!(GameState::new_computer_game(1, cfg(36, n)).is_err());
        assert!(manual(c(Hearts, Six), my_hand(), 0, cfg(36, n)).is_err());
    }
    for n in [2, 3] {
        cfg(36, n).validate().unwrap();
    }
}

#[test]
fn only_listed_deck_sizes_are_supported() {
    for size in [0, 4, 12, 24, 35, 37, 54, 1000] {
        assert!(cfg(size, 2).validate().is_err(), "deck of {size}");
        assert!(GameState::new_computer_game(1, cfg(size, 2)).is_err());
        assert!(full_deck(size).is_err());
    }
    for size in SUPPORTED_DECK_SIZES {
        assert_eq!(full_deck(size).unwrap().len(), size);
    }
}

#[test]
fn every_supported_table_deals_a_complete_valid_game() {
    for deck_size in SUPPORTED_DECK_SIZES {
        for n in [2, 3] {
            for seed in 0..5 {
                let s = GameState::new_computer_game(seed, cfg(deck_size, n)).unwrap();
                s.validate().unwrap();
                assert_eq!(s.hands.len(), n);
                assert!(s.hands.iter().all(|h| h.len() == HAND_SIZE));
                assert_eq!(s.stock.len(), deck_size - n * HAND_SIZE);
                assert_eq!(s.stock[0].suit(), s.trump);
                assert!(s.is_fully_known());
                assert_ne!(s.attacker, s.defender);
            }
        }
    }
}

#[test]
fn the_holder_of_the_lowest_trump_attacks_first() {
    for seed in 0..40 {
        let s = GameState::new_computer_game(seed, cfg(36, 3)).unwrap();
        let lowest = |h: &Vec<Card>| h.iter().filter(|x| x.suit() == s.trump).map(|x| x.rank()).min();
        let best = s.hands.iter().enumerate().filter_map(|(i, h)| lowest(h).map(|r| (r, i))).min();
        let expected = best.map_or(0, |(_, i)| i);
        assert_eq!(s.attacker.index(), expected, "seed {seed}");
    }
}

#[test]
fn manual_game_rejects_bad_hands() {
    let ok = manual(c(Hearts, Six), my_hand(), 0, cfg(36, 2)).unwrap();
    ok.validate().unwrap();
    assert_eq!(ok.hands[1], vec![Card::Unknown; 6]);

    let mut short = my_hand();
    short.pop();
    assert!(manual(c(Hearts, Six), short, 0, cfg(36, 2)).is_err());

    let mut long = my_hand();
    long.push(c(Spades, Six));
    assert!(manual(c(Hearts, Six), long, 0, cfg(36, 2)).is_err());

    let mut dup = my_hand();
    dup[5] = dup[0];
    assert!(manual(c(Hearts, Six), dup, 0, cfg(36, 2)).is_err());

    let mut unknown = my_hand();
    unknown[0] = Card::Unknown;
    assert!(manual(c(Hearts, Six), unknown, 0, cfg(36, 2)).is_err());

    // The trump card lies under the stock, so it cannot be in hand.
    assert!(manual(c(Clubs, Six), my_hand(), 0, cfg(36, 2)).is_err());
    assert!(manual(Card::Unknown, my_hand(), 0, cfg(36, 2)).is_err());
}

#[test]
fn manual_game_rejects_cards_outside_the_deck() {
    // A 36-card deck starts at six.
    let mut low = my_hand();
    low[0] = c(Spades, Two);
    assert!(manual(c(Hearts, Six), low.clone(), 0, cfg(36, 2)).is_err());
    assert!(manual(c(Hearts, Five), my_hand(), 0, cfg(36, 2)).is_err());
    // The same cards are fine in a 52-card deck.
    manual(c(Hearts, Five), low, 0, cfg(52, 2)).unwrap();
}

#[test]
fn manual_game_rejects_a_starter_who_is_not_at_the_table() {
    assert!(manual(c(Hearts, Six), my_hand(), 2, cfg(36, 2)).is_err());
    assert!(manual(c(Hearts, Six), my_hand(), 3, cfg(36, 3)).is_err());
    assert!(manual(c(Hearts, Six), my_hand(), 255, cfg(36, 3)).is_err());
    let s = manual(c(Hearts, Six), my_hand(), 2, cfg(36, 3)).unwrap();
    assert_eq!((s.attacker, s.defender), (P2, P0));
    assert_eq!(s.attackers, vec![P2, P1]);
}

#[test]
fn manual_game_checks_shown_opponent_trumps() {
    let start = |trumps| GameState::new_manual_game(c(Hearts, Six), my_hand(), 0, trumps, cfg(36, 3));

    let s = start(vec![(P1, Some(Nine)), (P2, None)]).unwrap();
    assert!(s.hands[1][0].matches(&c(Hearts, Nine)) && s.hands[1][0].is_public());
    assert_eq!(s.hands[1].iter().filter(|x| x.is_unknown()).count(), 5);

    assert!(start(vec![(P0, Some(Nine))]).is_err(), "not an opponent");
    assert!(start(vec![(PlayerId::P3, Some(Nine))]).is_err(), "not at the table");
    assert!(start(vec![(P1, Some(Six))]).is_err(), "that is the trump card itself");
    assert!(start(vec![(P1, Some(Two))]).is_err(), "not in a 36-card deck");
    assert!(start(vec![(P1, Some(Nine)), (P2, Some(Nine))]).is_err(), "same card twice");
    assert!(start(vec![(P1, Some(Nine)), (P1, Some(Ten))]).is_err(), "two entries for one player");
}

#[test]
fn validate_catches_corrupted_states() {
    let good = GameState::new_computer_game(7, cfg(36, 3)).unwrap();
    good.validate().unwrap();

    let mut s = good.clone();
    s.hands[0][0] = s.hands[1][0];
    assert!(s.validate().is_err(), "duplicate card");

    let mut s = good.clone();
    s.hands[0].pop();
    assert!(s.validate().is_err(), "missing card");

    let mut s = good.clone();
    s.hands.pop();
    assert!(s.validate().is_err(), "hand count does not match player count");

    let mut s = good.clone();
    s.defender = PlayerId::P4;
    assert!(s.validate().is_err(), "defender not at the table");

    let mut s = good.clone();
    s.defender = s.attacker;
    assert!(s.validate().is_err(), "attacker defends against themselves");

    let mut s = good.clone();
    s.phase = Phase::Defending;
    assert!(s.validate().is_err(), "defending with nothing to defend");

    let mut s = good.clone();
    s.current_attacker_idx = 9;
    assert!(s.validate().is_err(), "attacker index out of range");

    let mut s = good.clone();
    s.config.num_players = 5;
    assert!(s.validate().is_err(), "unsupported player count");

    let mut s = good.clone();
    let moved = s.hands[0].pop().unwrap();
    s.discard.push(Card::Unknown);
    s.stock.push(moved);
    assert!(s.validate().is_err(), "unknown card in the discard pile");
}

#[test]
fn states_survive_a_json_round_trip_and_bad_json_is_rejected() {
    let s = GameState::new_computer_game(3, cfg(36, 2)).unwrap();
    let json = serde_json::to_string(&s).unwrap();
    let back: GameState = serde_json::from_str(&json).unwrap();
    assert_eq!(back, s);

    // A six-player save from an older version parses but does not validate.
    let old = json.replace("\"num_players\":2", "\"num_players\":6");
    assert_ne!(old, json);
    let parsed: GameState = serde_json::from_str(&old).unwrap();
    assert!(parsed.validate().is_err());

    assert!(serde_json::from_str::<GameState>("{\"hands\": 3}").is_err());
}

#[test]
fn reveal_card_only_accepts_unseen_cards_for_unknown_slots() {
    let mut s = manual(c(Hearts, Six), my_hand(), 0, cfg(36, 2)).unwrap();
    assert!(s.reveal_card(P0, c(Spades, Ace)).is_err(), "my hand has no unknown cards");
    assert!(s.reveal_card(P1, c(Clubs, Six)).is_err(), "that card is in my hand");
    assert!(s.reveal_card(P1, c(Hearts, Six)).is_err(), "that is the trump card");
    assert!(s.reveal_card(P1, c(Spades, Two)).is_err(), "not in the deck");
    assert!(s.reveal_card(P2, c(Spades, Ace)).is_err(), "no such player");
    assert!(s.reveal_card(P1, Card::Unknown).is_err());

    s.reveal_card(P1, c(Spades, Ace)).unwrap();
    s.validate().unwrap();
    assert_eq!(s.hands[1].iter().filter(|x| x.is_unknown()).count(), 5);
    assert!(s.reveal_card(P1, c(Spades, Ace)).is_err(), "already revealed");
}
