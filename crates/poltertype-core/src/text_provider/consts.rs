//! Names and bounds of the text-provider channel.

/// Where the provider's service finds the channel: a path PolterType
/// listens on. Set only for a provider the user switched on.
pub const SOCKET_ENV: &str = "POLTERTYPE_PROVIDER_SOCKET";

/// The secret the provider's first line must carry. PolterType being
/// the listener stops another process posing as PolterType; this stops
/// a process that merely found the socket from connecting first and
/// posing as the provider. It is in the service's environment, which a
/// process of the same user can read — that one is inside the trust
/// boundary already.
pub const TOKEN_ENV: &str = "POLTERTYPE_PROVIDER_TOKEN";

/// Longest line either side may send, in bytes. A reply past it closes
/// the channel: a provider that has started streaming a novel is not
/// going to say anything useful in the next one either.
pub const MAX_LINE_BYTES: usize = 64 * 1024;

/// Most candidates one `suggest` reply may carry — one per F1–F9 key.
pub const MAX_CANDIDATES: usize = 9;

/// Longest candidate, in characters. A suggestion replaces one word.
pub const MAX_CANDIDATE_CHARS: usize = 64;

/// Longest `kind` label on a rewrite, in characters.
pub const MAX_KIND_CHARS: usize = 32;

/// A rewrite may differ in length from its input by this many
/// characters plus [`REWRITE_LENGTH_RATIO`] times the input. Past that
/// the model has started chatting instead of correcting.
pub const REWRITE_LENGTH_SLACK: usize = 16;

/// See [`REWRITE_LENGTH_SLACK`].
pub const REWRITE_LENGTH_RATIO: usize = 2;
