//! The endpoint's lifecycle, in one place.
//!
//! The listener is not something the user turns on: Zeus opens it while it runs and there is at least
//! one credential for it to authenticate, and closes it when the last one is deleted. The frame that
//! enforces that reconciliation and the Agents page that reports it both call this, rather than
//! keeping their own copy of the rule.

use crate::core::{ZeusContext, ZeusCtx};
use std::sync::Arc;
use zeus_mcp::{ServerConfig, ServerHandle, ServerStatus};

/// Open the listener on the process runtime.
pub fn start(ctx: &ZeusCtx) -> Result<ServerHandle, String> {
   let config = ServerConfig::new(
      ctx.clone(),
      Arc::new(crate::agent_ops::ZeusOps),
      crate::runtime::rt().clone(),
   );

   ServerHandle::start(config).map_err(|e| e.to_string())
}

/// The port while the listener runs.
pub fn running(server: &Option<ServerHandle>) -> Option<u16> {
   match server.as_ref().map(ServerHandle::status) {
      Some(ServerStatus::Running { port }) => Some(port),
      _ => None,
   }
}

/// One press that leaves no agent able to talk to this wallet.
///
/// The server is always listening, so the brake is the credentials rather than the socket: every
/// profile is turned off, so the next request authenticates against nothing. Nothing is revoked —
/// the grants and the profiles are still there to switch back on.
pub fn disconnect_all(ctx: &mut ZeusContext) -> String {
   let mut turned_off = 0;

   for profile in ctx.agent_profiles.profiles() {
      if profile.enabled {
         ctx.agent_profiles.set_enabled(&profile.id, false);
         turned_off += 1;
      }
   }

   match turned_off {
      0 => "No agent could connect already.".to_owned(),
      1 => "1 profile may no longer connect.".to_owned(),
      n => format!("{n} profiles may no longer connect."),
   }
}
