use super::app::{main_window, master_wallet, render_settings, settings_window, wallet_in_cache};
use super::harness::{
   accesskit_action, announced, clipboard_text, copy_said, focused_node, frame_nodes,
   frame_nodes_and_output, is_control, key_combo, key_release, label_words,
};
use crate::core::ZeusCtx;
use crate::gui::ui::settings::SettingsPage;
use crate::gui::ui::shortcuts::{Action as ShortcutAction, Binding, Chord};
use crate::gui::{GUI, Page};
use egui::accesskit::{Action, Node, NodeId, Role};
use egui::{Context, Event, Id, Key, Modifiers};

/// Draw the shortcut layer and the panels it moves the focus in, in the app's order.
pub(super) fn render_shortcuts(
   ctx: &Context,
   gui: &mut GUI,
   zeus: &ZeusCtx,
   events: Vec<Event>,
) -> Vec<(NodeId, Node)> {
   frame_nodes(ctx, events, |ui| {
      zeus.write(|zeus| {
         crate::gui::ui::shortcuts::handle(gui, zeus, ui.ctx());
         crate::gui::ui::panels::left_panel::show(gui, zeus, ui);
         crate::gui::ui::panels::central_panel::show(gui, zeus, ui);
      });
   })
}

/// Press one chord and let it go across two frames, the way a keyboard sends it.
///
/// A press the window never sees released is a key held down, and egui delivers the next press of
/// that key as an auto-repeat — which the layer ignores on purpose. A test that pressed the same
/// key twice without a release would therefore prove nothing about the second press.
pub(super) fn press_shortcut(
   ctx: &Context,
   gui: &mut GUI,
   zeus: &ZeusCtx,
   key: Key,
   modifiers: Modifiers,
) -> Vec<(NodeId, Node)> {
   render_shortcuts(ctx, gui, zeus, vec![key_combo(key, modifiers)]);

   // The release is a frame of its own, and it draws the same panels: a widget that is not drawn
   // again has its focus given up, so the frame that shows where the press landed is this one.
   render_shortcuts(ctx, gui, zeus, vec![key_release(key)])
}

/// Every `Alt` digit opens the page the nav puts in that position.
#[test]
fn alt_digits_open_the_page_on_their_number() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| zeus.vault_unlocked = true);

   let pages = [
      (Key::Num1, Page::Home),
      (Key::Num2, Page::Send),
      (Key::Num3, Page::Shield),
      (Key::Num4, Page::Swap),
      (Key::Num5, Page::Bridge),
      (Key::Num6, Page::Wallets),
      (Key::Num7, Page::Transactions),
      (Key::Num8, Page::Approvals),
   ];

   for (key, page) in pages {
      frame_nodes(&ctx, vec![key_combo(key, Modifiers::ALT)], |ui| {
         zeus.write(|zeus| crate::gui::ui::shortcuts::handle(&mut gui, zeus, ui.ctx()));
      });

      assert_eq!(
         gui.current_page(),
         page,
         "Alt+{key:?} must open {page:?}"
      );
   }

   frame_nodes(
      &ctx,
      vec![key_combo(Key::Num0, Modifiers::ALT)],
      |ui| {
         zeus.write(|zeus| crate::gui::ui::shortcuts::handle(&mut gui, zeus, ui.ctx()));
      },
   );

   assert!(
      gui.connected_dapps.is_open(),
      "Alt+0 must open Connected Dapps"
   );

   // A dialog keeps the keyboard until it closes, so the next check closes this one first.
   gui.connected_dapps.close();

   frame_nodes(
      &ctx,
      vec![key_combo(Key::Num9, Modifiers::ALT)],
      |ui| {
         zeus.write(|zeus| crate::gui::ui::shortcuts::handle(&mut gui, zeus, ui.ctx()));
      },
   );

   assert!(gui.settings.is_open(), "Alt+9 must open Settings");
}

/// The page a shortcut opens takes the focus, exactly as a nav click does.
#[test]
fn a_shortcut_opened_page_puts_the_focus_on_its_heading() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| zeus.vault_unlocked = true);
   wallet_in_cache(&zeus);

   let nodes = press_shortcut(&ctx, &mut gui, &zeus, Key::Num2, Modifiers::ALT);
   let focused = focused_node(&ctx, &nodes);

   assert_eq!(focused.role(), Role::Heading);
   assert_eq!(focused.label(), Some("Send"));
}

