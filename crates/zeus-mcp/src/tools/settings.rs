//! The settings screens, as values an agent can read.
//!
//! One generic pair rather than a tool per screen: an agent touches a screen rarely, and a screen's write
//! shape is what its own tool will carry when writes land. A screen this build cannot read is not listed,
//! so the list never promises an answer that cannot be given.

use super::{Call, ToolFuture, ToolOutput, ToolSpec, guard, read_error, string_argument};
use crate::error::{ErrorCode, ToolError};
use crate::protocol::types::Annotations;
use serde_json::Value;
use zeus_core::agent::{Intent, read, vault_open};

pub const LIST: ToolSpec = ToolSpec {
   name: "settings_list",
   title: "Settings screens",
   description: "The settings screens this build can read, each with the read and write switches \
      this profile holds for it. The screens this build cannot read are absent rather than false.",
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
