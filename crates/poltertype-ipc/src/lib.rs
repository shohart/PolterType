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
//! **Windows:** a named pipe with a random name, created as the first
//! and only instance of that name, refusing remote clients, and with a
//! DACL that admits this user's SID and nobody else. All I/O on it is
//! overlapped, so a read waiting for the provider never holds a write up.
//! **Anywhere else:** [`Endpoint::bind`] says
//! [`std::io::ErrorKind::Unsupported`].

// Denied rather than forbidden: the Windows module is all Win32 calls,
// and allows it there alone.
#![deny(unsafe_code)]

mod types;

pub use types::*;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use unix::*;

#[cfg(windows)]
mod windows_impl;
#[cfg(windows)]
pub use windows_impl::*;

#[cfg(not(any(unix, windows)))]
mod unsupported;
#[cfg(not(any(unix, windows)))]
pub use unsupported::*;
