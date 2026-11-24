//! The table the keyboard is read from: what the app can be asked to do, and the chord that asks.
//!
//! One table feeds three readers — the handler that takes the key, the `F1` sheet that lists the
//! keys, and the command palette that runs an action by name — so a chord that is described is a
//! chord that is wired, and a key that is remapped is remapped in all three places at once.
//!
//! What a user changes is an *override* on top of [`Action::default_binding`]. [`KeyBindings`] keeps
//! those, refuses the chords that would take a key away from typing or from another action, and
//! saves them beside the appearance settings — the same unencrypted `data/` file, for the same
//! reason: it is a preference, not wallet data.

use crate::core::context::appearance_settings_dir;
use crate::core::persisted::ensure_data_dir;
use crate::utils::write_private;
use egui::{Key, Modifiers};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// The name the saved file has, beside `appearance_settings.json`.
const SETTINGS_FILE: &str = "shortcut_settings.json";

/// A modifier set a chord is pressed with.
///
/// A closed set rather than raw modifiers: a binding has to be recognisable when the key arrives,
/// and egui hands the modifiers over as independent flags. [`Self::Command`] is the platform's
/// command key, so a default chord follows the platform the way egui's own text shortcuts do;
/// [`Self::Ctrl`] is the control key itself on every platform.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Chord {
   Plain,
   Shift,
   Alt,
   /// The control key itself, on every platform: `Ctrl+Tab` is not `Command+Tab` on a Mac.
   Ctrl,
   CtrlShift,
   Command,
   CommandShift,
}

impl Chord {
   /// Every chord a binding can use, in the order a press is matched against them.
   pub const ALL: [Self; 7] = [
      Self::Plain,
      Self::Shift,
      Self::Alt,
      Self::Ctrl,
      Self::CtrlShift,
      Self::Command,
      Self::CommandShift,
   ];

   /// Whether a press was made with exactly this chord.
   pub fn matches(self, modifiers: &Modifiers) -> bool {
      match self {
         Self::Plain => !modifiers.alt && !modifiers.command && !modifiers.ctrl && !modifiers.shift,
         Self::Shift => modifiers.shift && !modifiers.alt && !modifiers.command && !modifiers.ctrl,
         Self::Alt => modifiers.alt && !modifiers.command && !modifiers.ctrl,
         Self::Ctrl => modifiers.ctrl && !modifiers.alt && !modifiers.shift,
         Self::CtrlShift => modifiers.ctrl && modifiers.shift && !modifiers.alt,
         Self::Command => modifiers.command && !modifiers.alt && !modifiers.shift,
         Self::CommandShift => modifiers.command && modifiers.shift && !modifiers.alt,
      }
   }

   /// The chord a press was made with, or `None` for a combination no binding can hold (two
   /// modifier families at once: `Ctrl+Alt`, `Command+Alt`).
   pub fn from_modifiers(modifiers: &Modifiers) -> Option<Self> {
      Self::ALL.into_iter().find(|chord| chord.matches(modifiers))
   }

   /// The chord as it is written in front of a key: `Ctrl+Shift+` for a chord that needs both.
   ///
   /// The command key is written the way the platform writes it, which is also what makes the sheet
   /// right without a sentence about Macs on every row.
   pub fn prefix(self) -> &'static str {
      match self {
         Self::Plain => "",
         Self::Shift => "Shift+",
         Self::Alt => "Alt+",
         Self::Ctrl => "Ctrl+",
         Self::CtrlShift => "Ctrl+Shift+",
         Self::Command => {
            if cfg!(target_os = "macos") {
               "Command+"
            } else {
               "Ctrl+"
            }
         }
         Self::CommandShift => {
            if cfg!(target_os = "macos") {
               "Command+Shift+"
            } else {
               "Ctrl+Shift+"
            }
         }
      }
   }

   /// The chord as the saved file names it — stable, and not the label, so a label can change
   /// without moving every saved binding.
   pub fn name(self) -> &'static str {
      match self {
         Self::Plain => "plain",
         Self::Shift => "shift",
         Self::Alt => "alt",
         Self::Ctrl => "ctrl",
         Self::CtrlShift => "ctrl_shift",
         Self::Command => "command",
         Self::CommandShift => "command_shift",
      }
   }

   pub fn from_name(name: &str) -> Option<Self> {
      Self::ALL.into_iter().find(|chord| chord.name() == name)
   }

   /// The chord a press actually is on this platform: everywhere but a Mac `Ctrl` and `Command`
   /// are the same key, so two bindings stored with different chords can still fight over one press.
   fn effective(self) -> Self {
      if cfg!(target_os = "macos") {
         return self;
      }

      match self {
         Self::Command => Self::Ctrl,
         Self::CommandShift => Self::CtrlShift,
         other => other,
      }
   }
}

