//! The writable half of the settings screens: one validated patch per screen, and the one writer
//! that screen has.
//!
//! A read is a projection; a write is a **patch** — a struct with `deny_unknown_fields`, so a field the
//! screen does not have cannot be written even by accident, and one `write` function per screen that
//! goes through the same setter and the same clamp the GUI's own page uses. `settings_get` answers what
//! is there, `settings_describe` answers what may be sent, and this module is where the two lists are
//! allowed to differ: a read-only value is described as closed with its reason, never silently dropped.
//!
//! Three rules shape what is here:
//!
//! 1. **Only persisted state is writable.** A value that lives for the session is a per-call parameter
//!    of an action, not a setting, so it is named in `Closed` and refused with the sentence that says
//!    so rather than ignored.
//! 2. **The store's own clamps run.** A patch goes through the setter a slider goes through, so a value
//!    the app would not accept from a human is refused with the range it violates.
//! 3. **The save is the screen's own.** A screen backed by a sealed store writes through the helper that
//!    store already has; a write can never invent a second on-disk shape.
//!
//! Writes whose shape is a verb rather than a set of fields — adding a contact, adding an RPC endpoint,
//! deriving a wallet — are the tools that own them, not a patch here.

mod contacts;
mod general;
mod networks;
mod security;

#[cfg(test)]
mod tests;

use crate::ZeusCtx;
use crate::ui::{self, ConfirmRequest, PromptOpen};
use serde_json::{Value, json};
use std::fmt;
use std::time::Duration;

/// A field an agent may not write, and why.
#[derive(Clone, Copy, Debug)]
pub struct Closed {
   /// The field name, as the screen's schema would spell it.
   pub field: &'static str,
   /// True when the value is runtime state: it belongs to one call, not to the app.
   pub runtime: bool,
   /// One sentence, written to be spoken, saying what the field is and why it is out of reach.
   pub why: &'static str,
}

/// One screen an agent can write.
pub struct SettingsScreen {
   /// The name a tool takes, and the segment a resource URI carries.
   pub name: &'static str,
   /// The capability a write needs: the screen's own `.write` string from the catalog.
   pub capability: &'static str,
   /// The patch's JSON Schema, built beside the struct so the two cannot disagree about a field.
   pub schema: fn() -> Value,
   /// What this screen never lets an agent write, with the reason and whether it is runtime state.
   pub closed: &'static [Closed],
   /// Validate one patch, apply it, and persist the screen.
   pub write: fn(&ZeusCtx, &Value) -> Result<Applied, SettingsError>,
}

/// What one patch changed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Applied {
   /// The field names the patch set, in the order the screen applies them.
   pub changed: Vec<&'static str>,
}

impl Applied {
   /// A patch that named no field: nothing was written, and nothing needs persisting.
   pub fn none() -> Self {
      Self::default()
   }
}

/// Why a patch was refused.
///
/// Each variant is a different kind of "no", because the sentence a model has to say out loud differs:
/// a field the screen does not have is a typo, a runtime field belongs to a call, a closed field is a
/// permission this feature never hands out, and a store that will not write is a state the user has to
/// fix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SettingsError {
   /// The patch is not the shape this screen takes, names a field it does not have, or sets a value
   /// outside the range the app allows.
   Invalid { message: String },
   /// The field is runtime state: it is passed with the call that uses it, not written here.
   RuntimeOnly { field: String },
   /// A field no agent may write, and the sentence that says why.
   Closed {
      field: &'static str,
      why: &'static str,
   },
   /// The value names a chain this build does not support.
   Unsupported { chain: u64 },
   /// Another authorising prompt is already up: a second one would replace what the user is reading.
   Busy,
   /// The user was asked, and said no.
   Rejected,
   /// There is no Zeus window attached to ask.
   NoUi,
   /// The store refused the write: the screen's own file could not be sealed or written.
   Refused { message: String },
}

impl SettingsError {
   pub fn invalid(message: impl Into<String>) -> Self {
      Self::Invalid {
         message: message.into(),
      }
   }

   pub fn refused(message: impl Into<String>) -> Self {
      Self::Refused {
         message: message.into(),
      }
   }
}

