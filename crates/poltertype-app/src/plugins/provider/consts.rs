//! How long a provider's service has to come up.

use std::time::Duration;

/// From spawning the service to its authenticated hello. A provider that
/// loads a model before it connects will want much of this; one that
/// has not connected by then is given up on until the next start.
pub(super) const CONNECT_WITHIN: Duration = Duration::from_secs(60);

/// From a connection to its hello. A real provider sends it at once.
pub(super) const HELLO_WITHIN: Duration = Duration::from_secs(2);

/// Requests waiting to be written. The caller never blocks on a full
/// queue: a provider this far behind gets no more until it catches up.
pub(super) const REQUEST_QUEUE: usize = 4;

/// Replies read but not yet claimed. A reply to a request already given
/// up on is dropped on the next read, so this only has to absorb a burst.
pub(super) const REPLY_QUEUE: usize = 32;
