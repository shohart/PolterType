//! A secret for the client to prove itself with.

use std::io::Read;

/// 16 bytes from the kernel's generator, as 32 hex digits.
pub fn random_token() -> std::io::Result<String> {
    let mut bytes = [0u8; 16];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
