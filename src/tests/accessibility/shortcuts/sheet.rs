use super::super::app::{main_window, render_settings, settings_window, wallet_in_cache};
use super::super::harness::{accesskit_action, focused_node, frame_nodes, key_combo, label_words};
use super::keys::{press_shortcut, render_shortcuts};
use crate::gui::ui::shortcuts::{Action as ShortcutAction, Binding, Chord};
use crate::gui::{GUI, Page};
use egui::accesskit::{Action, Node, NodeId, Role};
use egui::{Context, Key, Modifiers};

/// Draw the app-wide overlays twice: the sheet is an `egui::Area` measured on the pass it opens,
/// and egui exposes no node for a widget it considers invisible.
pub(crate) fn draw_overlays(ctx: &Context, gui: &mut GUI) -> Vec<(NodeId, Node)> {
   frame_nodes(ctx, Vec::new(), |ui| gui.show_overlay_modals(ui));

   frame_nodes(ctx, Vec::new(), |ui| gui.show_overlay_modals(ui))
}

/// F1 puts the sheet up, and it lists the keys the app answers: the page rows come from the same
/// table the `Alt` digits are read from, so the sheet cannot promise a key that does nothing.
#[test]
fn f1_opens_the_sheet_of_every_binding() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| zeus.vault_unlocked = true);
   wallet_in_cache(&zeus);

   press_shortcut(&ctx, &mut gui, &zeus, Key::F1, Modifiers::NONE);

   assert!(
      gui.shortcut_sheet.is_open(),
      "F1 must put the sheet up"
   );

   let nodes = draw_overlays(&ctx, &mut gui);

   let dialog = nodes
      .iter()
      .map(|(_, node)| node)
      .find(|node| node.role() == Role::Dialog)
      .expect("the sheet is a dialog");

   assert_eq!(
      dialog.label(),
      Some("Keyboard shortcuts"),
      "the sheet names itself the way its heading says"
   );

   let labels = label_words(&nodes);

   for expected in [
      "Alt+1: Home",
      "Alt+2: Send",
      "Alt+3: Shield",
      "Alt+4: Swap",
      "Alt+5: Bridge",
      "Alt+6: Wallets",
      "Alt+7: Transactions",
      "Alt+8: Approvals",
      "Alt+9: Settings",
      "Alt+0: Connected Dapps",
      "Tab / Shift+Tab: the next or previous control",
      "F6: go to the next part: the account, the navigation, the page, the notification",
      "Shift+F6: go back to the previous part",
      "Ctrl+Tab: the next control, skipping the text between them",
      "Ctrl+Shift+Tab: the previous control, skipping the text between them",
      "Ctrl+Enter: press Send on the Send page",
      "Alt+N: go to the open notification",
      "Ctrl+Shift+C: copy your address",
      "F1: show or hide this list",
      "Ctrl+K: list every command, and run one by name",
   ] {
      assert!(
         labels.iter().any(|label| label == expected),
         "the sheet must list {expected:?}: {labels:?}"
      );
   }
}

/// The sheet takes the keyboard when it opens: its own close button has the focus, so Tab walks
/// the list from inside the dialog and Escape has something to act on.
#[test]
fn the_sheet_takes_the_focus_when_it_opens() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| zeus.vault_unlocked = true);
   wallet_in_cache(&zeus);

   press_shortcut(&ctx, &mut gui, &zeus, Key::F1, Modifiers::NONE);

   let nodes = draw_overlays(&ctx, &mut gui);
   let focused = focused_node(&ctx, &nodes);

   assert_eq!(focused.role(), Role::Button);
   assert_eq!(focused.label(), Some("Close"));
}

