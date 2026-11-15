//! `ProviderSuggester` — the tooltip's candidates, with whatever the
//! switched-on providers add.

use std::sync::Arc;

use poltertype_core::plugins::ProviderCapability;
use poltertype_core::text_provider::{Reply, Request};
use poltertype_detect::{Suggestion, SuggestionProvider};
use poltertype_types::LayoutId;

use crate::plugins::ProviderRegistry;

/// The built-in suggester, and the providers asked alongside it.
///
/// **MVP.** Each provider is asked on the engine's thread and waited for
/// up to its manifest's `suggest_deadline_ms` (at most 250 ms) — option
/// (a) of the open question in docs/TEXT_PROVIDERS.md. Whether the
/// tooltip should instead open at once and grow when an answer arrives is
/// the maintainer's call.
///
/// The provider adds; it does not reorder. Built-in candidates come
/// first, but up to [`RESERVED_FOR_PROVIDERS`] places are kept for the
/// providers, or a full list of near spellings would hide the one answer
/// only they can give — a dictionary the user taught another program.
pub struct ProviderSuggester {
    inner: Arc<dyn SuggestionProvider>,
    providers: ProviderRegistry,
}

/// Tooltip places kept for provider candidates when there are any.
const RESERVED_FOR_PROVIDERS: usize = 2;

impl ProviderSuggester {
    pub fn new(inner: Arc<dyn SuggestionProvider>, providers: ProviderRegistry) -> Self {
        Self { inner, providers }
    }

    fn ask_providers(&self, layout: &LayoutId, typed: &str, max: usize) -> Vec<String> {
        // Cloned out so the lock is not held across a wait.
        let links: Vec<_> = self.providers.read().clone();
        let mut out: Vec<String> = Vec::new();
        for link in links {
            if !link.is_connected() || !link.manifest().offers(ProviderCapability::Suggest) {
                continue;
            }
            let request = Request::Suggest {
                layout: layout.as_str().to_owned(),
                word: typed.to_owned(),
                max,
            };
            if let Reply::Candidates(c) = link.request(&request) {
                for cand in c {
                    if !out.contains(&cand.text) {
                        out.push(cand.text);
                    }
                }
            }
        }
        out
    }
}

impl SuggestionProvider for ProviderSuggester {
    fn is_known(&self, layout: &LayoutId, typed_rendering: &str) -> bool {
        self.inner.is_known(layout, typed_rendering)
    }

    fn suggest(&self, layout: &LayoutId, typed_rendering: &str, max: usize) -> Vec<Suggestion> {
        let built = self.inner.suggest(layout, typed_rendering, max);
        if max == 0 {
            return built;
        }
        let from_providers = self.ask_providers(layout, typed_rendering, max);
        merge(built, from_providers, max)
    }
}

/// Built-ins first, the providers' new candidates after them, with up to
/// [`RESERVED_FOR_PROVIDERS`] places guaranteed to the providers.
pub(super) fn merge(
    built: Vec<Suggestion>,
    from_providers: Vec<String>,
    max: usize,
) -> Vec<Suggestion> {
    let fresh: Vec<String> = from_providers
        .into_iter()
        .filter(|p| !built.iter().any(|b| b.text == *p))
        .collect();
    let reserved = fresh.len().min(RESERVED_FOR_PROVIDERS).min(max);
    let mut out: Vec<Suggestion> = built.into_iter().take(max - reserved).collect();
    for text in fresh {
        if out.len() >= max {
            break;
        }
        // Built-in scores are distances (lower is better); a provider
        // candidate has none here, and order is all the tooltip uses.
        out.push(Suggestion { text, score: 0.0 });
    }
    out
}