impl fmt::Display for SettingsError {
   fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
      match self {
         Self::Invalid { message } => f.write_str(message),
         Self::RuntimeOnly { field } => {
            write!(
               f,
               "{field} is runtime-only: pass it per call rather than setting it here"
            )
         }
         Self::Closed { field, why } => write!(f, "{field} cannot be written by an agent: {why}"),
         Self::Unsupported { chain } => write!(f, "chain {chain} is not one Zeus supports"),
         Self::Busy => f.write_str("another prompt is already up in Zeus"),
         Self::Rejected => f.write_str("the user declined the write"),
         Self::NoUi => f.write_str(ui::NO_UI_MSG),
         Self::Refused { message } => write!(f, "the screen could not be saved: {message}"),
      }
   }
}

impl std::error::Error for SettingsError {}

/// Ask the user before a write, when the profile's own mode says every change is confirmed.
///
/// The window is the app's generic yes/no prompt — the one a dapp's network switch already uses — and
/// the answer is the user's alone: there is no default, and a prompt that cannot be drawn refuses the
/// write rather than assuming it. It waits, so the caller runs it off the frame path.
pub async fn confirm(title: &str) -> Result<(), SettingsError> {
   match ui::host().open_confirm(ConfirmRequest {
      title: title.to_owned(),
      msg2: None,
      from_dapp: false,
   }) {
      PromptOpen::Opened => {}
      PromptOpen::Busy => return Err(SettingsError::Busy),
      PromptOpen::Unavailable => return Err(SettingsError::NoUi),
   }

   // Held until the answer is read, however this ends: a flow that dies must not wedge the next prompt.
   let _release = Release;

   let answer = loop {
      tokio::time::sleep(Duration::from_millis(50)).await;

      if let Some(answer) = ui::host().take_confirm() {
         break answer;
      }
   };

   if answer {
      Ok(())
   } else {
      Err(SettingsError::Rejected)
   }
}

/// Releases the confirmation window when the wait ends, on the answer or on a dropped future.
struct Release;

impl Drop for Release {
   fn drop(&mut self) {
      ui::host().release_confirm();
   }
}

/// Every screen this build can write, in the order the settings page shows them.
pub const SCREENS: &[SettingsScreen] = &[
   contacts::SCREEN,
   general::SCREEN,
   networks::SCREEN,
   security::SCREEN,
];

/// The screen with that name, if this build can write it.
pub fn screen(name: &str) -> Option<&'static SettingsScreen> {
   SCREENS.iter().find(|screen| screen.name == name)
}

/// One screen's patch schema, and the fields an agent may never send.
///
/// A screen this build cannot write still answers, with `writable: false`: saying "there is no patch
/// here" is the honest answer, and it is the one that stops a model trying.
pub fn describe(name: &str) -> Value {
   let Some(screen) = screen(name) else {
      return json!({
          "screen": name,
          "writable": false,
          "schema": Value::Null,
          "closed": [],
      });
   };

   let closed: Vec<Value> = screen
      .closed
      .iter()
      .map(|entry| {
         json!({
             "field": entry.field,
             "runtime": entry.runtime,
             "why": entry.why,
         })
      })
      .collect();

   json!({
       "screen": screen.name,
       "writable": true,
       "capability": screen.capability,
       "schema": (screen.schema)(),
       "closed": closed,
   })
}

/// Refuse a patch that names a field the screen never lets an agent write.
///
/// It runs before the patch is deserialised, so the answer is the field's own reason rather than
/// serde's list of the fields it expected — the difference between a model that can explain the
/// refusal and one that has to guess at the schema.
fn refuse_closed(value: &Value, closed: &[Closed]) -> Result<(), SettingsError> {
   let Some(object) = value.as_object() else {
      return Err(SettingsError::invalid(
         "a patch must be a JSON object",
      ));
   };

   for entry in closed {
      if object.contains_key(entry.field) {
         return Err(if entry.runtime {
            SettingsError::RuntimeOnly {
               field: entry.field.to_owned(),
            }
         } else {
            SettingsError::Closed {
               field: entry.field,
               why: entry.why,
            }
         });
      }
   }

   Ok(())
}

/// The patch a screen's own `check` produces, turned into a refusal that names the tool's shape.
fn deserialize<T: serde::de::DeserializeOwned>(
   value: &Value,
   screen: &'static str,
) -> Result<T, SettingsError> {
   serde_json::from_value(value.clone()).map_err(|error| {
      SettingsError::invalid(format!(
         "the patch is not one the {screen} screen takes: {error}"
      ))
   })
}
