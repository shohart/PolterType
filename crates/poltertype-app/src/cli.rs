//! Text for the command-line entry points `main` dispatches on.

use anyhow::{Context, Result, bail};

use crate::consts::APP_NAME;

pub(crate) fn print_help() {
    println!(
        "{APP_NAME} {ver}\n\
        \n\
        USAGE:\n  \
            poltertype                      start the tray app\n  \
            poltertype --settings           open the settings window\n  \
            poltertype --setup              open the settings window on the Setup pane\n  \
            poltertype --about              open the settings window on the About pane\n  \
            poltertype --install-plugin DIR install a plug-in for this user\n  \
            poltertype --remove-plugin ID   remove a plug-in this user installed\n  \
            poltertype --plugin-strings ID  print a plug-in's translatable strings\n  \
            poltertype --try-provider ID TEXT [LAYOUT]\n  \
                                            ask a text provider about TEXT, once\n  \
            poltertype --version            print version and exit\n  \
            poltertype --help               show this help",
        ver = env!("CARGO_PKG_VERSION"),
    );
}

/// Print the catalog file a translator starts from for one plug-in:
/// every string its settings pane draws, under the key PolterType will
/// look it up by.
///
/// Derived from the installed manifest rather than documented as a
/// naming rule, so the answer cannot drift from what the loader does.
pub(crate) fn print_plugin_strings(id: &str) -> Result<()> {
    let data_dir = poltertype_core::data_dir::resolve().context("find the data directory")?;
    let found = poltertype_core::plugins::extensions(&data_dir)
        .into_iter()
        .find(|e| e.id == id);
    let Some(ext) = found else {
        let known: Vec<String> = poltertype_core::plugins::extensions(&data_dir)
            .into_iter()
            .map(|e| e.id)
            .collect();
        bail!(
            "no plug-in with id {id:?}{}",
            if known.is_empty() {
                " — none are installed".to_owned()
            } else {
                format!(" — installed: {}", known.join(", "))
            }
        );
    };

    println!("# {} {} — settings pane strings.", ext.name, ext.version);
    println!("#");
    println!(
        "# Save as {}/i18n/<lang>.toml and translate the right-hand",
        ext.dir.display()
    );
    println!("# side. Keys are relative to this plug-in: PolterType keeps them");
    println!("# under `plugin.{id}.`, so nothing here can reach its own labels.");
    println!("# An untranslated line can simply be left out.");
    println!();
    for (key, english) in poltertype_core::plugins::strings(&ext.manifest) {
        println!("{} = {}", quoted(&key), quoted(&english));
    }
    Ok(())
}

/// Install the plug-in in `dir` for this user, replacing any earlier
/// copy of the same id.
///
/// Per user rather than beside the bundled data: on a packaged or
/// AppImage install that directory belongs to root or is read-only.
pub(crate) fn install_plugin(dir: &str) -> Result<()> {
    let root = poltertype_core::plugins::user_root().context("find this user's data directory")?;
    let pack = poltertype_core::plugins::install(std::path::Path::new(dir), &root)
        .with_context(|| format!("install the plug-in in {dir}"))?;
    println!(
        "Installed {} {} into {}.",
        pack.name,
        pack.version,
        pack.path.display()
    );
    for skipped in &pack.skipped {
        println!("  not copied: {skipped}");
    }
    println!("Restart PolterType to start it.");
    Ok(())
}

/// Remove a plug-in this user installed.
pub(crate) fn remove_plugin(id: &str) -> Result<()> {
    let root = poltertype_core::plugins::user_root().context("find this user's data directory")?;
    if !poltertype_core::plugins::uninstall(id, &root)? {
        bail!("no plug-in {id:?} is installed for this user");
    }
    println!("Removed {id}. Restart PolterType to stop it.");
    Ok(())
}

/// A TOML basic string. Labels are one line of prose, so this covers
/// what actually needs covering rather than the whole escape table.
fn quoted(text: &str) -> String {
    let escaped = text
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\t', "\\t");
    format!("\"{escaped}\"")
}

/// Start one text provider, ask it about `text` the way the engine will,
/// print what came back after PolterType's own checks, and stop it.
///
/// For a provider's author: the whole contract, end to end — the
/// service launched as the tray launches it, the channel, the hello, the
/// deadlines and every check on the reply — without typing into another
/// application to see whether a tooltip appears.
///
/// Running this is consent for this one run: the provider is started
/// with a channel whether or not it is switched on in Settings, and sees
/// only `text`.
pub(crate) fn try_provider(id: &str, text: &str, layout: &str) -> Result<()> {
    use poltertype_core::plugins::ProviderCapability;
    use poltertype_core::text_provider::{Reply, Request};

    let data_dir = poltertype_core::data_dir::resolve().context("find the data directory")?;
    let Some(ext) = poltertype_core::plugins::extensions(&data_dir)
        .into_iter()
        .find(|e| e.id == id)
    else {
        bail!("no plug-in with id {id:?}");
    };
    if ext.manifest.text_provider.is_none() {
        bail!("{id} declares no [extension.text_provider]");
    }
    if !poltertype_ipc::Endpoint::SUPPORTED {
        bail!("text providers have no transport on this platform yet");
    }

    let consent = poltertype_core::settings::TextProviderSettings {
        enabled: vec![id.to_owned()],
    };
    let mut sup = crate::plugins::Supervisor::new();
    sup.start_all(std::slice::from_ref(&ext), &consent);
    let Some(link) = sup.providers().into_iter().next() else {
        bail!("could not start {id} with a channel");
    };
    eprintln!("waiting for {} to connect…", link.id());
    let until = std::time::Instant::now() + crate::consts::TRY_PROVIDER_CONNECT;
    while !link.is_connected() {
        if std::time::Instant::now() >= until {
            sup.stop_all();
            bail!(
                "{id} did not connect — it must read {} and {} and send the hello first",
                poltertype_core::text_provider::SOCKET_ENV,
                poltertype_core::text_provider::TOKEN_ENV,
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    let mut asks: Vec<(&str, Request)> = Vec::new();
    if link.manifest().offers(ProviderCapability::Suggest) {
        asks.push((
            "suggest",
            Request::Suggest {
                layout: layout.to_owned(),
                word: text.to_owned(),
                max: poltertype_core::text_provider::MAX_CANDIDATES,
            },
        ));
    }
    if link.manifest().offers(ProviderCapability::Rewrite) {
        asks.push((
            "rewrite",
            Request::Rewrite {
                layout: layout.to_owned(),
                text: text.to_owned(),
            },
        ));
    }
    for (op, request) in &asks {
        let started = std::time::Instant::now();
        let reply = link.request(request);
        let ms = started.elapsed().as_millis();
        match reply {
            Reply::Candidates(c) => {
                let list: Vec<String> = c
                    .iter()
                    .map(|c| format!("{} ({:.2})", c.text, c.score))
                    .collect();
                println!("{op}: {} — {ms} ms", list.join(", "));
            }
            Reply::Rewrite { text, kind } => {
                println!(
                    "{op}: {text}{} — {ms} ms",
                    kind.map(|k| format!(" [{k}]")).unwrap_or_default()
                );
            }
            Reply::Pass => println!(
                "{op}: no opinion — {ms} ms (a pass, a late reply, or one that failed a check)"
            ),
        }
    }
    sup.stop_all();
    Ok(())
}
