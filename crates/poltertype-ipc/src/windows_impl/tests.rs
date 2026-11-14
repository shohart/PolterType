#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The pipe's promises: one checked client, the rest dropped, both
//! directions at once, and closing wakes a blocked reader.

use std::io::{BufRead, BufReader, Read, Write};
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
fn the_pipe_name_is_unguessable() {
    let ep = Endpoint::bind().unwrap();
    let name = ep.path().to_string_lossy().into_owned();
    assert!(name.starts_with(r"\\.\pipe\poltertype-"));
    assert_eq!(name.len(), r"\\.\pipe\poltertype-".len() + 32);
}

#[test]
fn the_descriptor_names_this_user_alone() {
    let sddl = security::owner_only_sddl().unwrap();
    assert!(sddl.starts_with("D:P(A;;GA;;;S-1-5-"), "{sddl}");
    assert_eq!(sddl.matches('(').count(), 1, "one entry only: {sddl}");
    security::Descriptor::from_sddl(&sddl).unwrap();
}

#[test]
fn a_client_without_the_token_is_dropped_and_the_real_one_still_gets_in() {
    let ep = Endpoint::bind().unwrap();
    let path = ep.path().to_path_buf();
    let handle = accept_in_background(ep, "s3cret");

    let impostor = connect(&path, "{\"hello\":\"guess\"}").unwrap();
    let mut buf = String::new();
    assert_eq!(
        BufReader::new(impostor.reader)
            .read_line(&mut buf)
            .unwrap_or(0),
        0
    );
    drop(impostor.writer);

    let client = connect(&path, "{\"hello\":\"s3cret\"}").unwrap();
    let (_ep, channel) = handle.join().unwrap();
    let mut channel = channel.expect("the real client is accepted");

    channel.writer.write_all(b"ping\n").unwrap();
    let mut client_reader = BufReader::new(client.reader);
    let mut got = String::new();
    client_reader.read_line(&mut got).unwrap();
    assert_eq!(got, "ping\n");

    let mut client_writer = client.writer;
    client_writer.write_all(b"pong\n").unwrap();
    let mut back = String::new();
    BufReader::new(channel.reader).read_line(&mut back).unwrap();
    assert_eq!(back, "pong\n");
}

/// The reason everything here is overlapped: a read waiting on one
/// thread must not hold up a write on another.
#[test]
fn a_blocked_read_does_not_hold_up_a_write() {
    let ep = Endpoint::bind().unwrap();
    let path = ep.path().to_path_buf();
    let handle = accept_in_background(ep, "tok");
    let client = connect(&path, "{\"hello\":\"tok\"}").unwrap();
    let (_ep, channel) = handle.join().unwrap();
    let channel = channel.unwrap();

    let mut server_reader = channel.reader;
    let reading = std::thread::spawn(move || {
        let mut b = [0u8; 4];
        server_reader.read_exact(&mut b).map(|()| b)
    });
    std::thread::sleep(Duration::from_millis(100));
    let mut server_writer = channel.writer;
    let started = Instant::now();
    server_writer.write_all(b"out\n").unwrap();
    assert!(started.elapsed() < Duration::from_secs(1));
    let mut got = String::new();
    BufReader::new(client.reader).read_line(&mut got).unwrap();
    assert_eq!(got, "out\n");

    let mut cw = client.writer;
    cw.write_all(b"in!\n").unwrap();
    assert_eq!(&reading.join().unwrap().unwrap(), b"in!\n");
}

#[test]
fn closing_wakes_a_blocked_reader() {
    let ep = Endpoint::bind().unwrap();
    let path = ep.path().to_path_buf();
    let handle = accept_in_background(ep, "tok");
    let _client = connect(&path, "{\"hello\":\"tok\"}").unwrap();
    let (_ep, channel) = handle.join().unwrap();
    let channel = channel.unwrap();
    let mut reader = channel.reader;
    let reading = std::thread::spawn(move || {
        let mut b = [0u8; 1];
        reader.read(&mut b)
    });
    std::thread::sleep(Duration::from_millis(100));
    (channel.close)();
    let r = reading.join().unwrap();
    assert!(matches!(r, Ok(0) | Err(_)), "{r:?}");
}

#[test]
fn a_silent_client_cannot_hold_the_door() {
    let ep = Endpoint::bind().unwrap();
    let path = ep.path().to_path_buf();
    let handle = accept_in_background(ep, "tok");
    let silent = std::thread::spawn({
        let path = path.clone();
        move || {
            // Connects and says nothing; the pipe hangs up after the
            // hello timeout and takes the next client.
            let ch = connect(&path, "").ok();
            std::thread::sleep(Duration::from_millis(800));
            drop(ch);
        }
    });
    std::thread::sleep(Duration::from_millis(50));
    let client = loop {
        match connect(&path, "{\"hello\":\"tok\"}") {
            Ok(c) => break c,
            Err(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    };
    let (_ep, channel) = handle.join().unwrap();
    assert!(channel.is_ok());
    drop(client);
    silent.join().unwrap();
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
