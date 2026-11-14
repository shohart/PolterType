//! What a data-only pack is allowed to contain.

/// Directory under `<data_dir>` holding installed packs.
pub const PLUGINS_DIR: &str = "plugins";

/// The manifest every pack must have.
pub const MANIFEST_NAME: &str = "manifest.toml";

/// Environment variable holding extra plug-in directories, separated
/// the way `PATH` is on this platform.
pub const PLUGIN_PATH_VAR: &str = "POLTERTYPE_PLUGIN_PATH";

/// Directory an extension's program must live in, inside the plug-in.
pub const EXTENSION_BIN_DIR: &str = "bin";

/// The argument a [`super::ControlKind::Query`] box's typed question
/// replaces, as a whole argument.
///
/// Here rather than beside the code that substitutes it, because the
/// manifest is checked *before* anything runs: a query box whose command
/// never mentions this would take a question and throw it away, and
/// finding that out at load is the difference between a refused plug-in
/// and a search box that silently ignores you.
pub const QUERY_PLACEHOLDER: &str = "{query}";

/// Sub-directories a pack may populate, and the file extensions
/// allowed in each.
///
/// An allow-list rather than a deny-list on purpose: a deny-list has
/// to guess every dangerous thing, and it will be wrong about the one
/// that matters. This says what a *language pack* is, and everything
/// else is left on the floor with a warning.
pub const ALLOWED_CONTENT: &[(&str, &[&str])] = &[
    ("layout-mappings", &["toml"]),
    ("wordlists", &["fst", "txt", "gz"]),
    ("i18n", &["toml"]),
];

/// Sub-directories an **extension** may populate on top of
/// [`ALLOWED_CONTENT`].
///
/// The whole difference between the two kinds lives in this one entry:
/// an extension may ship programs, in `bin/`, and nowhere else. Every
/// other guarantee — no traversal, no symlinks, a size budget, atomic
/// replacement — is identical.
///
/// The extension list is empty rather than enumerating `.exe`, `.so` and
/// friends, since a Unix executable has no extension. What bounds `bin/`
/// instead is that nothing in it is ever *loaded* — it is spawned by the
/// name the manifest declares.
pub const EXTENSION_CONTENT: &[(&str, &[&str])] = &[("bin", &[])];

/// Files permitted at the top level of a pack, beyond the manifest.
/// Documentation and licence text, so a pack can carry its own terms.
pub const ALLOWED_TOP_LEVEL: &[&str] = &[
    "manifest.toml",
    "README.md",
    "LICENSE",
    "LICENSE.txt",
    "LICENSE.md",
    "CREDITS.md",
];

/// Total bytes one pack may occupy. Generous next to a bundled
/// language (the Turkish FST alone is 15 MB) and still a bound: a
/// "language pack" that wants a gigabyte is not one.
pub const MAX_PACK_BYTES: u64 = 256 * 1024 * 1024;

/// Most files a pack may contain. Guards the enumeration itself, so a
/// directory with a million entries cannot make installation hang.
pub const MAX_PACK_FILES: usize = 512;

/// Longest a `suggest` request may be given, in ms. The same ceiling as
/// blocking AI mode (`poltertype_ai::MAX_BLOCKING_LATENCY_MS`): the
/// suggestion tooltip waits on it, and past this the wait is felt.
pub const SUGGEST_DEADLINE_CAP_MS: u64 = 250;

/// Longest a `rewrite` request may be given, in ms. A rewrite is asked
/// for by a hotkey and shown when it arrives, so it may take longer than
/// a suggestion — but it is still bounded.
pub const REWRITE_DEADLINE_CAP_MS: u64 = 5000;

/// `suggest_deadline_ms` when the manifest names none.
pub const DEFAULT_SUGGEST_DEADLINE_MS: u64 = 60;

/// `rewrite_deadline_ms` when the manifest names none.
pub const DEFAULT_REWRITE_DEADLINE_MS: u64 = 1500;