/// One key with the modifiers that make it a shortcut.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
   pub key: Key,
   pub chord: Chord,
}

impl Binding {
   pub fn new(key: Key, chord: Chord) -> Self {
      Self { key, chord }
   }

   /// The binding as it is written: `Ctrl+Shift+C`.
   pub fn label(self) -> String {
      format!("{}{}", self.chord.prefix(), self.key.name())
   }

   /// Whether the chord would be typed into a text field rather than run on the page: a bare or
   /// shifted letter, digit, punctuation key or space.
   fn types_text(self) -> bool {
      matches!(self.chord, Chord::Plain | Chord::Shift) && is_typed(self.key)
   }

   /// Whether the bare or shifted key is one the keyboard uses to move around or activate, so
   /// taking it would break Tab, Enter, Escape or a caret key on every page.
   fn moves_the_focus(self) -> bool {
      matches!(self.chord, Chord::Plain | Chord::Shift) && is_navigation(self.key)
   }

   /// Whether the chord is one the text fields already own on this platform's command key.
   ///
   /// The layer takes keys before a field draws, so binding one of these would break copy, paste,
   /// undo or a word-wise caret move in every text field in the app. A shifted chord is free:
   /// `Ctrl+Shift+C` is not how a field copies.
   fn edits_text(self) -> bool {
      let chord = self.chord.effective();
      let edits = matches!(
         self.key,
         Key::A
            | Key::C
            | Key::V
            | Key::X
            | Key::Y
            | Key::Z
            | Key::Home
            | Key::End
            | Key::Backspace
            | Key::Delete
            | Key::ArrowLeft
            | Key::ArrowRight
            | Key::ArrowUp
            | Key::ArrowDown
      );

      (chord == Chord::Ctrl && edits) || (chord == Chord::CtrlShift && matches!(self.key, Key::Z))
   }

   /// Whether two bindings answer the same press on this platform.
   fn same_press(self, other: Self) -> bool {
      self.key == other.key && self.chord.effective() == other.chord.effective()
   }
}

/// Everything the app answers with a chord of its own.
///
/// The order is the order the `F1` sheet lists them in. The keys egui itself owns — `Tab`,
/// `Shift+Tab`, `Enter`, `Space`, `Escape` — are not here: they are drawn for the reader on the
/// sheet, but rebinding them would take them away from every widget in the app.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Action {
   OpenPalette,
   NextRegion,
   PreviousRegion,
   NextControl,
   PreviousControl,
   ReviewSend,
   OpenNotification,
   CopyAddress,
   ShowShortcuts,
}

impl Action {
   /// Every action, in the order the sheet and the palette list them.
   pub const ALL: [Self; 9] = [
      Self::NextRegion,
      Self::PreviousRegion,
      Self::NextControl,
      Self::PreviousControl,
      Self::ReviewSend,
      Self::OpenNotification,
      Self::CopyAddress,
      Self::ShowShortcuts,
      // Last, so an empty palette with `Enter` on the first row runs a command that does
      // something rather than opening the palette that is already open.
      Self::OpenPalette,
   ];

