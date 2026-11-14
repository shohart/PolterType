//! A text provider that answers predictably, for trying the contract
//! end to end — `poltertype --try-provider poltertype-test-provider …` —
//! and nothing else. See `docs/TEXT_PROVIDERS.md`.
//!
//! `suggest` offers the word capitalised and reversed; `rewrite` gives
//! the sentence a capital and a full stop. `PT_TEST_PROVIDER_DELAY_MS`
//! delays every answer, to see a deadline pass. What it is sent is never
//! printed: its stderr is the plug-in's log.

mod consts;
mod reply;
mod serve;

#[cfg(test)]
mod tests;

fn main() -> std::process::ExitCode {
    match std::env::args().nth(1).as_deref() {
        Some("serve") => serve::serve(),
        _ => {
            eprintln!("usage: poltertype-test-provider serve  (started by PolterType)");
            std::process::ExitCode::from(2)
        }
    }
}
