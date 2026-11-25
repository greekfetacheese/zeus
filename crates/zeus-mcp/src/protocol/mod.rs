//! The protocol's constants, and the shape of the two eras one endpoint has to speak.
//!
//! A wallet answers two kinds of client for years: the harnesses that open with a handshake, and the
//! ones that state their revision on every request. The revision is a property of the client, never of
//! the connection — there is nothing to remember between two POSTs — so the only fixed points are the
//! version strings and the `_meta` keys below.

pub mod era;
pub mod jsonrpc;
pub mod types;

/// The revision this build speaks natively, with every obligation it names implemented.
pub const MODERN_VERSION: &str = "2026-07-28";

/// The newest handshake revision, and what a legacy client is told it negotiated.
pub const LEGACY_VERSION: &str = "2025-11-25";

/// Every handshake revision a legacy client may name without being refused.
///
/// The surface Zeus implements — tools and resources — is unchanged across them, so a client that
/// opened with an older one is answered with [`LEGACY_VERSION`] and works.
pub const LEGACY_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

/// What the server calls itself. Not a security claim: the protocol says a client must not trust it.
pub const SERVER_NAME: &str = "zeus";

/// How long a list result may be cached, in milliseconds.
///
/// The list is fixed for the life of a grant, so the number only has to be short enough that a grant
/// edit is noticed within a session, and long enough that a harness is not re-listing on every turn.
pub const LIST_TTL_MS: u64 = 300_000;

/// Where the app owner turns a capability on. Every refusal names it, because a refusal a model cannot
/// explain to a blind user is a dead end.
pub const GRANT_LOCATION: &str = "Settings → Agents";

/// What a harness is told the first time it asks what this server is.
pub const INSTRUCTIONS: &str = "Zeus is a self-custodial Ethereum wallet. Every tool acts under the \
   app owner's grant: a capability that was not granted is refused, and the refusal says where to turn \
   it on. Amounts are exact strings with their decimals, never floating-point numbers.";

/// The `_meta` keys the modern era defines, spelled once so a typo cannot split them.
pub mod meta {
   pub const PROTOCOL_VERSION: &str = "io.modelcontextprotocol/protocolVersion";
   pub const CLIENT_CAPABILITIES: &str = "io.modelcontextprotocol/clientCapabilities";
   pub const CLIENT_INFO: &str = "io.modelcontextprotocol/clientInfo";
   pub const SERVER_INFO: &str = "io.modelcontextprotocol/serverInfo";
}
