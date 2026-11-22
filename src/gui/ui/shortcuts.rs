//! Global keyboard shortcuts.
//!
//! One layer owns every chord, so a key is either acted on once or not at all. The layer consumes
//! the key and any text event the platform pairs with it (on X11 Alt+1 can arrive as the character
//! "1" as well), then acts only when the app is showing a page: the lock screen owns the keyboard,
//! and a dialog keeps it until it closes.
//!
//! A shortcut never signs or deletes anything. `Ctrl+Enter` asks the Send page to press its own
//! Send button, and the transaction still waits for its confirmation window.
//!
//! The sheet the `F1` key opens lives here too, and its rows are drawn from the tables below: a key
//! the handler does not read cannot end up in the list of keys it does.

use crate::core::ZeusContext;
use crate::gui::ui::common::address::copied_words;
use crate::gui::ui::panels::left_panel;
use crate::gui::{FocusRegion, GUI, Page};
use egui::{
   Context, Event, FocusDirection, Id, Key, Modifiers, RichText, Role, ScrollArea, Ui, WidgetInfo,
};
use egui_elements::{Modal, Theme};

/// The page each `Alt` digit opens, in the order the nav draws them.
const PAGE_KEYS: [(Key, Page); 10] = [
   (Key::Num1, Page::Home),
   (Key::Num2, Page::Send),
   (Key::Num3, Page::Shield),
   (Key::Num4, Page::Swap),
   (Key::Num5, Page::Bridge),
   (Key::Num6, Page::Wallets),
   (Key::Num7, Page::Transactions),
   (Key::Num8, Page::Approvals),
   (Key::Num9, Page::Settings),
   (Key::Num0, Page::ConnectedDapps),
];

/// The order `F6` walks the regions in; `Shift+F6` walks back.
const REGIONS: [FocusRegion; 4] = [
   FocusRegion::Account,
   FocusRegion::Navigation,
   FocusRegion::Main,
   FocusRegion::Status,
];

/// Handle this frame's shortcuts.
///
/// Called once, before any panel draws: a page a shortcut opens is then the page this frame draws,
/// and a region it asks for takes the focus from the panel that owns it.
pub fn handle(gui: &mut GUI, ctx: &mut ZeusContext, egui_ctx: &Context) {
   // A focus request belongs to the frame that made it. A region that did not draw must not take
   // the focus on a later frame, so the request is dropped at the top of every one.
   gui.clear_region_focus();

   // The pass that hopped over prose took the label focus out of its own tab order; that pass is
   // over, so the reader's model goes back on before anything draws.
   gui.restore_reader_model(egui_ctx);

   let previous_region = take_key(egui_ctx, Key::F6, Chord::Shift);
   let next_region = take_key(egui_ctx, Key::F6, Chord::Plain);
   let review = take_key(egui_ctx, Key::Enter, Chord::Command);
   let notification = take_key(egui_ctx, Key::N, Chord::Alt);
   let sheet = take_key(egui_ctx, Key::F1, Chord::Plain);
   let next_control = take_key(egui_ctx, Key::Tab, Chord::Ctrl);
   let previous_control = take_key(egui_ctx, Key::Tab, Chord::CtrlShift);
   let copy_address = take_key(egui_ctx, Key::C, Chord::CommandShift);

   let mut target = None;

   for (key, page) in PAGE_KEYS {
      if take_key(egui_ctx, key, Chord::Alt) {
         target = Some(page);
      }
   }

   // The sheet is the one surface this layer still answers while it is up: `F1` is what opened it,
   // so `F1` is what closes it. Every other surface keeps the keyboard until it closes.
   if sheet {
      if gui.shortcut_sheet.is_open() {
         gui.shortcut_sheet.close();
      } else if !ctx.locked && ctx.vault_unlocked && !overlay_open(gui) {
         gui.shortcut_sheet.open();
      }

      return;
   }

   // Behind the lock screen the card owns the keyboard; a dialog keeps it until it closes. The
   // keys above were still consumed, so a typing field cannot receive their characters.
   if ctx.locked || !ctx.vault_unlocked || overlay_open(gui) {
      return;
   }

   if let Some(page) = target {
      left_panel::open_page(gui, ctx, page);
   }

   if next_region || previous_region {
      let region = neighbour_region(gui, if previous_region { -1 } else { 1 });

      gui.request_region_focus(region);

      if region == FocusRegion::Status {
         gui.notification.request_focus();
      }
   }

   // The Send page presses its own button, so the amount, the recipient guard and the reasons a
   // blocked send is disabled stay exactly what a pointer click uses.
   if review && gui.current_page() == Page::Send {
      gui.send_crypto.request_send();
   }

   if notification {
      gui.notification.request_focus();
   }

   // The clipboard is invisible, so the words are the only way to check what landed there.
   if copy_address {
      match gui.account_panel.address_to_copy(ctx.privacy_mode) {
         Some(address) => {
            gui.announce(copied_words(&address));
            egui_ctx.copy_text(address);
         }
         None => gui.announce("This wallet has no privacy address to copy."),
      }
   }

   if next_control || previous_control {
      hop_to_control(
         gui,
         egui_ctx,
         if previous_control { -1 } else { 1 },
      );
   }
}

