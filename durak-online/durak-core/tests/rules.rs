//! Trick flow, limits, reflecting, refills, elimination and endings.

mod common;
use common::*;
use durak_core::*;

// ── Two players ─────────────────────────────────────────────────────────────

#[test]
fn defended_trick_is_discarded_and_roles_swap() {
    let mut pos = Pos::new(vec![vec![c(Clubs, Seven)], vec![c(Clubs, Nine), c(Diamonds, King)]]);
    // Drawn from the end: 8S, then 7S, then the trump card 6H last.
    pos.stock = vec![c(Hearts, Six), Card::private(Spades, Seven), Card::private(Spades, Eight)];
    let mut s = pos.build();

    play(&mut s, &[attack(c(Clubs, Seven)), defend(c(Clubs, Nine)), Action::PassAttack]);

    assert!(s.table.is_empty());
    assert!(s.discard.iter().any(|d| d.matches(&c(Clubs, Seven))));
    assert!(s.discard.iter().any(|d| d.matches(&c(Clubs, Nine))));
    // The attacker draws first and empties the three-card stock.
    assert_eq!(s.hand_size(P0), 3);
    assert_eq!(s.hand_size(P1), 1);
    assert!(s.stock.is_empty());
    // The trump card stays public, other drawn cards are private.
    let trump_in_hand = s.hands[0].iter().find(|x| x.matches(&c(Hearts, Six))).unwrap();
    assert!(trump_in_hand.is_public());
    assert!(s.hands[0].iter().find(|x| x.matches(&c(Spades, Eight))).unwrap().is_private());
    // The successful defender attacks next.
    assert_eq!((s.attacker, s.defender, s.phase), (P1, P0, Phase::Attacking));
}

#[test]
fn refill_order_is_attacker_then_defender() {
    let mut pos = Pos::new(vec![
        vec![c(Clubs, Seven), c(Spades, Ten)],
        vec![c(Clubs, Nine), c(Diamonds, King)],
    ]);
    pos.stock = vec![
        c(Hearts, Six),
        Card::private(Spades, Six),
        Card::private(Spades, Seven),
        Card::private(Spades, Eight),
        Card::private(Spades, Nine),
        Card::private(Diamonds, Six),
        Card::private(Diamonds, Seven),
    ];
    let mut s = pos.build();
    play(&mut s, &[attack(c(Clubs, Seven)), defend(c(Clubs, Nine)), Action::PassAttack]);

    // P0 had 1 card left and draws 5; P1 had 1 and gets the remaining 2.
    assert_eq!(s.hand_size(P0), 6);
    assert_eq!(s.hand_size(P1), 3);
    assert!(s.hands[0].iter().any(|x| x.matches(&c(Diamonds, Seven))), "attacker takes the top card");
    assert!(s.hands[1].iter().any(|x| x.matches(&c(Hearts, Six))), "defender draws last");
}

#[test]
fn taking_costs_the_defender_their_attack() {
    let pos = Pos::new(vec![
        vec![c(Clubs, Seven), c(Diamonds, Eight)],
        vec![c(Diamonds, Nine), c(Diamonds, King)],
    ]);
    let mut s = pos.build();
    play(&mut s, &[attack(c(Clubs, Seven)), Action::Take]);
    assert_eq!(s.phase, Phase::Throwing);
    assert_eq!(s.actor_to_move(), P0);
    assert_eq!(s.legal_actions(), vec![DONE], "nothing of rank 7 to throw in");

    play(&mut s, &[DONE]);
    assert_eq!(s.hand_size(P1), 3);
    assert!(s.hands[1].iter().all(Card::is_public), "taken cards are known to everyone");
    assert_eq!((s.attacker, s.defender), (P0, P1), "the attacker attacks again");
}

#[test]
fn undefended_cards_never_exceed_the_defenders_hand() {
    let pos = Pos::new(vec![
        vec![c(Clubs, Seven), c(Diamonds, Seven), c(Spades, Seven)],
        vec![c(Spades, Six)],
    ]);
    let mut s = pos.build();
    play(&mut s, &[attack(c(Clubs, Seven)), Action::Take]);
    assert_eq!(s.legal_actions(), vec![DONE]);
    assert!(s.apply(&throw(c(Diamonds, Seven))).is_err());
}

