//! What a request asks and what a checked reply says.

use super::types::Candidate;

/// A request PolterType makes of a provider.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    /// Candidates for one word the dictionary does not know.
    Suggest {
        layout: String,
        word: String,
        max: usize,
    },
    /// A rewrite of one sentence.
    Rewrite { layout: String, text: String },
}

/// A reply that passed every check, ready to be shown.
#[derive(Debug, Clone, PartialEq)]
pub enum Reply {
    /// For a `suggest`: candidates, best first, deduplicated.
    Candidates(Vec<Candidate>),
    /// For a `rewrite`: the sentence as the provider would write it.
    Rewrite { text: String, kind: Option<String> },
    /// No opinion — said outright, or what any failed check becomes.
    Pass,
}

/// Why a line from a provider could not even be matched to a request.
/// Logged once per session, never with the line itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineError {
    /// Not JSON, or JSON of the wrong shape.
    Malformed,
    /// Longer than [`super::MAX_LINE_BYTES`].
    TooLong,
}
