//! Platforms with no transport yet. Everything says so.

use std::path::Path;
use std::time::{Duration, Instant};

use crate::types::Channel;

/// Never constructed: [`Self::bind`] always fails.
pub struct Endpoint {
    never: std::convert::Infallible,
}

impl Endpoint {
    /// Whether this platform has a transport at all.
    pub const SUPPORTED: bool = false;

    pub fn bind() -> std::io::Result<Self> {
        Err(unsupported())
    }

    pub fn path(&self) -> &Path {
        match self.never {}
    }

    pub fn accept(
        &self,
        _deadline: Instant,
        _hello_timeout: Duration,
        _check: &dyn Fn(&str) -> bool,
        _stop: &dyn Fn() -> bool,
    ) -> std::io::Result<Channel> {
        match self.never {}
    }
}

pub fn connect(_path: &Path, _hello: &str) -> std::io::Result<Channel> {
    Err(unsupported())
}

pub fn random_token() -> std::io::Result<String> {
    Err(unsupported())
}

fn unsupported() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "text providers have no transport on this platform yet",
    )
}
