//! The binary's half of the agent edge.
//!
//! The crate that speaks MCP cannot call the wallet flows: they are private functions here, and lifting
//! them into a crate would be a refactor of the money path. So the binary installs this, the same way the
//! GUI installs a `UiHost` for the opposite direction, and the edge reaches a flow only through it.

use zeus_mcp::{AgentOps, AppInfo};

/// Zeus's own answer to the edge's port.
pub struct ZeusOps;

impl AgentOps for ZeusOps {
   fn app_info(&self) -> AppInfo {
      AppInfo {
         version: env!("CARGO_PKG_VERSION").to_owned(),
      }
   }
}
