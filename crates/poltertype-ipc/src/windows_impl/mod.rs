//! The Windows transport: a named pipe only this user can open.
//!
//! The one module of this crate that needs `unsafe`: every call below is
//! a Win32 function. Each block states what it relies on; the safe API
//! the rest of the crate sees is the same as on Unix.

#![allow(unsafe_code)]

mod endpoint;
mod io;
mod pipe;
mod random;
mod security;
mod stream;

pub use endpoint::{Endpoint, connect};
pub use random::random_token;

#[cfg(test)]
mod tests;
