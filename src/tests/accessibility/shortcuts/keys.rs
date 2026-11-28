use super::super::app::{main_window, master_wallet, settings_window, wallet_in_cache};
use super::super::harness::{
   announced, clipboard_text, copy_said, focused_node, frame_nodes, frame_nodes_and_output,
   is_control, key_combo, key_release,
};
use crate::core::ZeusCtx;
use crate::gui::{GUI, Page};
use egui::accesskit::{Node, NodeId, Role};
use egui::{Context, Event, Id, Key, Modifiers};

/// Draw the shortcut layer and the panels it moves the focus in, in the app's order.
pub(crate) fn render_shortcuts(
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
pub(crate) fn press_shortcut(
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
pub(super) fn render_account_card(
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
/// Nothing starts or stops the endpoint from the keyboard any more: it listens for as long as Zeus
/// runs, so the brake is the credentials rather than a chord. A key that quietly did nothing used to
/// be the one control a reader needed in a hurry, so the absence is asserted rather than assumed.
#[test]
fn no_chord_stops_the_agent_server() {
   let (ctx, mut gui, zeus) = settings_window();
   zeus.write(|zeus| zeus.vault_unlocked = true);

   let nodes = frame_nodes(
      &ctx,
      vec![key_combo(Key::A, Modifiers::COMMAND | Modifiers::SHIFT)],
      |ui| {
         zeus.write(|zeus| {
            crate::gui::ui::shortcuts::handle(&mut gui, zeus, ui.ctx());
            gui.show_announcer(ui);
         });
      },
   );

   assert!(
      gui.settings.is_open(),
      "the chord belongs to no action, so the window stays up"
   );

   assert!(
      announced(&nodes).is_none_or(|words| words.is_empty()),
      "and nothing is said about a server that was never touched: {:?}",
      announced(&nodes)
   );
}
