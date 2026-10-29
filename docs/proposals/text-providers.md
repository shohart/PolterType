# Proposal: text providers — letting an extension help with spelling and punctuation

> **Status: draft for discussion, not accepted.** Written against
> v0.38.2. Nothing here changes behaviour until each step is reviewed
> separately; the small steps in §3 are useful on their own whether or
> not §4 ever lands.

## The problem

PolterType sees every finished word, and it already knows how to offer
something better for it: the suggestion tooltip, F1–F9 / chord
acceptance, and a replacement through the same absorb → delete → replay
path that layout corrections use. What it can offer is limited to what
is compiled in. Candidates come from Hunspell-derived FSTs and the
user's overlay (`SuggestionProvider` in `poltertype-detect`). A
`WordRewriter` was designed in `PLAN.md` and has no consumer, so
`[[ai.rewriters]]` is ignored (`docs/AI.md`).

Meanwhile, people who would like more already run local models that do
it. The concrete case behind this proposal is
[SuperDictate Next](https://github.com/shohart/SuperDictate-Next), a
local dictation app. It keeps a personal vocabulary learned from the
user's own corrections, and runs a punctuation-restoration model and a
small grammar-correction model on the CPU. None of that should be
*inside* PolterType: it is hundreds of megabytes, it is one vendor's
choice, and `docs/AI.md` explains well why PolterType bundles neither
models nor clients.

The extension mechanism is the obvious home, and it is one seam short.
An extension can draw a pane, add tray entries and report text, but
**nothing flows from the engine to it**. It never sees a word, so it
cannot suggest one.

## What this proposal keeps

Every rule the existing subsystems were built on, unchanged:

- **Out of process.** A provider is an extension: its own process,
  supervised the way services are today. A provider that hangs or
  crashes costs its suggestions, never the keyboard hook.
- **PolterType draws.** A provider returns text. The tooltip, the
  accept keys and the replacement are PolterType's existing machinery.
  A provider never emits keystrokes.
- **The typing path never waits.** Every request has a deadline, and a
  late answer is dropped, as in `LlmDetector`'s `background` mode and
  for the same reason: a correction that arrives after the next word
  started is corruption.
- **Offered, never applied.** A rewrite is a suggestion the user
  accepts. There is no silent replacement mode.
- **Off by default, nothing logged.** A provider is configured by
  installing and enabling it. Words crossing the seam go through
  `redact_word` like everywhere else.
- **Local only.** The transport is a local socket. Whatever the
  provider does with a word is the provider's trust boundary, as
  `ARCHITECTURE.md` already says of extensions that read keystrokes.

## 3. Small, independent steps first

Each is a separate PR and is useful without the rest. All three came up
while wiring a vocabulary export into the user overlay, which works
today with no change at all.

### 3.1 A data pack may extend a layout it does not define

`layouts/plugins.rs` attaches a pack's `wordlists/<stem>.fst` only to a
layout the pack ships in `layout-mappings/`. A pack that only wants to
*add words* to `ru_ru` has to replace the whole Russian layout and its
dictionary, which would also shadow the bundled one as it improves.

The proposal: a pack's `wordlists/<stem>-extras.txt` (text, the existing
overlay shape) is merged into the overlay of the loaded layout `<stem>`,
in pack order, before the user's own overlay. A pack with only
`wordlists/` stops being refused as having no `layout-mappings/`. The
installer's allow-list already permits `wordlists/*.txt`.

### 3.2 Overlay words keep their surface form for suggestions

The overlay is stored in the lossy membership shape
(`letters_only_lower`), which is right for detection. But the suggester
then offers `github` and `ничипуренко` for words the user wrote as
`GitHub` and `Ничипуренко`. The proposal: keep a second map, from the
membership key to the first surface form read, and use it only when
rendering a suggestion. Detection is unchanged.

### 3.3 Watch the overlay directory

Overlays are re-read on "Reload Settings" (`reload_user_dictionaries`).
A file written by another program — a sync tool, a dotfiles checkout, an
exporter — does nothing until then. The proposal: watch
`<config-dir>/wordlists/` the way `config.toml` is watched, debounced,
reusing the same reload function.

## 4. The text-provider contract

### Manifest

