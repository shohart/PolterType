//! The channel to a text provider: opened only for a plug-in the user
//! switched on, and never allowed to make the caller wait past the
//! deadline its manifest declared.
//!
//! | File | Concern |
//! |---|---|
//! | [`link`] | `ProviderLink`: the socket, its two threads, a request |
//! | [`suggester`] | the tooltip's candidates, with the providers' merged in |
//! | [`lines`] | reading one bounded line off the channel |
//! | [`consts`] | how long the service has to connect and to say hello |
//! | [`types`] | the state a link shares with its threads |

mod consts;
mod lines;
mod link;
mod suggester;
mod types;

pub use link::ProviderLink;
pub use suggester::ProviderSuggester;
pub use types::ProviderEnv;

#[cfg(test)]
mod tests;
