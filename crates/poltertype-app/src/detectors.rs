//! Detector construction and dictionary (re)loading.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Context, Result};
use poltertype_core::layouts::LayoutDb;
use poltertype_core::settings::SettingsStore;
use poltertype_core::wordlist_profiles::{WordlistSettings, resolve_active_profile};
use poltertype_detect::{DictionaryDetector, WordPlausibilityDetector};
use poltertype_types::LayoutId;
use tracing::{info, warn};

use crate::consts::{WORDLIST_SETTLE, WORDLIST_SETTLE_ROUNDS};
use crate::types::*;

/// Build one dictionary set per configured wordlist profile, ready to
/// swap in when focus enters the matching app. No profiles → empty cache
/// and no watcher. Each profile reuses the bundled FSTs through the `Arc`
/// inside `LayoutDictionary`; only the user overlays are re-derived.
pub(crate) fn build_profile_dictionary_cache(
    layouts: &Arc<LayoutDb>,
    data_dir: &std::path::Path,
    wordlists: &WordlistSettings,
) -> HashMap<String, HashMap<LayoutId, poltertype_detect::LayoutDictionary>> {
    let mut out: HashMap<String, HashMap<LayoutId, poltertype_detect::LayoutDictionary>> =
        HashMap::new();
    for profile in &wordlists.profiles {
        let Some(dir) = poltertype_core::layouts::user_profile_wordlist_dir(&profile.id) else {
            warn!(
                profile = %profile.id,
                "no config dir resolved; profile cache entry skipped"
            );
            continue;
        };
        let dicts = layouts.build_profile_dictionaries(data_dir, &dir);
        info!(
            profile = %profile.id,
            ?dir,
            dicts = dicts.len(),
            "profile dictionaries cached"
        );
        out.insert(profile.id.clone(), dicts);
    }
    out
}

/// Poll `FocusTracker::focused_exe()` every ~250 ms and swap the
/// dictionary set when the resolved profile changes, or when
/// `force_reapply` is set.
///
/// `force_reapply` covers the case "swap on change" misses: the cache was
/// rebuilt while the user stayed on the same app, which is what happens
/// when they save wordlist edits from the Settings UI.
///
/// Transient tracker errors are swallowed — a flaky Wayland tracker is
/// not worth log spam and the next poll catches up.
pub(crate) fn spawn_profile_watcher(
    focus_tracker: Arc<dyn poltertype_input::FocusTracker>,
    settings: Arc<SettingsStore>,
    profile_cache: ProfileDictCache,
    force_reapply: Arc<AtomicBool>,
    dict_handle: poltertype_detect::DictionaryDetector,
) -> Result<()> {
    std::thread::Builder::new()
        .name("kb-profile-watcher".into())
        .spawn(move || {
            // Empty string = "no profile / global overlay active".
            let mut active: String = String::new();
            loop {
                let exe = focus_tracker.focused_exe();
                let basename = exe.as_deref().and_then(|e| {
                    std::path::Path::new(e)
                        .file_name()
                        .and_then(|f| f.to_str())
                });
                let snap = settings.snapshot();
                let resolved = resolve_active_profile(&snap.wordlists, basename)
                    .map(str::to_owned)
                    .unwrap_or_default();

                let forced = force_reapply.swap(false, Ordering::AcqRel);
                if resolved != active || forced {
                    let dicts_opt = profile_cache.read().get(&resolved).cloned();
                    if let Some(dicts) = dicts_opt {
                        info!(
                            previous = %active,
                            new_profile = if resolved.is_empty() { "<global>" } else { resolved.as_str() },
                            dicts = dicts.len(),
                            forced,
                            "wordlist profile (re-)applied"
                        );
                        dict_handle.replace_dicts(dicts);
                    } else {
                        warn!(
                            profile = %resolved,
                            "resolved profile has no cache entry; keeping current dicts"
                        );
                    }
                    active = resolved;
                }

                std::thread::sleep(Duration::from_millis(250));
            }
        })
        .context("spawn profile watcher thread")?;
    Ok(())
}

