//! The command palette: every action the app has, filtered by name and run with `Enter`.
//!
//! A chord is only discoverable by someone who already knows it, and a page's controls only by
//! someone already on that page. The palette is the other direction: type what you want — "copy",
//! "transactions", "F6" — and hear the matching commands with the keys that run them.
//!
//! It is a modal like every other dialog in the app, so while it is up the keyboard is inside it
//! and nothing behind it can be reached. The keys it answers (the arrows and `Enter`) are taken in
//! [`super::handle`] before the UI draws, which is what keeps them out of the filter field.

use super::PAGE_KEYS;
use super::bindings::{Action, KeyBindings};
use crate::gui::Page;
use egui::{Id, RichText, ScrollArea, Ui};
use egui_elements::{Modal, Theme};

/// How wide the palette is. Fixed, like the sheet: a shrink-wrapping modal measures itself for two
/// passes, and the field asks for the focus on the first painted one.
const PALETTE_WIDTH: f32 = 560.0;

/// How tall the list grows before it scrolls, so the card keeps clear of a short window.
const PALETTE_HEIGHT: f32 = 360.0;

/// What the palette can run.
///
/// A page jump and the settings page are commands without a chord of their own: they are reachable
/// by name here, and the pages keep their own `Alt` digits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
   /// An action from the binding table, which also has a key.
   Do(Action),
   /// Open one of the pages the nav lists.
   GoTo(Page),
   /// Open the Keyboard page in Settings.
   ChangeBindings,
}

impl Command {
   /// Every command, in the order the palette lists them with no query typed.
   pub fn all() -> Vec<Self> {
      let mut commands: Vec<Self> = Action::ALL.into_iter().map(Self::Do).collect();

      commands.extend(PAGE_KEYS.into_iter().map(|(_, page)| Self::GoTo(page)));
      commands.push(Self::ChangeBindings);

      commands
   }

   /// What the command is called in the list.
   pub fn label(self) -> String {
      match self {
         Self::Do(action) => action.label().to_string(),
         Self::GoTo(page) => format!("Go to {}", page.name()),
         Self::ChangeBindings => "Change keyboard shortcuts".to_string(),
      }
   }

   /// The chord that runs the command, written the way the rest of the app writes it.
   pub fn binding(self, bindings: &KeyBindings) -> Option<String> {
      match self {
         Self::Do(action) => Some(bindings.binding(action).label()),
         Self::GoTo(page) => super::page_binding(page),
         Self::ChangeBindings => None,
      }
   }

   /// The row as it is drawn and read: the name, then the chord that runs it.
   pub fn row(self, bindings: &KeyBindings) -> String {
      match self.binding(bindings) {
         Some(binding) => format!("{}: {binding}", self.label()),
         None => self.label(),
      }
   }

   /// Whether the command matches what the user typed: every word has to appear in the name or in
   /// the chord, so "copy" and "ctrl shift c" both find Copy address.
   fn matches(self, query: &str, bindings: &KeyBindings) -> bool {
      let haystack = self.row(bindings).to_lowercase();

      query.split_whitespace().all(|word| haystack.contains(&word.to_lowercase()))
   }
}

/// The palette's state: whether it is up, what was typed, and which row the arrows are on.
#[derive(Debug, Default)]
pub struct CommandPalette {
   open: bool,
   query: String,
   selected: usize,
   /// The row the arrows last landed on, so the words are said when the row changes and not every
   /// frame.
   said: Option<String>,
   /// The words this frame should speak; drained by the frame that drew the palette.
   notice: Option<String>,
   /// The command chosen with `Enter`, waiting for the frame that has the context to run it.
   ///
   /// The palette draws in the overlay pass, which has no `ZeusContext`; the shortcut layer has one
   /// and runs this on its next pass.
   pending: Option<Command>,
}

