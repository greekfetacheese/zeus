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

use super::{
   Call, ToolFuture, ToolOutput, ToolSpec, decide, guard, read_error, record_allowed,
   record_refusal, refusal, settings_error, string_argument,
};
use crate::error::{ErrorCode, ToolError};
use crate::protocol::types::Annotations;
use serde_json::{Value, json};
use zeus_core::agent::settings::SettingsError;
use zeus_core::agent::{Decision, Intent, read, settings, vault_open};

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
   schema: r#"{"type": "object", "properties": {"screen": {"type": "string", \
      "description": "A screen name from settings_list, e.g. general, networks, security."}}, \
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
   schema: r#"{"type": "object", "properties": {"screen": {"type": "string", \
      "description": "A screen name from settings_list."}}, \
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
   schema: r#"{"type": "object", "properties": {"screen": {"type": "string", \
      "description": "A screen name from settings_list."}, "patch": {"type": "object", \
      "description": "The fields to change, exactly as settings_describe lists them."}}, \
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

      Ok(ToolOutput::new(read::screens(&call.profile)))
   })
}

fn get(call: &Call, arguments: Value) -> ToolFuture<'_> {
   Box::pin(async move {
      let screen = string_argument(&arguments, "screen")?.to_owned();

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

      Ok(ToolOutput::new(value))
   })
}

fn describe(call: &Call, arguments: Value) -> ToolFuture<'_> {
   Box::pin(async move {
      let screen = string_argument(&arguments, "screen")?.to_owned();

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

      Ok(ToolOutput::new(settings::describe(&screen)))
   })
}

fn set(call: &Call, arguments: Value) -> ToolFuture<'_> {
   Box::pin(async move {
      let screen_name = string_argument(&arguments, "screen")?.to_owned();

      let patch = arguments.get("patch").cloned().ok_or_else(|| {
         ToolError::validation("patch is required: settings_describe lists the fields it takes")
      })?;

      let screen = settings::screen(&screen_name).ok_or_else(|| {
         ToolError::new(
            ErrorCode::NotFound,
            format!("the {screen_name} screen is read-only in this build"),
         )
      })?;

      // The screen names its own switch, and the argument is what names the screen, so the decision
      // happens here. It is the same gate every other tool reaches through run().
      let intent = Intent::GlobalWrite {
         capability: screen.capability,
      };

      match decide(call, "settings_set", &intent) {
         // The profile asked to be asked about every change: in that mode the grant is not the last
         // word, the user is, and there is no default answer.
         Decision::Allowed { prompt: true } => {
            let title = format!("Let the agent write the {screen_name} settings?");

            if let Err(error) = settings::confirm(&title).await {
               let detail = error.to_string();
               let code = match &error {
                  SettingsError::Busy => "BUSY",
                  SettingsError::Rejected => "USER_REJECTED",
                  _ => "NO_UI",
               };

               record_refusal(
                  call,
                  "settings_set",
                  screen.capability,
                  code,
                  &detail,
               );

               return Err(settings_error(error));
            }
         }
         decision => refusal(&intent, decision)?,
      }

      // Every store behind a screen is a file, and none of them is written on the runtime the request
      // is being answered on.
      let ctx = call.ctx.clone();
      let applied = tokio::task::spawn_blocking(move || (screen.write)(&ctx, &patch))
         .await
         .map_err(|error| ToolError::internal(format!("the settings write did not run: {error}")))?
         .map_err(settings_error)?;

      if applied.changed.is_empty() {
         return Ok(ToolOutput::new(json!({
             "screen": screen_name,
             "changed": [],
         })));
      }

      record_allowed(call, "settings_set", screen.capability);

      // A granted write runs without a prompt, so the announcement is the part of its trace a user who
      // is not watching the screen still receives.
      zeus_core::ui::announce(&format!(
         "Agent wrote the {screen_name} settings: {}.",
         applied.changed.join(", ")
      ));

      let mut data = json!({
          "screen": screen_name,
          "changed": applied.changed,
      });

      // Reading the screen back is a read, so it needs the read switch as well as the write.
      if let Some(readable) = read::screen_read(&screen_name)
         && call.profile.allows_global(readable)
      {
         let values = call
            .ctx
            .read(|ctx| read::screen(ctx, &call.profile, &screen_name))
            .map_err(read_error)?;

         data["values"] = values["values"].clone();
      }

      Ok(ToolOutput::new(data))
   })
}
