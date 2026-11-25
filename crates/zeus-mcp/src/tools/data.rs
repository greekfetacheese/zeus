//! The `data` directory: the export an agent may run, and the import only the owner can finish.
//!
//! An export is the one write whose human step happened before the call: the destination folder is fixed
//! in the grant, so a granted export runs on its own and the profile's "ask me about every change" mode
//! has nothing left to ask. An import is the opposite end of the same pipeline — the archive is a file
//! dialog and the vault's credentials are typed into Zeus — so the agent opens the page and stops there.

use super::{Call, ToolFuture, ToolOutput, ToolSpec, decide, record_allowed, refusal};
use crate::error::{ErrorCode, ToolError};
use crate::ops::Handback;
use crate::protocol::types::Annotations;
use serde_json::{Value, json};
use zeus_core::agent::{Decision, Intent};
use zeus_core::data_export::{ExportOptions, export_data_to_zip};
use zeus_core::persisted::data_dir;
use zeus_core::ui;

/// The switch both tools need: the screen's own write capability, from the catalog.
const CAPABILITY: &str = "settings.data.write";

pub const EXPORT: ToolSpec = ToolSpec {
   name: "data_export",
   title: "Export the data directory",
   description: "Write a zip of the sealed state files into the folder the grant names. The human \
      step — choosing that folder — already happened when the grant was made, so an export runs on its \
      own and the profile's ask-about-every-change mode does not apply. The answer says where the \
      archive went and how many files it holds.",
   schema: r#"{"type": "object", "properties": {"include": {"type": "array", "items":
      {"type": "string", "enum": ["railgun", "token_icons", "nft_icons"]}, "description": "Optional trees to carry on top of the core state files: the Railgun database, and the downloaded token and NFT images."}}, "additionalProperties": false}"#,
   annotations: Annotations::WRITE,
   capability: Some(CAPABILITY),
   call: export,
};

pub const IMPORT: ToolSpec = ToolSpec {
   name: "data_import",
   title: "Import a data backup",
   description: "Open the Data page, where the owner chooses an archive and types the vault's \
      credentials. An import replaces the wallets and state files, so it is always finished by the \
      human: this call opens the page and answers that the user has to act.",
   schema: r#"{"type": "object", "additionalProperties": false}"#,
   annotations: Annotations::DESTRUCTIVE,
   capability: Some(CAPABILITY),
   call: import,
};

fn export(call: &Call, arguments: Value) -> ToolFuture<'_> {
   Box::pin(async move {
      let options = options(&arguments)?;
      let intent = Intent::Export {
         capability: CAPABILITY,
      };

      match decide(call, "data_export", &intent) {
         Decision::Allowed { .. } => {}
         decision => refusal(&intent, decision)?,
      }

      let Some(folder) = call.profile.export_dir.clone() else {
         // Unreachable through the decision above, which answers NeedsUser without a folder; written out
         // rather than unwrapped so a future change to that rule cannot panic a tool.
         return Err(
            ToolError::new(
               ErrorCode::UserActionRequired,
               "an export needs a destination folder, chosen in Zeus",
            )
            .with_hint(
               "Ask the app owner to set the export folder when the grant is made, then retry.",
            ),
         );
      };

      let stamp = crate::now_secs();
      let destination = folder.join(format!("zeus-export-{stamp}.zip"));
      let target = destination.clone();

      // Packing a directory is blocking file work, and none of it runs on the runtime answering the call.
      let files = tokio::task::spawn_blocking(move || {
         let dir = data_dir().map_err(|error| error.to_string())?;

         export_data_to_zip(&dir, &target, options).map_err(|error| error.to_string())
      })
      .await
      .map_err(|error| ToolError::internal(format!("the export did not run: {error}")))?
      .map_err(|error| {
         ToolError::internal(format!("the export wrote nothing: {error}"))
            .with_hint("Nothing was written. The Data page exports by hand if this repeats.")
      })?;

      record_allowed(call, "data_export", CAPABILITY);

      // The path can carry the owner's own name, so the spoken trace says what happened, not where.
      ui::announce(&format!(
         "Agent exported the Zeus data: {files} files in a zip archive"
      ));

      Ok(ToolOutput::new(json!({
          "path": destination.display().to_string(),
          "files": files,
          "exported_at": stamp,
      })))
   })
}

fn import(call: &Call, _arguments: Value) -> ToolFuture<'_> {
   Box::pin(async move {
      let intent = Intent::NeedsHuman {
         capability: CAPABILITY,
      };

      match decide(call, "data_import", &intent) {
         Decision::NeedsUser { reason } => {
            call.ops.open_handback(Handback::DataImport).map_err(|error| {
               ToolError::internal(format!(
                  "the Data page could not be opened: {error}"
               ))
               .with_hint("Nothing was started.")
            })?;

            Err(ToolError::new(ErrorCode::UserActionRequired, reason).with_hint(
               "Zeus is showing the Data page: the archive is chosen there and the vault's credentials \
                are typed there. Ask settings_get for the data screen when it is done.",
            ))
         }
         decision => {
            refusal(&intent, decision)?;

            // A NeedsHuman intent the grant covers answers NeedsUser; anything else is a denial, which
            // the line above has already returned.
            Err(ToolError::internal(
               "the import was allowed without the human step it needs",
            ))
         }
      }
   })
}

/// The optional trees an export carries on top of the core state files.
fn options(arguments: &Value) -> Result<ExportOptions, ToolError> {
   let mut options = ExportOptions::default();

   let Some(include) = arguments.get("include") else {
      return Ok(options);
   };

   if include.is_null() {
      return Ok(options);
   }

   let Some(list) = include.as_array() else {
      return Err(ToolError::validation(
         "include must be an array of tree names: railgun, token_icons, nft_icons",
      ));
   };

   for entry in list {
      let Some(name) = entry.as_str() else {
         return Err(ToolError::validation(
            "include holds tree names, not other values",
         ));
      };

      match name {
         "railgun" => options.railgun = true,
         "token_icons" => options.token_icons = true,
         "nft_icons" => options.nft_icons = true,
         other => {
            return Err(ToolError::validation(format!(
               "{other} is not one of railgun, token_icons, nft_icons"
            )));
         }
      }
   }

   Ok(options)
}
