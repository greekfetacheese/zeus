//! The settings screens, as values an agent can read, describe and write.
//!
//! One generic quartet rather than a tool per screen: an agent touches a screen rarely, and a screen's
//! writable shape is what `settings_describe` answers on demand. A screen this build cannot read is not
//! listed, so the list never promises an answer that cannot be given; a screen it cannot write says so
//! both in the list and in the refusal.
//!
//! `settings_describe` and `settings_set` carry no static capability: the argument names the screen, and
//! the screen names its own switch. The decision is therefore taken inside the handler, through the same
//! gate as every other tool, so a locked vault and an ungranted screen read exactly as they do elsewhere.
//!
//! Three screens — `keyboard`, `appearance`, `bridge` — are typed in the binary, so their values, patch
//! and writer arrive through the port. They are listed, described and written exactly like the rest: the
//! gate, the record and the announcement happen here, whichever half of the app owns the type.

use super::{
   Call, ToolFuture, ToolOutput, ToolSpec, gate_write, guard, read_error, record_allowed,
   settings_error, string_argument,
};
use crate::error::{ErrorCode, ToolError};
use crate::ops::BinScreen;
use crate::protocol::types::Annotations;
use serde_json::{Value, json};
use zeus_core::agent::settings::Applied;
use zeus_core::agent::{Intent, read, settings, vault_open};
use zeus_core::ui;

pub const LIST: ToolSpec = ToolSpec {
   name: "settings_list",
   title: "Settings screens",
   description: "The settings screens this build can read, each with the read and write switches \
      this profile holds for it and whether this build can write it at all. A screen that is missing \
      cannot be read by this build; one marked not writable has no patch here.",
   schema: r#"{"type": "object", "additionalProperties": false}"#,
   annotations: Annotations::READ,
   capability: None,
   call: list,
};

pub const GET: ToolSpec = ToolSpec {
   name: "settings_get",
   title: "One settings screen",
   description: "The current values of one settings screen, in the same shape its write will take. \
      Reading a screen needs that screen's own read switch; the refusal names the switch to ask for.",
   schema: r#"{"type": "object", "properties": {"screen": {"type": "string",
      "description": "A screen name from settings_list, e.g. general, networks, security."}},
      "required": ["screen"], "additionalProperties": false}"#,
   annotations: Annotations::READ,
   capability: None,
   call: get,
};

pub const DESCRIBE: ToolSpec = ToolSpec {
   name: "settings_describe",
   title: "One settings screen's patch",
   description: "The JSON Schema of the patch one settings screen accepts, and the fields an agent \
      may not write with the reason for each. Ask for it before writing: an unknown field is refused \
      rather than ignored. Describing a screen needs that screen's read switch.",
   schema: r#"{"type": "object", "properties": {"screen": {"type": "string",
      "description": "A screen name from settings_list."}},
      "required": ["screen"], "additionalProperties": false}"#,
   annotations: Annotations::READ,
   capability: None,
   call: describe,
};

pub const SET: ToolSpec = ToolSpec {
   name: "settings_set",
   title: "Write one settings screen",
   description: "Apply a validated patch to one settings screen. Only the fields the patch names \
      change, and only fields the screen's own writer accepts: an unknown field is refused, and a \
      runtime value is refused with the sentence saying to pass it per call. A granted write runs \
      without a prompt, and is announced and recorded.",
   schema: r#"{"type": "object", "properties": {"screen": {"type": "string",
      "description": "A screen name from settings_list."}, "patch": {"type": "object",
      "description": "The fields to change, exactly as settings_describe lists them."}},
      "required": ["screen", "patch"], "additionalProperties": false}"#,
   annotations: Annotations::WRITE,
   capability: None,
   call: set,
};

