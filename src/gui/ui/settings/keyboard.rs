//! The Keyboard page: every chord the app answers, and the key that replaces it.
//!
//! A chord is recorded the way it will be pressed: the page takes the next key that arrives, with
//! its modifiers, and asks the bindings whether it may be used. While it waits, the shortcut layer
//! hands the keyboard over (`GUI::is_capturing`), so the chord being recorded is not also the
//! chord that runs something.
//!
//! A refused chord keeps the capture open and says why: the user is in the middle of choosing, and
//! the refusal is the reason the next press has to be different.

use crate::gui::ui::shortcuts::{Action, Binding, Chord, KeyBindings};
use egui::{Event, Key, RichText, Role, Ui, WidgetInfo, vec2};
use egui_elements::{Button, Theme};

pub struct KeyboardSettings {
   /// The action waiting for its new keys.
   capture: Option<Action>,
   /// The words this page raised, drained by the window and said by the app.
   notice: Option<String>,
}

impl KeyboardSettings {
   pub fn new() -> Self {
      Self {
         capture: None,
         notice: None,
      }
   }

   /// Whether an action is waiting for its new keys.
   pub fn is_capturing(&self) -> bool {
      self.capture.is_some()
   }

   /// Forget the capture and the words: the page is closing, and a key pressed elsewhere is not a
   /// binding.
   pub fn reset(&mut self) {
      self.capture = None;
      self.notice = None;
   }

   /// The words this page raised, drained by the window right after it draws.
   pub fn take_notice(&mut self) -> Option<String> {
      self.notice.take()
   }

   pub fn show(&mut self, bindings: &mut KeyBindings, theme: &Theme, ui: &mut Ui) {
      ui.spacing_mut().item_spacing = vec2(theme.spacing.xs, theme.spacing.sm);
      ui.spacing_mut().button_padding = vec2(theme.spacing.sm, theme.spacing.xs);

      // Before the rows draw, so the row of the action being recorded already says it is waiting.
      if self.capture.is_some() {
         self.capture_keys(bindings, ui);
      }

      let instruction = match self.capture {
         Some(action) => {
            format!(
               "Press the keys you want for {}. Escape cancels.",
               action.label()
            )
         }
         None => "Every key here works on every page. Choose Change, then press the keys you want."
            .to_string(),
      };

      ui.label(RichText::new(instruction).size(theme.typography.normal));
      ui.add_space(theme.spacing.sm);

      for action in Action::ALL {
         self.row(action, bindings, theme, ui);
      }

      ui.add_space(theme.spacing.lg);

      let reset = RichText::new("Reset every key").size(theme.typography.normal);

      if ui.add(Button::new(reset).visuals(theme.button_visuals())).clicked() {
         bindings.reset_all();

         if self.store(bindings) {
            self.capture = None;
            self.notice = Some("Every key is back to its default.".to_string());
         }
      }
   }

   /// One action: what it is called and the chord it answers, then the button that changes it.
   fn row(&mut self, action: Action, bindings: &KeyBindings, theme: &Theme, ui: &mut Ui) {
      let binding = bindings.binding(action);
      let line = format!("{}: {}", action.label(), binding.label());

      ui.horizontal(|ui| {
         ui.label(RichText::new(line).size(theme.typography.normal));

         let waiting = self.capture == Some(action);
         let word = if waiting { "Cancel" } else { "Change" };
         let text = RichText::new(word).size(theme.typography.normal);
         let response = ui.add(Button::new(text).visuals(theme.button_visuals()));

         // The visible word is the verb; the name a reader hears carries the action, so nine rows
         // are not nine identical "Change" stops.
         let name = format!("{word} {}", action.label());
         response.widget_info(|| WidgetInfo::labeled(Role::Button, ui.is_enabled(), name.as_str()));

         if response.clicked() {
            self.capture = if waiting { None } else { Some(action) };
         }
      });
   }

   /// Read the key the user pressed for the captured action.
   fn capture_keys(&mut self, bindings: &mut KeyBindings, ui: &Ui) {
      let Some(action) = self.capture else {
         return;
      };

      let (seen, binding, cancelled) = ui.ctx().input_mut(|input| {
         let mut seen = false;
         let mut binding = None;
         let mut cancelled = false;

         // Every key press this frame belongs to the recording, including the ones no binding can
         // use: a chord that fell through to the page would run the action instead of recording it.
         input.events.retain(|event| {
            let Event::Key {
               key,
               pressed: true,
               repeat: false,
               modifiers,
               ..
            } = event
            else {
               return true;
            };

            seen = true;

            if *key == Key::Escape {
               cancelled = true;
            } else {
               binding = Chord::from_modifiers(modifiers).map(|chord| Binding::new(*key, chord));
            }

            false
         });

         (seen, binding, cancelled)
      });

      if cancelled {
         let keeps = bindings.binding(action).label();

         self.capture = None;
         self.notice = Some(format!("{} keeps {keeps}.", action.label()));

         return;
      }

      if !seen {
         return;
      }

      let Some(binding) = binding else {
         self.notice = Some(
            "That combination is not a shortcut: hold one of Ctrl, Alt, Shift or Command, not two."
               .to_string(),
         );

         return;
      };

      match bindings.set(action, binding) {
         Ok(()) => {
            self.capture = None;

            if self.store(bindings) {
               self.notice = Some(format!(
                  "{} is now {}.",
                  action.label(),
                  binding.label()
               ));
            }
         }
         // The capture stays open: the next press is the one the refusal asks for.
         Err(refusal) => self.notice = Some(refusal.words(binding)),
      }
   }

   /// Write the changed chords down, and say so when the file could not be written.
   fn store(&mut self, bindings: &KeyBindings) -> bool {
      if let Err(e) = bindings.save() {
         tracing::error!("Failed to save the keyboard shortcuts: {e}");
         self.notice = Some("Your keys work for now, but saving them failed.".to_string());

         return false;
      }

      true
   }
}