/// Build the full per-profile cache, including the global baseline under
/// the empty-string key.
///
/// That key is what the watcher swaps back to when focus leaves a
/// profiled app; without it, moving from VS Code to Chrome would keep
/// the code overlay loaded for ever.
pub(crate) fn build_full_profile_cache(
    layouts: &Arc<LayoutDb>,
    data_dir: &Path,
    wordlists: &WordlistSettings,
    user_wordlist_dir: Option<&Path>,
) -> HashMap<String, HashMap<LayoutId, poltertype_detect::LayoutDictionary>> {
    let mut cache = build_profile_dictionary_cache(layouts, data_dir, wordlists);
    if !cache.is_empty() {
        let global = layouts
            .build_profile_dictionaries(data_dir, user_wordlist_dir.unwrap_or(Path::new("")));
        cache.insert(String::new(), global);
    }
    cache
}

pub(crate) fn build_plausibility_detector(layouts: &Arc<LayoutDb>) -> WordPlausibilityDetector {
    let profiles = layouts
        .iter()
        .map(|(id, m)| (id.clone(), m.detector_profile()))
        .collect();
    WordPlausibilityDetector::new(profiles)
}

pub(crate) fn build_dictionary_detector(layouts: &Arc<LayoutDb>) -> DictionaryDetector {
    DictionaryDetector::new(collect_dicts(layouts))
}

/// Build the spelling-suggestion provider. Shares the dictionary
/// detector's hot-swappable set, so profile swaps and "Reload Settings"
/// reach suggestions instantly, and derives one keyboard geometry per
/// layout — the ranking metric's "was this a finger slip?" signal.
pub(crate) fn build_suggester(
    layouts: &LayoutDb,
    dicts: DictionaryDetector,
) -> Arc<poltertype_detect::Suggester> {
    let geometry = layouts
        .iter()
        .map(|(id, m)| {
            let pairs = m.keys.iter().flat_map(|(&sc, &(plain, shift))| {
                std::iter::once((sc, plain)).chain(shift.map(|s| (sc, s)))
            });
            (
                id.clone(),
                poltertype_detect::KeyboardGeometry::from_scancode_chars(pairs),
            )
        })
        .collect();
    Arc::new(poltertype_detect::Suggester::new(dicts, geometry))
}

pub(crate) fn collect_dicts(
    layouts: &LayoutDb,
) -> std::collections::HashMap<poltertype_types::LayoutId, poltertype_detect::LayoutDictionary> {
    layouts
        .iter()
        .filter_map(|(id, m)| m.dictionary.as_ref().map(|d| (id.clone(), d.clone())))
        .collect()
}

/// Append `word` to the user's global overlay for `layout` and insert it
/// into the running set in place. Deliberately not a full
/// [`reload_all_wordlists`], which rebuilds every loaded dictionary —
/// and which, with a profiled app focused, would re-apply that profile's
/// set and drop the word from the very app it was added in.
///
/// The word also goes into the cached global set, so a later swap back
/// to it does not hide the word until a restart. A profile's own cached
/// set holds that profile's files, not the global overlay, so it is left
/// alone. The folder is then recorded as seen, so the wordlists watcher
/// does not reload over the change it already has.
pub(crate) fn add_word_to_user_overlay(
    layout: &poltertype_types::LayoutId,
    word: &str,
    w: &WordlistReload,
) -> anyhow::Result<()> {
    use std::io::Write as _;
    // One shape on disk and in memory. `parse_wordlist` runs every line
    // back through `letters_only_lower`, so writing the raw token left
    // the file saying `just-code.net` where the dictionary held
    // `justcodenet`.
    let word = poltertype_detect::letters_only_lower(word);
    let dir = crate::user_dirs::ensure_user_wordlist_dir()?;
    let stem = layout.as_str().to_lowercase().replace('-', "_");
    let path = dir.join(format!("{stem}.txt"));
    // Under the reload lock, so a reload cannot read the folder between
    // the write and the stamp and then be skipped as already seen.
    let mut seen = w.seen.lock();
    {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        writeln!(f, "{word}")?;
    }
    let live = w.dict_handle.add_overlay_word(layout, &word);
    if let Some(global) = w.profile_dict_cache.write().get_mut("") {
        if let Some(dict) = global.get_mut(layout) {
            dict.add_overlay_entry(&word);
        }
    }
    *seen = wordlist_stamp(&dir);
    // The word itself stays out of the log, as everywhere else.
    tracing::info!(%stem, live, "added a word to the user wordlist overlay");
    Ok(())
}