```toml
kind = "extension"

[extension]
exe          = "superdictate-bridge"
service_args = ["serve"]

[extension.text_provider]
# What the provider answers. Either or both.
capabilities = ["suggest", "rewrite"]
# Per-request budget. `suggest` is capped like blocking AI mode
# (MAX_BLOCKING_LATENCY_MS, 250); `rewrite` is asynchronous and may be
# longer, but is still bounded.
suggest_deadline_ms = 60
rewrite_deadline_ms = 1500
```

`check_extension` validates the section like the rest of the manifest:
known capabilities only, deadlines within the caps, refused otherwise.

### Transport

The service's stdin is null by design, so the channel is a local
socket. PolterType creates it per provider: a unix socket under the
runtime directory with mode `0600` on Linux and macOS, a named pipe with
an owner-only ACL on Windows. Its path is passed in
`POLTERTYPE_PROVIDER_SOCKET`, next to `POLTERTYPE_LOCALE` and
`POLTERTYPE_DATA_DIR`. PolterType is the listener, so a provider cannot
be impersonated by another process binding the name first.

Messages are JSON lines, one request and one response, matched by `id`:

```json
{"id": 41, "op": "suggest", "layout": "ru-RU", "word": "ничепуренко", "max": 5}
{"id": 41, "candidates": [{"text": "Ничипуренко", "score": 0.93}]}

{"id": 42, "op": "rewrite", "layout": "ru-RU", "text": "я думаю что это правильно"}
{"id": 42, "text": "Я думаю, что это правильно.", "kind": "punctuation"}
```

A response may also be `{"id": n, "pass": true}` (no opinion). A missing
or late response means the same.

The layout id is sent here, unlike in `docs/AI.md`'s remote case. The
peer is local, and a spelling provider cannot work without knowing the
language.

### suggest

`suggest` joins the existing `SuggestionProvider` path. On a word the
engine decided to keep and the dictionary does not know, the built-in
candidates are computed as now and the provider is asked in parallel.
Its candidates are merged in, deduplicated, and the tooltip shows the
union, all within the deadline. A provider candidate never displaces a
built-in one that scores higher. The provider adds; it does not
reorder.

### rewrite

`rewrite` gives `WordRewriter` its consumer, at sentence granularity
because that is where punctuation and grammar live:

- **Trigger:** a sentence boundary (`. ! ?`, Enter) or a pause after one.
  The request carries the sentence, as the engine's buffer saw it.
- **Offer:** the existing tooltip, showing the rewritten sentence with
  the accept keys. Never applied without acceptance.
- **Apply:** only if the screen model still vouches for the whole span,
  with no caret move, selection, focus change or mouse click since. That
  model covers one word and its trail today, so this needs a span-level
  extension of it. Without that, the offer is dismissed rather than
  guessed at.

Because the span problem is real, a first version may ship rewrite
**only behind an explicit hotkey**: "fix the last sentence". The engine
knows the text it typed since the last boundary. It asks, shows the
answer and replaces only on accept. The automatic offer can follow once
the span model exists.

### What PolterType does when a provider misbehaves

- It answers late or not at all: no opinion, every time. No retry and no
  queue.
- It returns something that is not JSON, or an unknown `id`: logged once
  per session (without content) and treated as no opinion.
- It returns text containing control characters, or a rewrite whose
  length is wildly different from the input (a model that started
  chatting): refused.
- It dies: as for any service. No restart loop, and its suggestions stop.

## 5. Not proposed

- No in-process plug-ins, WASM or dynamic libraries.
- No network transport. A provider that wants a remote model makes that
  call itself, inside its own trust boundary, and the user installed it
  knowing so.
- No automatic replacement of anything.
- No change to layout detection. `[[ai.plugins]]` stays the way a model
  votes on layouts.

## 6. Order of work

1. §3.1, §3.2, §3.3 — small PRs, each reviewable alone.
2. The manifest section, its validation, and the socket and supervision
   plumbing, with a test provider. No engine change yet.
3. `suggest` merged into the tooltip.
4. `rewrite` behind the explicit hotkey.
5. The automatic sentence offer, once the span model is designed.

The SuperDictate bridge extension would be developed alongside, in its
own repository, as the first real provider. It is a test of the contract
and in no way a dependency of PolterType.
