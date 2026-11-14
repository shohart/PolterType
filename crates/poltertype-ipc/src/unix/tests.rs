#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The listener's promises: a private place, one checked client, and
//! nothing left behind.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use super::*;
use crate::types::Channel;

fn accept_in_background(
    ep: Endpoint,
    token: &'static str,
) -> std::thread::JoinHandle<(Endpoint, std::io::Result<Channel>)> {
    std::thread::spawn(move || {
        let r = ep.accept(
            Instant::now() + Duration::from_secs(5),
            Duration::from_millis(500),
            &|line| line == format!("{{\"hello\":\"{token}\"}}"),
            &|| false,
        );
        (ep, r)
    })
}

#[test]
fn the_socket_lives_in_a_directory_only_its_owner_can_enter() {
    let ep = Endpoint::bind().unwrap();
    let dir = ep.path().parent().unwrap().to_path_buf();
    let mode = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o700);
    assert!(ep.path().as_os_str().len() < 100, "fits macOS's sun_path");
    drop(ep);
    assert!(!dir.exists(), "dropping the endpoint removes its directory");
}

#[test]
fn a_client_without_the_token_is_dropped_and_the_real_one_still_gets_in() {
    let ep = Endpoint::bind().unwrap();
    let path = ep.path().to_path_buf();
    let handle = accept_in_background(ep, "s3cret");

    let mut impostor = UnixStream::connect(&path).unwrap();
    impostor.write_all(b"{\"hello\":\"guess\"}\n").unwrap();
    let mut buf = String::new();
    // The impostor is hung up on.
    assert_eq!(
        BufReader::new(&impostor).read_line(&mut buf).unwrap_or(0),
        0
    );

    let mut client = UnixStream::connect(&path).unwrap();
    client.write_all(b"{\"hello\":\"s3cret\"}\n").unwrap();
    let (_ep, channel) = handle.join().unwrap();
    let mut channel = channel.expect("the real client is accepted");

    // Both directions work, and bytes sent right after the hello are
    // not lost to the greeting's read.
    channel.writer.write_all(b"ping\n").unwrap();
    let mut got = String::new();
    BufReader::new(&client).read_line(&mut got).unwrap();
    assert_eq!(got, "ping\n");
    client.write_all(b"pong\n").unwrap();
    let mut back = String::new();
    BufReader::new(channel.reader).read_line(&mut back).unwrap();
    assert_eq!(back, "pong\n");

    (channel.close)();
    let mut after = String::new();
    assert_eq!(
        BufReader::new(&client).read_line(&mut after).unwrap_or(0),
        0
    );
}

#[test]
fn a_silent_client_cannot_hold_the_door() {
    let ep = Endpoint::bind().unwrap();
    let path = ep.path().to_path_buf();
    let handle = accept_in_background(ep, "tok");
    let _silent = UnixStream::connect(&path).unwrap();
    std::thread::sleep(Duration::from_millis(50));
    let mut client = UnixStream::connect(&path).unwrap();
    client.write_all(b"{\"hello\":\"tok\"}\n").unwrap();
    let (_ep, channel) = handle.join().unwrap();
    assert!(channel.is_ok());
}

#[test]
fn stop_gives_up_the_wait() {
    let ep = Endpoint::bind().unwrap();
    let r = ep.accept(
        Instant::now() + Duration::from_secs(30),
        Duration::from_millis(100),
        &|_| true,
        &|| true,
    );
    assert_eq!(
        r.err().map(|e| e.kind()),
        Some(std::io::ErrorKind::Interrupted)
    );
}

#[test]
fn tokens_are_long_and_differ() {
    let (a, b) = (random_token().unwrap(), random_token().unwrap());
    assert_eq!(a.len(), 32);
    assert_ne!(a, b);
}
