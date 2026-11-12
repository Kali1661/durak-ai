//! Positions recorded from real games, one per line (see `tests/data`).
//!
//! Fields are `|`-separated: game, step, trump letter, classic ending (0/1),
//! reflecting, five-card first trick, deck size, attacker, phase letter, my
//! hand, opponent hand, table (attack+defence pairs, `--` = not beaten),
//! stock, discard, the move played, the scores shown, the game's result.
//! Cards are rank + suit letter, e.g. `TS` = ten of spades.
#![allow(dead_code)]

use durak_core::GameState;
use serde_json::{json, Value};

fn card(code: &str, kind: &str) -> Value {
    let rank = match &code[0..1] {
        "2" => "Two", "3" => "Three", "4" => "Four", "5" => "Five", "6" => "Six", "7" => "Seven",
        "8" => "Eight", "9" => "Nine", "T" => "Ten", "J" => "Jack", "Q" => "Queen", "K" => "King",
        "A" => "Ace",
        other => panic!("bad rank {other}"),
    };
    json!({ "type": kind, "suit": suit(&code[1..2]), "rank": rank })
}

fn suit(letter: &str) -> &'static str {
    match letter {
        "C" => "Clubs", "D" => "Diamonds", "H" => "Hearts", "S" => "Spades",
        other => panic!("bad suit {other}"),
    }
}

fn cards(codes: &str, kind: &str) -> Vec<Value> {
    (0..codes.len() / 2).map(|i| card(&codes[i * 2..i * 2 + 2], kind)).collect()
}

/// The position and the raw fields of one line.
pub fn parse(line: &str) -> (GameState, Vec<&str>) {
    let f: Vec<&str> = line.split('|').collect();
    let table: Vec<Value> = (0..f[11].len() / 4)
        .map(|i| {
            let pair = &f[11][i * 4..i * 4 + 4];
            let defense = if &pair[2..4] == "--" { Value::Null } else { card(&pair[2..4], "public") };
            json!({ "attack": card(&pair[0..2], "public"), "defense": defense })
        })
        .collect();
    let defender = if f[7] == "P0" { "P1" } else { "P0" };
    let phase = match f[8] { "A" => "Attacking", "D" => "Defending", _ => "Throwing" };
    let state = json!({
        "trump": suit(f[2]), "attacker": f[7], "defender": defender, "phase": phase,
        "attackers": [f[7]], "current_attacker_idx": 0, "last_played_attacker": f[7], "throw_start_idx": 0,
        "hands": [cards(f[9], "private"), cards(f[10], "private")],
        "stock": cards(f[12], "public"), "table": table, "discard": cards(f[13], "public"),
        "reflected_trumps": [],
        "config": {
            "deck_size": f[6].parse::<usize>().unwrap(), "num_players": 2, "trump_reflecting": false,
            "first_trick_limit": f[5] == "1", "classic_ending": f[3] == "1", "reflecting": f[4] == "1",
        },
    });
    (serde_json::from_value(state).expect("state parses"), f)
}

