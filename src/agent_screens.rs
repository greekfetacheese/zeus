//! The three settings screens whose types live in the binary, as the agent port's answer.
//!
//! `keyboard`, `appearance` and `bridge` are typed in the GUI's own modules, which the edge crate cannot
//! name. Their patch structs, schemas, values and writers are therefore here, beside the types they
//! write, and the edge reaches them through `AgentOps`. Nothing about *who may* write them is decided
//! here: the capability, the tier, the audit and the announcement stay in core and in the edge.
//!
//! Every `apply` follows the three steps the core screens do: deserialise a patch that denies unknown
//! fields, mutate a **copy** through the same setters and clamps the page uses, and only then commit and
//! save — so a patch refused halfway leaves the app, and the file, exactly as they were.

use crate::core::agent::settings::{Applied, Closed, SettingsError};
use crate::core::platform;
use crate::gui::SHARED_GUI;
use crate::gui::appearance::{AmbientKind, AppearanceSettings};
use crate::gui::ui::dapps::across::AcrossBridge;
use crate::gui::ui::shortcuts::{Action, Binding, KeyBindings};
use egui_elements::{HoldSettings, LightSettings, LightningSettings, SnowSettings};
use serde::Deserialize;
use serde_json::{Value, json};
use zeus_mcp::BinScreen;

/// The three screens, in the order `settings_list` lists them, with their values read now.
pub fn screens() -> Vec<BinScreen> {
   SHARED_GUI.read(|gui| {
      vec![
         appearance(&gui.appearance_settings),
         bridge(&gui.across_bridge),
         keyboard(&gui.keymap),
      ]
   })
}

/// Apply one patch to one of them.
///
/// The copy-then-commit shape is what keeps a refused entry from leaving half a patch applied; the
/// repaint is what makes a write an agent made show up in the window the user is looking at.
pub fn apply(name: &str, patch: &Value) -> Result<Applied, SettingsError> {
   SHARED_GUI.write(|gui| {
      let applied = match name {
         "appearance" => apply_appearance(&mut gui.appearance_settings, patch),
         "bridge" => apply_bridge(&mut gui.across_bridge, patch),
         "keyboard" => apply_keyboard(&mut gui.keymap, patch),
         other => Err(SettingsError::refused(format!(
            "the {other} screen is not written by the binary"
         ))),
      };

      if let Ok(applied) = &applied
         && !applied.changed.is_empty()
      {
         gui.request_repaint();
      }

      applied
   })
}

// --- appearance ---------------------------------------------------------------------------------

/// Nothing on this screen is out of reach: the persist it writes is a preference, and every value
/// passes the clamp the page's own sliders use.
const APPEARANCE_CLOSED: &[Closed] = &[];

fn appearance(settings: &AppearanceSettings) -> BinScreen {
   BinScreen {
      name: "appearance",
      read: "settings.appearance.read",
      write: "settings.appearance.write",
      values: appearance_values(settings),
      schema: appearance_schema(),
      closed: APPEARANCE_CLOSED.to_vec(),
   }
}

/// The file's own fields: the transient window flags a page drags around are not part of it.
fn appearance_values(settings: &AppearanceSettings) -> Value {
   let settings = settings.persisted();

   json!({
       "hold_to_confirm": settings.hold_to_confirm,
       "shift_second_colour": settings.shift_second_colour,
       "second_colour": settings.second_colour,
       "hold": settings.hold,
       "ambient_enabled": settings.ambient_enabled,
       "ambient_kind": settings.ambient_kind,
       "snow": settings.snow,
       "light": settings.light,
       "lightning": settings.lightning,
   })
}