/// The sheet closes the way a dialog does — Escape — and the key that opened it puts it away too.
#[test]
fn escape_and_f1_put_the_sheet_away() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| zeus.vault_unlocked = true);
   wallet_in_cache(&zeus);

   press_shortcut(&ctx, &mut gui, &zeus, Key::F1, Modifiers::NONE);
   assert!(
      gui.shortcut_sheet.is_open(),
      "F1 must put the sheet up"
   );

   draw_overlays(&ctx, &mut gui);

   frame_nodes(
      &ctx,
      vec![key_combo(Key::Escape, Modifiers::NONE)],
      |ui| gui.show_overlay_modals(ui),
   );

   assert!(
      !gui.shortcut_sheet.is_open(),
      "Escape must put the sheet away"
   );

   press_shortcut(&ctx, &mut gui, &zeus, Key::F1, Modifiers::NONE);
   assert!(gui.shortcut_sheet.is_open(), "F1 opens it again");

   press_shortcut(&ctx, &mut gui, &zeus, Key::F1, Modifiers::NONE);
   assert!(
      !gui.shortcut_sheet.is_open(),
      "a second F1 puts it away"
   );
}

/// The sheet holds the keyboard while it is up: a page key pressed behind it changes nothing.
#[test]
fn a_page_key_does_not_reach_behind_the_sheet() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| zeus.vault_unlocked = true);
   wallet_in_cache(&zeus);

   press_shortcut(&ctx, &mut gui, &zeus, Key::F1, Modifiers::NONE);
   render_shortcuts(
      &ctx,
      &mut gui,
      &zeus,
      vec![key_combo(Key::Num2, Modifiers::ALT)],
   );

   assert_eq!(
      gui.current_page(),
      Page::Home,
      "the sheet keeps the page where it was"
   );
}

/// The lock screen and a dialog both own the keyboard, so F1 reaches neither of them.
#[test]
fn f1_does_not_reach_past_the_lock_screen_or_a_dialog() {
   let (ctx, mut gui, zeus) = main_window();

   press_shortcut(&ctx, &mut gui, &zeus, Key::F1, Modifiers::NONE);

   assert!(
      !gui.shortcut_sheet.is_open(),
      "the lock screen owns the keyboard"
   );

   zeus.write(|zeus| zeus.vault_unlocked = true);
   wallet_in_cache(&zeus);
   gui.msg_window.open("A message over the page");

   press_shortcut(&ctx, &mut gui, &zeus, Key::F1, Modifiers::NONE);

   assert!(
      !gui.shortcut_sheet.is_open(),
      "a dialog keeps the keyboard until it closes"
   );
}

/// The same list is behind a button on the Settings page, for a user who never tries F1: the press
/// is handed up to the frame, which is the only thing that can draw a window over another one.
#[test]
fn the_general_page_opens_the_sheet_with_a_button() {
   let (ctx, mut gui, zeus) = settings_window();

   let nodes = render_settings(&ctx, &mut gui, &zeus, Vec::new());
   let button = nodes
      .iter()
      .find(|(_, node)| {
         node.role() == Role::Button && node.label() == Some("Show Keyboard Shortcuts")
      })
      .map(|(id, _)| *id)
      .expect("the General page draws the button that puts the sheet up");

   render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, button)],
   );

   // What the app does with the request, on the pass the page raised it.
   if gui.settings.take_shortcuts_request() {
      gui.shortcut_sheet.open();
   }

   assert!(
      gui.shortcut_sheet.is_open(),
      "the button must put the sheet up"
   );
}

/// The sheet prints the chord in the table, so a remapped key is described by the key that runs it.
#[test]
fn the_sheet_shows_the_chord_the_user_chose() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| zeus.vault_unlocked = true);
   wallet_in_cache(&zeus);

   gui.keymap
      .set(
         ShortcutAction::CopyAddress,
         Binding::new(Key::L, Chord::Alt),
      )
      .expect("Alt+L is a free chord");

   press_shortcut(&ctx, &mut gui, &zeus, Key::F1, Modifiers::NONE);

   let words = label_words(&draw_overlays(&ctx, &mut gui));

   assert!(
      words.iter().any(|word| *word == "Alt+L: copy your address"),
      "the sheet must describe the chord that answers: {words:?}"
   );
   assert!(
      !words.iter().any(|word| *word == "Ctrl+Shift+C: copy your address"),
      "the sheet must not keep describing the chord the action left: {words:?}"
   );
}