   /// What the action is called in the palette, the settings page and a refusal.
   pub fn label(self) -> &'static str {
      match self {
         Self::OpenPalette => "Command palette",
         Self::NextRegion => "Next part of the window",
         Self::PreviousRegion => "Previous part of the window",
         Self::NextControl => "Next control",
         Self::PreviousControl => "Previous control",
         Self::ReviewSend => "Press Send",
         Self::OpenNotification => "Open the notification",
         Self::CopyAddress => "Copy address",
         Self::ShowShortcuts => "Keyboard shortcuts",
      }
   }

   /// What the `F1` sheet says the key does, after the key: `Ctrl+Shift+C: copy your address`.
   pub fn description(self) -> &'static str {
      match self {
         Self::OpenPalette => "list every command, and run one by name",
         Self::NextRegion => {
            "go to the next part: the account, the navigation, the page, the notification"
         }
         Self::PreviousRegion => "go back to the previous part",
         Self::NextControl => "the next control, skipping the text between them",
         Self::PreviousControl => "the previous control, skipping the text between them",
         Self::ReviewSend => "press Send on the Send page",
         Self::OpenNotification => "go to the open notification",
         Self::CopyAddress => "copy your address",
         Self::ShowShortcuts => "show or hide this list",
      }
   }

   /// The chord the action answers until the user changes it.
   pub fn default_binding(self) -> Binding {
      match self {
         Self::OpenPalette => Binding::new(Key::K, Chord::Command),
         Self::NextRegion => Binding::new(Key::F6, Chord::Plain),
         Self::PreviousRegion => Binding::new(Key::F6, Chord::Shift),
         Self::NextControl => Binding::new(Key::Tab, Chord::Ctrl),
         Self::PreviousControl => Binding::new(Key::Tab, Chord::CtrlShift),
         Self::ReviewSend => Binding::new(Key::Enter, Chord::Command),
         Self::OpenNotification => Binding::new(Key::N, Chord::Alt),
         Self::CopyAddress => Binding::new(Key::C, Chord::CommandShift),
         Self::ShowShortcuts => Binding::new(Key::F1, Chord::Plain),
      }
   }

   /// The name the saved file uses for the action.
   pub fn name(self) -> &'static str {
      match self {
         Self::OpenPalette => "command_palette",
         Self::NextRegion => "next_region",
         Self::PreviousRegion => "previous_region",
         Self::NextControl => "next_control",
         Self::PreviousControl => "previous_control",
         Self::ReviewSend => "press_send",
         Self::OpenNotification => "open_notification",
         Self::CopyAddress => "copy_address",
         Self::ShowShortcuts => "keyboard_shortcuts",
      }
   }

   pub fn from_name(name: &str) -> Option<Self> {
      Self::ALL.into_iter().find(|action| action.name() == name)
   }
}

/// Why a chord cannot be bound to an action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
   /// A bare or shifted key that is typed, or that the keyboard uses to move around.
   Bare,
   /// A chord the text fields already own.
   TextField,
   /// A chord another action already answers.
   InUse(Action),
}

impl Refusal {
   /// The sentence the Keyboard page says when it refuses a chord.
   pub fn words(self, binding: Binding) -> String {
      match self {
         Self::Bare => format!(
            "{} on its own would be typed, or would move the focus, on every page. Hold Ctrl, Alt \
             or Shift, or pick a function key.",
            binding.label()
         ),
         Self::TextField => format!(
            "{} is one of the text fields' own keys (copy, paste, undo, or a caret move). Choose \
             another chord.",
            binding.label()
         ),
         Self::InUse(other) => format!(
            "{} already runs {}. Choose another chord.",
            binding.label(),
            other.label()
         ),
      }
   }
}

/// The chords the user changed, on top of every action's default.
///
/// Only the changed ones are kept, so the saved file is empty until the user changes something and
/// a new default in a later release reaches everyone who never touched that action.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct KeyBindings {
   overrides: BTreeMap<Action, Binding>,
}

impl KeyBindings {
   pub fn new() -> Self {
      Self::default()
   }

   /// The chord an action answers: its default, unless the user changed it.
   pub fn binding(&self, action: Action) -> Binding {
      self.overrides.get(&action).copied().unwrap_or_else(|| action.default_binding())
   }

