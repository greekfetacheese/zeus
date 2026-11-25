//! The port the binary implements.
//!
//! The flows an agent must be able to start — a send, a swap, a bridge, a shield, an approval revoke —
//! are private functions in the binary crate, which a library cannot call. Rather than move the money
//! path into a crate, the binary installs an implementation of this trait, the same way the GUI
//! installs a [`zeus_core::ui::UiHost`] for the opposite direction.
//!
//! Three settings screens are here for the same reason at a smaller scale. `keyboard`, `appearance` and
//! `bridge` are typed in the binary's own modules (`KeyBindings`, `AppearanceSettings`, the bridge
//! preferences), so their patch, their values and their writer cannot live in a crate either. What stays
//! in core is everything that decides *whether* a write happens: the capability, the tier and the audit.

use serde_json::Value;
use zeus_core::agent::settings::{Applied, Closed, SettingsError};

/// What the edge cannot know about the process it lives in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AppInfo {
   /// The build's own version, as the about screen shows it.
   pub version: String,
}

/// One settings screen whose writer lives in the binary.
///
/// The record is built when it is asked for, never cached at install: `settings_list`,
/// `settings_get` and `settings_describe` each read afresh, so a screen is not described from something
/// the app has since changed.
pub struct BinScreen {
   /// The name a tool takes, and the segment a resource URI carries.
   pub name: &'static str,
   /// The capability a read needs.
   pub read: &'static str,
   /// The capability a write needs.
   pub write: &'static str,
   /// The screen's current values, in the same shape its patch takes.
   pub values: Value,
   /// The patch's JSON Schema, built beside the struct that validates it.
   pub schema: Value,
   /// The fields an agent may never write, with the reason for each.
   pub closed: Vec<Closed>,
}

/// Everything the binary has to supply.
///
/// A method that starts a flow returns a boxed future: `async fn` in a trait is not dyn-compatible, and
/// this trait is held as `Arc<dyn AgentOps>`. The methods here answer from memory, so they are plain.
pub trait AgentOps: Send + Sync + 'static {
   /// The running Zeus build.
   fn app_info(&self) -> AppInfo;

   /// The settings screens this binary writes, with their values read now.
   ///
   /// Empty when nothing is installed, which is the honest answer for a build with no app around it: a
   /// screen that is not here is neither listed nor writable.
   fn bin_screens(&self) -> Vec<BinScreen> {
      Vec::new()
   }

   /// Apply one patch to a screen [`Self::bin_screens`] reported, and say what changed.
   fn apply_bin_screen(&self, name: &str, patch: &Value) -> Result<Applied, SettingsError> {
      let _ = patch;

      Err(SettingsError::refused(format!(
         "the {name} screen is not written by this build"
      )))
   }

   /// Install or remove the application-menu entry a `general` write asks for.
   ///
   /// It is here rather than behind the settings writer because the installer is the binary's: a write
   /// that recorded the flag without it would leave the file saying "on" with no entry beside it.
   fn set_desktop_integration(&self, on: bool) -> Result<(), String> {
      let _ = on;

      Err("this build cannot change the application menu".to_owned())
   }
}

/// A double for tests and for a binary that has not installed anything yet.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoOps;

impl AgentOps for NoOps {
   fn app_info(&self) -> AppInfo {
      AppInfo {
         version: "0.0.0-test".to_owned(),
      }
   }
}
