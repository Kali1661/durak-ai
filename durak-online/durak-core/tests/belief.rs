//! Guessing the opponent's hand from their play.

use durak_core::*;
use rand::{rngs::StdRng, SeedableRng};

fn game(seed: u64) -> GameState {
    let config = GameConfig { deck_size: 36, num_players: 2, reflecting: true, ..GameConfig::default() };
    GameState::new_computer_game(seed, config).unwrap()
}

/// Play on with the rules player until `stop` says so; returns the position
/// before the move, the move, and the position after.
fn play_until(mut s: GameState, stop: impl Fn(&GameState, &Action) -> bool) -> Option<(GameState, Action, GameState)> {
    for _ in 0..400 {
        if s.is_terminal() {
            return None;
        }
        let a = policy::heuristic_action(&s).unwrap();
        let before = s.clone();
        s.apply(&a).unwrap();
        if stop(&before, &a) {
            return Some((before, a, s));
        }
    }
    None
}

#[test]
fn knowing_nothing_deals_as_before() {
    let s = game(1);
    let blank = Beliefs::default();
    assert!(blank.is_blank());
    let plain = s.determinize(PlayerId::P0, &mut StdRng::seed_from_u64(4)).unwrap();
    let with = s.determinize_with(PlayerId::P0, &mut StdRng::seed_from_u64(4), Some(&blank)).unwrap();
    assert_eq!(plain, with);
}

#[test]
fn a_take_makes_higher_cards_of_the_suit_unlikely_and_dealing_follows() {
    // Find a game where P1 takes a single non-trump card.
    let found = (1..200).find_map(|seed| {
        play_until(game(seed), |before, a| {
            *a == Action::Take
                && before.actor_to_move() == PlayerId::P1
                && before.table.len() == 1
                && before.table[0].attack.id().is_some_and(|(suit, rank)| suit != before.trump && rank < Rank::Queen)
                && before.stock.len() > 8
        })
    });
    let (before, action, after) = found.expect("some game has such a take");
    let (suit, rank) = before.table[0].attack.id().unwrap();

    let mut beliefs = Beliefs::default();
    beliefs.observe(PlayerId::P0, &before, &action, &after);
    assert!(!beliefs.is_blank());
    let higher: Vec<CardId> = Rank::ALL.into_iter().filter(|r| *r > rank).map(|r| (suit, r)).collect();
    assert!(higher.iter().all(|id| beliefs.weight(*id) < 1.0));
    assert_eq!(beliefs.weight((suit, rank)), 1.0, "nothing is said about the card itself");
    let other = Suit::ALL.into_iter().find(|x| *x != suit && *x != before.trump).unwrap();
    assert_eq!(beliefs.weight((other, Rank::Ace)), 1.0);

    // Dealt by those beliefs, P1 holds such cards less often than by chance.
    let held = |beliefs: Option<&Beliefs>| -> usize {
        (0..400u64)
            .map(|d| {
                let deal = after.determinize_with(PlayerId::P0, &mut StdRng::seed_from_u64(d), beliefs).unwrap();
                // Cards P0 can see in P1's hand (the ones just taken) do not count.
                deal.hands[1]
                    .iter()
                    .zip(&after.hands[1])
                    .filter(|(c, was)| !was.is_public() && c.id().is_some_and(|id| higher.contains(&id)))
                    .count()
            })
            .sum()
    };
    let (plain, guided) = (held(None), held(Some(&beliefs)));
    assert!(plain > 0, "the test position has such cards hidden");
    assert!(guided * 10 < plain * 8, "guided {guided} against plain {plain}");
}

#[test]
fn what_was_learned_fades_as_the_opponent_draws() {
    let s = game(3);
    let mut beliefs = Beliefs::new(Strength::default());
    let mut lowest = 1.0f32;
    let mut seen_fade = false;
    let mut cur = s;
    for _ in 0..300 {
        if cur.is_terminal() {
            break;
        }
        let a = policy::heuristic_action(&cur).unwrap();
        let before = cur.clone();
        cur.apply(&a).unwrap();
        let low_before = Rank::ALL.iter().flat_map(|r| Suit::ALL.map(|x| beliefs.weight((x, *r)))).fold(1.0f32, f32::min);
        beliefs.observe(PlayerId::P0, &before, &a, &cur);
        let low_after = Rank::ALL.iter().flat_map(|r| Suit::ALL.map(|x| beliefs.weight((x, *r)))).fold(1.0f32, f32::min);
        lowest = lowest.min(low_after);
        // My own moves teach nothing; a weight only rises when cards were drawn.
        if low_after > low_before + 1e-6 {
            assert!(cur.stock.len() < before.stock.len());
            seen_fade = true;
        }
        assert!(low_after >= belief::FLOOR - 1e-6);
    }
    assert!(lowest < 1.0, "the game taught something");
    assert!(seen_fade, "and some of it faded");
}

#[test]
fn a_search_can_use_beliefs() {
    let s = game(5);
    let mut beliefs = Beliefs::default();
    let (before, action, after) = play_until(s, |before, _| before.actor_to_move() == PlayerId::P1).unwrap();
    beliefs.observe(PlayerId::P0, &before, &action, &after);
    let mut cur = after;
    while cur.actor_to_move() != PlayerId::P0 {
        let a = policy::heuristic_action(&cur).unwrap();
        cur.apply(&a).unwrap();
    }
    let r = Search::new(&cur, SolveParams::app(), 1).unwrap().with_beliefs(Some(beliefs)).run(2, 200).unwrap();
    assert_eq!(r.total_visits, 400);
    assert!(r.actions.iter().all(|a| (0.0..=1.0).contains(&a.score)));
}

#[test]
fn taking_again_and_again_says_nothing_new() {
    // An opponent who picks cards up on purpose does it repeatedly; the
    // cards they "cannot beat" must not sink lower with every take.
    let found = (1..200).find_map(|seed| {
        play_until(game(seed), |before, a| {
            *a == Action::Take
                && before.actor_to_move() == PlayerId::P1
                && before.table.len() == 1
                && before.table[0].attack.id().is_some_and(|(suit, rank)| suit != before.trump && rank < Rank::Queen)
        })
    });
    let (before, action, after) = found.expect("some game has such a take");
    let (suit, _) = before.table[0].attack.id().unwrap();
    let ace = (suit, Rank::Ace);

    let mut once = Beliefs::default();
    once.observe(PlayerId::P0, &before, &action, &after);
    let mut thrice = once.clone();
    thrice.observe(PlayerId::P0, &before, &action, &after);
    thrice.observe(PlayerId::P0, &before, &action, &after);
    assert!(once.weight(ace) < 1.0);
    assert_eq!(once.weight(ace), thrice.weight(ace));
    assert_eq!(once.weight((before.trump, Rank::Ace)), thrice.weight((before.trump, Rank::Ace)));

    // The old behaviour, kept for comparisons, multiplies each time.
    let stacking = Strength { take_stacks: true, ..Strength::default() };
    let mut old = Beliefs::new(stacking);
    old.observe(PlayerId::P0, &before, &action, &after);
    let first = old.weight(ace);
    old.observe(PlayerId::P0, &before, &action, &after);
    assert!(old.weight(ace) < first);
}
