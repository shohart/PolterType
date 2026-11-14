//! Plug-in language packs: enumerate `<data_dir>/plugins/*/`
//! and merge each pack's layouts into the load.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use poltertype_types::LayoutId;
use tracing::{info, warn};

use super::files::{build_dictionary, wordlist_lines};
use super::helpers::id_stem;
use super::types::{DictSource, LayoutMapping, PluginManifest};

/// Enumerate `<root>/plugins/*/` for every root — the install's data
/// dir, then this user's — and merge each pack's layouts into `by_id`. Loud but graceful at every level — one broken pack never
/// takes down the rest of the load.
///
/// **This loader reads data only.** From each pack it takes
/// `manifest.toml` (required), `layout-mappings/*.toml` in the same
/// schema as bundled layouts, and `wordlists/<stem>.fst` plus an
/// optional stop list in the same shape as `<data_dir>/wordlists/`;
/// nothing here executes anything — see `docs/DATA_LAYOUT.md` and
/// [`crate::plugins`] for the two plug-in kinds.
///
/// Returns every pack's `wordlists/<stem>-extras.txt`, parsed, in pack
/// order. They are *not* merged here: a later pack may still replace a
/// layout an earlier one added words to, and the user's own TOMLs load
/// after this. [`apply_pack_extras`] merges them once everything is in.
pub fn load_plugin_packs(
    roots: &[&Path],
    user_wordlist_dir: Option<&Path>,
    by_id: &mut HashMap<LayoutId, LayoutMapping>,
    sources: &mut HashMap<LayoutId, DictSource>,
) -> Vec<(String, Vec<String>)> {
    // One directory per pack name, a later root winning: the per-user
    // install is listed after the install's own, so it replaces a copy
    // of the same pack there, as it does for extensions.
    let mut by_name: std::collections::BTreeMap<std::ffi::OsString, PathBuf> =
        std::collections::BTreeMap::new();
    for root in roots {
        let plugins_dir = root.join("plugins");
        let entries = match std::fs::read_dir(&plugins_dir) {
            Ok(it) => it,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                warn!(?plugins_dir, err = %e, "could not enumerate plugins dir");
                continue;
            }
        };
        for e in entries.flatten() {
            if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                by_name.insert(e.file_name(), e.path());
            }
        }
    }

    // Deterministic load order: two packs claiming the same layout id
    // resolve alphabetically, not by whatever the filesystem returned.
    let mut extras = Vec::new();
    for pack_dir in by_name.into_values() {
        extras.extend(load_one_pack(&pack_dir, user_wordlist_dir, by_id, sources));
    }
    extras
}

/// Load one pack's layouts into `by_id` and return its extras, as
/// [`load_plugin_packs`] describes. A pack refused for its manifest
/// contributes no extras either.
pub fn load_one_pack(
    pack_dir: &Path,
    user_wordlist_dir: Option<&Path>,
    by_id: &mut HashMap<LayoutId, LayoutMapping>,
    sources: &mut HashMap<LayoutId, DictSource>,
) -> Vec<(String, Vec<String>)> {
    let manifest_path = pack_dir.join("manifest.toml");
    let manifest_text = match std::fs::read_to_string(&manifest_path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            warn!(
                ?pack_dir,
                "plug-in directory missing manifest.toml — skipping"
            );
            return Vec::new();
        }
        Err(e) => {
            warn!(?manifest_path, err = %e, "could not read plug-in manifest");
            return Vec::new();
        }
    };
    let manifest: PluginManifest = match toml::from_str(&manifest_text) {
        Ok(m) => m,
        Err(e) => {
            warn!(?manifest_path, err = %e, "invalid plug-in manifest TOML; skipping");
            return Vec::new();
        }
    };
    info!(
        pack_id = %manifest.id,
        name = %manifest.name,
        version = %manifest.version,
        supported = ?manifest.supported_layouts,
        ?pack_dir,
        "loading plug-in pack"
    );

    let extras = read_pack_extras(pack_dir, &manifest.id);

    // A pack may do nothing but add words to layouts it does not define, so
    // `layout-mappings/` is optional — but a pack with neither is
    // almost certainly a mistake worth a line in the log.
    let layouts_dir = pack_dir.join("layout-mappings");
    let entries = match std::fs::read_dir(&layouts_dir) {
        Ok(it) => it,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if extras.is_empty() {
                warn!(
                    ?layouts_dir,
                    "plug-in pack has neither layout-mappings/ nor wordlist extras"
                );
            }
            return extras;
        }
        Err(e) => {
            warn!(?layouts_dir, err = %e, "could not enumerate plug-in layouts");
            return extras;
        }
    };

    for entry in entries.flatten() {
        let toml_path = entry.path();
        if toml_path.extension().and_then(|s| s.to_str()) != Some("toml") {
            continue;
        }
        let stem = match toml_path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s.to_owned(),
            None => continue,
        };
        let body = match std::fs::read_to_string(&toml_path) {
            Ok(s) => s,
            Err(e) => {
                warn!(?toml_path, err = %e, "could not read plug-in TOML");
                continue;
            }
        };

        // A plug-in's FST and stop list sit in its own `wordlists/`, so
        // `build_dictionary` is reused with `pack_dir` standing in for
        // `data_dir`. The user-side overlay still applies on top.
        let dictionary = build_dictionary(pack_dir, &stem, user_wordlist_dir);
        match LayoutMapping::from_parts(&body, dictionary) {
            Ok(layout) => {
                let overriding = by_id.contains_key(&layout.id);
                info!(
                    layout = %layout.id,
                    keys = layout.keys.len(),
                    dict = layout.dictionary.is_some(),
                    pack = %manifest.id,
                    overriding,
                    "loaded plug-in layout"
                );
                sources.insert(
                    layout.id.clone(),
                    DictSource {
                        dir: Some(pack_dir.to_path_buf()),
                        stem: stem.clone(),
                    },
                );
                by_id.insert(layout.id.clone(), layout);
            }
            Err(e) => {
                warn!(?toml_path, pack = %manifest.id, err = %e, "failed to parse plug-in TOML; skipping");
            }
        }
    }
    extras
}

