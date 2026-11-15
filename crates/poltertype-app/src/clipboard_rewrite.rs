//! "Improve Text in Clipboard": a text provider's `rewrite`, offered
//! without touching anybody's window.
//!
//! **MVP**, standing in for the "fix the last sentence" hotkey the
//! proposal describes, which needs the engine to hold a whole sentence.
//! Here the user copies text, picks the tray entry, and the rewrite is
//! put back on the clipboard: their paste is the acceptance, so nothing
//! is ever replaced that they did not put there themselves. The text is
//! never logged or shown in a notification, only its length.

use poltertype_core::i18n::{tr, tr_args};
use poltertype_core::plugins::ProviderCapability;
use poltertype_core::text_provider::{Reply, Request};
use poltertype_types::LayoutId;
use tracing::{info, warn};

use crate::bridges::spawn_error_notification;
use crate::consts::MAX_CLIPBOARD_REWRITE_CHARS;
use crate::plugins::ProviderRegistry;

/// Ask the first switched-on provider that rewrites, off the UI thread —
/// a rewrite may take seconds — and say what happened in a notification.
pub(crate) fn spawn_clipboard_rewrite(providers: ProviderRegistry, layout: Option<LayoutId>) {
    let _ = std::thread::Builder::new()
        .name("poltertype-clipboard-rewrite".into())
        .spawn(move || {
            let message = rewrite_clipboard(&providers, layout.as_ref());
            spawn_error_notification(message);
        });
}

fn rewrite_clipboard(providers: &ProviderRegistry, layout: Option<&LayoutId>) -> String {
    let link = providers
        .read()
        .iter()
        .find(|l| l.is_connected() && l.manifest().offers(ProviderCapability::Rewrite))
        .cloned();
    let Some(link) = link else {
        return tr(
            "rewrite.no_provider",
            "No text provider that rewrites is switched on. Switch one on in Settings → Plugins, then restart PolterType.",
        )
        .to_owned();
    };
    let clipboard = match poltertype_input::clipboard() {
        Ok(c) => c,
        Err(e) => {
            warn!(?e, "clipboard rewrite: no clipboard on this session");
            return tr(
                "rewrite.no_clipboard",
                "PolterType cannot read the clipboard on this system.",
            )
            .to_owned();
        }
    };
    let text = match clipboard.text() {
        Ok(Some(t)) if !t.trim().is_empty() => t,
        _ => {
            return tr("rewrite.empty", "The clipboard holds no text to improve.").to_owned();
        }
    };
    let chars = text.chars().count();
    if chars > MAX_CLIPBOARD_REWRITE_CHARS {
        return tr_args(
            "rewrite.too_long",
            "The clipboard text is too long to improve ({} characters at most).",
            &[&MAX_CLIPBOARD_REWRITE_CHARS.to_string()],
        );
    }
    let request = Request::Rewrite {
        layout: layout.map_or_else(|| "und".to_owned(), |l| l.as_str().to_owned()),
        text,
    };
    match link.request(&request) {
        Reply::Rewrite { text, kind } => match clipboard.set_text(&text) {
            Ok(()) => {
                info!(
                    provider = link.id(),
                    chars_in = chars,
                    chars_out = text.chars().count(),
                    "clipboard rewrite offered"
                );
                let what = kind.unwrap_or_default();
                tr_args(
                    "rewrite.done",
                    "Improved text is in the clipboard{} — paste it to use it. Your original was replaced there, so copy it again if you need it.",
                    &[&if what.is_empty() {
                        String::new()
                    } else {
                        format!(" ({what})")
                    }],
                )
            }
            Err(e) => {
                warn!(?e, "clipboard rewrite: could not write the clipboard");
                tr(
                    "rewrite.no_clipboard",
                    "PolterType cannot read the clipboard on this system.",
                )
                .to_owned()
            }
        },
        _ => tr(
            "rewrite.no_change",
            "The text provider offered no improvement (or did not answer in time).",
        )
        .to_owned(),
    }
}
