//! The answers: predictable, so whoever is trying the contract knows
//! what should come back.

use serde_json::{Value, json};

/// The reply line for one request line, or `None` for a line that is
/// not a request this provider understands.
pub(crate) fn answer(line: &str) -> Option<String> {
    let req: Value = serde_json::from_str(line).ok()?;
    let id = req.get("id")?.as_u64()?;
    let reply = match req.get("op")?.as_str()? {
        "suggest" => {
            let word = req.get("word")?.as_str()?;
            let mut candidates = Vec::new();
            for text in [capitalised(word), word.chars().rev().collect()] {
                if text != word && !candidates.contains(&text) {
                    candidates.push(text);
                }
            }
            if candidates.is_empty() {
                json!({"id": id, "pass": true})
            } else {
                let list: Vec<Value> = candidates
                    .iter()
                    .zip([0.9, 0.5])
                    .map(|(t, s)| json!({"text": t, "score": s}))
                    .collect();
                json!({"id": id, "candidates": list})
            }
        }
        "rewrite" => {
            let text = req.get("text")?.as_str()?.trim();
            let mut out = capitalised(text);
            if !out.ends_with(['.', '!', '?']) {
                out.push('.');
            }
            json!({"id": id, "text": out, "kind": "punctuation"})
        }
        _ => json!({"id": id, "pass": true}),
    };
    Some(reply.to_string())
}

fn capitalised(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}
