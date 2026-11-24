use super::app::{main_window, master_wallet, render_settings, settings_window, wallet_in_cache};
use super::harness::{accesskit_action, clipboard_text, frame_nodes, key_combo, label_words};
use super::shortcuts::keys::{press_shortcut, render_account_card, render_shortcuts};
use super::shortcuts::sheet::draw_overlays;
use crate::core::ZeusCtx;
use crate::gui::ui::settings::SettingsPage;
use crate::gui::ui::shortcuts::{Action as ShortcutAction, Binding, Chord};
use crate::gui::{GUI, Page};
use egui::accesskit::{Action, Node, NodeId, Role};
use egui::{Context, Event, Key, Modifiers};

/// The palette lists every chord the app answers and every page the nav lists: a command a reader
/// cannot find is a command that does not exist.
#[test]
fn ctrl_k_lists_every_command() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| zeus.vault_unlocked = true);
   wallet_in_cache(&zeus);

   press_shortcut(&ctx, &mut gui, &zeus, Key::K, Modifiers::COMMAND);

   assert!(
      gui.palette.is_open(),
      "Ctrl+K must put the palette up"
   );

   let nodes = draw_overlays(&ctx, &mut gui);

   let dialog = nodes
      .iter()
      .map(|(_, node)| node)
      .find(|node| node.role() == Role::Dialog)
      .expect("the palette is a dialog");

   assert_eq!(
      dialog.label(),
      Some("Commands"),
      "the palette names itself the way its heading says"
   );

   let names: Vec<&str> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Button)
      .filter_map(|(_, node)| node.label())
      .collect();

   for action in ShortcutAction::ALL {
      let expected = format!(
         "{}: {}",
         action.label(),
         gui.keymap.binding(action).label()
      );

      assert!(
         names.contains(&expected.as_str()),
         "the palette must list {expected:?}: {names:?}"
      );
   }

   for page in [
      Page::Home,
      Page::Send,
      Page::Shield,
      Page::Swap,
      Page::Bridge,
      Page::Wallets,
      Page::Transactions,
      Page::Approvals,
      Page::Settings,
      Page::ConnectedDapps,
   ] {
      let prefix = format!("Go to {}:", page.name());

      assert!(
         names.iter().any(|name| name.starts_with(prefix.as_str())),
         "the palette must list {prefix:?}: {names:?}"
      );
   }

   assert!(
      names.contains(&"Change keyboard shortcuts"),
      "the page that changes the chords is a command too: {names:?}"
   );

   let words = label_words(&nodes);
   let count = ShortcutAction::ALL.len() + 10 + 1;

   assert!(
      words.iter().any(|word| *word == format!("{count} commands")),
      "the palette must say how many commands it holds: {words:?}"
   );
}

/// The chord that opened the palette is the chord that closes it, so a user who moves that key is
/// not left with a dialog only the old key leaves.
#[test]
fn the_chord_that_opened_the_palette_closes_it() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| zeus.vault_unlocked = true);
   wallet_in_cache(&zeus);

   gui.keymap
      .set(
         ShortcutAction::OpenPalette,
         Binding::new(Key::P, Chord::Alt),
      )
      .expect("Alt+P is a free chord");

   press_shortcut(&ctx, &mut gui, &zeus, Key::P, Modifiers::ALT);

   assert!(
      gui.palette.is_open(),
      "the chord the user chose must open it"
   );

   press_shortcut(&ctx, &mut gui, &zeus, Key::P, Modifiers::ALT);

   assert!(
      !gui.palette.is_open(),
      "the same chord must close it"
   );
}

/// Escape closes the palette the way it closes every other dialog.
#[test]
fn escape_puts_the_palette_away() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| zeus.vault_unlocked = true);
   wallet_in_cache(&zeus);

   press_shortcut(&ctx, &mut gui, &zeus, Key::K, Modifiers::COMMAND);
   draw_overlays(&ctx, &mut gui);

   frame_nodes(
      &ctx,
      vec![key_combo(Key::Escape, Modifiers::NONE)],
      |ui| gui.show_overlay_modals(ui),
   );

   assert!(
      !gui.palette.is_open(),
      "Escape must put the palette away"
   );
}

