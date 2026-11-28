//! The Railgun syncer: the block range the indexer walks, how wide it fans out, how often the private
//! balances are refreshed, and whether proving may fetch circuits.
//!
//! Every field goes through the clamp the Settings page's sliders use, and a save then tells the
//! running app — the indexer keeps the numbers it was built with, so a config that only reached the
//! file would be a setting the app ignores until the next launch.

use super::{Applied, Closed, SettingsError, SettingsScreen, deserialize, refuse_closed};
use crate::ZeusCtx;
use crate::types::RailgunConfig;
use serde::Deserialize;
use serde_json::{Value, json};
use zeus_eth::types::ChainId;

/// One patch: the fan-out knobs at the top level, and the per-chain entries underneath.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Patch {
   syncer_concurrency: Option<usize>,
   state_update_interval_minutes: Option<u64>,
   allow_circuit_download: Option<bool>,
   #[serde(default)]
   chains: Vec<ChainPatch>,
}

/// One chain's entry: either knob, or both.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChainPatch {
   chain: u64,
   enabled: Option<bool>,
   block_range: Option<u64>,
}

const CLOSED: &[Closed] = &[Closed {
   field: "bundler_url",
   runtime: false,
   why: "the bundler URL is the unshield screen's one field, because it is the unshield it belongs to",
}];

pub(super) fn schema() -> Value {
   json!({
       "type": "object",
       "properties": {
           "syncer_concurrency": {
               "type": "integer",
               "minimum": RailgunConfig::min_concurrency(),
               "maximum": RailgunConfig::max_concurrency(),
               "description": "How many requests the indexer has in flight at once.",
           },
           "state_update_interval_minutes": {
               "type": "integer",
               "minimum": RailgunConfig::min_state_update_interval(),
               "maximum": RailgunConfig::max_state_update_interval(),
               "description": "How often the private balances are refreshed.",
           },
           "allow_circuit_download": {
               "type": "boolean",
               "description": "Whether proving may fetch circuits from the artifact host.",
           },
           "chains": {
               "type": "array",
               "description": "Per-chain settings. Railgun runs on Ethereum and Ethereum Sepolia only.",
               "items": {
                   "type": "object",
                   "properties": {
                       "chain": {"type": "integer"},
                       "enabled": {"type": "boolean"},
                       "block_range": {
                           "type": "integer",
                           "minimum": RailgunConfig::min_block_range(),
                           "maximum": RailgunConfig::max_block_range(),
                       },
                   },
                   "required": ["chain"],
                   "additionalProperties": false,
               },
           },
       },
       "additionalProperties": false,
   })
}

pub(super) fn check(value: &Value) -> Result<Patch, SettingsError> {
   refuse_closed(value, CLOSED)?;

   let patch: Patch = deserialize(value, "railgun")?;

   if let Some(concurrency) = patch.syncer_concurrency {
      let (min, max) = (
         RailgunConfig::min_concurrency(),
         RailgunConfig::max_concurrency(),
      );

      if !(min..=max).contains(&concurrency) {
         return Err(SettingsError::invalid(format!(
            "syncer_concurrency is between {min} and {max}"
         )));
      }
   }

   if let Some(minutes) = patch.state_update_interval_minutes {
      let (min, max) = (
         RailgunConfig::min_state_update_interval(),
         RailgunConfig::max_state_update_interval(),
      );

      if !(min..=max).contains(&minutes) {
         return Err(SettingsError::invalid(format!(
            "state_update_interval_minutes is between {min} and {max}"
         )));
      }
   }

   let mut named: Vec<u64> = Vec::with_capacity(patch.chains.len());

   for entry in &patch.chains {
      if entry.enabled.is_none() && entry.block_range.is_none() {
         return Err(SettingsError::invalid(format!(
            "the entry for chain {} sets neither enabled nor block_range",
            entry.chain
         )));
      }

      if named.contains(&entry.chain) {
         return Err(SettingsError::invalid(format!(
            "chain {} is named twice, so the patch does not say which wins",
            entry.chain
         )));
      }
      named.push(entry.chain);

      if let Some(range) = entry.block_range {
         let (min, max) = (
            RailgunConfig::min_block_range(),
            RailgunConfig::max_block_range(),
         );

         if !(min..=max).contains(&range) {
            return Err(SettingsError::invalid(format!(
               "block_range on chain {} is between {min} and {max}",
               entry.chain
            )));
         }
      }
   }

   Ok(patch)
}

pub(super) fn apply(ctx: &ZeusCtx, patch: Patch) -> Result<Applied, SettingsError> {
   // Every chain is checked before any is written, so a patch that names a chain Railgun cannot run on
   // changes nothing at all.
   for entry in &patch.chains {
      let Some(chain) = ChainId::new(entry.chain).ok() else {
         return Err(SettingsError::Unsupported { chain: entry.chain });
      };

      if !ctx.railgun_is_supported(chain) {
         return Err(SettingsError::invalid(format!(
            "Railgun does not run on chain {}: it runs on Ethereum and Ethereum Sepolia",
            entry.chain
         )));
      }
   }

   let mut changed = Vec::new();

   ctx.write(|context| {
      let config = &mut context.railgun_config;

      if let Some(concurrency) = patch.syncer_concurrency {
         config.rpc_syncer_concurrency = concurrency;
         changed.push("syncer_concurrency");
      }

      if let Some(minutes) = patch.state_update_interval_minutes {
         config.state_update_interval_minutes = minutes;
         changed.push("state_update_interval_minutes");
      }

      if let Some(allow) = patch.allow_circuit_download {
         config.set_allow_circuit_download(allow);
         changed.push("allow_circuit_download");
      }

      if !patch.chains.is_empty() {
         for entry in &patch.chains {
            if let Some(enabled) = entry.enabled {
               config.set_enabled(entry.chain, enabled);
            }

            if let Some(range) = entry.block_range {
               config.rpc_syncer_block_range.insert(entry.chain, range);
            }
         }

         changed.push("chains");
      }
   });

   // A prover that was already built keeps its own copy of the download opt-in, and it is the one that
   // decides whether a missing circuit may be fetched.
   let allow = ctx.read(|ctx| ctx.railgun_config.allow_circuit_download());

   ctx.read(|ctx| {
      for provider in ctx.railgun_provider.values() {
         provider.prover().set_allow_download(allow);
      }
   });

   Ok(Applied { changed })
}

fn write(ctx: &ZeusCtx, value: &Value) -> Result<Applied, SettingsError> {
   let patch = check(value)?;
   let applied = apply(ctx, patch)?;

   if applied.changed.is_empty() {
      return Ok(applied);
   }

   ctx.read(|ctx| ctx.railgun_config.clone())
      .save()
      .map_err(|e| SettingsError::refused(e.to_string()))?;

   // The indexer in a running Zeus keeps the block range and concurrency it was built with until it is
   // told; the page's own Save does the same thing. It re-registers and re-syncs, so it goes to the
   // runtime rather than onto the worker that holds this write.
   let ctx = ctx.clone();
   ctx.rt().spawn(async move { ctx.apply_railgun_config().await });

   Ok(applied)
}

pub(super) const SCREEN: SettingsScreen = SettingsScreen {
   name: "railgun",
   capability: "settings.railgun.write",
   schema,
   closed: CLOSED,
   write,
};