/// Move the focus to the next or previous control, hopping over the lines of text between them.
///
/// `Tab` stops on every line of prose — that is what makes the page reachable by ear — so asking
/// egui for a step keeps landing on words. This asks for the step in a pass where a label is not a
/// tab stop at all: the focus the step lands on is a control.
fn hop_to_control(gui: &mut GUI, egui_ctx: &Context, direction: i32) {
   gui.skip_prose_this_pass(egui_ctx);

   let direction = match direction.signum() {
      -1 => FocusDirection::Previous,
      _ => FocusDirection::Next,
   };

   egui_ctx.memory_mut(|memory| memory.move_focus(direction));
}

/// The next region that can take the focus, walking around the circle.
fn neighbour_region(gui: &GUI, direction: i32) -> FocusRegion {
   let start = REGIONS
      .iter()
      .position(|region| *region == gui.last_focus_region())
      .unwrap_or(1);

   for step in 1..=REGIONS.len() {
      let index = (start as i32 + direction * step as i32).rem_euclid(REGIONS.len() as i32);
      let region = REGIONS[index as usize];

      if region_available(gui, region) {
         return region;
      }
   }

   gui.last_focus_region()
}

/// Whether a region has a control to focus right now. Navigation and Main always do; the account
/// card needs a wallet behind it, and the status region needs an open notification.
fn region_available(gui: &GUI, region: FocusRegion) -> bool {
   match region {
      FocusRegion::Account => gui.account_panel.is_open(),
      FocusRegion::Status => gui.notification.is_open(),
      FocusRegion::Navigation | FocusRegion::Main => true,
   }
}

/// Whether a dialog is up over the page: a shortcut must not reach behind it.
fn overlay_open(gui: &GUI) -> bool {
   gui.shortcut_sheet.is_open()
      || gui.settings.is_open()
      || gui.connected_dapps.is_open()
      || gui.wallet_ui.dialog_open()
      || gui.recipient_selection.is_open()
      || gui.token_selection.is_open()
      || gui.tx_confirmation_window.is_open()
      || gui.confirm_window.is_open()
      || gui.connect_dapp_window.is_open()
      || gui.sign_msg_window.is_open()
      || gui.tx_window.is_open()
      || gui.spent_note_window.is_open()
      || gui.msg_window.is_open()
      || gui.loading_window.is_open()
      || gui.update_window.is_open()
      || gui.account_panel.qr_window.is_open()
      || gui.merge_notes_window.is_open()
}

/// A modifier set a chord is pressed with.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Chord {
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
   fn matches(self, modifiers: &Modifiers) -> bool {
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
}

/// Take this frame's press of `key` with `chord`, with the text event it may have produced.
fn take_key(ctx: &Context, key: Key, chord: Chord) -> bool {
   let pressed = ctx.input(|input| {
      input.events.iter().any(|event| {
         matches!(
            event,
            Event::Key { key: pressed_key, pressed: true, repeat: false, modifiers, .. }
               if *pressed_key == key && chord.matches(modifiers)
         )
      })
   });

   if !pressed {
      return false;
   }

   ctx.input_mut(|input| {
      input.events.retain(|event| match event {
         Event::Key {
            key: pressed_key,
            pressed: true,
            modifiers,
            ..
         } => !(*pressed_key == key && chord.matches(modifiers)),
         // A shortcut is not text: the character an Alt chord produced goes with the key.
         Event::Text(text) if chord == Chord::Alt => match key_char(key) {
            Some(character) => {
               !(text.chars().count() == 1
                  && text
                     .chars()
                     .next()
                     .is_some_and(|typed| typed.eq_ignore_ascii_case(&character)))
            }
            None => true,
         },
         _ => true,
      });
   });

   true
}

/// The character a key types, for the chords whose text event has to go with the key.
fn key_char(key: Key) -> Option<char> {
   Some(match key {
      Key::Num0 => '0',
      Key::Num1 => '1',
      Key::Num2 => '2',
      Key::Num3 => '3',
      Key::Num4 => '4',
      Key::Num5 => '5',
      Key::Num6 => '6',
      Key::Num7 => '7',
      Key::Num8 => '8',
      Key::Num9 => '9',
      Key::N => 'n',
      _ => return None,
   })
}

