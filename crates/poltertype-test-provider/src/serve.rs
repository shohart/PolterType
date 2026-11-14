//! Connect to PolterType and answer until the channel ends.

use std::io::{BufRead, BufReader, Write};
use std::process::ExitCode;
use std::time::Duration;

use crate::consts::{DELAY_ENV, SOCKET_ENV, TOKEN_ENV};
use crate::reply::answer;

pub(crate) fn serve() -> ExitCode {
    let (Some(path), Some(token)) = (std::env::var_os(SOCKET_ENV), std::env::var(TOKEN_ENV).ok())
    else {
        // Started without consent: PolterType opened no channel. Stay up
        // like any service would, doing nothing, until stopped.
        eprintln!("test provider: no channel — not switched on in Settings; idle");
        loop {
            std::thread::sleep(Duration::from_secs(3600));
        }
    };
    let hello = serde_json::json!({ "hello": token }).to_string();
    let channel = match poltertype_ipc::connect(std::path::Path::new(&path), &hello) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("test provider: could not connect: {e}");
            return ExitCode::FAILURE;
        }
    };
    eprintln!("test provider: connected");
    let delay = std::env::var(DELAY_ENV)
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map(Duration::from_millis);
    let mut writer = channel.writer;
    let mut answered = 0u64;
    for line in BufReader::new(channel.reader).lines() {
        let Ok(line) = line else { break };
        let Some(reply) = answer(&line) else { continue };
        if let Some(d) = delay {
            std::thread::sleep(d);
        }
        if writer
            .write_all(reply.as_bytes())
            .and_then(|()| writer.write_all(b"\n"))
            .and_then(|()| writer.flush())
            .is_err()
        {
            break;
        }
        answered += 1;
    }
    eprintln!("test provider: channel closed after {answered} answers");
    ExitCode::SUCCESS
}
