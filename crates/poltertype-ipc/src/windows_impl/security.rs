//! A security descriptor that lets this user, and no one else, open the
//! pipe.

use windows::Win32::Foundation::{CloseHandle, HANDLE, HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{
    GetTokenInformation, PSECURITY_DESCRIPTOR, TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::core::{HSTRING, PWSTR};

use super::io::io_error;

/// `D:P(A;;GA;;;<this user's SID>)`: a protected DACL — nothing
/// inherited — with one entry, full access for this user. Everyone else,
/// administrators and other sessions included, is denied by its absence.
///
/// The user's own SID rather than `OW` (owner rights): a process running
/// elevated may create objects owned by the Administrators group, and
/// "the owner" would then be every administrator on the machine.
pub(super) fn owner_only_sddl() -> std::io::Result<String> {
    Ok(format!("D:P(A;;GA;;;{})", current_user_sid()?))
}

/// A self-relative security descriptor parsed from `sddl`, freed on drop.
pub(super) struct Descriptor(pub(super) PSECURITY_DESCRIPTOR);

impl Descriptor {
    pub(super) fn from_sddl(sddl: &str) -> std::io::Result<Self> {
        let mut sd = PSECURITY_DESCRIPTOR::default();
        // SAFETY: a valid wide string and an out-pointer we own.
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                &HSTRING::from(sddl),
                SDDL_REVISION_1,
                &mut sd,
                None,
            )
        }
        .map_err(|e| io_error(&e))?;
        Ok(Self(sd))
    }
}

impl Drop for Descriptor {
    fn drop(&mut self) {
        // SAFETY: allocated by the conversion above with LocalAlloc.
        unsafe {
            let _ = LocalFree(HLOCAL(self.0.0));
        }
    }
}

fn current_user_sid() -> std::io::Result<String> {
    let mut token = HANDLE::default();
    // SAFETY: the pseudo-handle of this process, and an out-pointer.
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }
        .map_err(|e| io_error(&e))?;
    let result = (|| {
        let mut len = 0u32;
        // SAFETY: a size query; failing with "insufficient buffer" is
        // the expected answer.
        let _ = unsafe { GetTokenInformation(token, TokenUser, None, 0, &mut len) };
        let mut buf = vec![0u8; len as usize];
        // SAFETY: a buffer of the size just reported.
        unsafe {
            GetTokenInformation(
                token,
                TokenUser,
                Some(buf.as_mut_ptr().cast()),
                len,
                &mut len,
            )
        }
        .map_err(|e| io_error(&e))?;
        // SAFETY: the buffer now holds a TOKEN_USER, followed by the SID
        // it points into; `read_unaligned` because a Vec<u8> promises no
        // alignment.
        let user: TOKEN_USER = unsafe { std::ptr::read_unaligned(buf.as_ptr().cast()) };
        let mut text = PWSTR::null();
        // SAFETY: the SID lives in `buf`, alive for this call.
        unsafe { ConvertSidToStringSidW(user.User.Sid, &mut text) }.map_err(|e| io_error(&e))?;
        // SAFETY: a NUL-terminated string LocalAlloc'd by the call above.
        let sid = unsafe { text.to_string() }.map_err(|_| std::io::Error::other("bad SID text"));
        // SAFETY: as above; freed once, after it was copied.
        unsafe {
            let _ = LocalFree(HLOCAL(text.0.cast()));
        }
        sid
    })();
    // SAFETY: opened above.
    unsafe {
        let _ = CloseHandle(token);
    }
    result
}
