//! Where a plug-in's own config file is: the one rule for both the
//! settings pane that edits it and the plug-in that reads it.

use std::path::{Path, PathBuf};

/// File a pane writes when the manifest names none.
const DEFAULT_CONFIG_FILE: &str = "config.toml";

/// `<config_root>/<id>/<config_file>`, where `config_root` holds the
/// per-application config directories — the parent of PolterType's own.
/// A plug-in is a separate program, so its config sits beside
/// PolterType's, not inside it.
pub fn plugin_config_path_in(config_root: &Path, id: &str, config_file: &str) -> PathBuf {
    config_root.join(id).join(if config_file.is_empty() {
        DEFAULT_CONFIG_FILE
    } else {
        config_file
    })
}

/// [`plugin_config_path_in`] for this user's real config root. `None`
/// where the OS names no config directory.
pub fn plugin_config_path(id: &str, config_file: &str) -> Option<PathBuf> {
    let own = crate::settings::SettingsStore::default_path().ok()?;
    let root = own.parent()?.parent()?;
    Some(plugin_config_path_in(root, id, config_file))
}