impl CommandPalette {
   pub fn new() -> Self {
      Self::default()
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   /// Put the palette up with an empty query.
   pub fn open(&mut self) {
      self.open = true;
      self.query.clear();
      self.selected = 0;
      self.said = None;
   }

   pub fn close(&mut self) {
      self.open = false;
      self.query.clear();
      self.selected = 0;
      self.said = None;
   }

   /// The id the filter field is drawn under, so the layer can ask whether it owns the focus.
   pub fn field_id() -> Id {
      Id::unique("zeus_command_palette_query")
   }

   pub fn query_mut(&mut self) -> &mut String {
      &mut self.query
   }

   pub fn set_query(&mut self, query: impl Into<String>) {
      self.query = query.into();
      self.selected = 0;
   }

   /// The commands the query leaves, in list order.
   fn filtered(&self, bindings: &KeyBindings) -> Vec<Command> {
      Command::all()
         .into_iter()
         .filter(|command| command.matches(&self.query, bindings))
         .collect()
   }

   /// The command `Enter` runs: the row the arrows are on, clamped to the list.
   pub fn selected_command(&self, bindings: &KeyBindings) -> Option<Command> {
      let rows = self.filtered(bindings);

      rows.get(self.selected.min(rows.len().saturating_sub(1))).copied()
   }

   /// Move the highlighted row, wrapping at both ends.
   pub fn move_selection(&mut self, direction: i32, bindings: &KeyBindings) {
      let count = self.filtered(bindings).len();

      if count == 0 {
         self.selected = 0;
         return;
      }

      let step = direction.signum() as i64;
      let moved = (self.selected as i64 + step).rem_euclid(count as i64);

      self.selected = moved as usize;
   }

   /// Take the command `Enter` chose, which the next [`super::handle`] pass runs.
   pub fn take_pending(&mut self) -> Option<Command> {
      self.pending.take()
   }

   pub fn set_pending(&mut self, command: Option<Command>) {
      self.pending = command;
   }

   /// The words this frame should say, if the highlighted row changed.
   pub fn take_notice(&mut self) -> Option<String> {
      self.notice.take()
   }

   /// Draw the palette, and return the command a click on a row chose.
   pub fn show(&mut self, bindings: &KeyBindings, theme: &Theme, ui: &mut Ui) -> Option<Command> {
      if !self.open {
         return None;
      }

      let mut open = self.open;
      let heading = RichText::new("Commands").size(theme.typography.very_large);

      let run = Modal::new(Id::unique("command_palette"), &mut open)
         .heading(heading)
         .width(PALETTE_WIDTH)
         .initial_focus(Self::field_id())
         .show(ui.ctx(), |ui| {
            ui.spacing_mut().item_spacing.y = theme.spacing.xs;

            let field = egui::TextEdit::singleline(&mut self.query)
               .id(Self::field_id())
               .hint_text("Type to filter commands")
               .desired_width(f32::INFINITY);

            if ui.add(field).changed() {
               self.selected = 0;
            }

            let rows = self.filtered(bindings);
            self.selected = self.selected.min(rows.len().saturating_sub(1));

            let count = match rows.len() {
               0 => "No command matches".to_string(),
               1 => "1 command".to_string(),
               count => format!("{count} commands"),
            };

            ui.label(RichText::new(count).size(theme.typography.small));

            let mut clicked = None;

            ScrollArea::vertical()
               .max_height(PALETTE_HEIGHT)
               .auto_shrink([false, true])
               .show(ui, |ui| {
                  if rows.is_empty() {
                     return;
                  }

                  ui.set_width(ui.available_width());

                  for (index, command) in rows.iter().enumerate() {
                     let response =
                        ui.selectable_label(index == self.selected, command.row(bindings));

                     if response.clicked() {
                        clicked = Some(*command);
                     }
                  }
               });

            self.note_selection(&rows, bindings);

            clicked
         });

      // `Escape`, the close button and a backdrop click all clear the flag the modal is bound to.
      if !open {
         self.close();
      }

      run.flatten()
   }

   /// Queue the highlighted row for the reader, once per change of row.
   fn note_selection(&mut self, rows: &[Command], bindings: &KeyBindings) {
      let words = rows.get(self.selected).map(|command| {
         format!(
            "{}, {} of {}",
            command.row(bindings),
            self.selected + 1,
            rows.len()
         )
      });

      if words == self.said {
         return;
      }

      self.said = words.clone();
      self.notice = words;
   }
}

#[cfg(test)]
mod tests {
   use super::*;

   fn palette_with(query: &str) -> CommandPalette {
      let mut palette = CommandPalette::new();
      palette.set_query(query);

      palette
   }

   #[test]
   fn typing_a_name_finds_the_command() {
      let bindings = KeyBindings::new();
      let palette = palette_with("copy");
      let rows = palette.filtered(&bindings);

      assert_eq!(
         rows.first(),
         Some(&Command::Do(Action::CopyAddress)),
         "{rows:?}"
      );
   }

   #[test]
   fn typing_a_chord_finds_the_command_that_answers_it() {
      let bindings = KeyBindings::new();
      let rows = palette_with("f6").filtered(&bindings);

      assert_eq!(
         rows,
         vec![
            Command::Do(Action::NextRegion),
            Command::Do(Action::PreviousRegion)
         ]
      );
   }

   #[test]
   fn a_page_is_a_command_by_its_name() {
      let bindings = KeyBindings::new();
      let rows = palette_with("transactions").filtered(&bindings);

      assert_eq!(
         rows.first(),
         Some(&Command::GoTo(Page::Transactions)),
         "{rows:?}"
      );
   }

   #[test]
   fn every_command_is_listed_until_something_is_typed() {
      let bindings = KeyBindings::new();
      let rows = CommandPalette::new().filtered(&bindings);

      assert_eq!(
         rows.len(),
         Action::ALL.len() + PAGE_KEYS.len() + 1
      );
      assert!(rows.contains(&Command::ChangeBindings));
   }

   #[test]
   fn a_query_nobody_matches_leaves_nothing() {
      let bindings = KeyBindings::new();

      assert!(palette_with("zzzz").filtered(&bindings).is_empty());
   }

   #[test]
   fn the_arrows_wrap_at_both_ends() {
      let bindings = KeyBindings::new();
      let mut palette = palette_with("copy");
      let count = palette.filtered(&bindings).len();

      palette.move_selection(-1, &bindings);
      assert_eq!(palette.selected, count.saturating_sub(1));

      palette.move_selection(1, &bindings);
      assert_eq!(palette.selected, 0);
   }
}
