//! What a provider's reply must get past before PolterType shows it.

use super::*;

fn suggest(word: &str, max: usize) -> Request {
    Request::Suggest {
        layout: "ru-RU".to_owned(),
        word: word.to_owned(),
        max,
    }
}

fn rewrite(text: &str) -> Request {
    Request::Rewrite {
        layout: "ru-RU".to_owned(),
        text: text.to_owned(),
    }
}

fn reply(request: &Request, line: &str) -> Reply {
    let (_, raw) = parse_reply(line).expect("parses");
    check_reply(request, raw)
}

#[test]
fn requests_are_one_json_line_in_the_documented_shape() {
    let line = encode_request(41, &suggest("ничепуренко", 5));
    assert!(line.ends_with('\n') && !line.trim_end().contains('\n'));
    let v: serde_json::Value = serde_json::from_str(&line).expect("json");
    assert_eq!(v["id"], 41);
    assert_eq!(v["op"], "suggest");
    assert_eq!(v["layout"], "ru-RU");
    assert_eq!(v["word"], "ничепуренко");
    assert_eq!(v["max"], 5);
    assert!(v.get("text").is_none());

    let line = encode_request(42, &rewrite("я думаю\nчто"));
    assert!(
        !line.trim_end().contains('\n'),
        "a newline in the text is escaped"
    );
    let v: serde_json::Value = serde_json::from_str(&line).expect("json");
    assert_eq!(v["op"], "rewrite");
    assert_eq!(v["text"], "я думаю\nчто");
}

#[test]
fn the_hello_must_carry_the_exact_token() {
    assert!(is_hello(r#"{"hello":"abc123"}"#, "abc123"));
    assert!(is_hello("{\"hello\":\"abc123\"}\n", "abc123"));
    assert!(!is_hello(r#"{"hello":"abc124"}"#, "abc123"));
    assert!(!is_hello(r#"{"hello":"abc12"}"#, "abc123"));
    assert!(!is_hello(r#"{"hello":""}"#, ""));
    assert!(!is_hello("abc123", "abc123"));
}

#[test]
fn suggest_candidates_are_deduplicated_ordered_and_capped() {
    let r = reply(
        &suggest("ничепуренко", 2),
        r#"{"id":1,"candidates":[
            {"text":"Ничипуренко","score":0.6},
            {"text":"ничепуренко","score":0.99},
            {"text":"Ничипоренко","score":0.9},
            {"text":"Ничипуренко","score":0.5},
            {"text":"Нечипуренко","score":0.1}]}"#,
    );
    let Reply::Candidates(c) = r else {
        panic!("expected candidates, got {r:?}")
    };
    let texts: Vec<&str> = c.iter().map(|c| c.text.as_str()).collect();
    assert_eq!(
        texts,
        ["Ничипоренко", "Ничипуренко"],
        "typed word dropped, best first"
    );
}

#[test]
fn one_bad_candidate_spoils_the_reply() {
    let req = suggest("teh", 5);
    for bad in [
        r#"{"id":1,"candidates":[{"text":"the"},{"text":"t\u0007he"}]}"#,
        r#"{"id":1,"candidates":[{"text":"the"},{"text":"the end"}]}"#,
        r#"{"id":1,"candidates":[{"text":""}]}"#,
    ] {
        assert_eq!(reply(&req, bad), Reply::Pass, "{bad}");
    }
    let long = format!(
        r#"{{"id":1,"candidates":[{{"text":"{}"}}]}}"#,
        "a".repeat(65)
    );
    assert_eq!(reply(&req, &long), Reply::Pass);
}

#[test]
fn scores_are_clamped_and_a_missing_one_is_zero() {
    let r = reply(
        &suggest("teh", 5),
        r#"{"id":1,"candidates":[{"text":"tea"},{"text":"the","score":7.5}]}"#,
    );
    let Reply::Candidates(c) = r else {
        panic!("expected candidates")
    };
    assert_eq!(c[0].text, "the");
    assert!((c[0].score - 1.0).abs() < f32::EPSILON);
    assert!(c[1].score.abs() < f32::EPSILON);
}

#[test]
fn a_reply_of_the_wrong_shape_is_no_opinion() {
    // Rewrite text in answer to a suggest, and candidates to a rewrite.
    assert_eq!(
        reply(&suggest("teh", 5), r#"{"id":1,"text":"the"}"#),
        Reply::Pass
    );
    assert_eq!(
        reply(&rewrite("a b"), r#"{"id":1,"candidates":[{"text":"x"}]}"#),
        Reply::Pass
    );
    assert_eq!(
        reply(&suggest("teh", 5), r#"{"id":1,"pass":true}"#),
        Reply::Pass
    );
    assert_eq!(
        reply(&suggest("teh", 5), r#"{"id":1,"candidates":[]}"#),
        Reply::Pass
    );
}

#[test]
fn a_rewrite_must_stay_the_same_sentence() {
    let input = "я думаю что это правильно";
    let ok = reply(
        &rewrite(input),
        r#"{"id":2,"text":"Я думаю, что это правильно.","kind":"punctuation"}"#,
    );
    assert_eq!(
        ok,
        Reply::Rewrite {
            text: "Я думаю, что это правильно.".to_owned(),
            kind: Some("punctuation".to_owned()),
        }
    );

    let chatty = format!(
        r#"{{"id":2,"text":"Sure! Here is the corrected sentence: {}"}}"#,
        "x".repeat(80)
    );
    assert_eq!(reply(&rewrite(input), &chatty), Reply::Pass, "far longer");
    assert_eq!(
        reply(&rewrite(input), r#"{"id":2,"text":"Я."}"#),
        Reply::Pass,
        "far shorter"
    );
    assert_eq!(
        reply(
            &rewrite(input),
            r#"{"id":2,"text":"Я думаю,\nчто это правильно."}"#
        ),
        Reply::Pass,
        "a newline would not replace in place"
    );
    assert_eq!(
        reply(&rewrite(input), &format!(r#"{{"id":2,"text":"{input}"}}"#)),
        Reply::Pass,
        "unchanged is nothing to offer"
    );
}

#[test]
fn an_odd_kind_label_is_dropped_not_fatal() {
    let r = reply(
        &rewrite("a b c"),
        r#"{"id":2,"text":"A b c.","kind":"<script>"}"#,
    );
    assert_eq!(
        r,
        Reply::Rewrite {
            text: "A b c.".to_owned(),
            kind: None
        }
    );
}

#[test]
fn lines_that_cannot_be_matched_are_errors_not_replies() {
    assert_eq!(parse_reply("not json").unwrap_err(), LineError::Malformed);
    assert_eq!(
        parse_reply(r#"{"text":"no id"}"#).unwrap_err(),
        LineError::Malformed
    );
    let huge = format!(r#"{{"id":1,"text":"{}"}}"#, "x".repeat(MAX_LINE_BYTES));
    assert_eq!(parse_reply(&huge).unwrap_err(), LineError::TooLong);
    assert_eq!(
        parse_reply(r#"{"id":7,"pass":true}"#).map(|(id, _)| id),
        Ok(7)
    );
}
