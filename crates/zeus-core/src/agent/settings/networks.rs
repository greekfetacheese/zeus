//! The `networks` screen: the request fan-out, and which chains are switched off.
//!
//! Endpoints are not a field of this patch. Adding, removing or switching one is a verb with its own
//! arguments and its own audit line: half a list is not something a patch can honestly express, and a
//! write that replaced the list would need the caller to send back everything it read.
//!
//! A chain this build does not support is refused before any of the patch is applied, so a patch cannot
//! land in half.

use super::{Applied, Closed, SettingsError, SettingsScreen, deserialize, refuse_closed};
use crate::ZeusCtx;
use crate::context::client::{MAX_CONCURRENCY, MIN_CONCURRENCY};
use serde::Deserialize;
use serde_json::{Value, json};
use zeus_eth::types::ChainId;

/// One patch of the networks screen.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Patch {
   concurrency: Option<u64>,
   enable_chains: Option<Vec<u64>>,
   disable_chains: Option<Vec<u64>>,
}

const CLOSED: &[Closed] = &[Closed {
   field: "rpcs",
   runtime: false,
   why: "an endpoint is added, removed or switched with its own tool, not by replacing a list",
}];

pub const SCREEN: SettingsScreen = SettingsScreen {
   name: "networks",
   capability: "settings.networks.write",
   schema,
   closed: CLOSED,
   write,
};

fn schema() -> Value {
   json!({
       "type": "object",
       "additionalProperties": false,
       "properties": {
           "concurrency": {
               "type": "integer",
               "persisted": true,
               "minimum": MIN_CONCURRENCY,
               "maximum": MAX_CONCURRENCY,
               "description": "How many batch and prefetch requests the client fans out at once.",
           },
           "enable_chains": {
               "type": "array",
               "persisted": true,
               "items": {"type": "integer"},
               "description": "Chain ids to switch back on.",
           },
           "disable_chains": {
               "type": "array",
               "persisted": true,
               "items": {"type": "integer"},
               "description": "Chain ids to switch off. Applied after enable_chains, so a chain named in both ends off.",
           },
       },
   })
}

fn write(ctx: &ZeusCtx, value: &Value) -> Result<Applied, SettingsError> {
   let patch = check(value)?;
   let applied = apply(ctx, patch)?;

   if applied.changed.contains(&"concurrency") {
      ctx.get_client_manager()
         .save_settings()
         .map_err(|error| SettingsError::refused(error.to_string()))?;
   }

   if applied.changed.iter().any(|field| *field != "concurrency") {
      ctx.try_save_disabled_chains()
         .map_err(|error| SettingsError::refused(error.to_string()))?;
   }

   Ok(applied)
}

pub(super) fn check(value: &Value) -> Result<Patch, SettingsError> {
   refuse_closed(value, CLOSED)?;
   deserialize(value, "networks")
}

pub(super) fn apply(ctx: &ZeusCtx, patch: Patch) -> Result<Applied, SettingsError> {
   let enable = patch.enable_chains.unwrap_or_default();
   let disable = patch.disable_chains.unwrap_or_default();

   // Every chain is checked before any of them is written, so a bad id in the middle cannot leave the
   // patch half applied.
   for chain in enable.iter().chain(disable.iter()) {
      if !ChainId::is_supported(*chain) {
         return Err(SettingsError::Unsupported { chain: *chain });
      }
   }

   let mut changed: Vec<&'static str> = Vec::new();

   if let Some(concurrency) = patch.concurrency {
      let concurrency = usize::try_from(concurrency).unwrap_or(usize::MAX);

      if !(MIN_CONCURRENCY..=MAX_CONCURRENCY).contains(&concurrency) {
         return Err(SettingsError::invalid(format!(
            "concurrency must be between {MIN_CONCURRENCY} and {MAX_CONCURRENCY}, got {concurrency}"
         )));
      }

      ctx.get_client_manager().set_concurrency(concurrency);
      changed.push("concurrency");
   }

   if !enable.is_empty() {
      ctx.write(|ctx| {
         for chain in &enable {
            ctx.disabled_chains.enable(*chain);
         }
      });
      changed.push("enable_chains");
   }

   if !disable.is_empty() {
      ctx.write(|ctx| {
         for chain in &disable {
            ctx.disabled_chains.disable(*chain);
         }
      });
      changed.push("disable_chains");
   }

   Ok(Applied { changed })
}