   /// Whether the action still answers the chord it shipped with.
   pub fn is_default(&self, action: Action) -> bool {
      !self.overrides.contains_key(&action)
   }

   /// Bind `action` to `binding`, refusing a chord that would take a key away from the app or from
   /// another action.
   pub fn set(&mut self, action: Action, binding: Binding) -> Result<(), Refusal> {
      if binding.types_text() || binding.moves_the_focus() {
         return Err(Refusal::Bare);
      }

      if binding.edits_text() {
         return Err(Refusal::TextField);
      }

      if let Some(other) = self.holder(action, binding) {
         return Err(Refusal::InUse(other));
      }

      // A chord put back on its default is no longer a change, and keeping it would make the saved
      // file say something happened when nothing did.
      if action.default_binding() == binding {
         self.overrides.remove(&action);
      } else {
         self.overrides.insert(action, binding);
      }

      Ok(())
   }

   /// Put one action back on its default chord.
   pub fn reset(&mut self, action: Action) {
      self.overrides.remove(&action);
   }

   /// Put every action back on its default chord.
   pub fn reset_all(&mut self) {
      self.overrides.clear();
   }

   /// The action that already answers `binding`.
   fn holder(&self, action: Action, binding: Binding) -> Option<Action> {
      Action::ALL
         .into_iter()
         .find(|other| *other != action && self.binding(*other).same_press(binding))
   }

   /// The keys saved last time.
   pub fn load_from_file() -> Result<Self, anyhow::Error> {
      let path = settings_path()?;
      let data = std::fs::read_to_string(path)?;

      Ok(Self::from_json(&data))
   }

   /// Read a saved document.
   ///
   /// Entry by entry on purpose: a hand-edited file with one chord the app cannot use loses that
   /// one binding, not the other keys saved beside it.
   pub fn from_json(data: &str) -> Self {
      let document: Document = match serde_json::from_str(data) {
         Ok(document) => document,
         Err(e) => {
            tracing::warn!("Ignoring the saved keyboard shortcuts: {e}");
            return Self::default();
         }
      };

      let mut bindings = Self::default();

      for (name, saved) in document.bindings {
         let Some(action) = Action::from_name(&name) else {
            tracing::warn!("Saved shortcut for an action this build does not have: {name}");
            continue;
         };

         let Some(binding) = Binding::from_parts(&saved.key, &saved.chord) else {
            tracing::warn!("Saved shortcut for {name} names a key this build does not have");
            continue;
         };

         if let Err(refusal) = bindings.set(action, binding) {
            tracing::warn!(
               "Saved shortcut {} for {name} dropped: {}",
               binding.label(),
               refusal.words(binding)
            );
         }
      }

      bindings
   }

   pub fn to_json(&self) -> Result<String, serde_json::Error> {
      let mut document = Document::default();

      for (action, binding) in &self.overrides {
         document.bindings.insert(
            action.name().to_string(),
            SavedBinding {
               key: binding.key.name().to_string(),
               chord: binding.chord.name().to_string(),
            },
         );
      }

      serde_json::to_string_pretty(&document)
   }

   /// Write the changed chords beside the appearance settings.
   pub fn save(&self) -> Result<(), anyhow::Error> {
      ensure_data_dir()?;
      write_private(&settings_path()?, self.to_json()?.as_bytes())?;

      Ok(())
   }
}

impl Binding {
   /// The binding a saved entry names, or `None` when this build has no such key or chord.
   fn from_parts(key: &str, chord: &str) -> Option<Self> {
      Some(Self::new(
         Key::from_name(key)?,
         Chord::from_name(chord)?,
      ))
   }
}

/// The saved file as it is written: a version for a future format, then the changed bindings.
#[derive(Debug, Serialize, Deserialize)]
struct Document {
   #[serde(default)]
   version: u32,
   #[serde(default)]
   bindings: BTreeMap<String, SavedBinding>,
}

impl Default for Document {
   fn default() -> Self {
      Self {
         version: 1,
         bindings: BTreeMap::new(),
      }
   }
}

/// One changed binding in the file: the key and the chord by name, not by index.
#[derive(Debug, Serialize, Deserialize)]
struct SavedBinding {
   key: String,
   chord: String,
}

