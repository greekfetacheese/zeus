//! The unshield broadcast options: of the four the page offers, exactly one is persisted.
//!
//! `self_broadcast`, `memo` and `unwrap_to_eth` describe one transfer — they are chosen when it is made,
//! so they are parameters of `railgun_unshield` and are refused here with that sentence (D21). The
//! bundler URL is the one that outlives a call: the page remembers it, and the agent that sends an
//! unshield would otherwise have to be trusted with the choice every time.

use super::{Applied, Closed, SettingsError, SettingsScreen, deserialize, refuse_closed};
use crate::ZeusCtx;
use crate::types::BundlerUrl;
use serde::Deserialize;
use serde_json::{Value, json};

/// One patch: the bundler URL, and nothing else.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Patch {
   bundler_url: Option<String>,
}

/// The broadcast choices that belong to a call rather than to the app.
const CLOSED: &[Closed] = &[
   Closed {
      field: "self_broadcast",
      runtime: true,
      why: "whether an unshield goes out from your own wallet or through the paymaster is chosen per \
             unshield: railgun_unshield takes it as a parameter",
   },
   Closed {
      field: "memo",
      runtime: true,
      why: "a memo belongs to the transfer it is attached to, so railgun_unshield takes it per call",
   },
   Closed {
      field: "unwrap_to_eth",
      runtime: true,
      why: "unwrapping WETH to ETH is a per-call choice: railgun_unshield takes it",
   },
];

pub(super) fn schema() -> Value {
   json!({
       "type": "object",
       "properties": {
           "bundler_url": {
               "type": "string",
               "description": "The bundler that submits an unshield UserOperation, e.g. \
                  https://api.example/rpc. The full URL may carry a provider key, so only its host is \
                  ever read back.",
           },
       },
       "additionalProperties": false,
   })
}

pub(super) fn check(value: &Value) -> Result<Patch, SettingsError> {
   refuse_closed(value, CLOSED)?;

   let patch: Patch = deserialize(value, "unshield")?;

   if let Some(url) = &patch.bundler_url {
      check_url(url)?;
   }

   Ok(patch)
}

/// A URL the bundler client can be pointed at.
///
/// It is a shape check, not a resolution: the client parses the URL again when it dials it, and an
/// endpoint that is down is a failure of that call rather than a reason to refuse the setting.
fn check_url(url: &str) -> Result<(), SettingsError> {
   let url = url.trim();

   if url.is_empty() {
      return Err(SettingsError::invalid(
         "bundler_url cannot be empty: pass the URL to use, or leave the field out",
      ));
   }

   if !(url.starts_with("http://") || url.starts_with("https://")) {
      return Err(SettingsError::invalid(
         "bundler_url must be an http:// or https:// URL",
      ));
   }

   if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
      return Err(SettingsError::invalid(
         "bundler_url cannot contain whitespace",
      ));
   }

   if crate::platform::url_host(url).is_none() {
      return Err(SettingsError::invalid(
         "bundler_url names no host",
      ));
   }

   Ok(())
}

/// Nothing is mutated: whoever sends an unshield reads the URL from the sealed file, so the patch's
/// whole effect is the save below.
pub(super) fn apply(_ctx: &ZeusCtx, patch: Patch) -> Result<Applied, SettingsError> {
   if patch.bundler_url.is_none() {
      return Ok(Applied::none());
   }

   Ok(Applied {
      changed: vec!["bundler_url"],
   })
}

fn write(ctx: &ZeusCtx, value: &Value) -> Result<Applied, SettingsError> {
   let patch = check(value)?;
   let url = patch.bundler_url.clone();

   let applied = apply(ctx, patch)?;

   let Some(url) = url else {
      return Ok(applied);
   };

   let url = url.trim().to_owned();

   ctx.save_bundler_url(BundlerUrl::new(url.clone()))
      .map_err(|e| SettingsError::refused(e.to_string()))?;

   // The page sends through the copy it holds, so the write hands it the same string it just sealed.
   crate::ui::set_bundler_url(url);

   Ok(applied)
}

pub(super) const SCREEN: SettingsScreen = SettingsScreen {
   name: "unshield",
   capability: "settings.unshield.write",
   schema,
   closed: CLOSED,
   write,
};