#[test]
fn a_trick_holds_at_most_six_attack_cards() {
    let pos = Pos::new(vec![
        vec![
            c(Clubs, Six), c(Diamonds, Six), c(Spades, Six),
            c(Clubs, Seven), c(Diamonds, Seven), c(Spades, Seven),
            c(Clubs, Eight),
        ],
        vec![
            c(Hearts, Seven), c(Hearts, Eight), c(Hearts, Nine), c(Hearts, Ten),
            c(Hearts, Jack), c(Hearts, Queen), c(Hearts, King), c(Hearts, Ace),
        ],
    ]);
    let mut s = pos.build();
    let rounds = [
        (c(Clubs, Six), c(Hearts, Seven)),
        (c(Diamonds, Six), c(Hearts, Eight)),
        (c(Spades, Six), c(Hearts, Nine)),
        (c(Clubs, Seven), c(Hearts, Ten)),
        (c(Diamonds, Seven), c(Hearts, Jack)),
        (c(Spades, Seven), c(Hearts, Queen)),
    ];
    for (i, (a, d)) in rounds.into_iter().enumerate() {
        play(&mut s, &[attack(a), defend_at(i, d)]);
    }
    assert_eq!(s.table.len(), MAX_PILES);
    // 8C matches a rank on the table and the defender still has cards, but the trick is full.
    assert_eq!(s.legal_actions(), vec![Action::PassAttack]);
    assert!(s.apply(&attack(c(Clubs, Eight))).is_err());
}

// ── Three players ───────────────────────────────────────────────────────────

fn three_player_start() -> GameState {
    Pos::new(vec![
        vec![c(Clubs, Seven), c(Spades, King)],
        vec![c(Clubs, Eight), c(Spades, Nine), c(Spades, Ten)],
        vec![c(Diamonds, Eight), c(Spades, Queen)],
    ])
    .build()
}

#[test]
fn second_attacker_may_throw_in_after_the_first_passes() {
    let mut s = three_player_start();
    assert_eq!(s.attackers, vec![P0, P2]);
    play(&mut s, &[attack(c(Clubs, Seven)), defend(c(Clubs, Eight))]);
    assert_eq!(s.actor_to_move(), P0);
    play(&mut s, &[Action::PassAttack]);
    assert_eq!(s.actor_to_move(), P2, "the other attacker gets a turn");
    assert!(has(&s, &attack(c(Diamonds, Eight))));
    assert!(!has(&s, &attack(c(Spades, Queen))), "only ranks already on the table");
}

#[test]
fn trick_ends_when_every_attacker_passes_and_defender_attacks_next() {
    let mut s = three_player_start();
    play(
        &mut s,
        &[attack(c(Clubs, Seven)), defend(c(Clubs, Eight)), Action::PassAttack, Action::PassAttack],
    );
    assert!(s.table.is_empty());
    assert_eq!((s.attacker, s.defender), (P1, P2));
    assert_eq!(s.attackers, vec![P1, P0]);
}

#[test]
fn after_a_take_the_player_behind_the_defender_attacks() {
    let mut s = three_player_start();
    play(
        &mut s,
        &[
            attack(c(Clubs, Seven)),
            defend(c(Clubs, Eight)),
            Action::PassAttack,
            attack(c(Diamonds, Eight)), // P2 throws in; P1 cannot beat it
            Action::Take,
        ],
    );
    assert_eq!(s.actor_to_move(), P2, "throwing starts with the attacker who played last");
    play(&mut s, &[DONE]);
    assert_eq!(s.actor_to_move(), P0, "every attacker gets a chance to throw");
    play(&mut s, &[DONE]);

    assert_eq!(s.hand_size(P1), 5);
    assert_eq!((s.attacker, s.defender), (P2, P0));
}

