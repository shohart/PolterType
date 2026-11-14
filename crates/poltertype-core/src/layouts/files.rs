//! On-disk reading: wordlist files, dictionary assembly, user
//! override directories.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use fst::Set as FstSet;
use poltertype_detect::LayoutDictionary;
use tracing::{info, warn};

/// Parse a one-word-per-line file into a `HashSet` normalised exactly
/// as a typed token is by the dictionary detector — non-letters
/// stripped, lowercased. Stored verbatim, an entry like `v-strel-zbook`
/// or `ім'я` would never match the lookup key.
///
/// Blank lines, `#` comments and lines with no alphabetic characters
/// are skipped; the last would normalise to the empty string.
pub fn parse_wordlist(input: &str) -> HashSet<String> {
    input
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(poltertype_detect::letters_only_lower)
        .filter(|w| !w.is_empty())
        .collect()
}

/// The lines of a wordlist as written: trimmed, without blanks and `#`
/// comments. For overlays, whose written form is kept for suggestions
/// ([`LayoutDictionary::add_overlay_entry`]); membership-only lists use
/// [`parse_wordlist`].
pub fn wordlist_lines(input: &str) -> Vec<String> {
    input
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

/// Add every line of `<dir>/<stem><suffix>.txt` to `dict`'s overlay as
/// written. `true` if the file was there.
pub fn merge_overlay_file(
    dict: &mut LayoutDictionary,
    overlay_dir: Option<&Path>,
    stem: &str,
    suffix: &str,
) -> bool {
    let Some(lines) = load_overlay_lines(overlay_dir, stem, suffix) else {
        return false;
    };
    for line in &lines {
        dict.add_overlay_entry(line);
    }
    true
}

/// Construct a [`LayoutDictionary`] for `stem` from the bundled FST and
/// stop-word file in `<data_dir>/wordlists/`.
///
/// `overlay_dir` can extend either layer at runtime:
///
/// | User file | Merged into | Use case |
/// |---|---|---|
/// | `<stem>.txt` | `user_overlay` | runtime additions |
/// | `<stem>-extras.txt` | `user_overlay` | same, separate file for organisation |
/// | `<stem>-stop.txt` | `short_stop_words` | extend the ≤2-letter list |
/// | `<stem>-weak.txt` | `weak` | Hunspell-valid but rare, so a strong cross-layout hit wins |
///
/// Missing files are silently fine; read errors are logged and the
/// bundled data keeps working.
pub fn build_dictionary(
    data_dir: &Path,
    stem: &str,
    overlay_dir: Option<&Path>,
) -> Option<LayoutDictionary> {
    let fst_path = data_dir.join("wordlists").join(format!("{stem}.fst"));
    let leaked = match super::fst_cache::leaked_bytes(&fst_path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            warn!(
                ?fst_path,
                stem, "FST not found; skipping dict for this layout"
            );
            return None;
        }
        Err(e) => {
            tracing::error!(?fst_path, stem, err = %e, "FST read failed; skipping dict");
            return None;
        }
    };
    let bundled_fst = match FstSet::new(leaked) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(stem, err = %e, "bundled FST is malformed; skipping dict for this layout");
            return None;
        }
    };

    let mut short_stop_words = read_stop_words(data_dir, stem);
    if let Some(extra) = load_overlay_file(overlay_dir, stem, "-stop") {
        short_stop_words.extend(extra);
    }

    let mut weak = read_weak_words(data_dir, stem);
    if let Some(extra) = load_overlay_file(overlay_dir, stem, "-weak") {
        weak.extend(extra);
    }

    let mut dict = LayoutDictionary::new(bundled_fst, HashSet::new(), short_stop_words, weak);
    // `<stem>.txt` before `-extras.txt`: the first written form of a
    // word is the one suggestions show.
    merge_overlay_file(&mut dict, overlay_dir, stem, "");
    merge_overlay_file(&mut dict, overlay_dir, stem, "-extras");

    // Surface-form FST (suggestions corpus), shared like the membership
    // one. Missing file is fine: older data dirs predate suggestions and
    // the feature degrades to overlay-only candidates.
    let surface_path = data_dir
        .join("wordlists")
        .join(format!("{stem}-surface.fst"));
    match super::fst_cache::leaked_bytes(&surface_path) {
        Ok(leaked) => match FstSet::new(leaked) {
            Ok(s) => dict = dict.with_surface(s),
            Err(e) => {
                tracing::error!(stem, err = %e, "surface FST is malformed; suggestions degraded for this layout");
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            info!(stem, "no surface FST; suggestions degraded for this layout");
        }
        Err(e) => {
            warn!(?surface_path, err = %e, "surface FST read failed; suggestions degraded");
        }
    }

    Some(dict)
}