/// F6 walks the regions clockwise and skips the ones with nothing to focus.
#[test]
fn f6_cycles_from_the_page_to_the_nav() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| zeus.vault_unlocked = true);
   wallet_in_cache(&zeus);
   gui.send_crypto.open();

   // The page change puts the focus on the heading and records the page.
   render_shortcuts(&ctx, &mut gui, &zeus, Vec::new());
   // Main -> Status (closed) -> Account (closed) -> Navigation.
   let nodes = press_shortcut(&ctx, &mut gui, &zeus, Key::F6, Modifiers::NONE);

   let focused = focused_node(&ctx, &nodes);

   assert_eq!(focused.role(), Role::Button);
   assert_eq!(focused.label(), Some("Send"));

   // Navigation -> Main: the same key comes back to the page heading — the heading node, not the
   // nav plate that carries the same word.
   let nodes = press_shortcut(&ctx, &mut gui, &zeus, Key::F6, Modifiers::NONE);
   let focused = focused_node(&ctx, &nodes);

   assert_eq!(focused.role(), Role::Heading);
   assert_eq!(focused.label(), Some("Send"));
}

/// The account card is a region too, and F6 reaches its address row when a wallet is shown.
#[test]
fn f6_reaches_the_account_card_when_a_wallet_is_shown() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| zeus.vault_unlocked = true);
   wallet_in_cache(&zeus);
   gui.account_panel.open();

   render_shortcuts(&ctx, &mut gui, &zeus, Vec::new());
   // Main -> Status (closed) -> Account.
   let nodes = press_shortcut(&ctx, &mut gui, &zeus, Key::F6, Modifiers::NONE);

   let focused = focused_node(&ctx, &nodes);

   assert_eq!(focused.role(), Role::Button);
   assert!(
      focused.label().is_some_and(|label| label.starts_with("0x")),
      "the account card's address row takes the focus"
   );
}

/// The lock screen owns the keyboard: a shortcut must not open a page behind it.
#[test]
fn shortcuts_are_inert_behind_the_lock_screen() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| {
      zeus.vault_unlocked = true;
      zeus.locked = true;
   });

   frame_nodes(
      &ctx,
      vec![key_combo(Key::Num2, Modifiers::ALT)],
      |ui| {
         zeus.write(|zeus| crate::gui::ui::shortcuts::handle(&mut gui, zeus, ui.ctx()));
      },
   );

   assert!(!gui.send_crypto.is_open());
}

/// A dialog keeps the keyboard until it closes: a shortcut must not move the page under it.
#[test]
fn shortcuts_do_not_reach_behind_a_dialog() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| {
      zeus.vault_unlocked = true;
      gui.settings.open(zeus, &mut gui.appearance_settings);
   });

   frame_nodes(
      &ctx,
      vec![key_combo(Key::Num2, Modifiers::ALT)],
      |ui| {
         zeus.write(|zeus| crate::gui::ui::shortcuts::handle(&mut gui, zeus, ui.ctx()));
      },
   );

   assert!(gui.settings.is_open());
   assert!(!gui.send_crypto.is_open());
}

/// Alt+N is the one way to reach a notification from any page.
#[test]
fn alt_n_moves_the_focus_to_the_notification() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| zeus.vault_unlocked = true);
   gui.notification.open_with_spinner(
      "Transfer".to_string(),
      crate::gui::ui::NotificationType::Other(String::new()),
   );

   let mut nodes = Vec::new();

   for pass in 0..2 {
      let events = if pass == 1 {
         vec![key_combo(Key::N, Modifiers::ALT)]
      } else {
         Vec::new()
      };
      let theme = gui.theme.clone();
      let icons = gui.icons.clone();

      nodes = frame_nodes(&ctx, events, |ui| {
         zeus.write(|zeus| {
            crate::gui::ui::shortcuts::handle(&mut gui, zeus, ui.ctx());
            gui.notification.show(zeus, &theme, icons.clone(), ui);
         });
      });
   }

   assert_eq!(
      focused_node(&ctx, &nodes).label(),
      Some("Dismiss")
   );
}