#[test]
fn a_player_without_cards_is_out_and_the_others_play_on() {
    let pos = Pos::new(vec![
        vec![c(Hearts, Ace)],
        vec![c(Clubs, Six), c(Diamonds, Seven)],
        vec![c(Diamonds, Eight), c(Diamonds, Nine)],
    ]);
    let mut s = pos.build();
    play(&mut s, &[attack(c(Hearts, Ace)), Action::Take, DONE, DONE]);

    assert!(!s.is_player_active(P0));
    assert_eq!(s.outcome(), Outcome::InProgress);
    assert_eq!((s.attacker, s.defender), (P2, P1));
    assert_eq!(s.attackers, vec![P2], "the finished player no longer attacks");
}

// ── Reflecting ──────────────────────────────────────────────────────────────

fn reflect_2p(reflecting: bool, p0_extra: Vec<Card>) -> GameState {
    let mut p0 = vec![c(Clubs, Seven)];
    p0.extend(p0_extra);
    let mut pos = Pos::new(vec![p0, vec![c(Diamonds, Seven), c(Spades, Jack)]]);
    pos.reflecting = reflecting;
    let mut s = pos.build();
    play(&mut s, &[attack(c(Clubs, Seven))]);
    s
}

#[test]
fn two_player_reflect_passes_the_attack_back_to_the_attacker() {
    let mut s = reflect_2p(true, vec![c(Spades, Nine), c(Spades, Ten)]);
    let reflect = Action::Reflect { card: c(Diamonds, Seven) };
    assert!(has(&s, &reflect));
    play(&mut s, &[reflect]);

    assert_eq!((s.attacker, s.defender, s.phase), (P1, P0, Phase::Defending));
    assert_eq!(s.table.len(), 2);
    assert_eq!(s.actor_to_move(), P0);
    assert!(s.table.iter().all(|p| p.defense.is_none()));
}

#[test]
fn reflect_is_off_unless_the_rule_is_enabled() {
    let s = reflect_2p(false, vec![c(Spades, Nine), c(Spades, Ten)]);
    assert!(!s.legal_actions().iter().any(|a| matches!(a, Action::Reflect { .. })));
    let mut t = s.clone();
    assert!(t.apply(&Action::Reflect { card: c(Diamonds, Seven) }).is_err());
    assert_eq!(t, s);
}

#[test]
fn reflect_needs_a_target_who_can_answer_every_card() {
    // After attacking, P0 holds one card but would face two.
    let s = reflect_2p(true, vec![c(Spades, Nine)]);
    assert!(!s.legal_actions().iter().any(|a| matches!(a, Action::Reflect { .. })));
}

#[test]
fn reflect_is_not_allowed_after_defending_a_card() {
    let mut pos = Pos::new(vec![
        vec![c(Clubs, Seven), c(Spades, Seven), c(Spades, Nine), c(Spades, Ten)],
        vec![c(Clubs, Jack), c(Diamonds, Seven), c(Diamonds, Queen)],
    ]);
    pos.reflecting = true;
    let mut s = pos.build();
    play(&mut s, &[attack(c(Clubs, Seven)), defend(c(Clubs, Jack)), attack(c(Spades, Seven))]);
    assert!(!s.legal_actions().iter().any(|a| matches!(a, Action::Reflect { .. })));
}

#[test]
fn three_player_reflect_moves_to_the_next_player_each_time() {
    let mut pos = Pos::new(vec![
        vec![c(Clubs, Seven), c(Spades, Nine), c(Spades, Ten), c(Diamonds, Jack)],
        vec![c(Diamonds, Seven), c(Spades, Jack)],
        vec![c(Spades, Seven), c(Spades, Queen), c(Spades, King)],
    ]);
    pos.reflecting = true;
    let mut s = pos.build();
    play(&mut s, &[attack(c(Clubs, Seven)), Action::Reflect { card: c(Diamonds, Seven) }]);
    assert_eq!((s.attacker, s.defender), (P1, P2));
    assert_eq!(s.attackers, vec![P1, P0]);

    play(&mut s, &[Action::Reflect { card: c(Spades, Seven) }]);
    assert_eq!((s.attacker, s.defender), (P2, P0));
    assert_eq!(s.attackers, vec![P2, P1]);
    assert_eq!(s.table.len(), 3);
}