/// Where the changed chords are saved: beside `appearance_settings.json`, because both are
/// unencrypted preferences and not wallet data.
fn settings_path() -> Result<PathBuf, anyhow::Error> {
   Ok(appearance_settings_dir()?.with_file_name(SETTINGS_FILE))
}

/// Whether a bare or shifted key would be typed into a text field.
fn is_typed(key: Key) -> bool {
   matches!(
      key,
      Key::A
         | Key::B
         | Key::C
         | Key::D
         | Key::E
         | Key::F
         | Key::G
         | Key::H
         | Key::I
         | Key::J
         | Key::K
         | Key::L
         | Key::M
         | Key::N
         | Key::O
         | Key::P
         | Key::Q
         | Key::R
         | Key::S
         | Key::T
         | Key::U
         | Key::V
         | Key::W
         | Key::X
         | Key::Y
         | Key::Z
         | Key::Num0
         | Key::Num1
         | Key::Num2
         | Key::Num3
         | Key::Num4
         | Key::Num5
         | Key::Num6
         | Key::Num7
         | Key::Num8
         | Key::Num9
         | Key::Space
         | Key::Colon
         | Key::Comma
         | Key::Minus
         | Key::Period
         | Key::Plus
         | Key::Equals
         | Key::Semicolon
         | Key::Backslash
         | Key::Slash
         | Key::Pipe
         | Key::Questionmark
         | Key::Exclamationmark
         | Key::OpenBracket
         | Key::CloseBracket
         | Key::OpenCurlyBracket
         | Key::CloseCurlyBracket
         | Key::Backtick
         | Key::Quote
   )
}

