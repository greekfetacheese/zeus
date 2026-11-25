//! Zeus's agent edge: the loopback server an agent harness speaks MCP to.
//!
//! This crate is the only thing an agent ever talks to. It answers the protocol — both the handshake
//! era and the per-request one — authenticates every request against the profile store, and hands the
//! call to a tool. The tool asks the policy in `zeus_core::agent` whether the grant covers it, and the
//! answer is enforced again where the transaction is confirmed, so a caller cannot get round it here.
//!
//! It never draws a frame, never holds a key and never decides a permission by itself. The flows a tool
//! can start live in the binary crate, behind the [`ops::AgentOps`] port. Keeping the edge apart is what
//! lets the protocol be tested with a plain HTTP client and the domain be tested without one.

pub mod auth;
pub mod error;
pub mod limit;
pub mod ops;
pub mod probe;
pub mod protocol;
pub mod resources;
pub mod server;
pub mod tools;

/// Unix seconds, the unit every timestamp Zeus stores uses.
pub(crate) fn now_secs() -> u64 {
   std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .map(|elapsed| elapsed.as_secs())
      .unwrap_or(0)
}

pub use auth::{AuthError, Caller};
pub use error::{ErrorCode, ProtocolError, ToolError};
pub use ops::{AgentOps, AppInfo, BinScreen, NoOps};
pub use probe::Report;
pub use server::{PREFERRED_PORT, ServerConfig, ServerHandle, ServerStatus};
