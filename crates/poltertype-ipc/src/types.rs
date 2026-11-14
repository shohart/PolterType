//! What an accepted connection hands back.

use std::io::{Read, Write};

/// One accepted, checked connection, split so a reader and a writer can
/// each have a thread. `close` shuts both directions down, which also
/// wakes a reader blocked on the other half.
pub struct Channel {
    pub reader: Box<dyn Read + Send>,
    pub writer: Box<dyn Write + Send>,
    pub close: Box<dyn Fn() + Send + Sync>,
}
