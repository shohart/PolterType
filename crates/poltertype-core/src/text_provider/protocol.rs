//! Encoding requests, and deciding whether a reply may be shown.

use super::consts::{
    MAX_CANDIDATE_CHARS, MAX_CANDIDATES, MAX_KIND_CHARS, MAX_LINE_BYTES, REWRITE_LENGTH_RATIO,
    REWRITE_LENGTH_SLACK,
};
use super::enums::{LineError, Reply, Request};
use super::types::{Candidate, Hello, RawReply, WireRequest};

/// `request` as one JSON line, newline included.
pub fn encode_request(id: u64, request: &Request) -> String {
    let wire = match request {
        Request::Suggest { layout, word, max } => WireRequest {
            id,
            op: "suggest",
            layout,
            word: Some(word),
            max: Some(*max),
            text: None,
        },
        Request::Rewrite { layout, text } => WireRequest {
            id,
            op: "rewrite",
            layout,
            word: None,
            max: None,
            text: Some(text),
        },
    };
    // Serialising a struct of strings and integers cannot fail; an
    // empty line, which every provider must ignore, is the fallback.
    let mut line = serde_json::to_string(&wire).unwrap_or_default();
    line.push('\n');
    line
}

/// Whether `line` is the hello carrying `token`. Compared over the
/// whole length whatever the first difference, so how long a guess
/// survived tells a guesser nothing.
pub fn is_hello(line: &str, token: &str) -> bool {
    let Ok(hello) = serde_json::from_str::<Hello>(line.trim_end()) else {
        return false;
    };
    let (a, b) = (hello.hello.as_bytes(), token.as_bytes());
    if a.len() != b.len() || b.is_empty() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Parse one line from a provider. The id is returned separately so a
/// reply to a request that was already given up on can be dropped
/// without looking further.
pub fn parse_reply(line: &str) -> Result<(u64, RawReply), LineError> {
    if line.len() > MAX_LINE_BYTES {
        return Err(LineError::TooLong);
    }
    let raw: RawReply = serde_json::from_str(line.trim_end()).map_err(|_| LineError::Malformed)?;
    let id = raw.id.ok_or(LineError::Malformed)?;
    Ok((id, raw))
}

/// Decide what `raw`, a reply to `request`, may show. Anything that
/// fails a check is [`Reply::Pass`]: a provider gets no say in how its
/// mistakes are repaired.
pub fn check_reply(request: &Request, raw: RawReply) -> Reply {
    if raw.pass {
        return Reply::Pass;
    }
    match request {
        Request::Suggest { word, max, .. } => match raw.candidates {
            Some(c) if raw.text.is_none() => check_candidates(word, c, *max),
            _ => Reply::Pass,
        },
        Request::Rewrite { text, .. } => match raw.text {
            Some(out) if raw.candidates.is_none() => check_rewrite(text, out, raw.kind),
            _ => Reply::Pass,
        },
    }
}

/// Candidates that are one plausible token each, not the typed word,
/// at most `max` (and [`MAX_CANDIDATES`]), best first.
fn check_candidates(typed: &str, candidates: Vec<Candidate>, max: usize) -> Reply {
    let mut out: Vec<Candidate> = Vec::new();
    for mut c in candidates {
        if c.text.is_empty()
            || c.text.chars().count() > MAX_CANDIDATE_CHARS
            || c.text
                .chars()
                .any(|ch| ch.is_control() || ch.is_whitespace())
        {
            // One bad candidate spoils the reply: a provider emitting
            // control characters is not one whose other answers to
            // trust.
            return Reply::Pass;
        }
        if c.text == typed || out.iter().any(|o| o.text == c.text) {
            continue;
        }
        c.score = if c.score.is_finite() {
            c.score.clamp(0.0, 1.0)
        } else {
            0.0
        };
        out.push(c);
    }
    out.sort_by(|a, b| b.score.total_cmp(&a.score));
    out.truncate(max.min(MAX_CANDIDATES));
    if out.is_empty() {
        Reply::Pass
    } else {
        Reply::Candidates(out)
    }
}

/// A rewrite that is still the same sentence: no control characters
/// (a newline included — a sentence is replaced in place), not empty,
/// not the input unchanged, and not wildly longer or shorter.
fn check_rewrite(input: &str, output: String, kind: Option<String>) -> Reply {
    if output.trim().is_empty() || output == input || output.chars().any(char::is_control) {
        return Reply::Pass;
    }
    let (n_in, n_out) = (input.chars().count(), output.chars().count());
    if n_out > n_in * REWRITE_LENGTH_RATIO + REWRITE_LENGTH_SLACK
        || n_in > n_out * REWRITE_LENGTH_RATIO + REWRITE_LENGTH_SLACK
    {
        return Reply::Pass;
    }
    let kind = kind.filter(|k| {
        !k.is_empty()
            && k.chars().count() <= MAX_KIND_CHARS
            && k.chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    });
    Reply::Rewrite { text: output, kind }
}