fn list(call: &Call, _arguments: Value) -> ToolFuture<'_> {
   Box::pin(async move {
      // The list says what the grant holds, so it is never the way to read a wallet while locked.
      if !vault_open(&call.ctx) {
         return Err(ToolError::new(
            ErrorCode::VaultLocked,
            "Zeus is locked",
         ));
      }

      let mut value = read::screens(&call.profile);

      // The binary's own screens are listed beside the core ones, from the same row builder, so one
      // list has one shape however a screen is written.
      if let Some(rows) = value.get_mut("screens").and_then(Value::as_array_mut) {
         for screen in call.ops.bin_screens() {
            rows.push(read::screen_row(
               screen.name,
               call.profile.screen(screen.read).is_some_and(|grant| grant.read),
               call.profile.allows_global(screen.write),
               true,
            ));
         }
      }

      Ok(ToolOutput::new(value))
   })
}

fn get(call: &Call, arguments: Value) -> ToolFuture<'_> {
   Box::pin(async move {
      let screen = string_argument(&arguments, "screen")?.to_owned();

      if let Some(bin) = bin_screen(call, &screen) {
         guard(
            call,
            "settings_get",
            &Intent::GlobalRead {
               capability: bin.read,
            },
         )?;

         record_allowed(call, "settings_get", bin.read);

         return Ok(ToolOutput::new(json!({
             "screen": bin.name,
             "values": bin.values,
         }))
         .recorded());
      }

      let Some(capability) = read::screen_read(&screen) else {
         return Err(ToolError::new(
            ErrorCode::NotFound,
            format!("no screen {screen} is readable by this build"),
         ));
      };

      // Decided here rather than in the table, because the capability depends on the argument. It is the
      // same guard every other tool reaches through run(), so a locked vault refuses before anything is
      // read and an ungranted screen names its switch.
      guard(
         call,
         "settings_get",
         &Intent::GlobalRead { capability },
      )?;

      let value = call
         .ctx
         .read(|ctx| read::screen(ctx, &call.profile, &screen))
         .map_err(read_error)?;

      record_allowed(call, "settings_get", capability);

      Ok(ToolOutput::new(value).recorded())
   })
}

fn describe(call: &Call, arguments: Value) -> ToolFuture<'_> {
   Box::pin(async move {
      let screen = string_argument(&arguments, "screen")?.to_owned();

      if let Some(bin) = bin_screen(call, &screen) {
         guard(
            call,
            "settings_describe",
            &Intent::GlobalRead {
               capability: bin.read,
            },
         )?;

         record_allowed(call, "settings_describe", bin.read);

         return Ok(ToolOutput::new(settings::describe_parts(
            bin.name,
            bin.write,
            bin.schema,
            &bin.closed,
         ))
         .recorded());
      }

      let Some(capability) = read::screen_read(&screen) else {
         return Err(ToolError::new(
            ErrorCode::NotFound,
            format!("no screen {screen} is readable by this build"),
         ));
      };

      guard(
         call,
         "settings_describe",
         &Intent::GlobalRead { capability },
      )?;

      record_allowed(call, "settings_describe", capability);

      Ok(ToolOutput::new(settings::describe(&screen)).recorded())
   })
}

