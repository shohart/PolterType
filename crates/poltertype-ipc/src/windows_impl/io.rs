//! One overlapped call at a time, waited for — up to a limit, or while
//! the caller still wants to.

use std::time::{Duration, Instant};

use windows::Win32::Foundation::{
    CloseHandle, ERROR_BROKEN_PIPE, ERROR_IO_PENDING, ERROR_PIPE_CONNECTED, HANDLE, WAIT_TIMEOUT,
};
use windows::Win32::Storage::FileSystem::{ReadFile, WriteFile};
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows::Win32::System::Pipes::ConnectNamedPipe;
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

/// How often a long wait wakes to ask whether it should go on.
const POLL: Duration = Duration::from_millis(20);

pub(super) fn io_error(e: &windows::core::Error) -> std::io::Error {
    // A Win32 error arrives as HRESULT 0x8007xxxx; its low word is the
    // code `std::io::Error` knows.
    std::io::Error::from_raw_os_error(e.code().0 & 0xFFFF)
}

/// Run `start` with a fresh OVERLAPPED and wait for it: for ever when
/// `keep_waiting` is `None`, else while it says so. A wait given up on
/// is cancelled and completed before the OVERLAPPED goes away, and
/// reported as `TimedOut`.
fn overlapped(
    handle: HANDLE,
    start: impl FnOnce(*mut OVERLAPPED) -> windows::core::Result<()>,
    keep_waiting: Option<&dyn Fn() -> bool>,
) -> std::io::Result<u32> {
    // SAFETY: a manual-reset, unnamed event with default security; closed
    // below on every path.
    let event = unsafe { CreateEventW(None, true, false, None) }.map_err(|e| io_error(&e))?;
    let mut ov = OVERLAPPED {
        hEvent: event,
        ..Default::default()
    };
    let result = (|| {
        match start(&mut ov) {
            Ok(()) => {}
            Err(e) if e.code() == ERROR_IO_PENDING.to_hresult() => {}
            Err(e) if e.code() == ERROR_PIPE_CONNECTED.to_hresult() => return Ok(0),
            Err(e) => return Err(e),
        }
        if let Some(keep_waiting) = keep_waiting {
            loop {
                // SAFETY: `event` is open for the whole of this function.
                let w = unsafe { WaitForSingleObject(event, POLL.as_millis() as u32) };
                if w != WAIT_TIMEOUT {
                    break;
                }
                if !keep_waiting() {
                    // SAFETY: `ov` stays alive until the cancelled call
                    // has completed, which the wait below ensures.
                    unsafe {
                        let _ = CancelIoEx(handle, Some(&ov));
                        let mut n = 0u32;
                        let _ = GetOverlappedResult(handle, &ov, &mut n, true);
                    }
                    return Err(windows::core::Error::from_hresult(
                        windows::Win32::Foundation::ERROR_TIMEOUT.to_hresult(),
                    ));
                }
            }
        }
        let mut n = 0u32;
        // SAFETY: `ov` was handed to the call that is completing.
        unsafe { GetOverlappedResult(handle, &ov, &mut n, true) }?;
        Ok(n)
    })();
    // SAFETY: created above, not used past this point.
    unsafe {
        let _ = CloseHandle(event);
    }
    result.map_err(|e| {
        if e.code() == windows::Win32::Foundation::ERROR_TIMEOUT.to_hresult() {
            std::io::Error::new(std::io::ErrorKind::TimedOut, "timed out")
        } else {
            io_error(&e)
        }
    })
}

/// Read into `buf`. A client gone away is end of file, as on a socket.
pub(super) fn read(
    handle: HANDLE,
    buf: &mut [u8],
    keep_waiting: Option<&dyn Fn() -> bool>,
) -> std::io::Result<usize> {
    let len = buf.len();
    let ptr = buf.as_mut_ptr();
    let r = overlapped(
        handle,
        // SAFETY: `buf` outlives the call, which `overlapped` completes
        // before returning.
        |ov| unsafe {
            ReadFile(
                handle,
                Some(std::slice::from_raw_parts_mut(ptr, len)),
                None,
                Some(ov),
            )
        },
        keep_waiting,
    );
    match r {
        Ok(n) => Ok(n as usize),
        Err(e) if e.raw_os_error() == Some(ERROR_BROKEN_PIPE.0 as i32) => Ok(0),
        Err(e) => Err(e),
    }
}

pub(super) fn write(handle: HANDLE, buf: &[u8]) -> std::io::Result<usize> {
    let n = overlapped(
        handle,
        // SAFETY: as in `read`.
        |ov| unsafe { WriteFile(handle, Some(buf), None, Some(ov)) },
        None,
    )?;
    Ok(n as usize)
}

/// Wait for a client on a server pipe until `deadline`, or until `stop`.
pub(super) fn connect(
    handle: HANDLE,
    deadline: Instant,
    stop: &dyn Fn() -> bool,
) -> std::io::Result<()> {
    let keep_waiting = || !stop() && Instant::now() < deadline;
    overlapped(
        handle,
        // SAFETY: a server handle opened for overlapped I/O.
        |ov| unsafe { ConnectNamedPipe(handle, Some(ov)) },
        Some(&keep_waiting),
    )
    .map(|_| ())
}