#[test]
fn reflect_skips_a_player_who_is_already_out() {
    let mut pos = Pos::new(vec![
        vec![c(Clubs, Seven), c(Spades, Nine), c(Spades, Ten)],
        vec![c(Diamonds, Seven), c(Spades, Jack)],
        vec![],
    ]);
    pos.reflecting = true;
    let mut s = pos.build();
    play(&mut s, &[attack(c(Clubs, Seven)), Action::Reflect { card: c(Diamonds, Seven) }]);
    assert_eq!(s.defender, P0);
}

#[test]
fn trump_reflect_shows_the_card_once_and_keeps_it_in_hand() {
    let mut pos = Pos::new(vec![
        vec![c(Clubs, Seven), c(Diamonds, Seven), c(Spades, Nine), c(Spades, Ten)],
        vec![Card::private(Hearts, Seven), c(Spades, Jack), c(Spades, Queen)],
    ]);
    pos.reflecting = true;
    pos.trump_reflecting = true;
    let mut s = pos.build();
    let show = Action::ReflectTrump { card: c(Hearts, Seven) };

    play(&mut s, &[attack(c(Clubs, Seven))]);
    assert!(has(&s, &show));
    play(&mut s, &[show.clone()]);
    assert_eq!(s.hand_size(P1), 3, "the trump is shown, not played");
    assert!(s.hands[1].iter().find(|x| x.matches(&c(Hearts, Seven))).unwrap().is_public());
    assert_eq!((s.defender, s.table.len()), (P0, 1));

    // P0 passes it back by playing a seven; P1 may not show the same trump again.
    play(&mut s, &[Action::Reflect { card: c(Diamonds, Seven) }]);
    assert_eq!(s.defender, P1);
    assert!(!has(&s, &show));
    let before = s.clone();
    assert!(s.apply(&show).is_err());
    assert_eq!(s, before);
}

#[test]
fn trump_reflect_requires_the_rule_and_a_trump() {
    let mut pos = Pos::new(vec![
        vec![c(Clubs, Seven), c(Spades, Nine)],
        vec![c(Hearts, Seven), c(Diamonds, Seven)],
    ]);
    pos.trump_reflecting = false;
    let mut s = pos.build();
    play(&mut s, &[attack(c(Clubs, Seven))]);
    assert!(s.apply(&Action::ReflectTrump { card: c(Hearts, Seven) }).is_err());

    let mut pos = Pos::new(vec![
        vec![c(Clubs, Seven), c(Spades, Nine)],
        vec![c(Hearts, Seven), c(Diamonds, Seven)],
    ]);
    pos.trump_reflecting = true;
    let mut s = pos.build();
    play(&mut s, &[attack(c(Clubs, Seven))]);
    assert!(has(&s, &Action::ReflectTrump { card: c(Hearts, Seven) }));
    assert!(s.apply(&Action::ReflectTrump { card: c(Diamonds, Seven) }).is_err(), "not a trump");
}

// ── Endings ─────────────────────────────────────────────────────────────────

#[test]
fn beating_the_last_attack_with_the_last_card_is_a_draw() {
    let mut s = Pos::new(vec![vec![c(Clubs, King)], vec![c(Clubs, Ace)]]).build();
    play(&mut s, &[attack(c(Clubs, King))]);
    assert_eq!(s.outcome(), Outcome::InProgress, "the defender can still get out");
    assert!(has(&s, &defend(c(Clubs, Ace))));

    play(&mut s, &[defend(c(Clubs, Ace))]);
    assert_eq!(s.outcome(), Outcome::Draw);
    assert_eq!(s.durak(), None);
    assert!(s.is_terminal());
    assert!(s.legal_actions().is_empty());
    assert!(s.apply(&Action::PassAttack).is_err());
}

#[test]
fn the_last_player_holding_cards_is_the_durak() {
    let mut s = Pos::new(vec![vec![c(Clubs, King)], vec![c(Diamonds, Six)]]).build();
    play(&mut s, &[attack(c(Clubs, King))]);
    assert_eq!(s.legal_actions(), vec![Action::Take]);
    play(&mut s, &[Action::Take]);
    assert_eq!(s.outcome(), Outcome::Durak { player: P1 });
    assert_eq!(s.durak(), Some(P1));
}

