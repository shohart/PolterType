#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use serde_json::Value;

use super::reply::answer;

fn parse(s: &str) -> Value {
    serde_json::from_str(s).unwrap()
}

#[test]
fn suggest_offers_the_word_capitalised_and_reversed() {
    let r =
        parse(&answer(r#"{"id":3,"op":"suggest","layout":"en-US","word":"teh","max":5}"#).unwrap());
    assert_eq!(r["id"], 3);
    let texts: Vec<&str> = r["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["text"].as_str().unwrap())
        .collect();
    assert_eq!(texts, ["Teh", "het"]);
}

#[test]
fn a_word_it_cannot_change_is_a_pass() {
    let r =
        parse(&answer(r#"{"id":4,"op":"suggest","layout":"en-US","word":"A","max":5}"#).unwrap());
    assert_eq!(r["pass"], true);
}

#[test]
fn rewrite_adds_a_capital_and_a_full_stop() {
    let r = parse(&answer(r#"{"id":5,"op":"rewrite","layout":"ru-RU","text":"я думаю"}"#).unwrap());
    assert_eq!(r["text"], "Я думаю.");
    assert_eq!(r["kind"], "punctuation");
}

#[test]
fn garbage_gets_no_answer() {
    assert!(answer("not json").is_none());
    assert!(answer(r#"{"op":"suggest"}"#).is_none());
}