fn set(call: &Call, arguments: Value) -> ToolFuture<'_> {
   Box::pin(async move {
      let screen_name = string_argument(&arguments, "screen")?.to_owned();

      let patch = arguments.get("patch").cloned().ok_or_else(|| {
         ToolError::validation("patch is required: settings_describe lists the fields it takes")
      })?;

      // A screen whose writer is in the binary is written through the port; one whose writer is here is
      // written here. The gate and the answer are the same either way.
      if let Some(screen) = bin_screen(call, &screen_name) {
         return write_bin(call, screen, patch).await;
      }

      let screen = settings::screen(&screen_name).ok_or_else(|| {
         ToolError::new(
            ErrorCode::NotFound,
            format!("the {screen_name} screen is read-only in this build"),
         )
      })?;

      // The screen names its own switch, and the argument is what names the screen, so the decision
      // happens here. It is the same gate every other tool reaches through run(), and it asks the user
      // when the profile's mode says every change is a question.
      let title = format!("Let the agent write the {screen_name} settings?");

      gate_write(
         call,
         "settings_set",
         &Intent::GlobalWrite {
            capability: screen.capability,
         },
         &title,
      )
      .await?;

      // The application-menu entry is installed before the flag that records it, so a host that cannot
      // install refuses the write rather than leaving the file saying "on" with no entry beside it.
      if screen.name == "general"
         && let Some(on) = patch.get("desktop_integration").and_then(Value::as_bool)
      {
         call.ops.set_desktop_integration(on).map_err(|error| {
            ToolError::internal(format!(
               "the application-menu entry could not be changed: {error}"
            ))
            .with_hint("Nothing was stored.")
         })?;
      }

      // Every store behind a screen is a file, and none of them is written on the runtime the request
      // is being answered on.
      let ctx = call.ctx.clone();
      let applied = tokio::task::spawn_blocking(move || (screen.write)(&ctx, &patch))
         .await
         .map_err(|error| ToolError::internal(format!("the settings write did not run: {error}")))?
         .map_err(settings_error)?;

      // Reading the screen back is a read, so it needs the read switch as well as the write.
      let mut values = None;

      if let Some(readable) = read::screen_read(&screen_name)
         && call.profile.allows_global(readable)
      {
         let value = call
            .ctx
            .read(|ctx| read::screen(ctx, &call.profile, &screen_name))
            .map_err(read_error)?;

         values = Some(value["values"].clone());
      }

      Ok(landed(
         call,
         &screen_name,
         screen.capability,
         applied,
         values,
      ))
   })
}

/// Write one of the binary's own screens.
///
/// The patch is the binary's to validate, because it is the only half that can name the type it
/// describes. Everything around it is the same as for a core screen: one gate, one record, one
/// announcement, and the values read back only when the profile may read them.
async fn write_bin(call: &Call, screen: BinScreen, patch: Value) -> Result<ToolOutput, ToolError> {
   let title = format!(
      "Let the agent write the {} settings?",
      screen.name
   );

   gate_write(
      call,
      "settings_set",
      &Intent::GlobalWrite {
         capability: screen.write,
      },
      &title,
   )
   .await?;

   let ops = call.ops.clone();
   let name = screen.name;

   let applied = tokio::task::spawn_blocking(move || ops.apply_bin_screen(name, &patch))
      .await
      .map_err(|error| ToolError::internal(format!("the settings write did not run: {error}")))?
      .map_err(settings_error)?;

   let values = if call.profile.allows_global(screen.read) {
      bin_screen(call, screen.name).map(|screen| screen.values)
   } else {
      None
   };

   Ok(landed(
      call,
      screen.name,
      screen.write,
      applied,
      values,
   ))
}

/// The answer one landed write gives.
///
/// A write that changed nothing is neither recorded nor announced: nothing happened, and the ring is for
/// what this thing has been doing.
fn landed(
   call: &Call,
   screen: &str,
   capability: &str,
   applied: Applied,
   values: Option<Value>,
) -> ToolOutput {
   if applied.changed.is_empty() {
      return ToolOutput::new(json!({
          "screen": screen,
          "changed": [],
      }))
      .recorded();
   }

   record_allowed(call, "settings_set", capability);

   // A granted write runs without a prompt, so the announcement is the part of its trace a user who
   // is not watching the screen still receives.
   ui::announce(&format!(
      "Agent wrote the {screen} settings: {}.",
      applied.changed.join(", ")
   ));

   let mut data = json!({
       "screen": screen,
       "changed": applied.changed,
   });

   if let Some(values) = values {
      data["values"] = values;
   }

   ToolOutput::new(data).recorded()
}

/// The binary's record of one screen, read now.
fn bin_screen(call: &Call, name: &str) -> Option<BinScreen> {
   call.ops.bin_screens().into_iter().find(|screen| screen.name == name)
}
