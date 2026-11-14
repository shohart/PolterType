//! The Unix transport: a socket in a private directory.

mod endpoint;
mod random;

pub use endpoint::{Endpoint, connect};
pub use random::random_token;

#[cfg(test)]
mod tests;
