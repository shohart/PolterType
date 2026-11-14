//! A private local channel between PolterType and one plug-in process —
//! the transport under text providers (`docs/TEXT_PROVIDERS.md`).
//!
//! PolterType is always the listener, so no other process can pose as
//! it by binding the name first; and the first line a client sends must
//! pass the caller's check (a token in the plug-in's environment), so a
//! process that merely found the socket cannot pose as the plug-in by
//! connecting first. One connection is accepted, then the listener goes
//! away. A process running as the same user can read that environment;
//! it is inside the trust boundary, as every extension is.
//!
//! **Unix:** a socket in a freshly created directory of mode `0700`
//! under `$XDG_RUNTIME_DIR` (or the temp dir), removed again on drop.
//! **Windows:** not yet — [`Endpoint::bind`] says
//! [`std::io::ErrorKind::Unsupported`]. A named pipe needs an owner-only
//! ACL, and getting that wrong would hand typed words to other users of
//! the machine, so it comes separately, where it can be tested.

#![forbid(unsafe_code)]

mod types;

pub use types::*;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use unix::*;

#[cfg(not(unix))]
mod unsupported;
#[cfg(not(unix))]
pub use unsupported::*;