/// Load the layouts `current` holds again, from disk, overlays and packs
/// included. `None` (logged) when the load fails.
fn load_fresh_layouts(current: &LayoutDb, data_dir: &Path) -> Option<LayoutDb> {
    let wordlist_dir = poltertype_core::layouts::user_wordlist_dir();
    let layout_dir = poltertype_core::layouts::user_layout_dir();
    let plugin_root = poltertype_core::plugins::user_root();
    let active: Vec<LayoutId> = current.ids().cloned().collect();
    match LayoutDb::load(poltertype_core::layouts::LoadOptions {
        data_dir: Some(data_dir),
        active_filter: Some(&active),
        user_layout_dir: layout_dir.as_deref(),
        user_wordlist_dir: wordlist_dir.as_deref(),
        user_plugin_root: plugin_root.as_deref(),
        os_keymaps: None,
    }) {
        Ok(db) => Some(db),
        Err(e) => {
            warn!(?e, "could not reload wordlists; keeping the current ones");
            None
        }
    }
}

fn swap_in_global_dicts(handle: &DictionaryDetector, db: &LayoutDb) -> usize {
    let new_dicts = collect_dicts(db);
    let n = new_dicts.len();
    handle.replace_dicts(new_dicts);
    info!(loaded = n, "user wordlist overlays reloaded");
    n
}

/// Re-read every wordlist from disk and atomically swap the engine's
/// dictionary set; returns how many loaded. The per-profile cache is
/// rebuilt from the same fresh load, and the profile watcher re-applies
/// it on its next tick.
///
/// Rebuilding the cache here is new. Only the global set used to be
/// reloaded, so the tray's "Reload Settings" did not reach a profile,
/// and while a profiled app had focus it installed the *global* set
/// until focus moved. Building the cache from the fresh load also picks
/// up pack extras and profile files that changed since startup.
///
/// Brand-new user layouts still need a restart: the engine holds a
/// snapshot `Arc<LayoutDb>`. Hotkeys do not — `config.toml` is watched
/// and the chords are re-applied from it (issue #45). `[[commands]]`
/// text triggers are read from `settings.snapshot()` on every word
/// boundary.
///
/// Limited to the layouts already loaded. Unfiltered, this loaded every
/// bundled language — each one's FST into memory and its dictionary
/// into the engine's candidate set, keyboards the user does not even
/// have (issue #76).
pub(crate) fn reload_all_wordlists(w: &WordlistReload) -> usize {
    // Held to the end: one reload at a time, and the stamp taken
    // *before* reading, so a write landing mid-reload is still seen as
    // a change by the watcher afterwards.
    let mut seen = w.seen.lock();
    if let Some(dir) = w.user_wordlist_dir.as_deref() {
        *seen = wordlist_stamp(dir);
    }
    let Some(db) = load_fresh_layouts(&w.layouts, &w.data_dir) else {
        return 0;
    };
    let n = swap_in_global_dicts(&w.dict_handle, &db);
    let db = Arc::new(db);
    let fresh_cache = build_full_profile_cache(
        &db,
        &w.data_dir,
        &w.settings.snapshot().wordlists,
        w.user_wordlist_dir.as_deref(),
    );
    let n_profiles = fresh_cache.len();
    *w.profile_dict_cache.write() = fresh_cache;
    w.profile_force_reapply.store(true, Ordering::Release);
    info!(
        profiles = n_profiles,
        "profile cache rebuilt; watcher will re-apply on next tick"
    );
    n
}

/// Stamp of the wordlists folder; see [`WordlistStamp`]. A missing
/// folder is an empty stamp, so creating it with a file in it counts as
/// a change.
pub(crate) fn wordlist_stamp(dir: &Path) -> WordlistStamp {
    fn add_txt_files(dir: &Path, out: &mut WordlistStamp) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("txt") {
                continue;
            }
            if let Ok(m) = entry.metadata() {
                if m.is_file() {
                    out.push((path, m.len(), m.modified().ok()));
                }
            }
        }
    }
    let mut out = WordlistStamp::new();
    add_txt_files(dir, &mut out);
    if let Ok(profiles) = std::fs::read_dir(dir.join("profiles")) {
        for p in profiles.flatten() {
            if p.file_type().is_ok_and(|t| t.is_dir()) {
                add_txt_files(&p.path(), &mut out);
            }
        }
    }
    out.sort();
    out
}