/// The character an Alt chord can produce on X11 goes with the key, not into the field.
#[test]
fn an_alt_digit_does_not_type_into_a_focused_field() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| zeus.vault_unlocked = true);

   let id = Id::unique("shortcut_probe");
   let mut text = String::new();

   frame_nodes(&ctx, Vec::new(), |ui| {
      ui.add(egui::TextEdit::singleline(&mut text).id(id));
      ui.memory_mut(|memory| memory.request_focus(id));
   });

   frame_nodes(
      &ctx,
      vec![
         key_combo(Key::Num2, Modifiers::ALT),
         Event::Text("2".to_string()),
      ],
      |ui| {
         zeus.write(|zeus| crate::gui::ui::shortcuts::handle(&mut gui, zeus, ui.ctx()));
         ui.add(egui::TextEdit::singleline(&mut text).id(id));
      },
   );

   assert_eq!(text, "");
   assert!(gui.send_crypto.is_open());
}

/// Draw the shortcut layer, the panels it moves the focus in and the app's live region, in the
/// The shortcut layer over the account card, with the app's live region after it.
///
/// The copy tests draw the card the shortcut copies from and not the pages behind it: those read a
/// wallet cache and an RPC this test does not stand up.
fn render_account_card(
   ctx: &Context,
   gui: &mut GUI,
   zeus: &ZeusCtx,
   events: Vec<Event>,
) -> (Vec<(NodeId, Node)>, Vec<egui::OutputCommand>) {
   frame_nodes_and_output(ctx, events, |ui| {
      zeus.write(|zeus| {
         crate::gui::ui::shortcuts::handle(gui, zeus, ui.ctx());
         crate::gui::ui::panels::left_panel::show(gui, zeus, ui);
      });

      gui.show_announcer(ui);
   })
}

/// `Ctrl+Tab` asks for the next control. `Tab` alone stops on every line of text — that is what puts
/// the page's words on the keyboard path — so the shortcut hops in a pass where a line is not a tab
/// stop at all.
#[test]
fn ctrl_tab_lands_on_the_next_control() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| zeus.vault_unlocked = true);
   wallet_in_cache(&zeus);

   // The reader model: every `Label` is focusable, which is what gives this key work to do.
   ctx.options_mut(|options| options.screen_reader = true);

   // Alt+2 opens Send and leaves the focus on its heading, so the walk starts on prose.
   press_shortcut(&ctx, &mut gui, &zeus, Key::Num2, Modifiers::ALT);

   let nodes = press_shortcut(&ctx, &mut gui, &zeus, Key::Tab, Modifiers::NONE);

   assert!(
      !is_control(focused_node(&ctx, &nodes).role()),
      "Tab alone must stop on the next line of text"
   );

   let nodes = press_shortcut(&ctx, &mut gui, &zeus, Key::Tab, Modifiers::CTRL);
   let focused = focused_node(&ctx, &nodes);

   assert!(
      is_control(focused.role()),
      "Ctrl+Tab must land on a control, not on another line: {:?}",
      focused.role()
   );

   assert!(
      ctx.options(|options| options.screen_reader),
      "the pass after the hop must have the reader's model back, or prose would leave the keyboard"
   );
}

/// `Ctrl+Shift+Tab` is the same hop backwards. From the first control it stops on the page's own
/// heading — the landmark the walk started at, which the page makes a tab stop on purpose — and a
/// hop forward is the control it came from.
#[test]
fn ctrl_shift_tab_is_the_same_hop_backwards() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| zeus.vault_unlocked = true);
   wallet_in_cache(&zeus);
   ctx.options_mut(|options| options.screen_reader = true);

   press_shortcut(&ctx, &mut gui, &zeus, Key::Num2, Modifiers::ALT);

   let nodes = press_shortcut(&ctx, &mut gui, &zeus, Key::Tab, Modifiers::CTRL);
   let forward = focused_node(&ctx, &nodes).label().map(str::to_string);

   let nodes = press_shortcut(
      &ctx,
      &mut gui,
      &zeus,
      Key::Tab,
      Modifiers::CTRL | Modifiers::SHIFT,
   );
   let back = focused_node(&ctx, &nodes);

   assert_eq!(
      back.role(),
      Role::Heading,
      "the hop back must not stop on a line of text"
   );
   assert_eq!(
      back.label(),
      Some("Send"),
      "it stops on the page's own heading, which is the landmark before the first control"
   );

   let nodes = press_shortcut(&ctx, &mut gui, &zeus, Key::Tab, Modifiers::CTRL);

   assert_eq!(
      focused_node(&ctx, &nodes).label().map(str::to_string),
      forward,
      "forward again is the control the walk came from"
   );
}

