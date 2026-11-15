//! `Supervisor` — every plug-in process this app started.

use super::types::Running;

/// Owns every plug-in process this app started.
#[derive(Default)]
pub struct Supervisor {
    pub(super) running: Vec<Running>,
    /// The open provider channels, kept in step with `running` for the
    /// parts of the app that ask them.
    pub(super) registry: crate::plugins::ProviderRegistry,
}

impl Supervisor {
    pub fn new() -> Self {
        Self::default()
    }

    /// The same, publishing its provider channels into `registry`.
    pub fn with_registry(registry: crate::plugins::ProviderRegistry) -> Self {
        Self {
            running: Vec::new(),
            registry,
        }
    }

    /// Make the registry match the channels still held by running
    /// services: called after anything that adds, closes or reaps one.
    pub(super) fn publish_providers(&self) {
        *self.registry.write() = self
            .running
            .iter()
            .filter_map(|r| r.provider.clone())
            .collect();
    }
}
