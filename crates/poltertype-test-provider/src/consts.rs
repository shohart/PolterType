//! What the test provider reads from its environment.

/// Where PolterType listens. Same name as
/// `poltertype_core::text_provider::SOCKET_ENV`; spelled again so this
/// stands alone the way a third-party provider would.
pub(crate) const SOCKET_ENV: &str = "POLTERTYPE_PROVIDER_SOCKET";

/// The token its first line must carry.
pub(crate) const TOKEN_ENV: &str = "POLTERTYPE_PROVIDER_TOKEN";

/// Milliseconds to wait before every answer, if set.
pub(crate) const DELAY_ENV: &str = "PT_TEST_PROVIDER_DELAY_MS";
