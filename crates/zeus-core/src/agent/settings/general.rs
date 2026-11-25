//! The `general` screen: the flags that change what Zeus fetches and how it behaves, all of them
//! persisted in `misc_config.json`.
//!
//! One switch on the page is deliberately not writable here: the agent endpoint's own switch is the
//! user's in both directions — a tool that could turn the server on would bypass the master switch, and
//! one that could turn it off would be a denial of service. It is named in `CLOSED` with the reason, so
//! the refusal is the same sentence the documentation would give.
//!
//! `desktop_integration` is the other shape: a flag here, and an entry installed outside Zeus's data
//! folder. This patch writes the flag, and the entry is the binary's — `settings_set` reaches the
//! installer through the port **before** the write runs, so the file never says "on" with no menu entry
//! beside it while an agent, rather than the page, is the one that asked.

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
   desktop_integration: Option<bool>,
}

/// What this screen never lets an agent write.
const CLOSED: &[Closed] = &[Closed {
   field: "agent_server",
   runtime: false,
   why: "starting or stopping the agent endpoint is the app owner's decision in both directions, so no agent can reach it",
}];

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
           "desktop_integration": {
               "type": "boolean",
               "persisted": true,
               "description": "Add Zeus to the application menu. The entry is installed when this is \
                  written, so turning it on with no application menu (anything but Linux) is refused.",
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

      if let Some(enabled) = patch.desktop_integration {
         config.set_desktop_integration(enabled);
         changed.push("desktop_integration");
      }
   });

   Ok(Applied { changed })
}
