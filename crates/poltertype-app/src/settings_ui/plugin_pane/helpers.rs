//! Free functions: parsing a command's output into rows, and loading
//! every plug-in that declares a pane.

use std::path::Path;

use poltertype_core::plugins::DiscoveredExtension;

use super::pane::PluginPane;
use super::types::ListRow;

/// Parse a list command's output into rows.
///
/// Tab-separated and tolerant in the same way the state protocol is —
/// a line with no tab is an id that is its own label, extra fields are
/// ignored, blank lines skipped. A plug-in should be able to print
/// something readable without it becoming a parsing contract.
pub(super) fn parse_list_rows(text: &str) -> Vec<ListRow> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let mut fields = line.split('\t');
            let id = fields.next().unwrap_or_default().trim().to_owned();
            let label = fields.next().unwrap_or_default().trim();
            let detail = fields.next().unwrap_or_default().trim();
            ListRow {
                label: if label.is_empty() {
                    id.clone()
                } else {
                    label.to_owned()
                },
                id,
                detail: detail.to_owned(),
            }
        })
        .filter(|row| !row.id.is_empty())
        .collect()
}

/// Load every discovered extension that actually declares a pane.
///
/// A plug-in with no controls gets no section: an empty box with a
/// name in it tells the user nothing and makes the list longer. A text
/// provider is the exception, for its consent switch.
pub fn load_all(extensions: Vec<DiscoveredExtension>, config_root: &Path) -> Vec<PluginPane> {
    // A pane writes `<config_root>/<id>/…`, and PolterType's own config
    // directory is one of those: a plug-in named after it — `poltertype`
    // on Linux, `config` on Windows — would edit PolterType's settings,
    // its own provider consent among them.
    let own_dir = poltertype_core::settings::SettingsStore::default_path()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf));
    extensions
        .into_iter()
        .filter(|e| {
            let collides = own_dir.as_deref() == Some(config_root.join(&e.id).as_path());
            if collides {
                tracing::warn!(
                    id = %e.id,
                    "plug-in's settings would live in PolterType's own config directory; not shown"
                );
            }
            !collides
        })
        // A provider is shown even with no pane of its own: the switch
        // that lets it see typed words is drawn on its card.
        .filter(|e| !e.manifest.pane.is_empty() || e.manifest.text_provider.is_some())
        .map(|mut e| {
            // Every word this pane draws that PolterType did not write
            // comes out of the manifest, so this is the one place a
            // plug-in's own translation has to be applied.
            poltertype_core::plugins::localise(&mut e.manifest, &e.id);
            PluginPane::load(e, config_root)
        })
        .collect()
}