#[test]
fn defender_left_with_cards_after_defending_everything_is_the_durak() {
    let mut s =
        Pos::new(vec![vec![c(Clubs, King)], vec![c(Clubs, Ace), c(Diamonds, Six)]]).build();
    play(&mut s, &[attack(c(Clubs, King)), defend(c(Clubs, Ace))]);
    assert_eq!(s.outcome(), Outcome::Durak { player: P1 });
}

#[test]
fn three_player_game_ends_with_a_single_durak() {
    let pos = Pos::new(vec![vec![c(Hearts, Ace)], vec![c(Clubs, Six)], vec![c(Clubs, Seven)]]);
    let mut s = pos.build();
    // P0 gets out; P1 takes the ace, then P2 attacks P1 with the 7C, which the ace beats.
    play(&mut s, &[attack(c(Hearts, Ace)), Action::Take, DONE, DONE]);
    assert_eq!((s.attacker, s.defender), (P2, P1));
    play(&mut s, &[attack(c(Clubs, Seven)), defend(c(Hearts, Ace))]);
    assert_eq!(s.outcome(), Outcome::Durak { player: P1 });
}

// ── Rejected actions ────────────────────────────────────────────────────────

#[test]
fn illegal_actions_are_rejected_and_change_nothing() {
    let start = Pos::new(vec![
        vec![c(Clubs, Seven), c(Diamonds, Eight)],
        vec![c(Clubs, Six), c(Clubs, Nine)],
    ])
    .build();

    for bad in [
        Action::Take,
        Action::PassAttack,
        DONE,
        attack(c(Clubs, Nine)),      // the opponent's card
        attack(c(Spades, Ace)),      // already discarded
        attack(Card::Unknown),
        defend(c(Clubs, Nine)),
    ] {
        let mut s = start.clone();
        assert!(s.apply(&bad).is_err(), "{bad:?} should be rejected");
        assert_eq!(s, start, "{bad:?} must not change the state");
    }

    let mut defending = start.clone();
    play(&mut defending, &[attack(c(Clubs, Seven))]);
    for bad in [
        defend(c(Clubs, Six)),             // lower card
        defend_at(1, c(Clubs, Nine)),      // no such pile
        defend(c(Diamonds, Eight)),        // the attacker's card
        attack(c(Clubs, Nine)),
        Action::PassAttack,
    ] {
        let mut s = defending.clone();
        assert!(s.apply(&bad).is_err(), "{bad:?} should be rejected");
        assert_eq!(s, defending, "{bad:?} must not change the state");
    }
}

#[test]
fn an_opponent_with_unknown_cards_can_only_play_unseen_cards() {
    let hand = vec![
        c(Clubs, Six), c(Clubs, Seven), c(Clubs, Eight),
        c(Clubs, Nine), c(Clubs, Ten), c(Clubs, Jack),
    ];
    let s = GameState::new_manual_game(c(Hearts, Six), hand.clone(), 1, vec![], GameConfig::default())
        .unwrap();
    assert_eq!(s.actor_to_move(), P1);
    let legal = s.legal_actions();
    assert_eq!(legal.len(), 36 - 6 - 1, "every card except mine and the trump card");
    for mine in &hand {
        assert!(!has(&s, &attack(*mine)));
        let mut t = s.clone();
        assert!(t.apply(&attack(*mine)).is_err());
        assert_eq!(t, s);
    }
    assert!(!has(&s, &attack(c(Hearts, Six))), "the trump card is under the stock");

    let mut t = s.clone();
    play(&mut t, &[attack(c(Spades, Ace))]);
    assert_eq!(t.hand_size(P1), 5, "one unknown card turned out to be the ace");
}

// ── Five-card first trick ───────────────────────────────────────────────────

