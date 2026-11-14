//! Deciding whether a plug-in's service gets a text-provider channel.

use std::sync::Arc;

use poltertype_core::plugins::DiscoveredExtension;
use poltertype_core::settings::TextProviderSettings;
use tracing::{info, warn};

use crate::plugins::{ProviderEnv, ProviderLink};

/// The channel for `ext` and the environment naming it — or nothing,
/// when `ext` is not a provider, the user has not switched it on, or
/// this platform has no transport. Failing to open one is logged and
/// the service starts without it: the plug-in's pane and tray entries do
/// not depend on it.
pub(super) fn open_provider(
    ext: &DiscoveredExtension,
    consent: &TextProviderSettings,
) -> (Option<Arc<ProviderLink>>, ProviderEnv) {
    let Some(manifest) = &ext.manifest.text_provider else {
        return (None, Vec::new());
    };
    if !consent.allows(&ext.id) {
        info!(
            id = %ext.id,
            "text provider not switched on in Settings; starting it without a channel"
        );
        return (None, Vec::new());
    }
    match ProviderLink::open(&ext.id, manifest) {
        Ok((link, env)) => (Some(link), env),
        Err(e) => {
            warn!(id = %ext.id, "could not open a text-provider channel: {e}");
            (None, Vec::new())
        }
    }
}
