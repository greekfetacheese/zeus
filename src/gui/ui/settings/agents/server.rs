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
   if let Some(port) = stop(server, ctx) {
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

/// Stop the listener, if it is up, and answer the port that was closed.
fn stop(server: &mut Option<ServerHandle>, ctx: &mut ZeusContext) -> Option<u16> {
   let port = running(server)?;

   // Dropping the handle is the stop: the flag it shares makes a kept-alive connection answer that
   // the server was stopped, and the task's abort closes the listener.
   *server = None;
   ctx.misc_config.set_agent_server(false);
   persist_switch(ctx);

   Some(port)
}

/// Stop the listener from anywhere: the keyboard switch D24 asks for.
///
/// A key that fires while nothing runs says so rather than nothing: a control whose only feedback is
/// a colour is useless to the person this exists for, and silence reads as a key that missed.
pub fn stop_agents(server: &mut Option<ServerHandle>, ctx: &mut ZeusContext) -> String {
   match stop(server, ctx) {
      Some(port) => format!("Agent server stopped; 127.0.0.1:{port} is closed."),
      None => "The agent server was already stopped.".to_owned(),
   }
}

/// Stop the listener **and** turn every credential off, so nothing may connect until the user says so.
///
/// The stronger of the two switches: Stop leaves a credential able to reconnect, and this one does
/// not. Nothing is revoked — the grants and the profiles are still there to start again from.
pub fn disconnect_all(
   server: &mut Option<ServerHandle>,
   ctx: &mut ZeusContext,
   handle: &ZeusCtx,
) -> String {
   let stopped = stop(server, ctx);
   let mut turned_off = 0;

   for profile in ctx.agent_profiles.profiles() {
      if profile.enabled {
         ctx.agent_profiles.set_enabled(&profile.id, false);
         turned_off += 1;
      }
   }

   if turned_off > 0 {
      super::persist_profiles(handle);
   }

   let credentials = match turned_off {
      1 => "1 profile may no longer connect".to_owned(),
      n => format!("{n} profiles may no longer connect"),
   };

   match (stopped, turned_off) {
      (Some(port), 0) => format!("Agent server stopped; 127.0.0.1:{port} is closed."),
      (Some(port), _) => format!(
         "Every agent is disconnected: the server on 127.0.0.1:{port} is closed and {credentials}."
      ),
      (None, 0) => "No agent could connect already.".to_owned(),
      (None, _) => format!("{credentials}; the server was already stopped."),
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
