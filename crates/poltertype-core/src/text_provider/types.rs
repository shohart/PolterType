//! Wire shapes, as serialised.

use serde::{Deserialize, Serialize};

/// One spelling candidate. `score` is the provider's confidence in
/// `0.0..=1.0`; PolterType clamps it and only ever uses it to order the
/// provider's own candidates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    pub text: String,
    #[serde(default)]
    pub score: f32,
}

/// A provider's line as read, before any check. Every field but `id` is
/// optional on the wire, so a reply's shape is decided by the checks,
/// not by the parser.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct RawReply {
    pub id: Option<u64>,
    pub candidates: Option<Vec<Candidate>>,
    pub text: Option<String>,
    pub kind: Option<String>,
    pub pass: bool,
}

/// A request on the wire: `{"id": 41, "op": "suggest", …}`.
#[derive(Debug, Clone, Serialize)]
pub struct WireRequest<'a> {
    pub id: u64,
    pub op: &'a str,
    pub layout: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub word: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<&'a str>,
}

/// The provider's first line: `{"hello": "<token>"}`.
#[derive(Debug, Clone, Deserialize)]
pub struct Hello {
    pub hello: String,
}
