//! State a link shares with its threads.

use std::sync::atomic::AtomicBool;

use parking_lot::Mutex;

/// What the link's threads and the link itself both touch.
pub(super) struct Shared {
    pub(super) connected: AtomicBool,
    pub(super) closed: AtomicBool,
    /// Shuts the channel down; taken by whoever closes first.
    pub(super) close: Mutex<Option<Box<dyn Fn() + Send + Sync>>>,
    /// A malformed reply is reported once per session, not per line.
    pub(super) reported_malformed: AtomicBool,
}

/// Environment a provider's service is started with: the channel's path
/// and the token its hello must carry.
pub type ProviderEnv = Vec<(&'static str, std::ffi::OsString)>;