/// Every `wordlists/<stem>-extras.txt` in one pack, as written (the
/// lines a user overlay would hold), sorted by file name so the order is the same on
/// every filesystem. Unreadable files are logged and skipped; words are
/// never logged, only how many.
fn read_pack_extras(pack_dir: &Path, pack_id: &str) -> Vec<(String, Vec<String>)> {
    let dir = pack_dir.join("wordlists");
    let entries = match std::fs::read_dir(&dir) {
        Ok(it) => it,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            warn!(?dir, pack = pack_id, err = %e, "could not enumerate plug-in wordlists");
            return Vec::new();
        }
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();

    let mut out = Vec::new();
    for path in paths {
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let Some(stem) = name.strip_suffix("-extras.txt") else {
            continue;
        };
        if stem.is_empty() {
            continue;
        }
        match std::fs::read_to_string(&path) {
            Ok(body) => {
                let words = wordlist_lines(&body);
                info!(
                    pack = pack_id,
                    stem,
                    words = words.len(),
                    "read plug-in wordlist extras"
                );
                out.push((stem.to_owned(), words));
            }
            Err(e) => {
                warn!(?path, pack = pack_id, err = %e, "could not read plug-in wordlist extras")
            }
        }
    }
    out
}

/// Merge pack extras into the overlays of the layouts they name, and
/// return what was merged per layout so profile dictionaries — which are
/// rebuilt from the data dir and never see packs — can merge it too.
///
/// A stem names a layout by the file-name convention bundled FSTs use:
/// the id lowercased, `-` → `_` (`ru_ru` → `ru-RU`). Extras for a layout
/// that is not loaded are dropped — they never create one, so the
/// active-layout filter holds. A layout with no dictionary at all is
/// left without one: it is detected by plausibility only, and a handful
/// of words is not a reason to change that.
pub fn apply_pack_extras(
    extras: Vec<(String, Vec<String>)>,
    by_id: &mut HashMap<LayoutId, LayoutMapping>,
) -> HashMap<LayoutId, Vec<String>> {
    let mut merged: HashMap<LayoutId, Vec<String>> = HashMap::new();
    if extras.is_empty() {
        return merged;
    }
    let by_stem: HashMap<String, LayoutId> = by_id
        .keys()
        .map(|id| (id_stem(id.as_str()), id.clone()))
        .collect();

    for (stem, words) in extras {
        let Some(id) = by_stem.get(&stem) else {
            info!(
                stem,
                "plug-in wordlist extras name a layout that is not loaded; ignored"
            );
            continue;
        };
        let Some(dict) = by_id.get_mut(id).and_then(|l| l.dictionary.as_mut()) else {
            info!(layout = %id, "plug-in wordlist extras for a layout without a dictionary; ignored");
            continue;
        };
        // After the user's own overlay was read, so a word both define
        // is shown the way the user wrote it.
        for line in &words {
            dict.add_overlay_entry(line);
        }
        merged.entry(id.clone()).or_default().extend(words);
    }
    merged
}