/// Notice wordlist files written by another program — a sync tool, a
/// dotfiles checkout, an exporter — and reload them, the way
/// `config.toml` is watched. Before, they did nothing until "Reload
/// Settings" or a restart.
///
/// Polled rather than subscribed, like the config watcher: one
/// directory listing a second is below noise, and it needs no
/// per-platform notification backend. A change is applied once the
/// folder has stopped moving for [`WORDLIST_SETTLE`], so an exporter
/// rewriting ten files costs one reload, not ten.
///
/// The app's own writes do not: "add to dictionary" applies its word in
/// place and then records the folder as seen (see
/// [`add_word_to_user_overlay`]). A full reload there would re-apply the
/// focused app's profile set, which holds that profile's files only, and
/// take the word the user just added away from the app they added it in.
pub(crate) fn spawn_wordlist_watcher(w: WordlistReload) -> Result<()> {
    let Some(dir) = w.user_wordlist_dir.clone() else {
        return Ok(());
    };
    std::thread::Builder::new()
        .name("poltertype-wordlist-watcher".into())
        .spawn(move || {
            tracing::debug!(?dir, "wordlist watcher thread started");
            loop {
                std::thread::sleep(crate::consts::CONFIG_WATCH_INTERVAL);
                let mut now = wordlist_stamp(&dir);
                if now == *w.seen.lock() {
                    continue;
                }
                // Wait for the writer to finish, within reason: a folder
                // that never settles is still reloaded.
                for _ in 0..WORDLIST_SETTLE_ROUNDS {
                    std::thread::sleep(WORDLIST_SETTLE);
                    let again = wordlist_stamp(&dir);
                    if again == now {
                        break;
                    }
                    now = again;
                }
                // Applied by the app meanwhile — its own write, or a
                // reload from the tray or the Settings window.
                if now == *w.seen.lock() {
                    continue;
                }
                info!(?dir, "wordlists changed on disk; reloading them");
                reload_all_wordlists(&w);
            }
        })
        .context("spawn wordlist watcher thread")?;
    Ok(())
}

// ─── AI plug-ins ──────────────────────────────────────────────────────

/// Detectors declared under `[[ai.plugins]]`, or nothing at all.
///
/// Three gates stand between a config file and a detector voting, and
/// all three must be open: the binary was built with `--features ai`
/// (otherwise this is the `#[cfg]`-ed-out version below),
/// `[ai].enabled = true`, and the individual entry builds — see
/// `poltertype_ai::build_detectors`, which skips a bad entry rather
/// than losing the others. Remote plug-ins carry a fourth,
/// `[ai].allow_remote`, checked per judgement so flipping it needs no
/// config edit.
///
/// The result is *appended* to the built-in detectors, never
/// substituted: the offline pipeline keeps working exactly as before if
/// the AI voice says nothing.
#[cfg(feature = "ai")]
pub(crate) fn build_ai_detectors(
    ai: &poltertype_core::settings::AiSettings,
) -> Vec<Box<dyn poltertype_detect::Detector>> {
    if !ai.enabled {
        // Silent on purpose: this is the default, and a log line about a
        // feature nobody asked for is noise.
        return Vec::new();
    }
    if ai.plugins.is_empty() {
        info!("[ai].enabled = true but no [[ai.plugins]] are configured; nothing to load");
        return Vec::new();
    }
    let built = poltertype_ai::build_detectors(&ai.plugins, ai.allow_remote);
    info!(
        configured = ai.plugins.len(),
        loaded = built.len(),
        allow_remote = ai.allow_remote,
        "AI detectors ready"
    );
    built
}

/// Without the `ai` feature the crate is not even linked, so there is
/// nothing to build. The settings still parse — a config file must not
/// stop being readable because of how the binary was compiled.
#[cfg(not(feature = "ai"))]
pub(crate) fn build_ai_detectors(
    ai: &poltertype_core::settings::AiSettings,
) -> Vec<Box<dyn poltertype_detect::Detector>> {
    if ai.enabled {
        warn!(
            "[ai].enabled = true, but this build has no AI subsystem compiled in \
             (build with `--features ai`). Continuing without it."
        );
    }
    Vec::new()
}

#[cfg(test)]
mod tests;
