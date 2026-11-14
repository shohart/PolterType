//! The listening end, in a directory only this user can enter.

use std::io::{ErrorKind, Read};
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::types::Channel;

use super::random::random_token;

/// How often a waiting accept looks again. Short enough that a client
/// starting up is not kept waiting, long enough to cost nothing.
const ACCEPT_POLL: Duration = Duration::from_millis(20);

/// Longest first line a client may send. A hello is a few dozen bytes.
const MAX_HELLO_BYTES: usize = 1024;

/// A listening socket at [`Self::path`], in a directory created for it
/// alone. Dropping it removes both.
pub struct Endpoint {
    dir: PathBuf,
    path: PathBuf,
    listener: UnixListener,
}

impl Endpoint {
    /// Whether this platform has a transport at all.
    pub const SUPPORTED: bool = true;

    /// Create the directory (mode `0700`, refused if the name is taken)
    /// and listen in it.
    ///
    /// A directory this call created is this user's by construction, so
    /// nothing has to be checked about who owns it. The names are short
    /// on purpose: a socket path is capped near 104 bytes on macOS, and
    /// its temp dir is long already.
    pub fn bind() -> std::io::Result<Self> {
        let base = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|p| p.is_dir())
            .unwrap_or_else(std::env::temp_dir);
        for _ in 0..8 {
            let token = random_token()?;
            let dir = base.join(format!("pt-{}", &token[..12]));
            match std::fs::DirBuilder::new().mode(0o700).create(&dir) {
                Ok(()) => {}
                Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
            let path = dir.join("s");
            let listener = match UnixListener::bind(&path) {
                Ok(l) => l,
                Err(e) => {
                    let _ = std::fs::remove_dir(&dir);
                    return Err(e);
                }
            };
            listener.set_nonblocking(true)?;
            return Ok(Self {
                dir,
                path,
                listener,
            });
        }
        Err(std::io::Error::new(
            ErrorKind::AlreadyExists,
            "no free name for the provider socket directory",
        ))
    }

    /// Where a client connects.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Wait until `deadline` for a client whose first line passes
    /// `check`, giving each one `hello_timeout` to send it. A client
    /// that fails is dropped and the wait goes on, so a stray connection
    /// cannot lock the real one out. `stop` returning true gives up early.
    pub fn accept(
        &self,
        deadline: Instant,
        hello_timeout: Duration,
        check: &dyn Fn(&str) -> bool,
        stop: &dyn Fn() -> bool,
    ) -> std::io::Result<Channel> {
        loop {
            if stop() {
                return Err(std::io::Error::new(ErrorKind::Interrupted, "stopped"));
            }
            match self.listener.accept() {
                Ok((stream, _)) => {
                    if let Some(channel) = greet(stream, hello_timeout, check)? {
                        return Ok(channel);
                    }
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => {}
                Err(e) => return Err(e),
            }
            if Instant::now() >= deadline {
                return Err(std::io::Error::new(
                    ErrorKind::TimedOut,
                    "no client proved itself in time",
                ));
            }
            std::thread::sleep(ACCEPT_POLL);
        }
    }
}

/// Read the client's first line and, if it passes, hand back the
/// channel. Read a byte at a time so nothing after the newline is
/// swallowed by a buffer this function then throws away.
fn greet(
    stream: UnixStream,
    hello_timeout: Duration,
    check: &dyn Fn(&str) -> bool,
) -> std::io::Result<Option<Channel>> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(hello_timeout))?;
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    let mut reader = &stream;
    loop {
        match reader.read(&mut byte) {
            Ok(1) if byte[0] == b'\n' => break,
            Ok(1) if line.len() < MAX_HELLO_BYTES => line.push(byte[0]),
            // Too long, closed, timed out or failed: not our client.
            _ => return Ok(None),
        }
    }
    let ok = std::str::from_utf8(&line).is_ok_and(check);
    if !ok {
        return Ok(None);
    }
    stream.set_read_timeout(None)?;
    let reader = stream.try_clone()?;
    let writer = stream.try_clone()?;
    Ok(Some(Channel {
        reader: Box::new(reader),
        writer: Box::new(writer),
        close: Box::new(move || {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }),
    }))
}

/// The client's side: connect to `path`, send `hello` as the first line,
/// and hand back the channel. For a provider written in Rust, and for
/// tests that play one.
pub fn connect(path: &Path, hello: &str) -> std::io::Result<Channel> {
    use std::io::Write;
    let stream = UnixStream::connect(path)?;
    let mut w = &stream;
    w.write_all(hello.trim_end().as_bytes())?;
    w.write_all(b"\n")?;
    let reader = stream.try_clone()?;
    let writer = stream.try_clone()?;
    Ok(Channel {
        reader: Box::new(reader),
        writer: Box::new(writer),
        close: Box::new(move || {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }),
    })
}

impl Drop for Endpoint {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_dir(&self.dir);
    }
}