/// Read `<data_dir>/wordlists/<stem>-stop.txt` if it exists. Missing
/// → empty set (treated identically by `LayoutDictionary`).
pub fn read_stop_words(data_dir: &Path, stem: &str) -> HashSet<String> {
    let path = data_dir.join("wordlists").join(format!("{stem}-stop.txt"));
    match std::fs::read_to_string(&path) {
        Ok(s) => parse_wordlist(&s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => HashSet::new(),
        Err(e) => {
            warn!(?path, err = %e, "could not read stop-words file");
            HashSet::new()
        }
    }
}

/// Read `<data_dir>/wordlists/<stem>-weak.txt` if it exists. Same
/// shape as the stop / extras readers — one canonicalised word per
/// line, missing file is silently fine. See [`LayoutDictionary::weak`]
/// for the semantics.
pub fn read_weak_words(data_dir: &Path, stem: &str) -> HashSet<String> {
    let path = data_dir.join("wordlists").join(format!("{stem}-weak.txt"));
    match std::fs::read_to_string(&path) {
        Ok(s) => parse_wordlist(&s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => HashSet::new(),
        Err(e) => {
            warn!(?path, err = %e, "could not read weak-words file");
            HashSet::new()
        }
    }
}

/// User-layout dictionary builder: like [`build_dictionary`] but
/// without a bundled FST (user layouts can't ship binary blobs). The
/// dictionary is purely the overlay files in `overlay_dir`, read under
/// each of `stems` in turn — so the first stem's written form of a word
/// wins.
///
/// More than one stem because a user layout has two names: its TOML's
/// file name, and the one derived from its id, which is what the
/// Settings editor and "add to dictionary" write to. Reading only the
/// first lost every word added from the GUI to a layout whose file is
/// named otherwise.
pub fn build_user_dictionary(
    stems: &[&str],
    overlay_dir: Option<&Path>,
) -> Option<LayoutDictionary> {
    let mut dict =
        LayoutDictionary::from_overlay_only(HashSet::new(), HashSet::new(), HashSet::new());
    let mut any = false;
    let mut seen: Vec<&str> = Vec::new();
    for &stem in stems {
        if seen.contains(&stem) {
            continue;
        }
        seen.push(stem);
        any |= merge_overlay_file(&mut dict, overlay_dir, stem, "");
        any |= merge_overlay_file(&mut dict, overlay_dir, stem, "-extras");
        if let Some(extra) = load_overlay_file(overlay_dir, stem, "-stop") {
            dict.short_stop_words.extend(extra);
            any = true;
        }
        if let Some(extra) = load_overlay_file(overlay_dir, stem, "-weak") {
            dict.weak.extend(extra);
            any = true;
        }
    }
    any.then_some(dict)
}

/// Read `<dir>/<stem><suffix>.txt` if present and parse it. Returns
/// `None` for both "no overlay dir configured" and "file does not
/// exist" — the caller treats both as "no user additions". Other I/O
/// errors are logged and treated the same way so a transient read
/// problem doesn't take the dict offline.
pub fn load_overlay_file(
    overlay_dir: Option<&Path>,
    stem: &str,
    suffix: &str,
) -> Option<HashSet<String>> {
    let dir = overlay_dir?;
    let path = dir.join(format!("{stem}{suffix}.txt"));
    match std::fs::read_to_string(&path) {
        Ok(s) => {
            let parsed = parse_wordlist(&s);
            info!(
                ?path,
                lines = s.lines().count(),
                words = parsed.len(),
                "merged user wordlist override"
            );
            Some(parsed)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            warn!(?path, err = %e, "could not read user wordlist override");
            None
        }
    }
}

/// Like [`load_overlay_file`] but the lines as written, for the overlay
/// proper, whose written forms suggestions show.
pub fn load_overlay_lines(
    overlay_dir: Option<&Path>,
    stem: &str,
    suffix: &str,
) -> Option<Vec<String>> {
    let dir = overlay_dir?;
    let path = dir.join(format!("{stem}{suffix}.txt"));
    match std::fs::read_to_string(&path) {
        Ok(s) => {
            let lines = wordlist_lines(&s);
            info!(?path, words = lines.len(), "merged user wordlist override");
            Some(lines)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            warn!(?path, err = %e, "could not read user wordlist override");
            None
        }
    }
}

/// Path under which user-supplied wordlist overrides live:
/// `<config-dir>/poltertype/wordlists/`. Optional files per layout are
/// listed in [`build_dictionary`].
pub fn user_wordlist_dir() -> Option<PathBuf> {
    crate::settings::SettingsStore::project_dirs()
        .ok()
        .map(|dirs| dirs.config_dir().join("wordlists"))
}

/// Path to the per-profile wordlist directory:
/// `<config-dir>/poltertype/wordlists/profiles/<profile-id>/`.
/// The id is taken verbatim — the caller is responsible for
/// rejecting path-unsafe ids before letting them reach this
/// function (see `wordlist_profiles::WordlistProfile::id` docs
/// for the expected shape).
pub fn user_profile_wordlist_dir(profile_id: &str) -> Option<PathBuf> {
    user_wordlist_dir().map(|d| d.join("profiles").join(profile_id))
}

/// Path under which user-supplied **layout mapping** TOML files live:
/// `<config-dir>/poltertype/layouts/`. Any `*.toml` here is loaded
/// alongside the bundled layouts.
pub fn user_layout_dir() -> Option<PathBuf> {
    crate::settings::SettingsStore::project_dirs()
        .ok()
        .map(|dirs| dirs.config_dir().join("layouts"))
}

/// Read every `*.toml` file in `dir` and return parsed
/// `(file_stem, body)` pairs sorted by file name.
pub fn read_user_layout_files(dir: &Path) -> Vec<(String, String)> {
    let entries = match std::fs::read_dir(dir) {
        Ok(it) => it,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            warn!(?dir, err = %e, "could not enumerate user layouts dir");
            return Vec::new();
        }
    };
    let mut out: Vec<(String, String)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("toml") {
            continue;
        }
        let stem = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s.to_owned(),
            None => continue,
        };
        match std::fs::read_to_string(&path) {
            Ok(body) => out.push((stem, body)),
            Err(e) => warn!(?path, err = %e, "could not read user layout file"),
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}