fn appearance_schema() -> Value {
   json!({
       "type": "object",
       "additionalProperties": false,
       "properties": {
           "hold_to_confirm": {
               "type": "boolean",
               "persisted": true,
               "description": "Require a hold on Confirm and Sign. Off by default.",
           },
           "shift_second_colour": {
               "type": "boolean",
               "persisted": true,
               "description": "Shift the hold's sweep to a second colour as it completes.",
           },
           "second_colour": {
               "persisted": true,
               "description": "Four bytes (red, green, blue, alpha) for the second colour, or null \
                  to fall back to the theme accent at 80 alpha.",
               "anyOf": [
                   {
                       "type": "array",
                       "items": {"type": "integer", "minimum": 0, "maximum": 255},
                       "minItems": 4,
                       "maxItems": 4,
                   },
                   {"type": "null"},
               ],
           },
           "hold": {
               "type": "object",
               "persisted": true,
               "additionalProperties": true,
               "description": "The hold sweep and the completion burst, in the crate's own settings \
                  shape. Every value is clamped to the range the page's slider offers, so an \
                  out-of-range number is corrected rather than refused.",
           },
           "ambient_enabled": {
               "type": "boolean",
               "persisted": true,
               "description": "Paint the app-wide ambient particles.",
           },
           "ambient_kind": {
               "type": "string",
               "enum": ["Snow", "Glow", "Lightning"],
               "persisted": true,
               "description": "Which ambient effect is painted. Exactly one kind is active at a time, \
                  and only that kind's knobs are used.",
           },
           "snow": {
               "type": "object",
               "persisted": true,
               "additionalProperties": true,
               "description": "The crate's snow knobs. Clamped, like every other value on this screen.",
           },
           "light": {
               "type": "object",
               "persisted": true,
               "additionalProperties": true,
               "description": "The crate's glow knobs. Clamped, like every other value on this screen.",
           },
           "lightning": {
               "type": "object",
               "persisted": true,
               "additionalProperties": true,
               "description": "The crate's lightning knobs. Clamped, like every other value on this \
                  screen.",
           },
       },
   })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AppearancePatch {
   hold_to_confirm: Option<bool>,
   shift_second_colour: Option<bool>,
   /// Named as a value rather than a colour, so `null` can mean "back to the theme accent" instead of
   /// "not mentioned".
   second_colour: Option<Value>,
   hold: Option<HoldSettings>,
   ambient_enabled: Option<bool>,
   ambient_kind: Option<AmbientKind>,
   snow: Option<SnowSettings>,
   light: Option<LightSettings>,
   lightning: Option<LightningSettings>,
}

fn apply_appearance(
   settings: &mut AppearanceSettings,
   value: &Value,
) -> Result<Applied, SettingsError> {
   let patch: AppearancePatch = serde_json::from_value(value.clone()).map_err(|error| {
      SettingsError::invalid(format!(
         "the patch is not one the appearance screen takes: {error}"
      ))
   })?;

   let mut next = settings.clone();
   let mut changed: Vec<&'static str> = Vec::new();

   if let Some(enabled) = patch.hold_to_confirm {
      next.hold_to_confirm = enabled;
      note(&mut changed, "hold_to_confirm");
   }

   if let Some(enabled) = patch.shift_second_colour {
      next.shift_second_colour = enabled;
      note(&mut changed, "shift_second_colour");
   }

   if let Some(colour) = patch.second_colour {
      next.second_colour = match colour {
         Value::Null => None,
         colour => Some(serde_json::from_value(colour).map_err(|error| {
            SettingsError::invalid(format!("second_colour is not a colour: {error}"))
         })?),
      };

      note(&mut changed, "second_colour");
   }

   if let Some(hold) = patch.hold {
      next.hold = hold;
      note(&mut changed, "hold");
   }

   if let Some(enabled) = patch.ambient_enabled {
      next.ambient_enabled = enabled;
      note(&mut changed, "ambient_enabled");
   }

   if let Some(kind) = patch.ambient_kind {
      next.ambient_kind = kind;
      note(&mut changed, "ambient_kind");
   }

   if let Some(snow) = patch.snow {
      next.snow = snow;
      note(&mut changed, "snow");
   }

   if let Some(light) = patch.light {
      next.light = light;
      note(&mut changed, "light");
   }

   if let Some(lightning) = patch.lightning {
      next.lightning = lightning;
      note(&mut changed, "lightning");
   }

   if changed.is_empty() {
      return Ok(Applied::none());
   }

   // The same clamp the page runs on a slider, on load and before a save. It runs before the write, so
   // the value the answer names is the value the file now holds.
   next.clamp();
   next.save().map_err(|error| SettingsError::refused(error.to_string()))?;

   *settings = next;

   Ok(Applied { changed })
}

// --- bridge -------------------------------------------------------------------------------------

/// The two fields an agent may not write are the two that spend money or speak for the user.
const BRIDGE_CLOSED: &[Closed] = &[
   Closed {
      field: "fee_to_pay",
      runtime: false,
      why: "it is the share of a bridge the relayer keeps, set on the page's own slider: a write that              could raise it spends more of the user's money without changing what they see",
   },
   Closed {
      field: "api_notice_shown",
      runtime: false,
      why: "it records that the user has read the API notice, and an agent cannot read it for them",
   },
];

fn bridge(page: &AcrossBridge) -> BinScreen {
   let url = page.agent_api_url();

   BinScreen {
      name: "bridge",
      read: "settings.bridge.read",
      write: "settings.bridge.write",
      values: json!({
          "api": {
              "host": platform::url_host(url),
              "default": url == AcrossBridge::agent_default_api_url(),
              "use_api": page.agent_use_api(),
          },
      }),
      schema: bridge_schema(),
      closed: BRIDGE_CLOSED.to_vec(),
   }
}

fn bridge_schema() -> Value {
   json!({
       "type": "object",
       "additionalProperties": false,
       "properties": {
           "api_url": {
               "type": "string",
               "persisted": true,
               "description": "The Across API endpoint, e.g. https://across.to/api. The full URL may \
                  carry a key, so only its host is ever read back.",
           },
           "use_api": {
               "type": "boolean",
               "persisted": true,
               "description": "Ask the API for a suggested fee. Turned off, Zeus pays the fee the \
                  page's slider holds, which this screen does not let an agent set.",
           },
       },
   })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BridgePatch {
   api_url: Option<String>,
   use_api: Option<bool>,
}

fn apply_bridge(page: &mut AcrossBridge, value: &Value) -> Result<Applied, SettingsError> {
   let patch: BridgePatch = serde_json::from_value(value.clone()).map_err(|error| {
      SettingsError::invalid(format!(
         "the patch is not one the bridge screen takes: {error}"
      ))
   })?;

   let api_url = match patch.api_url {
      Some(url) => {
         let url = url.trim();
         platform::check_url("api_url", url).map_err(SettingsError::invalid)?;

         Some(url.to_owned())
      }
      None => None,
   };

   let changed = page
      .apply_agent_settings(api_url, patch.use_api)
      .map_err(|error| SettingsError::refused(error.to_string()))?;

   Ok(Applied { changed })
}

// --- keyboard -----------------------------------------------------------------------------------

/// Every action the app answers is a row of this screen, and every row is writable: what an agent may
/// not do is take a key the app needs, and the bindings refuse that themselves.
const KEYBOARD_CLOSED: &[Closed] = &[];

fn keyboard(bindings: &KeyBindings) -> BinScreen {
   let rows: Vec<Value> = Action::ALL
      .iter()
      .map(|action| {
         let binding = bindings.binding(*action);

         json!({
             "action": action.name(),
             "label": action.label(),
             "description": action.description(),
             "binding": binding.label(),
             "key": binding.key.name(),
             "chord": binding.chord.name(),
             "default": bindings.is_default(*action),
         })
      })
      .collect();

   BinScreen {
      name: "keyboard",
      read: "settings.keyboard.read",
      write: "settings.keyboard.write",
      values: json!({"bindings": rows}),
      schema: keyboard_schema(),
      closed: KEYBOARD_CLOSED.to_vec(),
   }
}

fn keyboard_schema() -> Value {
   json!({
       "type": "object",
       "additionalProperties": false,
       "properties": {
           "bindings": {
               "type": "array",
               "persisted": true,
               "description": "Chords to give an action. Each names the action exactly as \
                  settings_get shows it, the key by the name the saved file uses (K, F6, Tab) and the \
                  chord by its own name (plain, shift, alt, ctrl, ctrl_shift, command, command_shift). \
                  A bare or shifted letter is refused: it would be typed instead of running the action.",
               "items": {
                   "type": "object",
                   "additionalProperties": false,
                   "properties": {
                       "action": {"type": "string"},
                       "key": {"type": "string"},
                       "chord": {"type": "string"},
                   },
                   "required": ["action", "key", "chord"],
               },
           },
           "reset": {
               "type": "array",
               "persisted": true,
               "description": "Actions to put back on the chord they shipped with.",
               "items": {"type": "string"},
           },
       },
   })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct KeyboardPatch {
   bindings: Option<Vec<KeyboardBinding>>,
   reset: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct KeyboardBinding {
   action: String,
   key: String,
   chord: String,
}

fn apply_keyboard(bindings: &mut KeyBindings, value: &Value) -> Result<Applied, SettingsError> {
   let patch: KeyboardPatch = serde_json::from_value(value.clone()).map_err(|error| {
      SettingsError::invalid(format!(
         "the patch is not one the keyboard screen takes: {error}"
      ))
   })?;

   let mut next = bindings.clone();
   let mut changed: Vec<&'static str> = Vec::new();

   for entry in patch.bindings.unwrap_or_default() {
      let action = Action::from_name(&entry.action).ok_or_else(|| {
         SettingsError::invalid(format!(
            "no shortcut action is called {}",
            entry.action
         ))
      })?;

      let binding = Binding::from_parts(&entry.key, &entry.chord).ok_or_else(|| {
         SettingsError::invalid(format!(
            "{} with {} is not a chord this build can hold: pass a key name the saved file uses (K,              F6, Tab) and a chord name (plain, shift, alt, ctrl, ctrl_shift, command, command_shift)",
            entry.key, entry.chord
         ))
      })?;

      next
         .set(action, binding)
         .map_err(|refusal| SettingsError::invalid(refusal.words(binding)))?;
      note(&mut changed, "bindings");
   }

   for name in patch.reset.unwrap_or_default() {
      let action = Action::from_name(&name)
         .ok_or_else(|| SettingsError::invalid(format!("no shortcut action is called {name}")))?;

      next.reset(action);
      note(&mut changed, "reset");
   }

   if changed.is_empty() {
      return Ok(Applied::none());
   }

   // Written here rather than on a worker, which is what the Keyboard page already does: the file is a
   // few hundred bytes, and a write that cannot be made has to be able to say so.
   next.save().map_err(|error| SettingsError::refused(error.to_string()))?;

   *bindings = next;

   Ok(Applied { changed })
}

/// Name a field once, however many entries of it the patch carried.
fn note(changed: &mut Vec<&'static str>, field: &'static str) {
   if !changed.contains(&field) {
      changed.push(field);
   }
}
