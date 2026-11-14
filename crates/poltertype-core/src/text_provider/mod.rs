//! The text-provider wire contract: what PolterType sends an extension
//! that offered to help with spelling or punctuation, and what it
//! accepts back.
//!
//! Pure data and checks — no sockets, no threads; the transport lives
//! in `poltertype-ipc` and the plumbing in the app. What is pinned here
//! is the part a provider can get wrong: every reply is checked before
//! PolterType would show it, and a reply that fails a check is treated
//! as no opinion, never repaired. See `docs/TEXT_PROVIDERS.md`.

mod consts;
mod enums;
mod protocol;
mod types;

pub use consts::*;
pub use enums::*;
pub use protocol::*;
pub use types::*;

#[cfg(test)]
mod tests;