/// Enter runs the row the query leaves first, and the command runs on the pass that has the context
/// to run it: the palette itself draws in a pass without one.
#[test]
fn enter_runs_the_command_the_query_found() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| zeus.vault_unlocked = true);
   wallet_in_cache(&zeus);

   press_shortcut(&ctx, &mut gui, &zeus, Key::K, Modifiers::COMMAND);

   // The field asks for the focus on the pass the palette first paints in.
   draw_overlays(&ctx, &mut gui);

   gui.palette.set_query("transactions");

   frame_nodes(
      &ctx,
      vec![key_combo(Key::Enter, Modifiers::NONE)],
      |ui| {
         zeus.write(|zeus| crate::gui::ui::shortcuts::handle(&mut gui, zeus, ui.ctx()));
      },
   );

   assert!(
      !gui.palette.is_open(),
      "Enter runs the command and puts the palette away"
   );

   frame_nodes(&ctx, Vec::new(), |ui| {
      zeus.write(|zeus| crate::gui::ui::shortcuts::handle(&mut gui, zeus, ui.ctx()));
   });

   assert_eq!(
      gui.current_page(),
      Page::Transactions,
      "the command must open the page the query named"
   );
}

/// The palette owns the keyboard while it is up: a page key pressed behind it changes nothing.
#[test]
fn a_page_key_does_not_reach_behind_the_palette() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| zeus.vault_unlocked = true);
   wallet_in_cache(&zeus);

   press_shortcut(&ctx, &mut gui, &zeus, Key::K, Modifiers::COMMAND);
   render_shortcuts(
      &ctx,
      &mut gui,
      &zeus,
      vec![key_combo(Key::Num2, Modifiers::ALT)],
   );

   assert_eq!(
      gui.current_page(),
      Page::Home,
      "the palette keeps the page where it was"
   );
}

/// The lock screen owns the keyboard, so the palette cannot open over it.
#[test]
fn the_palette_is_inert_behind_the_lock_screen() {
   let (ctx, mut gui, zeus) = main_window();

   press_shortcut(&ctx, &mut gui, &zeus, Key::K, Modifiers::COMMAND);

   assert!(
      !gui.palette.is_open(),
      "the lock screen owns the keyboard"
   );
}

/// A chord the user moved takes its action with it, and the chord it left does nothing: the layer
/// reads the table, not a default written into the handler.
#[test]
fn a_remapped_chord_copies_the_address_and_the_old_one_stops() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| zeus.vault_unlocked = true);

   let wallet = master_wallet("Main");
   let address = wallet.address.to_string();

   gui.account_panel.open();
   gui.account_panel.set_current_wallet(wallet);

   gui.keymap
      .set(
         ShortcutAction::CopyAddress,
         Binding::new(Key::L, Chord::Alt),
      )
      .expect("Alt+L is a free chord");

   render_account_card(&ctx, &mut gui, &zeus, Vec::new());
   render_account_card(&ctx, &mut gui, &zeus, Vec::new());

   let (_nodes, commands) = render_account_card(
      &ctx,
      &mut gui,
      &zeus,
      vec![key_combo(Key::L, Modifiers::ALT)],
   );

   assert_eq!(
      clipboard_text(&commands),
      Some(address),
      "the chord the user chose must copy the address"
   );

   let (_nodes, commands) = render_account_card(
      &ctx,
      &mut gui,
      &zeus,
      vec![key_combo(Key::C, Modifiers::COMMAND | Modifiers::SHIFT)],
   );

   assert_eq!(
      clipboard_text(&commands),
      None,
      "the chord the action was moved off must do nothing"
   );
}

/// The Keyboard page offers a recorder for every action, each named after the action it changes.
#[test]
fn the_keyboard_page_names_every_action() {
   let (ctx, mut gui, zeus) = settings_window();
   zeus.write(|zeus| {
      gui.settings.open_page(
         SettingsPage::Keyboard,
         zeus,
         &mut gui.appearance_settings,
      );
   });

   let nodes = render_settings(&ctx, &mut gui, &zeus, Vec::new());

   let names: Vec<&str> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Button)
      .filter_map(|(_, node)| node.label())
      .collect();

   for action in ShortcutAction::ALL {
      let expected = format!("Change {}", action.label());

      assert!(
         names.contains(&expected.as_str()),
         "the page must offer {expected:?}: {names:?}"
      );
   }
}

/// One frame with the Settings window up, the shortcut layer in front of it and one key in the
/// frame's input, which is the order the app runs them in.
fn record_chord(
   ctx: &Context,
   gui: &mut GUI,
   zeus: &ZeusCtx,
   events: Vec<Event>,
) -> Vec<(NodeId, Node)> {
   let icons = gui.icons.clone();
   let theme = gui.theme.clone();

   frame_nodes(ctx, events, |ui| {
      zeus.write(|zeus| {
         crate::gui::ui::shortcuts::handle(gui, zeus, ui.ctx());
         gui.settings.show(
            zeus,
            icons.clone(),
            &theme,
            &mut gui.appearance_settings,
            &mut gui.keymap,
            ui,
         );
      });
   })
}

