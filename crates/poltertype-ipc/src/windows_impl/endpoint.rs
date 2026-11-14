//! The listening end: one pipe instance, named at random, that only this
//! user can open.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{ERROR_PIPE_BUSY, GENERIC_READ, GENERIC_WRITE, HANDLE};
use windows::Win32::Security::SECURITY_ATTRIBUTES;
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, FILE_SHARE_NONE,
    OPEN_EXISTING, PIPE_ACCESS_DUPLEX, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT,
};
use windows::Win32::System::Pipes::{
    CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
    WaitNamedPipeW,
};
use windows::core::HSTRING;

use crate::types::Channel;

use super::io;
use super::pipe::Pipe;
use super::random::random_token;
use super::security::{Descriptor, owner_only_sddl};
use super::stream::PipeStream;

/// Longest first line a client may send. A hello is a few dozen bytes.
const MAX_HELLO_BYTES: usize = 1024;

/// Kernel buffer for each direction. A request or reply is one line.
const PIPE_BUFFER: u32 = 64 * 1024;

/// How long a client waits for a pipe that is busy with somebody else.
const CLIENT_BUSY_WAIT_MS: u32 = 2000;

/// A named pipe at [`Self::path`] waiting for one client.
pub struct Endpoint {
    path: PathBuf,
    pipe: Arc<Pipe>,
}

impl Endpoint {
    /// Whether this platform has a transport at all.
    pub const SUPPORTED: bool = true;

    /// Create the pipe. `FILE_FLAG_FIRST_PIPE_INSTANCE` refuses a name
    /// somebody else created first, so nothing can be listening there
    /// already; one instance only, local clients only, and a DACL that
    /// admits this user and nobody else.
    pub fn bind() -> std::io::Result<Self> {
        let name = format!(r"\\.\pipe\poltertype-{}", random_token()?);
        let sd = Descriptor::from_sddl(&owner_only_sddl()?)?;
        let sa = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd.0.0,
            bInheritHandle: false.into(),
        };
        // SAFETY: a valid name, flags, and security attributes alive for
        // the call.
        let handle = unsafe {
            CreateNamedPipeW(
                &HSTRING::from(name.as_str()),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                PIPE_BUFFER,
                PIPE_BUFFER,
                0,
                Some(&sa),
            )
        };
        if handle.is_invalid() {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Self {
            path: PathBuf::from(name),
            pipe: Arc::new(Pipe::new(handle, true)),
        })
    }

    /// Where a client connects.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Wait until `deadline` for a client whose first line passes
    /// `check`, giving each one `hello_timeout` to send it. A client that
    /// fails is hung up on and the wait goes on. `stop` gives up early.
    pub fn accept(
        &self,
        deadline: Instant,
        hello_timeout: Duration,
        check: &dyn Fn(&str) -> bool,
        stop: &dyn Fn() -> bool,
    ) -> std::io::Result<Channel> {
        loop {
            if stop() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Interrupted,
                    "stopped",
                ));
            }
            match io::connect(self.pipe.handle(), deadline, stop) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
                    return Err(if stop() {
                        std::io::Error::new(std::io::ErrorKind::Interrupted, "stopped")
                    } else {
                        std::io::Error::new(
                            std::io::ErrorKind::TimedOut,
                            "no client proved itself in time",
                        )
                    });
                }
                Err(e) => return Err(e),
            }
            if greet(&self.pipe, hello_timeout, check) {
                return Ok(channel(Arc::clone(&self.pipe)));
            }
            self.pipe.disconnect();
        }
    }
}

/// Read the first line, a byte at a time so nothing after it is lost.
fn greet(pipe: &Pipe, hello_timeout: Duration, check: &dyn Fn(&str) -> bool) -> bool {
    let until = Instant::now() + hello_timeout;
    let keep_waiting = || Instant::now() < until;
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        match io::read(pipe.handle(), &mut byte, Some(&keep_waiting)) {
            Ok(1) if byte[0] == b'\n' => break,
            Ok(1) if line.len() < MAX_HELLO_BYTES => line.push(byte[0]),
            _ => return false,
        }
    }
    std::str::from_utf8(&line).is_ok_and(|l| check(l.trim_end_matches('\r')))
}

fn channel(pipe: Arc<Pipe>) -> Channel {
    let closer = Arc::clone(&pipe);
    Channel {
        reader: Box::new(PipeStream(Arc::clone(&pipe))),
        writer: Box::new(PipeStream(pipe)),
        close: Box::new(move || closer.shut_down()),
    }
}

/// The client's side: open the pipe at `path`, send `hello` as the first
/// line, and hand back the channel. For a provider written in Rust, and
/// for tests that play one.
///
/// Opened for identification only: a process posing as the server could
/// learn who connected, but not act as them.
pub fn connect(path: &Path, hello: &str) -> std::io::Result<Channel> {
    let name = HSTRING::from(path.as_os_str());
    let open = || {
        // SAFETY: a valid name; no template handle.
        unsafe {
            CreateFileW(
                &name,
                (GENERIC_READ | GENERIC_WRITE).0,
                FILE_SHARE_NONE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                HANDLE::default(),
            )
        }
    };
    let handle = match open() {
        Ok(h) => h,
        Err(e) if e.code() == ERROR_PIPE_BUSY.to_hresult() => {
            // SAFETY: a valid name.
            let _ = unsafe { WaitNamedPipeW(&name, CLIENT_BUSY_WAIT_MS) };
            open().map_err(|e| io::io_error(&e))?
        }
        Err(e) => return Err(io::io_error(&e)),
    };
    let pipe = Arc::new(Pipe::new(handle, false));
    let mut line = hello.trim_end().as_bytes().to_vec();
    line.push(b'\n');
    let mut sent = 0;
    while sent < line.len() {
        sent += io::write(pipe.handle(), &line[sent..])?;
    }
    Ok(channel(pipe))
}
