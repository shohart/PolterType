//! A secret for the client to prove itself with.

use windows::Win32::Security::Cryptography::{BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom};

/// 16 bytes from the system generator, as 32 hex digits.
pub fn random_token() -> std::io::Result<String> {
    let mut bytes = [0u8; 16];
    // SAFETY: no algorithm handle with the system-preferred flag, and a
    // buffer we own.
    let status = unsafe { BCryptGenRandom(None, &mut bytes, BCRYPT_USE_SYSTEM_PREFERRED_RNG) };
    if status.is_err() {
        return Err(std::io::Error::other("the system random generator failed"));
    }
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
