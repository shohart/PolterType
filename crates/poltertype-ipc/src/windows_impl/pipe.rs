//! `Pipe` — one pipe handle, shared by a reader, a writer and a closer.

use std::sync::atomic::{AtomicBool, Ordering};

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::IO::CancelIoEx;
use windows::Win32::System::Pipes::DisconnectNamedPipe;

/// A pipe handle opened for overlapped I/O. Closed on drop.
///
/// Overlapped on purpose: with synchronous I/O Windows serialises calls
/// on one file object, so a read blocked waiting for the provider would
/// hold every write up behind it.
pub(super) struct Pipe {
    handle: HANDLE,
    /// This end is the server's, which may disconnect the client.
    server: bool,
    closed: AtomicBool,
}

// SAFETY: a HANDLE is an opaque kernel object reference; the kernel
// synchronises concurrent overlapped calls on it, and each call here
// carries its own OVERLAPPED and event.
unsafe impl Send for Pipe {}
// SAFETY: as above — no state behind the handle lives in this process.
unsafe impl Sync for Pipe {}

impl Pipe {
    pub(super) fn new(handle: HANDLE, server: bool) -> Self {
        Self {
            handle,
            server,
            closed: AtomicBool::new(false),
        }
    }

    pub(super) fn handle(&self) -> HANDLE {
        self.handle
    }

    pub(super) fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    /// Cancel every pending call on the handle — waking a blocked reader
    /// — and, on the server's end, hang up on the client.
    pub(super) fn shut_down(&self) {
        self.closed.store(true, Ordering::Release);
        // SAFETY: the handle is open until drop; cancelling I/O that is
        // not pending, or disconnecting a pipe with no client, fails
        // harmlessly.
        unsafe {
            let _ = CancelIoEx(self.handle, None);
            if self.server {
                let _ = DisconnectNamedPipe(self.handle);
            }
        }
    }

    /// Hang up on a client that failed its hello, so the same pipe
    /// instance can wait for the next one.
    pub(super) fn disconnect(&self) {
        // SAFETY: as in `shut_down`.
        unsafe {
            let _ = DisconnectNamedPipe(self.handle);
        }
    }
}

impl Drop for Pipe {
    fn drop(&mut self) {
        // SAFETY: the last reference is going; nothing else will use the
        // handle again.
        unsafe {
            let _ = CancelIoEx(self.handle, None);
            let _ = CloseHandle(self.handle);
        }
    }
}
