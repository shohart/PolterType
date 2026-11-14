#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! A provider played in-process, over the real socket: what PolterType
//! does when it answers well, late, wrongly or not at all.

use std::io::{BufRead, BufReader, Write};
use std::sync::Arc;
use std::time::{Duration, Instant};

use poltertype_core::plugins::{ProviderCapability, TextProviderManifest};
use poltertype_core::text_provider::{Reply, Request, SOCKET_ENV, TOKEN_ENV};
use poltertype_ipc::Endpoint;

use super::ProviderLink;

fn manifest() -> TextProviderManifest {
    TextProviderManifest {
        capabilities: vec![ProviderCapability::Suggest, ProviderCapability::Rewrite],
        suggest_deadline_ms: 200,
        rewrite_deadline_ms: 400,
    }
}

fn suggest(word: &str) -> Request {
    Request::Suggest {
        layout: "en-US".to_owned(),
        word: word.to_owned(),
        max: 5,
    }
}

/// Open a link and connect a fake provider to it with `token` (the
/// right one unless overridden). `answer` gets each request and writes
/// whatever it likes back.
fn with_provider(
    token_override: Option<&str>,
    answer: impl Fn(&serde_json::Value, &mut dyn Write) + Send + 'static,
) -> Option<Arc<ProviderLink>> {
    if !Endpoint::SUPPORTED {
        return None;
    }
    let (link, env) = ProviderLink::open("fake", &manifest()).unwrap();
    let get = |k: &str| env.iter().find(|(n, _)| *n == k).unwrap().1.clone();
    let path = std::path::PathBuf::from(get(SOCKET_ENV));
    let token = token_override
        .map(str::to_owned)
        .unwrap_or_else(|| get(TOKEN_ENV).into_string().unwrap());
    std::thread::spawn(move || {
        let Ok(ch) = poltertype_ipc::connect(&path, &format!("{{\"hello\":\"{token}\"}}")) else {
            return;
        };
        let mut writer = ch.writer;
        for line in BufReader::new(ch.reader).lines() {
            let Ok(line) = line else { return };
            let v: serde_json::Value = serde_json::from_str(&line).unwrap();
            answer(&v, &mut writer);
            let _ = writer.flush();
        }
    });
    Some(link)
}

fn wait_connected(link: &ProviderLink) {
    let until = Instant::now() + Duration::from_secs(5);
    while !link.is_connected() {
        assert!(Instant::now() < until, "provider never connected");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_timely_answer_is_checked_and_returned() {
    let Some(link) = with_provider(None, |req, w| {
        assert_eq!(req["op"], "suggest");
        assert_eq!(req["word"], "githib");
        writeln!(
            w,
            r#"{{"id":{},"candidates":[{{"text":"GitHub","score":0.9}}]}}"#,
            req["id"]
        )
        .unwrap();
    }) else {
        return;
    };
    wait_connected(&link);
    let Reply::Candidates(c) = link.request(&suggest("githib")) else {
        panic!("expected candidates")
    };
    assert_eq!(c[0].text, "GitHub");
}

/// A late answer is dropped, and does not get mistaken for the answer
/// to the next request.
#[test]
fn a_late_answer_is_no_opinion_and_does_not_leak_into_the_next() {
    let Some(link) = with_provider(None, |req, w| {
        if req["word"] == "slow" {
            std::thread::sleep(Duration::from_millis(400));
            writeln!(
                w,
                r#"{{"id":{},"candidates":[{{"text":"late"}}]}}"#,
                req["id"]
            )
            .unwrap();
        } else {
            writeln!(
                w,
                r#"{{"id":{},"candidates":[{{"text":"fast"}}]}}"#,
                req["id"]
            )
            .unwrap();
        }
    }) else {
        return;
    };
    wait_connected(&link);
    let started = Instant::now();
    assert_eq!(link.request(&suggest("slow")), Reply::Pass);
    assert!(
        started.elapsed() < Duration::from_millis(350),
        "the deadline, not the provider, decides the wait"
    );
    std::thread::sleep(Duration::from_millis(300));
    let Reply::Candidates(c) = link.request(&suggest("quick")) else {
        panic!("expected the second request's own answer")
    };
    assert_eq!(c[0].text, "fast");
}

#[test]
fn a_provider_with_the_wrong_token_is_never_asked() {
    let Some(link) = with_provider(Some("not-the-token"), |_, _| {
        panic!("an unauthenticated client must never see a request")
    }) else {
        return;
    };
    std::thread::sleep(Duration::from_millis(200));
    assert!(!link.is_connected());
    assert_eq!(link.request(&suggest("teh")), Reply::Pass);
}

#[test]
fn garbage_is_ignored_and_the_real_reply_still_counts() {
    let Some(link) = with_provider(None, |req, w| {
        writeln!(w, "not json at all").unwrap();
        writeln!(w, r#"{{"id":{},"text":"I am chatting"}}"#, req["id"]).unwrap();
    }) else {
        return;
    };
    wait_connected(&link);
    // Wrong shape for a suggest: checked into no opinion.
    assert_eq!(link.request(&suggest("teh")), Reply::Pass);
    assert!(link.is_connected(), "garbage does not cost the channel");
}

#[test]
fn an_overlong_line_closes_the_channel() {
    let Some(link) = with_provider(None, |_, w| {
        let big = "x".repeat(poltertype_core::text_provider::MAX_LINE_BYTES + 10);
        let _ = writeln!(w, "{big}");
    }) else {
        return;
    };
    wait_connected(&link);
    assert_eq!(link.request(&suggest("teh")), Reply::Pass);
    let until = Instant::now() + Duration::from_secs(2);
    while link.is_connected() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!link.is_connected());
}

/// Withdrawing consent is dropping the link: the provider sees its
/// channel end, and nothing more is sent.
#[test]
fn closing_ends_the_providers_channel() {
    let (tx, rx) = crossbeam_channel::bounded::<()>(1);
    let Some(link) = with_provider(None, move |req, w| {
        writeln!(w, r#"{{"id":{},"pass":true}}"#, req["id"]).unwrap();
        let _ = tx.try_send(());
    }) else {
        return;
    };
    wait_connected(&link);
    assert_eq!(link.request(&suggest("teh")), Reply::Pass);
    rx.recv_timeout(Duration::from_secs(1)).unwrap();
    link.close();
    assert!(!link.is_connected());
    assert_eq!(link.request(&suggest("teh")), Reply::Pass);
}

#[test]
fn a_rewrite_waits_for_its_own_deadline() {
    let Some(link) = with_provider(None, |req, w| {
        std::thread::sleep(Duration::from_millis(250));
        writeln!(
            w,
            r#"{{"id":{},"text":"I think, that is right.","kind":"punctuation"}}"#,
            req["id"]
        )
        .unwrap();
    }) else {
        return;
    };
    wait_connected(&link);
    let r = link.request(&Request::Rewrite {
        layout: "en-US".to_owned(),
        text: "i think that is right".to_owned(),
    });
    assert!(matches!(r, Reply::Rewrite { .. }), "{r:?}");
}