/// `Ctrl+Shift+C` copies the address the account card shows, and says which one: the clipboard is
/// invisible, so the words are the only way to check what landed there.
#[test]
fn ctrl_shift_c_copies_the_address_and_says_which_one() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| zeus.vault_unlocked = true);

   let wallet = master_wallet("Main");
   let address = wallet.address.to_string();

   gui.account_panel.open();
   gui.account_panel.set_current_wallet(wallet);

   // Quiet passes first: the node a message is written to has to exist before the one that says it.
   render_account_card(&ctx, &mut gui, &zeus, Vec::new());
   render_account_card(&ctx, &mut gui, &zeus, Vec::new());

   let (nodes, commands) = render_account_card(
      &ctx,
      &mut gui,
      &zeus,
      vec![key_combo(Key::C, Modifiers::COMMAND | Modifiers::SHIFT)],
   );

   assert_eq!(
      clipboard_text(&commands),
      Some(address.clone()),
      "the whole address the card shows must land on the clipboard"
   );

   assert_eq!(
      announced(&nodes).as_deref(),
      Some(copy_said(&address).as_str()),
      "the copy must say which address it copied"
   );
}

/// In privacy mode the card shows a zk address. A wallet without one has nothing to copy, and the
/// shortcut must say so rather than hand over the public address the card is hiding.
#[test]
fn ctrl_shift_c_says_when_there_is_no_privacy_address() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| {
      zeus.vault_unlocked = true;
      zeus.privacy_mode = true;
   });

   gui.account_panel.open();
   gui.account_panel.set_current_wallet(master_wallet("Main"));

   render_account_card(&ctx, &mut gui, &zeus, Vec::new());
   render_account_card(&ctx, &mut gui, &zeus, Vec::new());

   let (nodes, commands) = render_account_card(
      &ctx,
      &mut gui,
      &zeus,
      vec![key_combo(Key::C, Modifiers::COMMAND | Modifiers::SHIFT)],
   );

   assert_eq!(
      clipboard_text(&commands),
      None,
      "the public address the card is hiding must not be handed over"
   );

   assert_eq!(
      announced(&nodes).as_deref(),
      Some("This wallet has no privacy address to copy."),
      "the shortcut must say why nothing was copied"
   );
}

/// Ctrl+Enter asks the Send page to press its own button; it never signs anything itself.
#[test]
fn ctrl_enter_asks_the_send_page_to_press_its_own_button() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| zeus.vault_unlocked = true);
   gui.send_crypto.open();

   frame_nodes(
      &ctx,
      vec![key_combo(Key::Enter, Modifiers::COMMAND)],
      |ui| {
         zeus.write(|zeus| crate::gui::ui::shortcuts::handle(&mut gui, zeus, ui.ctx()));
      },
   );

   assert!(gui.send_crypto.send_requested());
}

/// On any other page the same key is not a Send.
#[test]
fn ctrl_enter_on_another_page_does_not_press_send() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| zeus.vault_unlocked = true);

   frame_nodes(
      &ctx,
      vec![key_combo(Key::Enter, Modifiers::COMMAND)],
      |ui| {
         zeus.write(|zeus| crate::gui::ui::shortcuts::handle(&mut gui, zeus, ui.ctx()));
      },
   );

   assert!(!gui.send_crypto.send_requested());
}

/// Draw the app-wide overlays twice: the sheet is an `egui::Area` measured on the pass it opens,
/// and egui exposes no node for a widget it considers invisible.
fn draw_overlays(ctx: &Context, gui: &mut GUI) -> Vec<(NodeId, Node)> {
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