/// Nothing discarded yet: every card not in a hand is still in the stock.
fn first_trick(limit: bool) -> GameState {
    let p0 = vec![
        c(Clubs, Six), c(Diamonds, Six), c(Spades, Six),
        c(Clubs, Seven), c(Diamonds, Seven), c(Spades, Seven),
    ];
    let p1 = vec![
        c(Hearts, Seven), c(Hearts, Eight), c(Hearts, Nine),
        c(Hearts, Ten), c(Hearts, Jack), c(Hearts, Queen),
    ];
    let in_hand: Vec<Card> = p0.iter().chain(&p1).copied().collect();
    let mut stock: Vec<Card> = full_deck(36)
        .unwrap()
        .into_iter()
        .filter(|x| !in_hand.iter().any(|h| h.matches(x)) && !x.matches(&c(Hearts, King)))
        .map(|x| x.as_private())
        .collect();
    stock.insert(0, c(Hearts, King)); // the face-up trump card
    let mut pos = Pos::new(vec![p0, p1]);
    pos.stock = stock;
    pos.first_trick_limit = limit;
    let s = pos.build();
    assert!(s.discard.is_empty());
    s
}

fn play_five_piles(s: &mut GameState) {
    let rounds = [
        (c(Clubs, Six), c(Hearts, Seven)),
        (c(Diamonds, Six), c(Hearts, Eight)),
        (c(Spades, Six), c(Hearts, Nine)),
        (c(Clubs, Seven), c(Hearts, Ten)),
        (c(Diamonds, Seven), c(Hearts, Jack)),
    ];
    for (i, (a, d)) in rounds.into_iter().enumerate() {
        play(s, &[attack(a), defend_at(i, d)]);
    }
}

#[test]
fn first_trick_stops_at_five_cards_when_the_rule_is_on() {
    let mut s = first_trick(true);
    assert_eq!(s.pile_limit(), FIRST_TRICK_PILES);
    play_five_piles(&mut s);
    assert_eq!(s.legal_actions(), vec![Action::PassAttack], "7S matches but the first trick is full");
    let before = s.clone();
    assert!(s.apply(&attack(c(Spades, Seven))).is_err());
    assert_eq!(s, before);

    // After the first discard the normal six-card limit applies.
    play(&mut s, &[Action::PassAttack]);
    assert!(!s.discard.is_empty());
    assert_eq!(s.pile_limit(), MAX_PILES);
}

#[test]
fn first_trick_allows_six_cards_when_the_rule_is_off() {
    let mut s = first_trick(false);
    assert_eq!(s.pile_limit(), MAX_PILES);
    play_five_piles(&mut s);
    assert!(has(&s, &attack(c(Spades, Seven))));
}

#[test]
fn a_take_does_not_end_the_five_card_period() {
    let mut s = first_trick(true);
    // P1 takes instead of defending: nothing is discarded, so the limit stays at five.
    play(&mut s, &[attack(c(Clubs, Six)), Action::Take, DONE]);
    assert!(s.discard.is_empty());
    assert_eq!(s.pile_limit(), FIRST_TRICK_PILES);
}

/// Five undefended attack cards, nothing discarded yet, and the defender holds a
/// seven that could pass the attack on.
fn five_open_piles(limit: bool) -> GameState {
    let table = vec![
        pile(c(Clubs, Seven), None),
        pile(c(Diamonds, Seven), None),
        pile(c(Clubs, Eight), None),
        pile(c(Diamonds, Eight), None),
        pile(c(Spades, Eight), None),
    ];
    let p0 = vec![c(Clubs, Nine), c(Clubs, Ten), c(Clubs, Jack), c(Clubs, Queen), c(Clubs, King), c(Clubs, Ace)];
    let p1 = vec![c(Spades, Seven), c(Spades, Nine), c(Spades, Ten), c(Spades, Jack), c(Spades, Queen), c(Spades, King)];
    let used: Vec<Card> = p0.iter().chain(&p1).copied().chain(table.iter().map(|p| p.attack)).collect();
    let mut stock: Vec<Card> = full_deck(36)
        .unwrap()
        .into_iter()
        .filter(|x| !used.iter().any(|u| u.matches(x)) && !x.matches(&c(Hearts, Ace)))
        .map(|x| x.as_private())
        .collect();
    stock.insert(0, c(Hearts, Ace));
    let mut pos = Pos::new(vec![p0, p1]);
    pos.table = table;
    pos.stock = stock;
    pos.phase = Phase::Defending;
    pos.reflecting = true;
    pos.first_trick_limit = limit;
    let s = pos.build();
    assert!(s.discard.is_empty());
    s
}