/// How wide the sheet's card is.
///
/// Fixed rather than shrink-wrapped: a shrink-wrapping modal spends two invisible passes measuring
/// itself, and the focus request that opens the sheet waits for a pass it can be handed.
const SHEET_WIDTH: f32 = 620.0;

/// How tall the list grows before it scrolls, so the card keeps clear of a short window.
const SHEET_HEIGHT: f32 = 430.0;

/// The keys that move the focus or close a dialog, as the sheet lists them: each row is the keys,
/// then what they do.
const MOVING: &[(&str, &str)] = &[
   ("Tab / Shift+Tab", "the next or previous control"),
   (
      "Enter / Space",
      "press the control the focus is on",
   ),
   (
      "Ctrl+Tab / Ctrl+Shift+Tab",
      "the next or previous control, skipping the text between them",
   ),
   (
      "F6 / Shift+F6",
      "the next or previous part of the window: the account, the navigation, the page, the \
       notification",
   ),
   ("Escape", "close the open dialog"),
];

/// The keys that act on what is in front, as the sheet lists them.
const ACTIONS: &[(&str, &str)] = &[
   (
      "Ctrl+Enter",
      "press Send on the Send page (Command+Enter on macOS)",
   ),
   (
      "Ctrl+Shift+C",
      "copy your address (Command+Shift+C on macOS)",
   ),
   ("Alt+N", "go to the open notification"),
   ("F1", "show or hide this list"),
];

/// The list of every binding, opened with `F1`.
///
/// It is a modal like any other dialog: the keyboard is inside it while it is up, `Escape` and the
/// close button are two ways out, and the focus goes back to the control it came from when it
/// closes. Nothing behind it can be reached, which is what makes it worth opening from a page: it
/// explains the keys without taking the page away.
pub struct ShortcutSheet {
   open: bool,
}

impl ShortcutSheet {
   pub fn new() -> Self {
      Self { open: false }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   /// Put the sheet up. Whether the app is in a state to answer the key that asks for it is the
   /// caller's question.
   pub fn open(&mut self) {
      self.open = true;
   }

   pub fn close(&mut self) {
      self.open = false;
   }

   pub fn show(&mut self, theme: &Theme, ui: &mut Ui) {
      if !self.open {
         return;
      }

      let mut open = self.open;
      let heading = RichText::new("Keyboard shortcuts").size(theme.typography.very_large);

      Modal::new(Id::unique("shortcut_sheet"), &mut open)
         .heading(heading)
         .width(SHEET_WIDTH)
         .show(ui.ctx(), |ui| {
            let list = ScrollArea::vertical().max_height(SHEET_HEIGHT).auto_shrink([false, true]);

            list.show(ui, |ui| {
               ui.set_width(ui.available_width());
               ui.spacing_mut().item_spacing.y = theme.spacing.xs;

               section(theme, ui, "Moving around", MOVING);
               section(theme, ui, "Actions", ACTIONS);

               section_title(theme, ui, "Pages");

               for (key, page) in PAGE_KEYS {
                  // Every page key is a digit, so the chord writes itself. A table entry with no
                  // character would be a bug in `PAGE_KEYS`, not a row to print half of.
                  if let Some(digit) = key_char(key) {
                     binding(theme, ui, &format!("Alt+{digit}"), page.name());
                  }
               }
            });
         });

      // `Escape` and the close button both clear the flag the modal is bound to.
      if !open {
         self.close();
      }
   }
}

/// One named list of bindings.
fn section(theme: &Theme, ui: &mut Ui, title: &'static str, lines: &[(&str, &str)]) {
   section_title(theme, ui, title);

   for (keys, action) in lines {
      binding(theme, ui, keys, action);
   }
}

/// A section's name, drawn and marked as a heading so a reader hears where one list ends.
fn section_title(theme: &Theme, ui: &mut Ui, text: &'static str) {
   let title = ui.label(RichText::new(text).size(theme.typography.large));
   title.widget_info(|| WidgetInfo::labeled(Role::Heading, ui.is_enabled(), text));
}

/// One binding as a single node: the keys and what they do are said together, so a reader walking
/// the list hears "Alt+1: Home" instead of two stops per row.
fn binding(theme: &Theme, ui: &mut Ui, keys: &str, action: &str) {
   let text = format!("{keys}: {action}");

   ui.label(RichText::new(text).size(theme.typography.normal));
}