/// Whether a bare or shifted key is one the keyboard uses to move or activate, rather than to type.
fn is_navigation(key: Key) -> bool {
   matches!(
      key,
      Key::Tab
         | Key::Enter
         | Key::Escape
         | Key::Backspace
         | Key::Delete
         | Key::Insert
         | Key::Home
         | Key::End
         | Key::PageUp
         | Key::PageDown
         | Key::ArrowUp
         | Key::ArrowDown
         | Key::ArrowLeft
         | Key::ArrowRight
   )
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn a_binding_says_the_key_it_answers() {
      assert_eq!(
         Binding::new(Key::C, Chord::CommandShift).label(),
         "Ctrl+Shift+C"
      );
      assert_eq!(Binding::new(Key::N, Chord::Alt).label(), "Alt+N");
      assert_eq!(Binding::new(Key::F6, Chord::Plain).label(), "F6");
      assert_eq!(
         Binding::new(Key::Num3, Chord::Alt).label(),
         "Alt+3"
      );
   }

   #[test]
   fn a_default_binding_is_what_the_action_ships_with() {
      for action in Action::ALL {
         let bindings = KeyBindings::new();

         assert_eq!(
            bindings.binding(action),
            action.default_binding(),
            "{action:?} must start on its default chord"
         );
         assert!(bindings.is_default(action));
      }
   }

   #[test]
   fn every_action_name_round_trips_through_the_file() {
      for action in Action::ALL {
         assert_eq!(Action::from_name(action.name()), Some(action));
      }
   }

   #[test]
   fn a_changed_chord_is_kept_and_the_default_ones_are_not_written() {
      let mut bindings = KeyBindings::new();
      let line = Binding::new(Key::L, Chord::Alt);

      assert_eq!(bindings.set(Action::CopyAddress, line), Ok(()));
      assert_eq!(bindings.binding(Action::CopyAddress), line);
      assert!(!bindings.is_default(Action::CopyAddress));

      let json = bindings.to_json().expect("a changed binding must serialize");

      assert!(json.contains("copy_address"), "{json}");
      assert!(json.contains("\"alt\""), "{json}");

      let loaded = KeyBindings::from_json(&json);

      assert_eq!(loaded.binding(Action::CopyAddress), line);
      assert_eq!(
         loaded.binding(Action::ShowShortcuts),
         Action::ShowShortcuts.default_binding()
      );
   }

   #[test]
   fn resetting_a_chord_takes_it_out_of_the_file() {
      let mut bindings = KeyBindings::new();
      bindings
         .set(
            Action::CopyAddress,
            Binding::new(Key::L, Chord::Alt),
         )
         .unwrap();
      bindings.reset(Action::CopyAddress);

      assert_eq!(bindings, KeyBindings::new());
      assert!(bindings.to_json().unwrap().contains("\"bindings\": {}"));
   }

   #[test]
   fn a_chord_another_action_answers_is_refused_by_name() {
      let mut bindings = KeyBindings::new();

      assert_eq!(
         bindings.set(
            Action::CopyAddress,
            Binding::new(Key::Tab, Chord::Ctrl)
         ),
         Err(Refusal::InUse(Action::NextControl))
      );

      assert_eq!(
         bindings,
         KeyBindings::new(),
         "a refused chord changes nothing"
      );
   }

   #[test]
   fn on_a_pc_ctrl_and_command_are_the_same_key() {
      // The default Ctrl+Shift+C and a literal Ctrl+Shift+C are one press everywhere but a Mac.
      let bindings = KeyBindings::new();
      let copy = bindings.binding(Action::CopyAddress);
      let held = Binding::new(Key::C, Chord::CtrlShift);

      assert_eq!(copy.same_press(held), !cfg!(target_os = "macos"));
   }

   #[test]
   fn a_bare_key_that_is_typed_or_moves_the_focus_is_refused() {
      let mut bindings = KeyBindings::new();

      assert_eq!(
         bindings.set(
            Action::CopyAddress,
            Binding::new(Key::C, Chord::Plain)
         ),
         Err(Refusal::Bare),
         "a letter would fire while the user is typing"
      );

      assert_eq!(
         bindings.set(
            Action::CopyAddress,
            Binding::new(Key::Tab, Chord::Plain)
         ),
         Err(Refusal::Bare),
         "Tab already moves the focus"
      );

      assert_eq!(
         bindings.set(
            Action::CopyAddress,
            Binding::new(Key::Tab, Chord::Shift)
         ),
         Err(Refusal::Bare),
         "Shift+Tab already moves the focus back"
      );

      assert_eq!(
         bindings.set(
            Action::CopyAddress,
            Binding::new(Key::F2, Chord::Plain)
         ),
         Ok(()),
         "a function key types nothing and moves nothing"
      );
   }

   #[test]
   fn a_text_fields_own_chord_is_refused() {
      let mut bindings = KeyBindings::new();

      for key in [Key::C, Key::V, Key::X, Key::A, Key::Z] {
         assert_eq!(
            bindings.set(
               Action::OpenNotification,
               Binding::new(key, Chord::Command)
            ),
            Err(Refusal::TextField),
            "{key:?} is a text field's own key"
         );
      }

      assert_eq!(
         bindings.set(
            Action::OpenNotification,
            Binding::new(Key::M, Chord::CommandShift)
         ),
         Ok(()),
         "a shifted chord is not how a field copies"
      );
   }

   #[test]
   fn a_hand_edited_file_loses_only_the_line_it_cannot_use() {
      let json = r#"{
         "version": 1,
         "bindings": {
            "copy_address": { "key": "L", "chord": "alt" },
            "open_notification": { "key": "NotAKey", "chord": "alt" },
            "keyboard_shortcuts": { "key": "F1", "chord": "plain" },
            "no_such_action": { "key": "P", "chord": "alt" }
         }
      }"#;

      let bindings = KeyBindings::from_json(json);

      assert_eq!(
         bindings.binding(Action::CopyAddress),
         Binding::new(Key::L, Chord::Alt),
         "the good line survives"
      );
      assert_eq!(
         bindings.binding(Action::OpenNotification),
         Action::OpenNotification.default_binding(),
         "the line with a key this build has not keeps the default"
      );
   }

   #[test]
   fn a_file_that_is_not_json_leaves_every_default_in_place() {
      assert_eq!(
         KeyBindings::from_json("not json at all"),
         KeyBindings::new()
      );
   }
}
