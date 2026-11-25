//! The `general` screen: the flags that change what Zeus fetches and how it behaves, all of them
//! persisted in `misc_config.json`.
//!
//! Two switches on the page are deliberately not writable here. The agent endpoint's own switch is the
//! user's in both directions — a tool that could turn the server on would bypass the master switch, and
//! one that could turn it off would be a denial of service — and the desktop-integration entry installs
//! a file outside Zeus's data folder from the GUI's own handler, so it is not a field this patch can
//! honestly set on its own. Both are named in `CLOSED` with the reason, so the refusal is the same
//! sentence the documentation would give.

use super::{Applied, Closed, SettingsError, SettingsScreen, deserialize, refuse_closed};
use crate::ZeusCtx;
use serde::Deserialize;
use serde_json::{Value, json};

/// One patch of the general screen. Every field is optional: a patch sets what it names and leaves the
/// rest exactly as it was.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Patch {
   fetch_asset_images: Option<bool>,
   fetch_contract_names: Option<bool>,
   check_for_updates: Option<bool>,
   minimize_to_tray: Option<bool>,
}

/// What this screen never lets an agent write.
const CLOSED: &[Closed] = &[
   Closed {
      field: "agent_server",
      runtime: false,
      why: "starting or stopping the agent endpoint is the app owner's decision in both directions, so no agent can reach it",
   },
   Closed {
      field: "desktop_integration",
      runtime: false,
      why: "the application-menu entry it installs lives outside Zeus's data folder, so that write arrives with the port that can reach the installer",
   },
];

pub const SCREEN: SettingsScreen = SettingsScreen {
   name: "general",
   capability: "settings.general.write",
   schema,
   closed: CLOSED,
   write,
};

fn schema() -> Value {
   json!({
       "type": "object",
       "additionalProperties": false,
       "properties": {
           "fetch_asset_images": {
               "type": "boolean",
               "persisted": true,
               "description": "Allow downloading unknown token icons and NFT images from the network.",
           },
           "fetch_contract_names": {
               "type": "boolean",
               "persisted": true,
               "description": "Allow looking unknown contract names up in Sourcify.",
           },
           "check_for_updates": {
               "type": "boolean",
               "persisted": true,
               "description": "Allow asking GitHub whether a newer Zeus exists.",
           },
           "minimize_to_tray": {
               "type": "boolean",
               "persisted": true,
               "description": "Hide to the system tray instead of the taskbar when the window is minimized.",
           },
       },
   })
}

fn write(ctx: &ZeusCtx, value: &Value) -> Result<Applied, SettingsError> {
   let patch = check(value)?;
   let applied = apply(ctx, patch)?;

   if applied.changed.is_empty() {
      return Ok(applied);
   }

   ctx.save_misc_config()
      .map_err(|error| SettingsError::refused(error.to_string()))?;

   Ok(applied)
}

pub(super) fn check(value: &Value) -> Result<Patch, SettingsError> {
   refuse_closed(value, CLOSED)?;
   deserialize(value, "general")
}

pub(super) fn apply(ctx: &ZeusCtx, patch: Patch) -> Result<Applied, SettingsError> {
   let mut changed: Vec<&'static str> = Vec::new();

   ctx.write(|ctx| {
      let config = &mut ctx.misc_config;

      if let Some(allow) = patch.fetch_asset_images {
         config.set_fetch_asset_images(allow);
         changed.push("fetch_asset_images");
      }

      if let Some(allow) = patch.fetch_contract_names {
         config.set_fetch_contract_names(allow);
         changed.push("fetch_contract_names");
      }

      if let Some(allow) = patch.check_for_updates {
         config.set_check_for_updates(allow);
         changed.push("check_for_updates");
      }

      if let Some(enabled) = patch.minimize_to_tray {
         config.set_minimize_to_tray(enabled);
         changed.push("minimize_to_tray");
      }
   });

   Ok(Applied { changed })
}
