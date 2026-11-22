//! Global keyboard shortcuts.
//!
//! One layer owns every chord, so a key is either acted on once or not at all. The layer consumes
//! the key and any text event the platform pairs with it (on X11 Alt+1 can arrive as the character
//! "1" as well), then acts only when the app is showing a page: the lock screen owns the keyboard,
//! and a dialog keeps it until it closes.
//!
//! A shortcut never signs or deletes anything. `Ctrl+Enter` asks the Send page to press its own
//! Send button, and the transaction still waits for its confirmation window.

use crate::core::ZeusContext;
use crate::gui::ui::panels::left_panel;
use crate::gui::{FocusRegion, GUI, Page};
use egui::{Context, Event, Key, Modifiers};

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

   let previous_region = take_key(egui_ctx, Key::F6, Chord::Shift);
   let next_region = take_key(egui_ctx, Key::F6, Chord::Plain);
   let review = take_key(egui_ctx, Key::Enter, Chord::Command);
   let notification = take_key(egui_ctx, Key::N, Chord::Alt);

   let mut target = None;

   for (key, page) in PAGE_KEYS {
      if take_key(egui_ctx, key, Chord::Alt) {
         target = Some(page);
      }
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
   gui.settings.is_open()
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
   Command,
}

impl Chord {
   fn matches(self, modifiers: &Modifiers) -> bool {
      match self {
         Self::Plain => !modifiers.alt && !modifiers.command && !modifiers.ctrl && !modifiers.shift,
         Self::Shift => modifiers.shift && !modifiers.alt && !modifiers.command && !modifiers.ctrl,
         Self::Alt => modifiers.alt && !modifiers.command && !modifiers.ctrl,
         Self::Command => modifiers.command && !modifiers.alt && !modifiers.shift,
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
