//! The endpoint's lifecycle, in one place.
//!
//! The page, the status bar and the frame that enforces the stored switch all have to start and
//! stop the same listener the same way, so they call this rather than three copies of it.

use crate::core::{ZeusContext, ZeusCtx};
use std::sync::Arc;
use zeus_mcp::{ServerConfig, ServerHandle, ServerStatus};

/// Start the listener on the process runtime.
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

/// The one Start/Stop control's action, so the page and the status bar cannot disagree.
///
/// Returns the sentence the app announces: a control whose only feedback is a colour is useless to
/// the person this exists for (D24).
pub fn toggle(
   server: &mut Option<ServerHandle>,
   ctx: &mut ZeusContext,
   handle: &ZeusCtx,
) -> String {
   if let Some(port) = running(server) {
      // Dropping the handle is the stop: the flag it shares makes a kept-alive connection answer
      // that the server was stopped, and the task's abort closes the listener.
      *server = None;
      ctx.misc_config.set_agent_server(false);
      persist_switch(ctx);
      return format!("Agent server stopped; 127.0.0.1:{port} is closed.");
   }

   if ctx.agent_profiles.profiles().is_empty() {
      return "Create a profile first: an endpoint with no credential accepts no one.".to_owned();
   }

   match start(handle) {
      Ok(started) => {
         let port = started.port();
         *server = Some(started);
         ctx.misc_config.set_agent_server(true);
         persist_switch(ctx);
         format!("Agent server listening on 127.0.0.1:{port}/mcp.")
      }
      Err(e) => format!("Could not start the agent server: {e}"),
   }
}

/// Persist the switch beside the other app-level ones.
pub fn persist_switch(ctx: &ZeusContext) {
   let config = ctx.misc_config.clone();

   crate::runtime::rt().spawn_blocking(move || {
      if let Err(e) = config.save() {
         tracing::error!("Could not save the agent server switch: {e:?}");
      }
   });
}
