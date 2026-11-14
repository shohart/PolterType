# Text providers

> An extension that helps with what is typed: spelling candidates for a
> word, a punctuated or corrected sentence. Discussion
> [#82](https://github.com/Just-Code-NET/PolterType/discussions/82)
> has the proposal; this is the contract as built.

**What exists today:** the manifest section and its checks, the
consent switch, the channel, its supervision, the reply checks, and
`poltertype --try-provider` to exercise all of it. **What does not, yet:**
the engine asking. Merging `suggest` candidates into the tooltip and
the "fix the last sentence" hotkey for `rewrite` are the next steps, each
its own change. Until then a switched-on provider is connected and never
asked.

## The rules it keeps

- **Out of process.** A provider is an extension: its own process,
  supervised like any service. One that hangs or crashes costs its
  suggestions, never the keyboard hook.
- **PolterType draws.** A provider returns text. The tooltip, the accept
  keys and the replacement are PolterType's own. A provider never emits
  keystrokes.
- **The typing path never waits.** Every request has a deadline and a
  late answer is dropped. A correction that arrives after the next word
  started is corruption.
- **Offered, never applied.** There is no silent replacement mode.
- **Off until switched on.** Installing a provider does not let it see
  anything. Nothing it is sent is logged by PolterType.
- **Local only.** The channel is a local socket. What a provider does
  with a word is its own trust boundary, as for any extension.

## Manifest

```toml
kind = "extension"

[extension]
exe          = "superdictate-bridge"
service_args = ["serve"]

[extension.text_provider]
capabilities        = ["suggest", "rewrite"]
suggest_deadline_ms = 60      # 1..=250, default 60
rewrite_deadline_ms = 1500    # 1..=5000, default 1500
```

Refused at load, like any manifest error:

- no `service_args` — the channel goes to the running service;
- a deadline of 0 or past its cap. `suggest` is capped like blocking AI
  mode (250 ms), because the tooltip waits on it;
- no capability this version knows. An unknown one *beside* a known one
  is ignored, so a provider written for a newer PolterType still loads
  here — the same rule as for an unknown pane control.

## Consent

`config.toml`:

```toml
[text_providers]
enabled = ["superdictate-bridge"]
```

set by the switch on the plug-in's card in **Settings → Plugins**, shown
in basic mode too, and for a provider with no settings pane of its own.
Without it the service starts as any other plug-in — its pane and tray
entries work — but no channel is opened and its environment names none.

Switching a provider **off** closes its channel at once (on Save, or on
any change to `config.toml`). Switching one **on** takes effect at the
next start, since the service has to be launched knowing where its
channel is.

This is consent, not containment. An extension runs as the user and
could edit `config.toml` itself; installing it is the boundary against
malicious code, as for every extension. What is closed is the
accidental path: a plug-in whose settings directory would be
PolterType's own (named `poltertype` on Linux, `config` on Windows) gets
no pane, so its pane cannot write the consent key.

## The channel

PolterType listens; the provider connects. Started with consent, the
service's environment carries:

| Variable | Meaning |
|---|---|
| `POLTERTYPE_PROVIDER_SOCKET` | path of the socket to connect to |
| `POLTERTYPE_PROVIDER_TOKEN` | the secret its first line must carry |

Its first line must be `{"hello": "<token>"}`. A connection that sends
anything else, or nothing within 2 s, is dropped and the next one is
waited for; one authenticated connection is accepted and the socket is
then removed. The service has 60 s from its start to connect.

Being the listener means no other process can pose as PolterType by
binding the name first. The token means a process that merely found the
socket cannot connect first and pose as the provider. It does not stop a
process running as the same user: that one can read the service's
environment, and the socket directory's `0700` does not keep it out
either. Such a process is inside the trust boundary already, as every
extension is — see *Consent* above.

**Unix:** the socket is in a directory created for it with mode `0700`
under `$XDG_RUNTIME_DIR` (or the temp dir), removed on exit.
**Windows:** not yet. A named pipe needs an owner-only ACL, and a
mistake there would hand typed words to other users of the machine, so
it comes separately. Until then the switch is shown disabled.

## Messages

JSON, one object per line, both ways. Requests carry an `id`; a reply
carries the same `id`.

```json
{"id": 41, "op": "suggest", "layout": "ru-RU", "word": "ничепуренко", "max": 5}
{"id": 41, "candidates": [{"text": "Ничипуренко", "score": 0.93}]}

{"id": 42, "op": "rewrite", "layout": "ru-RU", "text": "я думаю что это правильно"}
{"id": 42, "text": "Я думаю, что это правильно.", "kind": "punctuation"}

{"id": 43, "pass": true}
```

The layout is sent: the peer is local, and a spelling provider cannot
work without knowing the language.

## What happens to a reply

Anything that fails a check is **no opinion**, never repaired:

- **Late, or not at all:** no opinion. No retry, no queue. A reply to a
  request already given up on is dropped, and never taken for the answer
  to a later one.
- **Not JSON, or no `id`:** ignored, and logged once per session by kind,
  never by content.
- **The wrong shape** — `text` in answer to a `suggest`, `candidates` to
  a `rewrite`: no opinion.
- **A candidate** that is empty, longer than 64 characters, or holds
  whitespace or a control character spoils the whole reply: a provider
  emitting those is not one whose other answers to trust. The typed
  word itself and duplicates are dropped; scores are clamped to
  `0..=1`; at most `max`, and never more than 9.
- **A rewrite** with a control character (a newline included — a
  sentence is replaced in place), the input unchanged, or a length past
  twice the input plus 16 either way — a model that started chatting —
  is refused. An odd `kind` label is dropped.
- **A line past 64 KiB** closes the channel.
- **A provider that dies** is reaped like any service: no restart loop,
  and its channel is closed.

## Trying a provider

```sh
poltertype --try-provider <id> "<text>" [layout]
```

starts the provider as the tray would, waits for its hello, asks
whatever it declared, and prints each reply after the checks above,
with how long it took. Running it is consent for that one run: the
provider sees only the text given. It starts a second copy of the
service, so quit the tray first if the tray runs that provider too —
both would write `logs/plugin-<id>.log`, and a provider that allows one
instance only will refuse the second. During development,
`POLTERTYPE_PLUGIN_PATH` can point at an uninstalled plug-in's
directory.

A provider in a dozen lines of Python, enough to see the contract work:

```python
#!/usr/bin/env python3
import json, os, socket, sys
if sys.argv[1:] != ["serve"]:
    sys.exit(0)
s = socket.socket(socket.AF_UNIX)
s.connect(os.environ["POLTERTYPE_PROVIDER_SOCKET"])
f = s.makefile("rw", encoding="utf-8", newline="\n")
f.write(json.dumps({"hello": os.environ["POLTERTYPE_PROVIDER_TOKEN"]}) + "\n"); f.flush()
for line in f:
    req = json.loads(line)
    if req["op"] == "suggest":
        out = {"id": req["id"], "candidates": [{"text": "Ничипуренко", "score": 0.93}]}
    else:
        out = {"id": req["id"], "text": req["text"][0].upper() + req["text"][1:] + "."}
    f.write(json.dumps(out, ensure_ascii=False) + "\n"); f.flush()
```

A provider written in Rust can use `poltertype_ipc::connect` for its
side of the channel.