#[test]
fn reflecting_respects_the_first_trick_limit() {
    let reflect = Action::Reflect { card: c(Spades, Seven) };
    // A reflect would make a sixth card, which the first trick does not allow.
    let s = five_open_piles(true);
    assert!(!has(&s, &reflect));
    let mut t = s.clone();
    assert!(t.apply(&reflect).is_err());
    assert_eq!(t, s);
    // Without the rule the reflect is fine.
    assert!(has(&five_open_piles(false), &reflect));
}

// ── Classic ending ──────────────────────────────────────────────────────────

fn last_cards(classic: bool) -> GameState {
    let mut pos = Pos::new(vec![vec![c(Clubs, King)], vec![c(Clubs, Ace)]]);
    pos.classic_ending = classic;
    let mut s = pos.build();
    play(&mut s, &[attack(c(Clubs, King))]);
    s
}

#[test]
fn classic_ending_makes_the_last_attacker_the_durak_instead_of_a_draw() {
    let mut s = last_cards(true);
    assert_eq!(s.outcome(), Outcome::InProgress, "the defender still has to answer");
    play(&mut s, &[defend(c(Clubs, Ace))]);
    assert_eq!(s.outcome(), Outcome::Durak { player: P0 });
    assert_eq!(s.durak(), Some(P0));
    assert!(s.legal_actions().is_empty());

    let mut d = last_cards(false);
    play(&mut d, &[defend(c(Clubs, Ace))]);
    assert_eq!(d.outcome(), Outcome::Draw);
}

#[test]
fn classic_ending_changes_nothing_when_the_defender_takes() {
    let mut s = last_cards(true);
    play(&mut s, &[Action::Take]);
    assert_eq!(s.outcome(), Outcome::Durak { player: P1 });
}

#[test]
fn classic_ending_blames_the_player_who_reflected_with_their_last_card() {
    // P1 passes the attack back with their last card; P0 beats both sevens and
    // empties their hand at the same moment.
    let mut pos = Pos::new(vec![
        vec![c(Clubs, Seven), c(Clubs, Ace), c(Diamonds, Ace)],
        vec![c(Diamonds, Seven)],
    ]);
    pos.reflecting = true;
    pos.classic_ending = true;
    let mut s = pos.build();
    play(&mut s, &[attack(c(Clubs, Seven))]);
    // P0 holds two cards and would face two: the reflect is allowed.
    play(&mut s, &[Action::Reflect { card: c(Diamonds, Seven) }]);
    assert_eq!((s.attacker, s.defender), (P1, P0));
    play(&mut s, &[defend_at(0, c(Clubs, Ace)), defend_at(1, c(Diamonds, Ace))]);
    assert_eq!(s.outcome(), Outcome::Durak { player: P1 });
}

#[test]
fn the_search_follows_the_chosen_ending() {
    // Draw rules: beating the last card is worth 1 to the defender, so is the
    // classic ending. For the attacker the classic ending turns it into a loss.
    let p = SolveParams { determinizations: 1, rollouts: 500, ..SolveParams::default() };
    for classic in [false, true] {
        let s = last_cards(classic);
        let r = solve(&s, p, 1).unwrap();
        let beat = r.actions.iter().find(|e| matches!(e.action, Action::Defend { .. })).unwrap();
        assert_eq!(beat.score, 1.0, "classic = {classic}");
    }
    // From the attacker's seat one move earlier: with draws the king leads to a
    // draw at worst (worth 1); classic makes it a loss once the ace is played.
    let attacker_view = |classic: bool| {
        let mut pos = Pos::new(vec![vec![c(Clubs, King)], vec![c(Clubs, Ace)]]);
        pos.classic_ending = classic;
        let s = pos.build();
        solve(&s, p, 2).unwrap().actions[0].score
    };
    assert_eq!(attacker_view(false), 1.0);
    // The score averages playouts, so the few early tries of "take" leave a trace.
    assert!(attacker_view(true) < 0.1, "classic: {}", attacker_view(true));
}
