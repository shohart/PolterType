#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Tests for the AI plug-in gate, and for the wordlists folder stamp.
//!
//! What is being pinned here is the *gating*, not the detectors: that a
//! disabled subsystem builds nothing, that an enabled one with no
//! entries builds nothing, and that plug-ins are appended rather than
//! substituted. Those are the properties that decide whether turning
//! `[ai].enabled` on can hurt a user who was happy before.

use poltertype_core::settings::AiSettings;
use poltertype_types::AiPluginConfig;

use super::build_ai_detectors;

/// An entry pointing at a third-party API the user holds a key for.
fn remote_entry(id: &str) -> AiPluginConfig {
    AiPluginConfig {
        r#type: "llm".to_owned(),
        id: id.to_owned(),
        provider: Some("anthropic".to_owned()),
        model: Some("claude-sonnet-4".to_owned()),
        api_key_ref: Some("keyring:anthropic".to_owned()),
        ..Default::default()
    }
}

/// An entry pointing at a model on the user's own machine. Needs no
/// key and no `allow_remote`, because nothing leaves the computer.
fn local_entry(id: &str) -> AiPluginConfig {
    AiPluginConfig {
        r#type: "llm".to_owned(),
        id: id.to_owned(),
        provider: Some("ollama".to_owned()),
        model: Some("llama3".to_owned()),
        ..Default::default()
    }
}

/// The default. Nothing is built, and nothing is said.
#[test]
fn a_disabled_subsystem_builds_nothing() {
    let ai = AiSettings {
        enabled: false,
        allow_remote: false,
        plugins: vec![remote_entry("claude")],
    };
    assert!(
        build_ai_detectors(&ai).is_empty(),
        "plug-ins must not load while [ai].enabled is false"
    );
}

#[test]
fn enabled_with_no_plugins_builds_nothing() {
    let ai = AiSettings {
        enabled: true,
        allow_remote: false,
        plugins: Vec::new(),
    };
    assert!(build_ai_detectors(&ai).is_empty());
}

/// With the `ai` feature the entry becomes a detector; without it the
/// crate is not linked and nothing can be built. Both are correct — the
/// test asserts the one this build is compiled for, so it is honest
/// either way rather than silently vacuous.
#[test]
fn an_enabled_entry_loads_only_where_the_feature_exists() {
    let ai = AiSettings {
        enabled: true,
        allow_remote: false,
        plugins: vec![remote_entry("claude")],
    };
    let built = build_ai_detectors(&ai);
    if cfg!(feature = "ai") {
        assert_eq!(built.len(), 1, "the entry should have produced a detector");
    } else {
        assert!(
            built.is_empty(),
            "a build without the ai feature must produce nothing"
        );
    }
}

/// `allow_remote` gates the *call*, not the construction — so that
/// switching it on is a settings change rather than a config rewrite.
#[test]
fn allow_remote_does_not_decide_whether_the_plugin_loads() {
    let plugins = vec![remote_entry("claude")];
    let off = build_ai_detectors(&AiSettings {
        enabled: true,
        allow_remote: false,
        plugins: plugins.clone(),
    });
    let on = build_ai_detectors(&AiSettings {
        enabled: true,
        allow_remote: true,
        plugins,
    });
    assert_eq!(off.len(), on.len());
}

/// A model the user runs themselves loads with `allow_remote` off.
/// That switch exists to gate typed words *leaving the machine*, and
/// a request to loopback does not — requiring it here would make
/// people enable network access they are not using.
#[test]
fn a_local_model_needs_no_network_permission() {
    let built = build_ai_detectors(&AiSettings {
        enabled: true,
        allow_remote: false,
        plugins: vec![local_entry("ollama")],
    });
    if cfg!(feature = "ai") {
        assert_eq!(built.len(), 1, "a loopback endpoint must load");
    } else {
        assert!(built.is_empty());
    }
}

/// A config written for 0.9.0 names plug-in kinds that no longer
/// exist. It must cost the user a log line, not the app: the entry is
/// skipped and anything valid beside it still loads.
#[test]
fn a_retired_plugin_kind_is_skipped_without_taking_the_rest_down() {
    let mut old = local_entry("stale");
    old.r#type = "local-onnx".to_owned();

    let built = build_ai_detectors(&AiSettings {
        enabled: true,
        allow_remote: false,
        plugins: vec![old, local_entry("current")],
    });
    if cfg!(feature = "ai") {
        assert_eq!(
            built.len(),
            1,
            "the retired entry goes, the valid one stays"
        );
    } else {
        assert!(built.is_empty());
    }
}

// ─── Wordlists folder stamp ─────────────────────────────────────

/// A throwaway directory, removed on drop.
struct ScratchDir(std::path::PathBuf);

impl ScratchDir {
    fn new(label: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "poltertype-app-{label}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("mkdir scratch");
        Self(path)
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// What the watcher compares: a written, grown or added `.txt` — in the
/// folder or a profile's — changes the stamp; anything else does not.
#[test]
fn wordlist_stamp_sees_overlay_and_profile_files_only() {
    use super::wordlist_stamp;

    let dir = ScratchDir::new("stamp");
    let empty = wordlist_stamp(&dir.0);
    assert!(empty.is_empty());

    std::fs::write(dir.0.join("README.txt.bak"), "x").unwrap();
    std::fs::write(dir.0.join("notes.md"), "x").unwrap();
    assert_eq!(wordlist_stamp(&dir.0), empty, "non-.txt files are ignored");

    std::fs::write(dir.0.join("en_us.txt"), "GitHub\n").unwrap();
    let one = wordlist_stamp(&dir.0);
    assert_eq!(one.len(), 1);

    std::fs::write(dir.0.join("en_us.txt"), "GitHub\nkubectl\n").unwrap();
    let grown = wordlist_stamp(&dir.0);
    assert_ne!(grown, one, "a rewrite that changes the size is seen");

    let profile = dir.0.join("profiles").join("code");
    std::fs::create_dir_all(&profile).unwrap();
    std::fs::write(profile.join("en_us.txt"), "serde\n").unwrap();
    let with_profile = wordlist_stamp(&dir.0);
    assert_eq!(with_profile.len(), 2, "profile files are watched too");

    assert_eq!(
        wordlist_stamp(&dir.0),
        with_profile,
        "stable when untouched"
    );
    assert!(wordlist_stamp(&dir.0.join("missing")).is_empty());
}

/// A pack's own `wordlists/` under this user's plug-in root is watched:
/// that is where a bridge writes the words it exports.
#[test]
fn wordlist_stamp_sees_installed_packs_wordlists() {
    use super::wordlist_stamp_with;

    let dir = ScratchDir::new("stamp-overlay");
    let root = ScratchDir::new("stamp-plugins");
    let before = wordlist_stamp_with(&dir.0, Some(&root.0));
    let words = root.0.join("plugins").join("bridge").join("wordlists");
    std::fs::create_dir_all(&words).unwrap();
    std::fs::write(words.join("ru_ru-extras.txt"), "Ничипуренко\n").unwrap();
    let after = wordlist_stamp_with(&dir.0, Some(&root.0));
    assert_ne!(before, after);
    assert_eq!(after.len(), 1);
    // The pack's other files are not words.
    std::fs::write(
        root.0.join("plugins").join("bridge").join("manifest.toml"),
        "x",
    )
    .unwrap();
    assert_eq!(wordlist_stamp_with(&dir.0, Some(&root.0)), after);
}