/// The button that records Copy address, in the tree the page drew.
fn copy_address_recorder(nodes: &[(NodeId, Node)]) -> NodeId {
   nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some("Change Copy address"))
      .map(|(id, _)| *id)
      .expect("the page draws the button that records Copy address")
}

/// A refused chord is said out loud and the recording stays open for the next attempt.
///
/// The chord chosen here is the one Previous control answers, which is what proves the shortcut
/// layer stood down: were it still reading the keyboard, it would have taken the key and the page
/// would never have seen it.
#[test]
fn a_refused_chord_keeps_the_recording_open() {
   let (ctx, mut gui, zeus) = settings_window();
   zeus.write(|zeus| {
      gui.settings.open_page(
         SettingsPage::Keyboard,
         zeus,
         &mut gui.appearance_settings,
      );
   });

   let nodes = render_settings(&ctx, &mut gui, &zeus, Vec::new());
   let recorder = copy_address_recorder(&nodes);

   render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, recorder)],
   );

   assert!(
      gui.settings.is_capturing(),
      "the button starts recording"
   );

   record_chord(
      &ctx,
      &mut gui,
      &zeus,
      vec![key_combo(Key::Tab, Modifiers::CTRL | Modifiers::SHIFT)],
   );

   let notice = gui.settings.take_notice().expect("a refused chord must be said");

   assert!(
      notice.contains("Previous control"),
      "the refusal must name the action that already answers the chord: {notice:?}"
   );
   assert!(
      gui.settings.is_capturing(),
      "a refused chord keeps the recording open for the next attempt"
   );
}

/// A text field's own chord cannot be recorded: the layer takes keys before a field draws, so the
/// binding would break copy, paste and undo in every text field in the app.
#[test]
fn a_text_fields_chord_cannot_be_recorded() {
   let (ctx, mut gui, zeus) = settings_window();
   zeus.write(|zeus| {
      gui.settings.open_page(
         SettingsPage::Keyboard,
         zeus,
         &mut gui.appearance_settings,
      );
   });

   let nodes = render_settings(&ctx, &mut gui, &zeus, Vec::new());
   let recorder = copy_address_recorder(&nodes);

   render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, recorder)],
   );

   record_chord(
      &ctx,
      &mut gui,
      &zeus,
      vec![key_combo(Key::C, Modifiers::COMMAND)],
   );

   let notice = gui.settings.take_notice().expect("a refused chord must be said");

   assert!(
      notice.contains("text fields"),
      "the refusal must say why the chord cannot be used: {notice:?}"
   );
   assert_eq!(
      gui.keymap.binding(ShortcutAction::CopyAddress),
      ShortcutAction::CopyAddress.default_binding(),
      "a refused chord changes nothing"
   );
}

/// The palette can reach the page that changes the chords, so a user who never opens Settings still
/// finds it.
#[test]
fn the_palette_opens_the_keyboard_settings() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| zeus.vault_unlocked = true);
   wallet_in_cache(&zeus);

   press_shortcut(&ctx, &mut gui, &zeus, Key::K, Modifiers::COMMAND);
   draw_overlays(&ctx, &mut gui);

   gui.palette.set_query("change keyboard");

   frame_nodes(
      &ctx,
      vec![key_combo(Key::Enter, Modifiers::NONE)],
      |ui| {
         zeus.write(|zeus| crate::gui::ui::shortcuts::handle(&mut gui, zeus, ui.ctx()));
      },
   );

   frame_nodes(&ctx, Vec::new(), |ui| {
      zeus.write(|zeus| crate::gui::ui::shortcuts::handle(&mut gui, zeus, ui.ctx()));
   });

   assert!(
      gui.settings.is_open(),
      "the command must open Settings"
   );

   let words = label_words(&render_settings(
      &ctx,
      &mut gui,
      &zeus,
      Vec::new(),
   ));

   assert!(
      words.iter().any(|word| *word == "Keyboard"),
      "Settings must land on the Keyboard page: {words:?}"
   );
}

/// A reader's window never hides in the tray: the icon behind it is a pointer target, so a hidden
/// window leaves the keyboard and alt-tab both, with nothing but a mouse to bring it back.
#[test]
fn a_reader_window_never_minimizes_into_the_tray() {
   use crate::gui::tray::minimize_hides_window;

   assert!(
      minimize_hides_window(true, false),
      "a sighted user's preference still hides the window"
   );

   assert!(
      !minimize_hides_window(true, true),
      "a reader's window must stay where the keyboard and alt-tab can reach it"
   );

   assert!(
      !minimize_hides_window(false, false),
      "the preference off is the preference off"
   );
}
