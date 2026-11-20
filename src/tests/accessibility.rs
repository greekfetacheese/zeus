//! Accessibility tree tests.
//!
//! A screen reader is handed a `TreeUpdate` by the platform, and egui builds the same value on
//! every AccessKit-enabled frame, on `FullOutput::platform_output`. Asserting on that value is how
//! a role, a name or an announcement is proven without a reader: the check fails when the node
//! stops being emitted, not when a pixel moves.
//!
//! The helpers below are the shared harness; every screen test renders a real screen headlessly.

use crate::assets::icons::Icons;
use crate::core::clear_signing::ClearDisplay;
use crate::core::signature::msg::Permit2612Details;
use crate::core::tx::{FeeTiers, PriorityTier};
use crate::core::types::{Contact, RailgunConfig};
use crate::core::{
   SignMsgType, TransactionAnalysis, TransactionRich, WalletInfo, WalletValue, ZeusCtx,
};
use crate::gui::appearance::AppearanceSettings;
use crate::gui::ui::sign_msg_window::SignMsgWindow;
use crate::gui::ui::tx::{
   FeeSelection, TxConfirmationWindow, TxWindow, show_analysis_buttons, show_calldata_modal,
   show_tx_diffs_modal, show_tx_options_modal,
};
use crate::gui::{GUI, LockScreen};
use egui::accesskit::{Action, ActionRequest, Live, Node, NodeId, Role, Toggled, TreeId};
use egui::{Context, Event, Key, Modifiers, PointerButton, RawInput, Rect, Ui, pos2, vec2};
use egui_elements::{Theme, ThemeKind};
use std::sync::Arc;
use std::time::{Duration, Instant};
use zeus_eth::alloy_primitives::{Address, U256};
use zeus_eth::currency::{Currency, ERC20Token, NativeCurrency};
use zeus_eth::types::ChainId;
use zeus_eth::utils::NumericValue;

/// Run one AccessKit-enabled frame and return the nodes of the tree update it produced.
///
/// egui's own recipe: enable the tree, run a pass, take
/// `platform_output.accesskit_update` — the same value the platform gives a reader.
fn frame(ctx: &Context, add: impl FnMut(&mut Ui)) -> Vec<Node> {
   frame_nodes(ctx, Vec::new(), add).into_iter().map(|(_, node)| node).collect()
}

/// Run one AccessKit-enabled frame with `events` in its input, keeping the id each node is
/// addressed by: a reader sends an action back to that id, so a test that drives a widget needs it.
fn frame_nodes(ctx: &Context, events: Vec<Event>, add: impl FnMut(&mut Ui)) -> Vec<(NodeId, Node)> {
   ctx.enable_accesskit();

   let mut output = ctx.run_ui(
      RawInput {
         screen_rect: Some(Rect::from_min_size(
            pos2(0.0, 0.0),
            vec2(1290.0, 950.0),
         )),
         events,
         ..Default::default()
      },
      add,
   );

   let update = output
      .platform_output
      .accesskit_update
      .take()
      .expect("an AccessKit-enabled frame must produce a tree update");

   let nodes = update.nodes.into_iter().collect();
   output.drop_without_applying_deltas();

   nodes
}

/// The unlock screen and what it needs: the simplest real screen Zeus has — a heading, a line
/// of prose, the credentials form and its Unlock button — and the only one that renders without
/// a wallet, an RPC or a single icon.
fn unlock_screen() -> (ZeusCtx, LockScreen, Theme) {
   let locked = ZeusCtx::new(crate::runtime::test_handle());
   locked.write(|ctx| ctx.locked = true);

   (
      locked,
      LockScreen::new(),
      Theme::new(ThemeKind::TokyoNight),
   )
}

/// Render the unlock screen and return the tree of its second frame.
///
/// Two passes on purpose: the screen is an `egui::Window`, which is measured on its first one,
/// and egui exposes no node for a widget it considers invisible (`is_accesskit_visible`).
fn render_unlock_screen(
   ctx: &Context,
   locked: &ZeusCtx,
   screen: &mut LockScreen,
   theme: &Theme,
) -> Vec<Node> {
   frame(ctx, |ui| {
      locked.write(|zeus| screen.show(zeus, theme, ui));
   });

   frame(ctx, |ui| {
      locked.write(|zeus| screen.show(zeus, theme, ui));
   })
}

/// The text of every `Role::Label` node. AccessKit puts a label's text in `value` rather than
/// `label` so that `labelled_by` still works.
fn label_texts(nodes: &[Node]) -> Vec<&str> {
   nodes
      .iter()
      .filter(|node| node.role() == Role::Label)
      .filter_map(|node| node.value())
      .collect()
}

/// The main window with no wallet open behind it: the nav and the page heading draw, while the
/// pages they navigate to stay closed. Nothing here needs a vault, an RPC or an icon.
fn main_window() -> (Context, GUI, ZeusCtx) {
   let ctx = Context::default();
   let mut gui = GUI::default();
   gui.theme.install(&ctx);

   (
      ctx,
      gui,
      ZeusCtx::new(crate::runtime::test_handle()),
   )
}

/// Render the left panel headlessly, carrying this frame's input events.
fn render_nav(
   ctx: &Context,
   gui: &mut GUI,
   zeus: &ZeusCtx,
   events: Vec<Event>,
) -> Vec<(NodeId, Node)> {
   frame_nodes(ctx, events, |ui| {
      zeus.write(|zeus| crate::gui::ui::panels::left_panel::show(gui, zeus, ui));
   })
}

/// Render the central panel headlessly: the page heading is its first widget.
fn render_central(
   ctx: &Context,
   gui: &mut GUI,
   zeus: &ZeusCtx,
   events: Vec<Event>,
) -> Vec<(NodeId, Node)> {
   frame_nodes(ctx, events, |ui| {
      zeus.write(|zeus| crate::gui::ui::panels::central_panel::show(gui, zeus, ui));
   })
}

/// The main window with Settings open over it: opening the window needs no vault, RPC or icon.
fn settings_window() -> (Context, GUI, ZeusCtx) {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| gui.settings.open(zeus, &mut gui.appearance_settings));

   (ctx, gui, zeus)
}

/// Render the Settings window headlessly, twice: an `egui::Window` is measured on its first pass,
/// and a page is drawn through the fade helper.
///
/// `events` go to the first pass only, so a key that closes the window is not pressed twice.
fn render_settings(
   ctx: &Context,
   gui: &mut GUI,
   zeus: &ZeusCtx,
   events: Vec<Event>,
) -> Vec<(NodeId, Node)> {
   let mut nodes = Vec::new();

   for pass in 0..2 {
      let icons = gui.icons.clone();
      let theme = gui.theme.clone();

      let events = if pass == 0 {
         events.clone()
      } else {
         Vec::new()
      };

      nodes = frame_nodes(ctx, events, |ui| {
         zeus.write(|zeus| {
            gui.settings.show(
               zeus,
               icons.clone(),
               &theme,
               &mut gui.appearance_settings,
               ui,
            );
         });
      });
   }

   nodes
}

/// Open the Appearance page the way a reader does — an AccessKit Click on its tab — and return the
/// tree of the page that opened.
fn open_appearance_page(ctx: &Context, gui: &mut GUI, zeus: &ZeusCtx) -> Vec<(NodeId, Node)> {
   let nodes = render_settings(ctx, gui, zeus, Vec::new());
   let appearance = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Tab && node.label() == Some("Appearance"))
      .map(|(id, _)| *id)
      .expect("the Appearance page must be a tab in the tree");

   render_settings(
      ctx,
      gui,
      zeus,
      vec![accesskit_action(Action::Click, appearance)],
   )
}

/// Render the central panel and then the Settings window in the order the app draws them: the panel
/// raises the overlay dialogs, and Settings is a window over it.
fn render_central_with_settings(
   ctx: &Context,
   gui: &mut GUI,
   zeus: &ZeusCtx,
   events: Vec<Event>,
) -> Vec<(NodeId, Node)> {
   frame_nodes(ctx, events, |ui| {
      zeus.write(|zeus| {
         crate::gui::ui::panels::central_panel::show(gui, zeus, ui);

         let icons = gui.icons.clone();
         let theme = gui.theme.clone();

         gui.settings.show(
            zeus,
            icons,
            &theme,
            &mut gui.appearance_settings,
            ui,
         );
      });
   })
}

/// A key press as the window delivers it.
fn key_press(key: Key) -> Event {
   Event::Key {
      key,
      physical_key: None,
      pressed: true,
      repeat: false,
      modifiers: Modifiers::NONE,
   }
}

/// An action a screen reader sends to the widget at `target`.
fn accesskit_action(action: Action, target: NodeId) -> Event {
   Event::AccessKitActionRequest(ActionRequest {
      action,
      target_tree: TreeId::ROOT,
      target_node: target,
      data: None,
   })
}

/// With `accesskit` compiled in, a frame has a tree at all: the root window node plus a node
/// per widget drawn in it.
#[test]
fn an_enabled_frame_produces_a_tree_update() {
   let ctx = Context::default();

   let nodes = frame(&ctx, |ui| {
      ui.label("Zeus");
   });

   assert!(
      nodes.iter().any(|node| node.role() == Role::Window),
      "the update must start from the root window node"
   );

   assert!(
      label_texts(&nodes).contains(&"Zeus"),
      "the drawn label must be a node of its own: {:?}",
      label_texts(&nodes)
   );
}

/// A real screen's prose reaches the tree: the unlock screen's heading is the first thing a
/// reader hears from Zeus after the window itself.
#[test]
fn the_unlock_screen_heading_is_a_label_node() {
   let ctx = Context::default();
   let (locked, mut screen, mut theme) = unlock_screen();
   theme.install(&ctx);

   let nodes = render_unlock_screen(&ctx, &locked, &mut screen, &theme);

   assert!(
      label_texts(&nodes).contains(&"Zeus is locked"),
      "the unlock screen's heading is missing from the tree: {:?}",
      label_texts(&nodes)
   );
}

/// The card counts the tries: after two refusals the next one is the third of five, drawn as
/// one label a reader reaches like the rest of the card's prose.
#[test]
fn the_lock_card_says_which_attempt_this_is() {
   let ctx = Context::default();
   let (locked, mut screen, mut theme) = unlock_screen();
   theme.install(&ctx);
   locked.write(|ctx| ctx.security.credential_attempts = 2);

   let nodes = render_unlock_screen(&ctx, &locked, &mut screen, &theme);

   assert!(
      label_texts(&nodes).contains(&"Attempt 3 of 5"),
      "the card must say which try this is: {:?}",
      label_texts(&nodes)
   );
}

/// Nothing has been refused yet, so the card keeps exactly the shape a first-time user sees:
/// no attempt line at all.
#[test]
fn the_lock_card_has_no_attempt_line_before_a_failure() {
   let ctx = Context::default();
   let (locked, mut screen, mut theme) = unlock_screen();
   theme.install(&ctx);

   let nodes = render_unlock_screen(&ctx, &locked, &mut screen, &theme);
   let texts = label_texts(&nodes);

   assert!(
      texts.contains(&"Zeus is locked"),
      "the card must still render: {texts:?}"
   );

   assert!(
      !texts.iter().any(|text| text.starts_with("Attempt")),
      "a first try must draw no attempt line: {texts:?}"
   );
}

/// The announcement channel: one `Role::Status` node marked `Live::Polite`
/// whose label is the announced text. A change to that label is what a reader speaks, so the
/// node has to outlive the message and the text has to go through `GUI::announce`.
#[test]
fn an_announcement_is_a_polite_status_node() {
   let ctx = Context::default();
   let mut gui = GUI::default();

   let nodes = frame(&ctx, |ui| {
      gui.announce("Transaction confirmed");
      gui.show_announcer(ui);
   });

   let status = nodes
      .iter()
      .find(|node| node.role() == Role::Status)
      .expect("the announcer must keep a status node in the tree");

   assert_eq!(status.live(), Some(Live::Polite));
   assert_eq!(status.label(), Some("Transaction confirmed"));
}

/// Detection: a frame that builds a tree means a reader is attached, and
/// the sticky flag is what hands egui `Options::screen_reader` — which nothing native sets —
/// so that `Label`s become focusable, i.e. prose becomes reachable with Tab.
#[test]
fn an_attached_screen_reader_turns_on_the_focus_model() {
   let ctx = Context::default();
   let mut gui = GUI::default();

   assert!(
      !gui.at_active,
      "no assistive technology before the first frame"
   );

   frame(&ctx, |ui| gui.track_assistive_tech(ui));

   assert!(
      gui.at_active,
      "a frame that builds a tree means an assistive technology is attached"
   );
   assert!(
      ctx.options(|options| options.screen_reader),
      "the flag must hand egui its screen-reader focus model"
   );
}

/// `egui_kittest` drives the same screen and queries it by role and label: the shape the
/// interaction tests take, and an independent reader of the same tree.
#[test]
fn kittest_queries_the_unlock_screen_by_role_and_label() {
   use egui_kittest::Harness;
   use egui_kittest::kittest::Queryable;

   let (locked, mut screen, theme) = unlock_screen();

   let harness = Harness::new_ui(|ui| {
      locked.write(|zeus| screen.show(zeus, &theme, ui));
   });

   let heading = harness.get_by_label("Zeus is locked");
   assert_eq!(
      heading.value().as_deref(),
      Some("Zeus is locked"),
      "a label node carries its text in `value`"
   );

   harness.get_by_role_and_label(Role::Button, "Unlock");
}

/// Every nav entry is one button node named exactly like the word it draws, only the open page's
/// plate reports itself selected, and the label inside a plate is not a second spoken node.
#[test]
fn every_nav_plate_is_one_named_button() {
   let (ctx, mut gui, zeus) = main_window();
   gui.send_crypto.open();

   let nodes: Vec<Node> = render_nav(&ctx, &mut gui, &zeus, Vec::new())
      .into_iter()
      .map(|(_, node)| node)
      .collect();

   let names = [
      "Home",
      "Send",
      "Shield",
      "Swap",
      "Bridge",
      "Wallets",
      "Transactions",
      "Approvals",
      "Settings",
      "Connected Dapps",
   ];
   #[cfg(feature = "dev")]
   let names = names.into_iter().chain(["Dev UI"]).collect::<Vec<_>>();

   let spoken: Vec<&str> = nodes
      .iter()
      .filter(|node| node.role() == Role::Label && !node.is_hidden())
      .filter_map(|node| node.value())
      .collect();

   for &name in &names {
      let plates: Vec<&Node> = nodes
         .iter()
         .filter(|node| node.role() == Role::Button && node.label() == Some(name))
         .collect();

      assert_eq!(
         plates.len(),
         1,
         "the {name} nav entry must be exactly one button node named {name}"
      );

      assert!(
         !spoken.contains(&name),
         "the label inside the {name} plate must be hidden, or the entry is spoken twice: {spoken:?}"
      );
   }

   let selected: Vec<&str> = nodes
      .iter()
      .filter(|node| node.role() == Role::Button && node.toggled() == Some(Toggled::True))
      .filter_map(|node| node.label())
      .collect();

   assert_eq!(
      selected,
      vec!["Send"],
      "exactly the plate of the open page is selected, got {selected:?}"
   );
}

/// A reader activates Send the way it activates any button: a `Click` request aimed at the node
/// id from the tree. It is the same navigation hook as a mouse click, so the page opens.
#[test]
fn activating_the_send_nav_node_opens_send() {
   let (ctx, mut gui, zeus) = main_window();

   render_nav(&ctx, &mut gui, &zeus, Vec::new());
   let nodes = render_nav(&ctx, &mut gui, &zeus, Vec::new());

   let send = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some("Send"))
      .map(|(id, _)| *id)
      .expect("the Send nav entry must be in the tree");

   render_nav(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, send)],
   );

   assert!(
      gui.send_crypto.is_open(),
      "an AccessKit Click on the Send plate must navigate to Send"
   );
}

/// The open page has a Heading node named after it, and a page change moves keyboard focus to the
/// heading — that focus move is how the new page's name reaches a reader.
#[test]
fn the_page_heading_names_the_open_page_and_takes_focus_when_it_changes() {
   let (ctx, mut gui, zeus) = main_window();

   let first = render_central(&ctx, &mut gui, &zeus, Vec::new());
   let (id, node) = first
      .iter()
      .find(|(_, node)| node.role() == Role::Heading)
      .expect("the open page must have a heading node");

   assert_eq!(
      node.label(),
      Some("Home"),
      "no page is open, so the heading names Home"
   );

   let focused = ctx.memory(|memory| memory.focused());
   assert_eq!(
      focused.map(|id| id.accesskit_id()),
      Some(*id),
      "the Home heading must take focus when the page appears"
   );

   gui.send_crypto.open();

   let second = render_central(&ctx, &mut gui, &zeus, Vec::new());
   let (id, node) = second
      .iter()
      .find(|(_, node)| node.role() == Role::Heading)
      .expect("the new page must have a heading node");

   assert_eq!(
      node.label(),
      Some("Send"),
      "the heading names the page that just opened"
   );

   let focused = ctx.memory(|memory| memory.focused());
   assert_eq!(
      focused.map(|id| id.accesskit_id()),
      Some(*id),
      "the page change must move focus to the new page's heading"
   );
}

/// The keyboard path works as well as the AccessKit one: Tab from the Home plate reaches Send, and
/// Enter on the focused plate opens the page.
#[test]
fn tab_reaches_the_next_nav_plate_and_enter_opens_it() {
   let (ctx, mut gui, zeus) = main_window();

   render_nav(&ctx, &mut gui, &zeus, Vec::new());
   let nodes = render_nav(&ctx, &mut gui, &zeus, Vec::new());
   let home = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some("Home"))
      .map(|(id, _)| *id)
      .expect("the Home nav entry must be in the tree");

   render_nav(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Focus, home)],
   );

   let nodes = render_nav(&ctx, &mut gui, &zeus, vec![key_press(Key::Tab)]);
   let send = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some("Send"))
      .map(|(id, _)| *id)
      .expect("the Send nav entry must be in the tree");

   let focused = ctx.memory(|memory| memory.focused());
   assert_eq!(
      focused.map(|id| id.accesskit_id()),
      Some(send),
      "Tab from the Home plate must move focus to the Send plate"
   );

   render_nav(&ctx, &mut gui, &zeus, vec![key_press(Key::Enter)]);

   assert!(
      gui.send_crypto.is_open(),
      "Enter on the focused Send plate must open Send"
   );
}

/// Settings is a window in the tree: one named window node a reader can hear, and the Close button
/// egui draws for it. Neither existed while Settings was a second OS window.
#[test]
fn the_settings_window_is_one_named_window() {
   let (ctx, mut gui, zeus) = settings_window();

   let nodes = render_settings(&ctx, &mut gui, &zeus, Vec::new());

   // The tree starts from the viewport's own window node; the one below it is Settings.
   let root = egui::accesskit_root_id().accesskit_id();
   let windows: Vec<&Node> = nodes
      .iter()
      .filter(|(id, node)| *id != root && node.role() == Role::Window)
      .map(|(_, node)| node)
      .collect();

   assert_eq!(
      windows.len(),
      1,
      "Settings must be exactly one window node, got {}",
      windows.len()
   );

   let name = windows[0].label().or_else(|| windows[0].value());
   assert_eq!(
      name,
      Some("Settings"),
      "the window node must be named"
   );

   assert!(
      nodes
         .iter()
         .any(|(_, node)| node.role() == Role::Button && node.label() == Some("Close window")),
      "the window must offer the Close button a reader can activate"
   );
}

/// Escape closes the window, and the close asks the left panel for the focus back so the Settings
/// plate is where a reader lands again.
#[test]
fn the_settings_window_closes_on_escape_and_says_so_to_the_left_panel() {
   let (ctx, mut gui, zeus) = settings_window();

   render_settings(&ctx, &mut gui, &zeus, Vec::new());
   assert!(
      gui.settings.is_open(),
      "the window must be drawn before it can close"
   );

   render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![key_press(Key::Escape)],
   );

   assert!(
      !gui.settings.is_open(),
      "Escape must close the Settings window"
   );
   assert!(
      gui.settings.take_focus_return(),
      "a close the user asked for must ask for the focus back"
   );

   // The panel reads the request while it draws the plate, so close it once more with the flag
   // still set, then draw the nav.
   zeus.write(|zeus| gui.settings.open(zeus, &mut gui.appearance_settings));
   render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![key_press(Key::Escape)],
   );

   let nodes = render_nav(&ctx, &mut gui, &zeus, Vec::new());
   let settings_plate = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some("Settings"))
      .map(|(id, _)| *id)
      .expect("the Settings plate must be in the nav tree");

   let focused = ctx.memory(|memory| memory.focused());
   assert_eq!(
      focused.map(|id| id.accesskit_id()),
      Some(settings_plate),
      "the close must put the focus back on the Settings plate"
   );

   render_nav(&ctx, &mut gui, &zeus, Vec::new());
   assert_eq!(
      ctx.memory(|memory| memory.focused()),
      focused,
      "the plate must keep the focus on the next frame"
   );
}

/// A locked wallet must not leave Settings usable: the window closes and nothing of it is drawn.
#[test]
fn a_locked_wallet_hides_the_settings_window() {
   let (ctx, mut gui, zeus) = settings_window();
   assert!(
      gui.settings.is_open(),
      "Settings must be open before the lock"
   );

   zeus.write(|zeus| zeus.locked = true);

   let nodes = render_settings(&ctx, &mut gui, &zeus, Vec::new());

   assert!(
      !gui.settings.is_open(),
      "a locked wallet must close Settings"
   );

   let root = egui::accesskit_root_id().accesskit_id();
   assert!(
      !nodes.iter().any(|(id, node)| *id != root && node.role() == Role::Window),
      "no Settings window may be drawn while the wallet is locked"
   );
}

/// The Settings pages are a tab list: one `TabList` named after what the tabs switch between, seven
/// `Tab` nodes named exactly like the pages, only the open page's tab selected, and no page word
/// spoken a second time as the label inside a plate.
#[test]
fn every_settings_page_is_a_named_tab() {
   let (ctx, mut gui, zeus) = settings_window();

   let nodes = render_settings(&ctx, &mut gui, &zeus, Vec::new());

   let lists: Vec<&Node> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::TabList)
      .map(|(_, node)| node)
      .collect();

   assert_eq!(
      lists.len(),
      1,
      "the page switcher must be one tab list"
   );
   assert_eq!(
      lists[0].label(),
      Some("Settings pages"),
      "the tab list must name what the tabs switch between"
   );

   let names = [
      "General",
      "Appearance",
      "Networks",
      "Security",
      "Contacts",
      "Railgun",
      "Data",
   ];

   let spoken: Vec<&str> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Label && !node.is_hidden())
      .filter_map(|(_, node)| node.value())
      .collect();

   for &name in &names {
      let tabs: Vec<&Node> = nodes
         .iter()
         .filter(|(_, node)| node.role() == Role::Tab && node.label() == Some(name))
         .map(|(_, node)| node)
         .collect();

      assert_eq!(
         tabs.len(),
         1,
         "the {name} page must be exactly one tab named {name}"
      );

      assert_eq!(
         tabs[0].toggled() == Some(Toggled::True),
         name == "General",
         "only the open page's tab may report itself selected: {name}"
      );

      assert!(
         !spoken.contains(&name),
         "the label inside the {name} tab must be hidden, or the tab is spoken twice: {spoken:?}"
      );
   }
}

/// A reader opens a Settings page the way it opens any tab: a `Click` request on the tab's node. The
/// page changes, its heading names it, and a word only that page draws is in the tree.
#[test]
fn activating_a_settings_tab_opens_its_page() {
   let (ctx, mut gui, zeus) = settings_window();

   let nodes = render_settings(&ctx, &mut gui, &zeus, Vec::new());
   let security = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Tab && node.label() == Some("Security"))
      .map(|(id, _)| *id)
      .expect("the Security page must be a tab in the tree");

   let nodes = render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, security)],
   );

   // The page heading is the one the page change focused: the page draws section headings of
   // its own now, and the tree hands its nodes back in no particular order.
   let focused = ctx.memory(|memory| memory.focused()).map(|id| id.accesskit_id());
   let heading = nodes
      .iter()
      .find(|(id, node)| node.role() == Role::Heading && Some(*id) == focused)
      .map(|(_, node)| node)
      .expect("the Security page must have a heading node");

   assert_eq!(
      heading.label(),
      Some("Security"),
      "the heading must name the page the tab opened"
   );

   // The page's words, whether they are drawn as prose or as a section heading.
   let texts: Vec<&str> = nodes
      .iter()
      .filter_map(|(_, node)| match node.role() {
         Role::Label => node.value(),
         Role::Heading => node.label(),
         _ => None,
      })
      .collect();

   assert!(
      texts.contains(&"Change Credentials"),
      "the Security page must draw its own contents: {texts:?}"
   );
}

/// Opening Settings lands on the open page's heading, and a page change moves the focus to the new
/// heading — that focus move is how the page's name reaches a reader.
#[test]
fn the_settings_page_heading_names_the_page_and_takes_focus() {
   let (ctx, mut gui, zeus) = settings_window();

   let first = render_settings(&ctx, &mut gui, &zeus, Vec::new());
   let focused = ctx.memory(|memory| memory.focused()).map(|id| id.accesskit_id());
   let (id, node) = first
      .iter()
      .find(|(id, node)| node.role() == Role::Heading && Some(*id) == focused)
      .expect("the open page must have a heading node");

   assert_eq!(
      node.label(),
      Some("General"),
      "no page was chosen, so the heading names General"
   );

   let focused = ctx.memory(|memory| memory.focused());
   assert_eq!(
      focused.map(|id| id.accesskit_id()),
      Some(*id),
      "the General heading must take focus when Settings opens"
   );

   let appearance = first
      .iter()
      .find(|(_, node)| node.role() == Role::Tab && node.label() == Some("Appearance"))
      .map(|(id, _)| *id)
      .expect("the Appearance page must be a tab in the tree");

   let second = render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, appearance)],
   );

   let focused = ctx.memory(|memory| memory.focused()).map(|id| id.accesskit_id());
   let (id, node) = second
      .iter()
      .find(|(id, node)| node.role() == Role::Heading && Some(*id) == focused)
      .expect("the new page must have a heading node");

   assert_eq!(
      node.label(),
      Some("Appearance"),
      "the heading names the page that just opened"
   );

   let focused = ctx.memory(|memory| memory.focused());
   assert_eq!(
      focused.map(|id| id.accesskit_id()),
      Some(*id),
      "the page change must move focus to the new page's heading"
   );
}

/// Every Settings page draws when its tab is opened: the heading names the page for all seven, so a
/// page that stops being drawn fails here without asserting any one page's contents.
#[test]
fn every_settings_page_appears_in_the_tree() {
   let (ctx, mut gui, zeus) = settings_window();

   for name in [
      "General",
      "Appearance",
      "Networks",
      "Security",
      "Contacts",
      "Railgun",
      "Data",
   ] {
      let nodes = render_settings(&ctx, &mut gui, &zeus, Vec::new());
      let tab = nodes
         .iter()
         .find(|(_, node)| node.role() == Role::Tab && node.label() == Some(name))
         .map(|(id, _)| *id)
         .unwrap_or_else(|| panic!("the {name} page must be a tab in the tree"));

      let nodes = render_settings(
         &ctx,
         &mut gui,
         &zeus,
         vec![accesskit_action(Action::Click, tab)],
      );

      // The page heading is the one the page change focused.
      let focused = ctx.memory(|memory| memory.focused()).map(|id| id.accesskit_id());
      let heading = nodes
         .iter()
         .find(|(id, node)| node.role() == Role::Heading && Some(*id) == focused)
         .map(|(_, node)| node)
         .unwrap_or_else(|| panic!("the {name} page must draw a heading"));

      assert_eq!(
         heading.label(),
         Some(name),
         "the heading must name the page the tab opened"
      );
   }
}

/// Render the top panel headlessly and put this frame's announcement in the tree: the app draws
/// the announcer right after the panels, and a node created together with its message never
/// speaks.
fn render_top_panel(ctx: &Context, gui: &mut GUI, zeus: &ZeusCtx) -> Vec<Node> {
   frame(ctx, |ui| {
      zeus.write(|zeus| {
         crate::gui::ui::panels::top_panel::show(gui, zeus, ui);
         gui.show_announcer(ui);
      });
   })
}

/// The text of this frame's announcement node.
fn status_text(nodes: &[Node]) -> Option<String> {
   nodes
      .iter()
      .find(|node| node.role() == Role::Status)
      .and_then(|node| node.label().map(str::to_string))
}

/// The status pill is the only sign that work is running, and it is drawn in an `Area` that a
/// reader never lands on. Its transitions are what speaks: the message when it appears, "Zeus is
/// ready." when it goes, and nothing at all while it stays the same.
#[test]
fn the_status_pill_speaks_when_it_appears_and_when_it_goes() {
   let (ctx, mut gui, zeus) = main_window();

   zeus.write(|zeus| zeus.data_syncing = true);
   let nodes = render_top_panel(&ctx, &mut gui, &zeus);
   assert_eq!(
      status_text(&nodes).as_deref(),
      Some("Zeus is still syncing important data"),
      "the pill appearing is its message, which is already a full sentence"
   );

   zeus.write(|zeus| zeus.data_syncing = false);
   let nodes = render_top_panel(&ctx, &mut gui, &zeus);
   assert_eq!(
      status_text(&nodes).as_deref(),
      Some("Zeus is ready."),
      "the pill going away is the completion a reader hears"
   );

   let nodes = render_top_panel(&ctx, &mut gui, &zeus);
   assert_eq!(
      status_text(&nodes).as_deref(),
      Some(""),
      "a pill that did not change says nothing on the next frame"
   );
}

/// Render the account panel headlessly, twice: the panel measures its body on the first pass and
/// clips it to that height, and egui exposes no node for a widget it considers invisible.
///
/// `events` go to the first pass only, so a click moves the panel before the second pass draws the
/// body it opened.
fn render_account_panel(
   ctx: &Context,
   gui: &mut GUI,
   zeus: &ZeusCtx,
   events: Vec<Event>,
) -> Vec<(NodeId, Node)> {
   let mut nodes = Vec::new();

   for pass in 0..2 {
      let events = if pass == 0 {
         events.clone()
      } else {
         Vec::new()
      };

      nodes = frame_nodes(ctx, events, |ui| {
         let icons = gui.icons.clone();
         zeus.write(|zeus| gui.account_panel.show(zeus, &gui.theme, icons, ui));
      });
   }

   nodes
}

/// The delegation row used to draw a dot whose state is inverted — Off exactly when the wallet is
/// delegated — so a reader heard "status: off" where the words say the opposite. The dot is hidden
/// from the tree, which leaves the word as the row's node.
#[test]
fn the_delegation_row_is_one_node_and_says_the_words() {
   let (ctx, mut gui, zeus) = main_window();
   gui.account_panel.open();

   let nodes = render_account_panel(&ctx, &mut gui, &zeus, Vec::new());

   let spoken: Vec<&str> = nodes
      .iter()
      .map(|(_, node)| node)
      .filter(|node| !node.is_hidden())
      .filter_map(|node| node.value())
      .collect();

   assert!(
      spoken.contains(&"Not Delegated"),
      "the row's word must be the row's node: {spoken:?}"
   );

   let dot_states: Vec<&str> =
      spoken.iter().copied().filter(|text| text.starts_with("status:")).collect();

   assert!(
      dot_states.is_empty(),
      "a status dot has nothing to say beside the word that already says it: {dot_states:?}"
   );
}

/// The two service rows live on the panel's second tab. Opening it the way a reader does — an
/// AccessKit Click on its tab — proves each row is one node that names the service and its state,
/// instead of a bare service name beside a dot saying "status: on".
#[test]
fn the_service_rows_say_the_service_and_their_state() {
   let (ctx, mut gui, zeus) = main_window();
   gui.account_panel.open();

   let nodes = render_account_panel(&ctx, &mut gui, &zeus, Vec::new());

   let diagnostics = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Tab && node.label() == Some("Diagnostics"))
      .map(|(id, _)| *id)
      .expect("the panel's second tab must be a Tab node named Diagnostics");

   let nodes = render_account_panel(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, diagnostics)],
   );

   let spoken: Vec<&str> = nodes
      .iter()
      .map(|(_, node)| node)
      .filter(|node| !node.is_hidden())
      .filter_map(|node| node.value())
      .collect();

   assert!(
      spoken.iter().any(|text| text.starts_with("Railgun, ")),
      "the Railgun row must say the service and its state: {spoken:?}"
   );
   assert!(
      spoken.contains(&"Wallet Connector, starting"),
      "the connector row must say the service and its state: {spoken:?}"
   );
}

/// A service row's dot is hidden as well, so its label has to carry the whole sentence: the
/// service and its state. AccessKit names a `Role::Label` node from its value rather than its
/// label, which is where the row's name lands; the drawn word it replaces stays on screen.
#[test]
fn a_service_row_label_carries_the_whole_sentence() {
   let ctx = Context::default();

   let nodes = frame(&ctx, |ui| {
      let label = ui.label("Railgun");
      crate::gui::ui::account::name_service_row(ui, label, "Railgun, syncing".to_string());
   });

   let label = nodes
      .iter()
      .find(|node| node.role() == Role::Label)
      .expect("the row's label must be a node of its own");

   assert_eq!(
      label.value(),
      Some("Railgun, syncing"),
      "a label node is named from its value, so the sentence must go there"
   );
   assert_eq!(
      label.label(),
      Some("Railgun, syncing"),
      "the node must say the same sentence whichever property a consumer reads"
   );
}

/// A message dialog has a name of its own, keeps its text as a label, and offers the deliberate
/// exits: a Close button beside the OK that already dismissed it.
#[test]
fn a_message_window_is_a_named_dialog() {
   let (ctx, mut gui, zeus) = main_window();
   gui.open_msg_window("RPC added successfully");

   render_central(&ctx, &mut gui, &zeus, Vec::new());
   render_central(&ctx, &mut gui, &zeus, Vec::new());
   let nodes = render_central(&ctx, &mut gui, &zeus, Vec::new());

   let dialogs: Vec<&Node> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Dialog)
      .map(|(_, node)| node)
      .collect();

   assert_eq!(
      dialogs.len(),
      1,
      "the message must be exactly one dialog"
   );
   assert_eq!(
      dialogs[0].label(),
      Some("Notification"),
      "the dialog must be named"
   );

   let message = nodes.iter().any(|(_, node)| {
      node.role() == Role::Label && node.value() == Some("RPC added successfully")
   });

   assert!(
      message,
      "the message must stay in the tree as a label"
   );

   let close = nodes
      .iter()
      .any(|(_, node)| node.role() == Role::Button && node.label() == Some("Close"));

   assert!(close, "the dialog must offer the Close button");
}

/// The Close button is a real exit: an AccessKit Click on it closes the dialog without hunting for
/// OK.
#[test]
fn a_message_window_closes_from_its_close_button() {
   let (ctx, mut gui, zeus) = main_window();
   gui.open_msg_window("RPC added successfully");

   render_central(&ctx, &mut gui, &zeus, Vec::new());
   render_central(&ctx, &mut gui, &zeus, Vec::new());
   let nodes = render_central(&ctx, &mut gui, &zeus, Vec::new());

   let close = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some("Close"))
      .map(|(id, _)| *id)
      .expect("the dialog must offer the Close button");

   render_central(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, close)],
   );

   assert!(
      !gui.msg_window.is_open(),
      "an AccessKit Click on Close must dismiss the message"
   );
}

/// A message is spoken through the app's announcement channel, so a reader hears it when it appears
/// instead of finding the dialog later.
#[test]
fn the_message_window_says_its_text() {
   let ctx = Context::default();
   let mut gui = GUI::default();

   gui.open_msg_window("RPC added successfully");

   let nodes = frame(&ctx, |ui| gui.show_announcer(ui));

   assert_eq!(
      status_text(&nodes).as_deref(),
      Some("RPC added successfully"),
      "the dialog's text must reach the announcement node"
   );
}

/// The first Escape belongs to the dialog on top: it closes the message and leaves the Settings
/// window behind it open.
#[test]
fn an_escape_goes_to_the_message_window_and_not_to_settings() {
   let (ctx, mut gui, zeus) = settings_window();
   gui.open_msg_window("RPC added successfully");

   render_central_with_settings(&ctx, &mut gui, &zeus, Vec::new());
   render_central_with_settings(&ctx, &mut gui, &zeus, Vec::new());
   let nodes = render_central_with_settings(&ctx, &mut gui, &zeus, Vec::new());

   assert!(
      nodes.iter().any(|(_, node)| node.role() == Role::Dialog),
      "the message dialog must be drawn above Settings before the key is sent"
   );
   assert!(
      gui.msg_window.is_open(),
      "the dialog must be open before the key is sent"
   );

   render_central_with_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![key_press(Key::Escape)],
   );

   assert!(
      !gui.msg_window.is_open(),
      "Escape must close the message dialog"
   );
   assert!(
      gui.settings.is_open(),
      "the Escape that closed the dialog must not also close Settings"
   );
}

/// Every section the General page draws is a Heading node named exactly like the words it draws,
/// so a reader can jump between sections instead of walking every control. The page's own heading
/// is still there beside them.
#[test]
fn the_general_page_headings_are_headings() {
   let (ctx, mut gui, zeus) = settings_window();

   let nodes = render_settings(&ctx, &mut gui, &zeus, Vec::new());

   let headings: Vec<&str> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Heading)
      .filter_map(|(_, node)| node.label())
      .collect();

   #[cfg(target_os = "linux")]
   let sections = [
      "External Data",
      "Window",
      "Desktop",
      "Pool Manager",
      "Balance Manager",
   ];
   #[cfg(not(target_os = "linux"))]
   let sections = ["External Data", "Window", "Pool Manager", "Balance Manager"];

   for section in sections {
      assert!(
         headings.contains(&section),
         "the {section} section title must be a heading node: {headings:?}"
      );
   }

   assert!(
      headings.contains(&"General"),
      "the page heading must still be there: {headings:?}"
   );
}

/// Each slider is one node that says what it sets and which range it covers: the sentence drawn
/// above it belongs to the slider, not to a sibling label a reader has to tie to it on its own.
#[test]
fn each_slider_says_what_it_sets() {
   let (ctx, mut gui, zeus) = settings_window();

   let nodes = render_settings(&ctx, &mut gui, &zeus, Vec::new());

   let sliders: Vec<&Node> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Slider)
      .map(|(_, node)| node)
      .collect();

   assert_eq!(
      sliders.len(),
      4,
      "the General page draws four sliders"
   );

   let sentences = [
      (
         "Concurrency for Discovering & Updating Pools",
         "1 to 10",
      ),
      ("Batch Size when updating pools state", "1 to 50"),
      ("Concurrency for syncing balances", "1 to 10"),
      ("Batch Size for syncing balances", "1 to 50"),
   ];

   for (sentence, range) in sentences {
      let slider = sliders
         .iter()
         .find(|node| node.label() == Some(sentence))
         .unwrap_or_else(|| panic!("the {sentence} slider must be named with its sentence"));

      assert_eq!(
         slider.description(),
         Some(range),
         "the {sentence} slider must say the range it covers"
      );
      assert!(
         slider.numeric_value().is_some(),
         "the {sentence} slider must still report the value the crate gave it"
      );
   }

   let spoken: Vec<&str> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Label && !node.is_hidden())
      .filter_map(|(_, node)| node.value())
      .collect();

   for (sentence, _) in sentences {
      assert!(
         !spoken.contains(&sentence),
         "the {sentence} label must be hidden, or the slider is spoken twice: {spoken:?}"
      );
   }
}

/// The (?) chip is not a destination of its own: its tip is the control's description, and the
/// chip is hidden so a reader hears the explanation beside the checkbox instead of hunting for a
/// question mark after it.
#[test]
fn the_help_text_reaches_the_control_it_explains() {
   let (ctx, mut gui, zeus) = settings_window();

   let nodes = render_settings(&ctx, &mut gui, &zeus, Vec::new());

   let updates = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::CheckBox && node.label() == Some("Check for Updates"))
      .map(|(_, node)| node)
      .expect("the Check for Updates box must be in the tree");

   let prefix = "Allow Zeus to check GitHub for a newer Zeus release";
   let description = updates.description().unwrap_or_default();

   assert!(
      description.starts_with(prefix),
      "the checkbox must carry the tip as its description: {description:?}"
   );

   let questions: Vec<&str> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Label && !node.is_hidden())
      .filter_map(|(_, node)| node.value())
      .filter(|text| *text == "?")
      .collect();

   assert!(
      questions.is_empty(),
      "no question-mark chip may be left in the tree: {questions:?}"
   );
}

/// The Appearance page is two collapsible sections a reader can name and open: a named disclosure
/// node per title that reports its state, closed on arrival, with the page's own heading still
/// beside them.
#[test]
fn the_appearance_page_is_two_named_sections() {
   let (ctx, mut gui, zeus) = settings_window();

   let nodes = open_appearance_page(&ctx, &mut gui, &zeus);

   for title in ["Hold to confirm", "Ambient"] {
      let header = nodes
         .iter()
         .find(|(_, node)| node.role() == Role::DisclosureTriangle && node.label() == Some(title))
         .map(|(_, node)| node)
         .unwrap_or_else(|| panic!("the {title} section must be a named disclosure node"));

      assert_eq!(
         header.is_expanded(),
         Some(false),
         "the {title} section must start closed"
      );
   }

   let headings: Vec<&str> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Heading)
      .filter_map(|(_, node)| node.label())
      .collect();

   assert!(
      headings.contains(&"Appearance"),
      "the page heading must still be there: {headings:?}"
   );
}

/// A control row is one node: the sentence the page draws in its label cell is the slider's own
/// name, its "(?)" tip is the description, and the value the crate reports is still there.
#[test]
fn a_slider_in_the_appearance_page_says_what_it_sets() {
   let (ctx, mut gui, zeus) = settings_window();

   let nodes = open_appearance_page(&ctx, &mut gui, &zeus);
   let hold = nodes
      .iter()
      .find(|(_, node)| {
         node.role() == Role::DisclosureTriangle && node.label() == Some("Hold to confirm")
      })
      .map(|(id, _)| *id)
      .expect("the Hold to confirm section must be a named disclosure node");

   let nodes = render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, hold)],
   );

   let slider = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Slider && node.label() == Some("Duration"))
      .map(|(_, node)| node)
      .expect("the hold duration row must be a slider named after its word");

   let description = slider.description().unwrap_or_default();

   assert!(
      description.starts_with("How long the affirmative button must be held"),
      "the slider must carry the row's tip as its description: {description:?}"
   );
   assert!(
      slider.numeric_value().is_some(),
      "the slider must still report the value the crate gave it"
   );

   let spoken: Vec<&str> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Label && !node.is_hidden())
      .filter_map(|(_, node)| node.value())
      .collect();

   assert!(
      !spoken.contains(&"Duration"),
      "the row's word must be hidden, or the slider is spoken twice: {spoken:?}"
   );
}

/// A colour well is named after the row that draws it, and no "(?)" chip is left as a destination of
/// its own: the tip is the well's description and the chip leaves the tree.
#[test]
fn a_colour_well_says_what_it_colours() {
   let (ctx, mut gui, zeus) = settings_window();

   let nodes = open_appearance_page(&ctx, &mut gui, &zeus);
   let hold = nodes
      .iter()
      .find(|(_, node)| {
         node.role() == Role::DisclosureTriangle && node.label() == Some("Hold to confirm")
      })
      .map(|(id, _)| *id)
      .expect("the Hold to confirm section must be a named disclosure node");

   let nodes = render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, hold)],
   );

   let well = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::ColorWell && node.label() == Some("End colour"))
      .map(|(_, node)| node)
      .expect("the End colour well must be named after its row");

   let description = well.description().unwrap_or_default();

   assert!(
      description.starts_with("Colour the sweep ends at"),
      "the well must carry the row's tip as its description: {description:?}"
   );

   // Only a row whose control names itself keeps its question mark: the "Kind" picker is the one
   // combo box in this section, and the crate draws its plate, so its tip has nowhere else to go.
   let questions: Vec<&str> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Label && !node.is_hidden())
      .filter_map(|(_, node)| node.value())
      .filter(|text| *text == "?")
      .collect();

   assert_eq!(
      questions,
      vec!["?"],
      "only the row the page cannot name keeps its question mark"
   );
}

/// Reset writes the defaults back and says so: the page raises the words, the app drains them
/// right after the window is drawn, and the announcer speaks them in the same frame.
#[test]
fn resetting_the_pool_and_balance_settings_says_so() {
   let (ctx, mut gui, zeus) = settings_window();

   let nodes = render_settings(&ctx, &mut gui, &zeus, Vec::new());
   let reset = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some("Reset Settings"))
      .map(|(id, _)| *id)
      .expect("the General page must offer the Reset Settings button");

   render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, reset)],
   );

   let text = gui.settings.take_notice().expect("a reset must raise the words the app says");

   assert_eq!(
      text,
      "Pool and balance settings reset to their defaults."
   );

   let nodes = frame(&ctx, |ui| {
      gui.announce(text.as_str());
      gui.show_announcer(ui);
   });

   assert_eq!(
      status_text(&nodes).as_deref(),
      Some("Pool and balance settings reset to their defaults."),
      "the reset's words must reach the announcement node"
   );
}

/// A save made by leaving the page is spoken too: a keyboard-changed slider is written back when
/// the General page hands over, and the window raises the words for the app to announce.
#[test]
fn leaving_the_general_page_speaks_the_save() {
   let (ctx, mut gui, zeus) = settings_window();

   let nodes = render_settings(&ctx, &mut gui, &zeus, Vec::new());
   let (slider, before) = nodes
      .iter()
      .find(|(_, node)| {
         node.role() == Role::Slider
            && node.label() == Some("Concurrency for Discovering & Updating Pools")
      })
      .map(|(id, node)| (*id, node.numeric_value()))
      .expect("the first General slider must be in the tree");

   render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Focus, slider)],
   );

   let nodes = render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![key_press(Key::ArrowRight)],
   );

   let after = nodes
      .iter()
      .find(|(_, node)| {
         node.role() == Role::Slider
            && node.label() == Some("Concurrency for Discovering & Updating Pools")
      })
      .and_then(|(_, node)| node.numeric_value());

   assert_eq!(
      after,
      before.map(|value| value + 1.0),
      "ArrowRight on the focused slider must move its value by one step"
   );

   let nodes = render_settings(&ctx, &mut gui, &zeus, Vec::new());
   let security = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Tab && node.label() == Some("Security"))
      .map(|(id, _)| *id)
      .expect("the Security tab must be in the tree");

   render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, security)],
   );

   assert_eq!(
      gui.settings.take_notice().as_deref(),
      Some("Settings saved."),
      "leaving the page must speak the save it made"
   );
}

/// The Security page's three section titles are heading nodes as well.
#[test]
fn the_security_page_headings_are_headings() {
   let (ctx, mut gui, zeus) = settings_window();

   let nodes = render_settings(&ctx, &mut gui, &zeus, Vec::new());
   let security = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Tab && node.label() == Some("Security"))
      .map(|(id, _)| *id)
      .expect("the Security page must be a tab in the tree");

   let nodes = render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, security)],
   );

   let headings: Vec<&str> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Heading)
      .filter_map(|(_, node)| node.label())
      .collect();

   for section in ["Auto-Lock", "Change Credentials", "Vault encryption"] {
      assert!(
         headings.contains(&section),
         "the {section} section title must be a heading node: {headings:?}"
      );
   }
}

/// Open the Networks page the way a reader does — an AccessKit Click on its tab — and return the
/// tree of the page that opened.
fn open_networks_page(ctx: &Context, gui: &mut GUI, zeus: &ZeusCtx) -> Vec<(NodeId, Node)> {
   let nodes = render_settings(ctx, gui, zeus, Vec::new());
   let networks = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Tab && node.label() == Some("Networks"))
      .map(|(id, _)| *id)
      .expect("the Networks page must be a tab in the tree");

   render_settings(
      ctx,
      gui,
      zeus,
      vec![accesskit_action(Action::Click, networks)],
   )
}

/// Open a Settings page the way a reader does — an AccessKit Click on its tab — and return the
/// tree of the page that opened.
fn open_settings_page(
   ctx: &Context,
   gui: &mut GUI,
   zeus: &ZeusCtx,
   page: &str,
) -> Vec<(NodeId, Node)> {
   let nodes = render_settings(ctx, gui, zeus, Vec::new());
   let tab = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Tab && node.label() == Some(page))
      .map(|(id, _)| *id)
      .unwrap_or_else(|| panic!("the {page} page must be a tab in the tree"));

   render_settings(
      ctx,
      gui,
      zeus,
      vec![accesskit_action(Action::Click, tab)],
   )
}

/// The host a row may be named by: what sits between `://` and the first path or query.
fn host_of(url: &str) -> &str {
   url.split_once("://")
      .map(|(_, rest)| rest)
      .unwrap_or(url)
      .split(['/', '?'])
      .next()
      .unwrap_or(url)
}

/// The Networks page names itself and the controls it draws: the page heading, the chain toggle
/// that says which way a press flips the chain, the way into the add-RPC form, and the concurrency
/// field with the button that applies it.
#[test]
fn the_networks_page_names_its_controls() {
   let (ctx, mut gui, zeus) = settings_window();

   let nodes = open_networks_page(&ctx, &mut gui, &zeus);

   let headings: Vec<&str> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Heading)
      .filter_map(|(_, node)| node.label())
      .collect();

   assert!(
      headings.contains(&"Networks"),
      "the page heading must name the page: {headings:?}"
   );

   // The drawn word flips with the chain; the name has to match the action the press performs.
   let disabled = zeus.read(|zeus| zeus.is_chain_disabled(1));
   let toggle = if disabled {
      "Enable this network"
   } else {
      "Disable this network"
   };

   assert!(
      nodes
         .iter()
         .any(|(_, node)| node.role() == Role::Button && node.label() == Some(toggle)),
      "the chain toggle must be named after the action it performs ({toggle})"
   );

   for name in ["Add RPC Url", "Apply concurrency"] {
      assert!(
         nodes
            .iter()
            .any(|(_, node)| node.role() == Role::Button && node.label() == Some(name)),
         "the {name} button must be a named node in the tree"
      );
   }

   let concurrency = nodes
      .iter()
      .find(|(_, node)| {
         node.role() == Role::TextInput && node.label() == Some("Request Concurrency")
      })
      .map(|(_, node)| node)
      .expect("the concurrency field must be named after the sentence beside it");

   let description = concurrency.description().unwrap_or_default();

   assert!(
      description.contains("How many requests Zeus may run at once"),
      "the field must carry the range hint as its description: {description:?}"
   );

   // A row names its own controls: the URL field, the box that enables it, and the three actions.
   let rpcs = zeus.read(|zeus| zeus.client_manager.get_rpcs(1));
   let url = rpcs
      .values()
      .next()
      .map(|rpc| rpc.url.to_string())
      .expect("the default chain must ship at least one endpoint");
   let host = host_of(&url);

   for name in [
      format!("RPC URL at {host}"),
      format!("Enabled, {host}"),
      format!("Endpoint settings for {host}"),
      format!("Test {host}"),
      format!("Remove {host}"),
   ] {
      assert!(
         nodes.iter().any(|(_, node)| node.label() == Some(name.as_str())),
         "the row for {host} must carry a {name} node"
      );
   }
}

/// A row is identified without its URL: the URL text never enters the tree, and the row's host —
/// the part that holds no provider key — is what names the row's controls.
#[test]
fn no_rpc_url_reaches_the_tree() {
   let (ctx, mut gui, zeus) = settings_window();

   let rpcs = zeus.read(|zeus| zeus.client_manager.get_rpcs(1));
   let urls: Vec<String> = rpcs.values().map(|rpc| rpc.url.to_string()).collect();

   assert!(
      !urls.is_empty(),
      "the headless profile must ship the built-in endpoints this test reads"
   );

   let nodes = open_networks_page(&ctx, &mut gui, &zeus);

   for (_, node) in &nodes {
      for text in [node.label(), node.value(), node.description()] {
         for url in &urls {
            assert!(
               !text.unwrap_or_default().contains(url.as_str()),
               "a full URL must never reach the tree: {url:?} is in {text:?}"
            );
         }
      }
   }

   let field = nodes
      .iter()
      .find(|(_, node)| {
         node.role() == Role::TextInput
            && node.label().is_some_and(|label| label.starts_with("RPC URL at "))
      })
      .map(|(_, node)| node)
      .expect("a row's URL field must be named after its host");

   assert_eq!(
      field.value(),
      None,
      "the row's URL field must carry no value a reader could hear"
   );

   let names: Vec<&str> = nodes.iter().filter_map(|(_, node)| node.label()).collect();

   for url in &urls {
      let host = host_of(url);

      assert!(
         names.iter().any(|name| name.contains(host)),
         "the row for {host} must be named after its host: {names:?}"
      );
   }
}

/// The add-RPC form draws its error inline instead of opening a dialog, so the page raises the
/// words through the window's announcement channel on the frame the line starts showing.
#[test]
fn an_invalid_rpc_url_is_said() {
   let (ctx, mut gui, zeus) = settings_window();

   let nodes = open_networks_page(&ctx, &mut gui, &zeus);
   let add_url = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some("Add RPC Url"))
      .map(|(id, _)| *id)
      .expect("the page must offer the Add RPC Url button");

   let nodes = render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, add_url)],
   );

   let field = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::TextInput && node.label() == Some("RPC URL"))
      .map(|(id, _)| *id)
      .expect("the add-RPC form must name its URL field");

   render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Focus, field)],
   );

   // "Add" stays disabled while the text is not a URL, so the error is the form's own line: it is
   // spoken the frame it appears rather than waiting for a press that cannot happen.
   render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![Event::Text("not-a-url".to_owned())],
   );

   assert_eq!(
      gui.settings.take_notice().as_deref(),
      Some("Invalid URL"),
      "an invalid URL must be said, not only drawn"
   );
}

/// The roles egui's single-line text fields use.
fn is_text_input(role: Role) -> bool {
   matches!(
      role,
      Role::TextInput | Role::MultilineTextInput | Role::PasswordInput
   )
}

/// A node's accessible name: its own label, or the value of the label that names it.
///
/// `InputField` draws a visible label and ties it to the field with `labelled_by`, so the field
/// itself carries no label.
fn node_name(nodes: &[(NodeId, Node)], node: &Node) -> Option<String> {
   node.label().map(str::to_owned).or_else(|| {
      node.labelled_by().iter().find_map(|id| {
         nodes
            .iter()
            .find(|(node_id, _)| node_id == id)
            .and_then(|(_, label)| label.value())
            .map(str::to_owned)
      })
   })
}

/// Each Railgun slider is one node named with the sentence that used to sit above it, says the
/// range it covers, and keeps the value the crate gave it; the sentence is not left behind as a
/// label too.
#[test]
fn the_railgun_page_names_its_sliders() {
   let (ctx, mut gui, zeus) = settings_window();

   let nodes = open_settings_page(&ctx, &mut gui, &zeus, "Railgun");

   let sliders: Vec<&Node> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Slider)
      .map(|(_, node)| node)
      .collect();

   assert_eq!(
      sliders.len(),
      3,
      "the Railgun page draws three sliders"
   );

   let block_range = format!(
      "{} to {}",
      RailgunConfig::min_block_range(),
      RailgunConfig::max_block_range()
   );
   let concurrency = format!(
      "{} to {}",
      RailgunConfig::min_concurrency(),
      RailgunConfig::max_concurrency()
   );
   let interval = format!(
      "{} to {}",
      RailgunConfig::min_state_update_interval(),
      RailgunConfig::max_state_update_interval()
   );

   let ranges = [
      ("RPC Syncer Block Range", block_range),
      ("RPC Syncer Concurrency", concurrency),
      ("State Update Interval (Minutes)", interval),
   ];

   for (sentence, range) in &ranges {
      let slider = sliders
         .iter()
         .find(|node| node.label() == Some(*sentence))
         .unwrap_or_else(|| panic!("the {sentence} slider must be named with its sentence"));

      assert_eq!(
         slider.description(),
         Some(range.as_str()),
         "the {sentence} slider must say the range it covers"
      );
      assert!(
         slider.numeric_value().is_some(),
         "the {sentence} slider must still report the value the crate gave it"
      );
   }

   let spoken: Vec<&str> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Label && !node.is_hidden())
      .filter_map(|(_, node)| node.value())
      .collect();

   for (sentence, _) in &ranges {
      assert!(
         !spoken.contains(sentence),
         "the {sentence} label must be hidden, or the slider is spoken twice: {spoken:?}"
      );
   }

   for name in ["Save", "Reset"] {
      assert!(
         nodes
            .iter()
            .any(|(_, node)| node.role() == Role::Button && node.label() == Some(name)),
         "the Railgun page must still offer the {name} button"
      );
   }
}

/// The add-contact form names its own field: the "Name" label above it is hidden and the field
/// carries the name. The two address fields label themselves; their names arrive through the
/// labels that name them.
#[test]
fn a_contact_form_names_its_fields() {
   let (ctx, mut gui, zeus) = settings_window();

   let nodes = open_settings_page(&ctx, &mut gui, &zeus, "Contacts");
   let add = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some("Add Contact"))
      .map(|(id, _)| *id)
      .expect("the Contacts page must offer the Add Contact button");

   let nodes = render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, add)],
   );

   let fields: Vec<(Role, Option<String>)> = nodes
      .iter()
      .filter(|(_, node)| is_text_input(node.role()))
      .map(|(_, node)| (node.role(), node_name(&nodes, node)))
      .collect();

   assert!(
      fields.iter().any(|(_, name)| name.as_deref() == Some("Contact name")),
      "the name field must be named Contact name: {fields:?}"
   );

   for expected in ["Public Address", "Railgun Address"] {
      assert!(
         fields.iter().any(|(_, name)| name.as_deref() == Some(expected)),
         "the {expected} field must carry its own name: {fields:?}"
      );
   }
}

/// A contact row is one group node named after the contact, and its two unnamed buttons say
/// whose options they open and which address they copy. The name label inside the row is hidden,
/// so the row says it once.
#[test]
fn a_contact_row_says_who_it_is() {
   let name = "Ada Lovelace";
   let address = "0x1111111111111111111111111111111111111111";

   let (ctx, mut gui, zeus) = settings_window();
   zeus.write_wallet_state(|ws| {
      ws.contacts.push(Contact::new(
         name.to_string(),
         address.to_string(),
         String::new(),
      ));
   });

   let nodes = open_settings_page(&ctx, &mut gui, &zeus, "Contacts");

   assert!(
      nodes
         .iter()
         .any(|(_, node)| node.role() == Role::Group && node.label() == Some(name)),
      "the contact row must be one group node named after the contact"
   );

   let options = format!("Options for {name}");
   assert!(
      nodes
         .iter()
         .any(|(_, node)| node.role() == Role::Button && node.label() == Some(options.as_str())),
      "the row's dots button must say whose options it opens"
   );

   let copy = format!("Copy address {address}");
   assert!(
      nodes
         .iter()
         .any(|(_, node)| node.role() == Role::Button && node.label() == Some(copy.as_str())),
      "the row's address button must say what it copies"
   );

   let spoken: Vec<&str> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Label && !node.is_hidden())
      .filter_map(|(_, node)| node.value())
      .collect();

   assert!(
      !spoken.contains(&name),
      "the row's own name must not also be a plain label: {spoken:?}"
   );
}

/// The money rows draw an address shortened, so two addresses that agree on their first and last
/// six characters look the same on screen and are different addresses. The row's node has to
/// carry the whole address instead, with the drawn form left as its description.
#[test]
fn the_recipient_row_names_the_full_address() {
   let ctx = Context::default();
   let zeus = ZeusCtx::new(crate::runtime::test_handle());
   let theme = Theme::new(ThemeKind::TokyoNight);

   let chain = ChainId::Ethereum;
   let account = Address::from([0x11; 20]);

   // An unnamed address asks for a name off-frame the first time it is drawn. Claiming that
   // once-per-session request first keeps the test away from the network.
   zeus.write(|zeus| {
      zeus.address_name_requested(chain.id(), account);
   });

   let nodes = frame(&ctx, |ui| {
      zeus.write(|zeus| {
         crate::gui::ui::tx::rows::address_with_picked_name(
            zeus,
            chain,
            "Recipient",
            account,
            None,
            &theme,
            ui,
         );
      });
   });

   let link = nodes
      .iter()
      .find(|node| node.role() == Role::Link)
      .expect("the recipient row draws its address as a link");

   let name = link.label().expect("the link must be named");

   assert_eq!(
      name.replace(' ', ""),
      account.to_string(),
      "the link must carry the whole address, not its two ends"
   );
   assert_ne!(
      name, "0x1111...111111",
      "the drawn truncation is not a name"
   );
   assert_eq!(
      link.description(),
      Some("0x1111...111111"),
      "the drawn form stays reachable as the description"
   );
}

/// What a row of the portfolio says to a reader, decided away from the page that draws it.
///
/// The total is spoken once when the page is opened, and again only when the user's own Refresh
/// moves it: a rebuild nobody asked for, a wallet switch and a frame tick all leave a number that
/// is already known alone, and leaving the page makes the next arrival a new one.
#[test]
fn the_portfolio_total_is_announced_once_per_arrival() {
   use crate::gui::ui::portfolio::announcement;

   let chain = "Ethereum";

   // The rows are built off the GUI thread, so the page opens a frame or two before it has a
   // total at all.
   assert_eq!(
      announcement(None, None, false, chain),
      None,
      "a total that has not landed says nothing"
   );

   assert_eq!(
      announcement(None, Some("$1,234.56"), false, chain).as_deref(),
      Some("Portfolio value on Ethereum, $1,234.56"),
      "the first sight of a known total is what a reader hears"
   );

   assert_eq!(
      announcement(Some("$1,234.56"), Some("$1,234.56"), false, chain),
      None,
      "the same total a frame later is not a second announcement"
   );

   assert_eq!(
      announcement(Some("$1,234.56"), Some("$1,300.00"), false, chain),
      None,
      "a total that moved on its own is not the user asking about it"
   );

   assert_eq!(
      announcement(Some("$1,300.00"), Some("$1,310.00"), true, chain).as_deref(),
      Some("Portfolio refreshed. Value $1,310.00"),
      "a number the user's own Refresh moved is said, and said as the answer to that press"
   );

   assert_eq!(
      announcement(Some("$1,310.00"), Some("$1,310.00"), true, chain),
      None,
      "a refresh that found the same number has nothing to add"
   );

   // Leaving the page forgets what was read out, so coming back is an arrival again — with the
   // number as it is now, which may have moved while the page was away.
   assert_eq!(
      announcement(None, Some("$1,310.00"), false, chain).as_deref(),
      Some("Portfolio value on Ethereum, $1,310.00"),
      "coming back to the page announces the total once"
   );
}

/// A themed label is named by the sentence the row means rather than by the glyphs it draws, and a
/// widget that reported no node at all is given one. Either way the words land in both the value a
/// reader reads a label from and the label it falls back to.
#[test]
fn a_themed_label_is_named_with_its_sentence() {
   let ctx = Context::default();

   let nodes = frame(&ctx, |ui| {
      let label = ui.add(egui_elements::Label::new("ETH", None));
      crate::gui::ui::common::name_text(ui, &label, "Ethereum");

      // The node-less case: a widget that never reported anything has nothing to rename.
      let silent = ui.allocate_response(vec2(10.0, 10.0), egui::Sense::hover());
      assert!(
         !ui.ctx().has_accesskit_node(silent.id),
         "the stand-in must start without a node, or this asserts nothing"
      );
      crate::gui::ui::common::name_text(ui, &silent, "Balance 0.00");
   });

   let names: Vec<&str> = nodes
      .iter()
      .filter(|node| node.role() == Role::Label)
      .filter_map(|node| node.value())
      .collect();

   assert_eq!(
      names.iter().filter(|name| **name == "Ethereum").count(),
      1,
      "a label that already had a node keeps exactly one, named with its sentence: {names:?}"
   );

   let named = nodes
      .iter()
      .find(|node| node.value() == Some("Ethereum"))
      .expect("the themed label must be a node of its own");

   assert_eq!(
      named.label(),
      Some("Ethereum"),
      "a label node must say the sentence whichever property a consumer reads"
   );

   assert!(
      names.contains(&"Balance 0.00"),
      "a widget with no node of its own must be given one: {names:?}"
   );
}
/// The amount field's node says how much it holds and what it is worth, never what was typed: the
/// amount is on screen, and a node that repeated it would read the whole field back on every
/// keystroke. Everything the field draws beside itself is folded into that one node.
#[test]
fn the_amount_field_never_reads_out_its_text() {
   use crate::gui::ui::common::{AmountField, AmountFieldParams};
   use zeus_eth::currency::{Currency, NativeCurrency};
   use zeus_eth::utils::NumericValue;

   let ctx = Context::default();
   let mut gui = GUI::default();
   gui.theme.install(&ctx);

   let theme = gui.theme.clone();
   let icons = gui.icons.clone();
   let currency = Currency::from(NativeCurrency::from_chain_id(1).unwrap());

   let mut field = AmountField::new();
   field.amount = "123.45".to_string();

   let mut nodes = Vec::new();

   // The percent row measures its items on one pass and places them on the next.
   for _ in 0..3 {
      nodes = frame(&ctx, |ui| {
         field.show(
            AmountFieldParams::new(
               &theme,
               icons.clone(),
               &currency,
               Address::from([0x11; 20]),
               1,
            )
            .balance(NumericValue::from_f64(10.0))
            .max_amount(NumericValue::from_f64(10.0))
            // What the amount is worth, which is a different figure from the amount itself.
            .value(NumericValue::from_f64(308.62))
            .label("Amount")
            .show_slider(true),
            ui,
         );
      });
   }

   let field = nodes
      .iter()
      .find(|node| is_text_input(node.role()))
      .expect("the amount field must be a text field in the tree");

   assert_eq!(
      field.label(),
      Some("Amount in ETH"),
      "the field is named after what it holds and the token it is in"
   );

   assert_eq!(
      field.value(),
      Some("6 characters"),
      "the value says how much the field holds, not what it holds"
   );

   let value = field.value().unwrap_or_default();
   let description = field.description().unwrap_or_default();

   assert!(
      !value.contains("123.45") && !description.contains("123.45"),
      "the typed amount must never be read out: value {value:?}, description {description:?}"
   );

   assert_eq!(
      description, "Value $308.62",
      "the fiat value the field draws under itself is its description"
   );

   let spoken: Vec<&str> = nodes
      .iter()
      .filter(|node| node.role() == Role::Label && !node.is_hidden())
      .filter_map(|node| node.value())
      .collect();

   assert!(
      !spoken.contains(&"Amount"),
      "the drawn Amount label must leave the tree, or the field is named twice: {spoken:?}"
   );

   assert!(
      !spoken.iter().any(|text| text.ends_with('%')),
      "the percent readout repeats the slider's own value: {spoken:?}"
   );

   let slider = nodes
      .iter()
      .find(|node| node.role() == Role::Slider)
      .expect("the percent slider must be in the tree");

   assert_eq!(
      slider.label(),
      Some("Amount percent"),
      "the slider is named after the amount share it sets"
   );

   assert!(
      nodes
         .iter()
         .any(|node| node.role() == Role::Button && node.label() == Some("Change token (ETH)")),
      "the currency button says what it shows and what pressing it does"
   );

   assert!(
      spoken.contains(&"Balance 10.00"),
      "the balance is named, not left as a bare number: {spoken:?}"
   );
}

/// Render the recipient row headlessly, with `events` delivered to the first pass.
fn render_recipient(
   ctx: &Context,
   gui: &mut GUI,
   zeus: &ZeusCtx,
   events: Vec<Event>,
) -> Vec<(NodeId, Node)> {
   frame_nodes(ctx, events, |ui| {
      let theme = gui.theme.clone();
      let icons = gui.icons.clone();
      let recipient_selection = &mut gui.recipient_selection;
      let contacts = &mut gui.settings.contacts_ui;

      zeus.write(|zeus| {
         crate::gui::ui::common::recipient_field(
            zeus,
            &theme,
            icons,
            recipient_selection,
            contacts,
            false,
            1,
            ChainId::Ethereum,
            ui,
         )
      });
   })
}

/// The recipient field says how much of an address it holds and how it ends, never the address; the
/// name the address resolved to is the description, and the block-explorer link is a named button.
#[test]
fn the_recipient_field_says_how_much_of_the_address_it_holds() {
   const ADDRESS: &str = "0x1111111111111111111111111111111111111111";

   let (ctx, mut gui, zeus) = main_window();
   gui.recipient_selection.recipient.evm_address = ADDRESS.to_string();

   let nodes = render_recipient(&ctx, &mut gui, &zeus, Vec::new());

   let field = nodes
      .iter()
      .find(|(_, node)| is_text_input(node.role()))
      .map(|(_, node)| node)
      .expect("the recipient row must draw its address field");

   assert_eq!(
      field.label(),
      Some("Recipient address"),
      "the field carries the word the drawn label used to say"
   );

   let value = field.value().unwrap_or_default();

   assert_eq!(
      value, "42 characters, ends 1111",
      "the value says how much of the address is there and how it ends"
   );

   assert!(
      !value.contains(ADDRESS),
      "the address itself must never be the value: {value:?}"
   );

   assert_eq!(
      field.description(),
      Some("Press Enter to choose a recipient."),
      "with no name the field still says how to reach the picker"
   );

   assert!(
      nodes.iter().any(|(_, node)| node.role() == Role::Button
         && node.label() == Some("Open the recipient in the block explorer")),
      "the explorer link must be a named button a reader can land on"
   );

   // The name the address resolved to is the description, so the field is recognisable without
   // the address being read back to the user.
   gui.recipient_selection.recipient.name = Some("ada.eth".to_string());

   let nodes = render_recipient(&ctx, &mut gui, &zeus, Vec::new());

   let field = nodes
      .iter()
      .find(|(_, node)| is_text_input(node.role()))
      .map(|(_, node)| node)
      .expect("the recipient row must still draw its field");

   assert_eq!(
      field.description(),
      Some("ada.eth. Press Enter to choose a recipient."),
      "the resolved name leads the description, and the way into the picker follows it"
   );
}

/// The picker opened on a mouse click on this field and by nothing else, so someone typing an
/// address could never reach their contacts. A single-line edit gives its focus up on Enter rather
/// than inserting anything, which leaves the key free to open the same picker.
#[test]
fn the_recipient_field_opens_the_picker_when_enter_is_pressed() {
   let (ctx, mut gui, zeus) = main_window();

   let nodes = render_recipient(&ctx, &mut gui, &zeus, Vec::new());

   let field = nodes
      .iter()
      .find(|(_, node)| is_text_input(node.role()))
      .map(|(id, _)| *id)
      .expect("the recipient row must draw its address field");

   assert!(
      !gui.recipient_selection.is_open(),
      "the picker starts closed, or this asserts nothing"
   );

   render_recipient(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Focus, field)],
   );
   render_recipient(&ctx, &mut gui, &zeus, vec![key_press(Key::Enter)]);

   assert!(
      gui.recipient_selection.is_open(),
      "Enter in the field must open the picker the click opens"
   );
}

/// Draw the Send form the way the central panel does, and return its tree.
///
/// The panel gates the pages on the unlocked vault; the form itself does not, so a context with no
/// vault still draws every node a reader has to land on.
fn render_send(ctx: &Context, gui: &mut GUI, zeus: &ZeusCtx) -> Vec<Node> {
   let mut nodes = Vec::new();

   // The form measures its frame on the first pass and draws into it on the second.
   for _ in 0..2 {
      nodes = frame(ctx, |ui| {
         let icons = gui.icons.clone();
         let theme = gui.theme.clone();
         let send = &mut gui.send_crypto;
         let token_selection = &mut gui.token_selection;
         let recipient_selection = &mut gui.recipient_selection;
         let contacts = &mut gui.settings.contacts_ui;

         zeus.write(|zeus| {
            send.show(
               zeus,
               icons,
               &theme,
               token_selection,
               recipient_selection,
               contacts,
               ui,
            )
         });
      });
   }

   nodes
}

/// Stand the zero-address wallet up in the cache the Send form reads its wallet from.
///
/// Without a vault that cache is empty, and the form asks it which wallet it is on screen for.
fn wallet_in_cache(zeus: &ZeusCtx) {
   zeus.write(|ctx| {
      let info = crate::core::WalletInfo::default();

      ctx.wallet_info_cache = crate::core::types::WalletInfoCache::new(
         std::collections::HashMap::from([(info.address, info.clone())]),
         vec![info],
      );
   });
}

/// The reason a Send is blocked used to be the disabled button's label, which no reader can reach.
/// It is a line of its own now, said once when it appears, and the button keeps its name.
#[test]
fn a_blocked_send_says_why_beside_a_button_named_send() {
   let (ctx, mut gui, zeus) = main_window();
   wallet_in_cache(&zeus);

   gui.send_crypto.open();
   gui.send_crypto.amount_field.amount = "0.5".to_string();

   let nodes = render_send(&ctx, &mut gui, &zeus);

   let send = nodes
      .iter()
      .find(|node| node.role() == Role::Button && node.label() == Some("Send"))
      .expect("the form must draw its Send button");

   assert!(
      send.is_disabled(),
      "the button must be blocked, which is the state this test is about"
   );

   assert!(
      nodes
         .iter()
         .any(|node| node.role() == Role::Heading && node.label() == Some("Send Crypto")),
      "the form's own title must be a heading of the page"
   );

   let reason = "Insufficient ETH Balance";

   let spoken: Vec<&str> = nodes
      .iter()
      .filter(|node| node.role() == Role::Label && !node.is_hidden())
      .filter_map(|node| node.value())
      .collect();

   assert!(
      spoken.contains(&reason),
      "the reason must be a node of its own, reachable while the button is not: {spoken:?}"
   );

   let line = nodes
      .iter()
      .find(|node| node.role() == Role::Label && node.value() == Some(reason))
      .and_then(|node| node.bounds())
      .expect("the reason is drawn, so its node has bounds");

   let button = send.bounds().expect("the button is drawn, so its node has bounds");

   assert!(
      ((line.x0 + line.x1) - (button.x0 + button.x1)).abs() < 1.0,
      "the line must sit in the button's own column: {line:?} against {button:?}"
   );

   assert!(
      line.y1 <= button.y0,
      "the line must be drawn above the button: {line:?} against {button:?}"
   );

   assert_eq!(
      gui.send_crypto.take_notice().as_deref(),
      Some(reason),
      "the reason must be said when it appears"
   );

   // The same block a frame later is not a second announcement.
   render_send(&ctx, &mut gui, &zeus);

   assert_eq!(
      gui.send_crypto.take_notice(),
      None,
      "a reason that has not changed says nothing twice"
   );

   // A different reason is a new sentence, drawn and said.
   gui.send_crypto.amount_field.amount = "abc".to_string();

   let nodes = render_send(&ctx, &mut gui, &zeus);

   let reason = "Invalid Amount";

   let spoken: Vec<&str> = nodes
      .iter()
      .filter(|node| node.role() == Role::Label && !node.is_hidden())
      .filter_map(|node| node.value())
      .collect();

   assert!(
      spoken.contains(&reason),
      "the new reason must be the line that is drawn: {spoken:?}"
   );

   assert_eq!(
      gui.send_crypto.take_notice().as_deref(),
      Some(reason),
      "a reason that changed is a new sentence"
   );
}

/// The NFT side of the form is named as well: the selector says what it changes, the quantity field
/// is the amount, and the same line explains why Send is blocked.
#[test]
fn the_nft_send_form_names_its_selector_and_amount() {
   use zeus_eth::alloy_primitives::U256;
   use zeus_eth::nft::{NftStandard, NftToken};

   let (ctx, mut gui, zeus) = main_window();
   wallet_in_cache(&zeus);

   gui.send_crypto.open();
   gui.send_crypto.set_nft(NftToken {
      chain_id: 1,
      collection: Address::from([0xbc; 20]),
      token_id: U256::from(7),
      standard: NftStandard::Erc1155,
      metadata_uri: None,
   });

   let nodes = render_send(&ctx, &mut gui, &zeus);

   assert!(
      nodes
         .iter()
         .any(|node| node.role() == Role::Button && node.label() == Some("Change NFT")),
      "the selector must say what it changes"
   );

   let amount = nodes
      .iter()
      .find(|node| is_text_input(node.role()))
      .expect("the quantity field must be a text field in the tree");

   assert_eq!(
      amount.label(),
      Some("Amount"),
      "the quantity field is the amount of the token picked above"
   );

   assert!(
      nodes
         .iter()
         .any(|node| node.role() == Role::Button && node.label() == Some("Send")),
      "the button keeps its name on this path too"
   );

   let spoken: Vec<&str> = nodes
      .iter()
      .filter(|node| node.role() == Role::Label && !node.is_hidden())
      .filter_map(|node| node.value())
      .collect();

   assert!(
      spoken.contains(&"Invalid Amount"),
      "the reason a block is drawn in the same place on this path: {spoken:?}"
   );
}

/// The announcement rule the Send form follows: a reason that appears or changes is a new sentence,
/// the same reason a frame later is not, and a block that lifts says nothing.
#[test]
fn the_send_block_is_announced_when_it_appears_or_changes() {
   use crate::gui::ui::send_crypto::blocked_announcement;

   for (previous, current, expected) in [
      (
         None,
         Some("Invalid Amount"),
         Some("Invalid Amount"),
      ),
      (
         Some("Invalid Amount"),
         Some("Invalid Amount"),
         None,
      ),
      (
         Some("Invalid Amount"),
         Some("Insufficient ETH Balance"),
         Some("Insufficient ETH Balance"),
      ),
      (Some("Invalid Amount"), None, None),
      (None, None, None),
   ] {
      assert_eq!(
         blocked_announcement(previous, current).as_deref(),
         expected,
         "{previous:?} then {current:?}"
      );
   }
}

/// Draw the portfolio page the way the frame does, out of the `SHARED_GUI` the frame gives it.
///
/// The rows are built on a worker that reads the handle from that global and writes them back into
/// it, so a `GUI` of the test's own would draw a header with nothing under it for ever.
fn render_portfolio(ctx: &Context) -> Vec<(NodeId, Node)> {
   frame_nodes(ctx, Vec::new(), |ui| {
      crate::gui::SHARED_GUI.write(|gui| {
         gui.portofolio.open();

         let zeus = gui.ctx.clone();
         let theme = gui.theme.clone();
         let icons = gui.icons.clone();
         let page = &mut gui.portofolio;
         let selection = &mut gui.token_selection;

         zeus.write(|zeus| page.show(zeus, &theme, icons, selection, ui));
      });
   })
}

/// Every surface the portfolio paints says what it is: the asset cell is read by the token's name
/// where Zeus has one, the price, the balance and the value each say which figure they are, and the
/// total says what the number is and which chain it is on. The page also hands its arrival
/// sentence back to the frame, which owns the announcement channel.
#[test]
fn the_portfolio_names_its_total_and_every_cell() {
   let _turn = shared_gui_turn();
   let ctx = Context::default();

   // The page reads its theme out of the context it draws into, and names the wallet it is on
   // screen for: that name comes from the cache unlock fills, so the zero address — the wallet a
   // context with no vault is on — is stood up in it here. No vault, RPC or icon is needed for the
   // rows themselves.
   crate::gui::SHARED_GUI.write(|gui| {
      let mut theme = gui.theme.clone();
      theme.install(&ctx);

      let zeus = gui.ctx.clone();

      zeus.write(|ctx| {
         let info = crate::core::WalletInfo::default();

         ctx.wallet_info_cache = crate::core::types::WalletInfoCache::new(
            std::collections::HashMap::from([(info.address, info.clone())]),
            vec![info],
         );
      });
   });

   let mut nodes = Vec::new();

   // The rows land a frame or two after the page opens, because they are built off the GUI thread.
   for _ in 0..200 {
      nodes = render_portfolio(&ctx);

      if nodes.iter().any(|(_, node)| node.label() == Some("Ethereum")) {
         break;
      }

      std::thread::sleep(std::time::Duration::from_millis(10));
   }

   let drawn: Vec<&str> = nodes.iter().filter_map(|(_, node)| node.label()).collect();

   let asset = nodes
      .iter()
      .find(|(_, node)| node.label() == Some("Ethereum"))
      .map(|(_, node)| node)
      .unwrap_or_else(|| panic!("the asset cell must be read by the token's name: {drawn:?}"));

   assert_eq!(
      asset.description(),
      Some("ETH"),
      "the symbol the cell draws stays reachable as the description"
   );

   for sentence in ["Price $0.00", "Balance 0.00", "Value $0.00"] {
      assert!(
         nodes.iter().any(|(_, node)| node.label() == Some(sentence)),
         "the cell must say which figure it is, and {sentence:?} is missing from {drawn:?}"
      );
   }

   let total = "Portfolio value on Ethereum, $0.00";

   assert!(
      nodes
         .iter()
         .any(|(_, node)| node.value() == Some(total) || node.label() == Some(total)),
      "the total must say what the number is and which chain it is on: {drawn:?}"
   );

   assert_eq!(
      crate::gui::SHARED_GUI.write(|gui| gui.portofolio.take_notice()).as_deref(),
      Some(total),
      "the page must hand its arrival sentence to the frame, which says it"
   );
}

/// The turn every test that draws out of the frame's one `SHARED_GUI` takes.
///
/// That global holds the picker and the portfolio page together, and the page takes whatever the
/// picker holds as its selection — it resets the picker as soon as one arrives. Two tests drawing it
/// at once would take each other's rows and selections, so they take turns.
static SHARED_GUI_TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn shared_gui_turn() -> std::sync::MutexGuard<'static, ()> {
   SHARED_GUI_TURN.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The USDC row's name: the symbol, the full name Zeus knows for the token, and the balance the row
/// draws beside them.
const USDC_ROW: &str = "USDC, USD Coin, 12.50 USDC";

/// What the menu button must say it opens.
const USDC_OPTIONS: &str = "Options for USDC, USD Coin, 12.50 USDC";

/// Two rows for the picker: a USDC balance worth a figure, and the empty native currency, which
/// draws no balance and so says nothing about one.
fn picker_rows() -> Vec<(Currency, NumericValue, NumericValue)> {
   use zeus_eth::alloy_primitives::U256;

   let usdc = ERC20Token::from_components(
      1,
      Address::from([0xaa; 20]),
      "USDC",
      "USD Coin",
      6,
      U256::ZERO,
   );

   vec![
      (
         Currency::from(usdc),
         NumericValue::from_f64(12.5),
         NumericValue::from_f64(31.25),
      ),
      (
         Currency::from(NativeCurrency::eth()),
         NumericValue::from_f64(0.0),
         NumericValue::from_f64(0.0),
      ),
   ]
}

/// Draw the picker for one frame, out of the `SHARED_GUI` the frame gives it.
///
/// The list a real opening shows is fetched on a worker that writes it — and the end of the loading
/// state — back into that same global, so a `GUI` of the test's own would draw its spinner for ever.
/// The rows are seeded here instead, in the same exclusive step that draws them, so nothing else can
/// seed over them between the two.
fn render_picker(
   ctx: &Context,
   rows: &[(Currency, NumericValue, NumericValue)],
   events: Vec<Event>,
) -> Vec<(NodeId, Node)> {
   let rows = rows.to_vec();

   frame_nodes(ctx, events, |ui| {
      crate::gui::SHARED_GUI.write(|gui| {
         gui.token_selection.set_processed_currencies(rows.clone());

         let theme = gui.theme.clone();
         let icons = gui.icons.clone();
         let zeus = gui.ctx.clone();
         let picker = &mut gui.token_selection;

         zeus.write(|zeus| {
            picker.show(
               zeus,
               &theme,
               icons,
               1,
               Address::from([0x00; 20]),
               ui,
            )
         });
      });
   })
}

/// Open the picker on `rows` and draw it until the list is on screen.
///
/// The opening starts a balance fetch, and that fetch's writeback is what ends the loading state the
/// picker begins in: the first frames draw a spinner and nothing else. A search box in the tree is
/// the frame where the list itself is drawn.
fn open_picker(
   ctx: &Context,
   rows: &[(Currency, NumericValue, NumericValue)],
) -> Vec<(NodeId, Node)> {
   crate::gui::SHARED_GUI.write(|gui| {
      let mut theme = gui.theme.clone();
      theme.install(ctx);
      // The picker keeps its query across openings, so an earlier opening's search would decide
      // what this one lists.
      gui.token_selection.search_query.clear();
      gui.token_selection.open(false, 1, Address::from([0x00; 20]));
   });

   for _ in 0..300 {
      let nodes = render_picker(ctx, rows, Vec::new());

      if nodes.iter().any(|(_, node)| is_text_input(node.role())) {
         return nodes;
      }

      std::thread::sleep(std::time::Duration::from_millis(10));
   }

   panic!("the picker never finished loading its list");
}

/// The token the picker holds as its selection, by symbol.
fn picked_symbol() -> Option<String> {
   crate::gui::SHARED_GUI
      .write(|gui| gui.token_selection.get_selected_currency().cloned())
      .map(|currency| currency.symbol().to_string())
}

/// A picker row used to be a frame with a click sense bolted on: a hit target with no role, no name
/// and no way in from a reader. The row is one named button now, and an AccessKit Click on its node
/// is the same selection hook a mouse click is.
#[test]
fn a_token_row_can_be_picked_by_a_reader() {
   let _turn = shared_gui_turn();
   let ctx = Context::default();
   let rows = picker_rows();

   let nodes = open_picker(&ctx, &rows);

   let (row, node) = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some(USDC_ROW))
      .expect("the USDC row must be one button named after what it holds");

   assert!(
      node.supports_action(Action::Click),
      "the row has to answer a Click request, or a reader cannot activate it"
   );

   assert!(
      node.supports_action(Action::Focus),
      "the row has to be reachable by keyboard as well"
   );

   assert_eq!(
      node.description(),
      Some("$31.25"),
      "the fiat value the row draws is its description"
   );

   assert_eq!(
      picked_symbol(),
      None,
      "nothing is picked before the row is used"
   );

   render_picker(
      &ctx,
      &rows,
      vec![accesskit_action(Action::Click, *row)],
   );

   assert_eq!(
      picked_symbol().as_deref(),
      Some("USDC"),
      "an AccessKit Click on the row must pick that token"
   );
}

/// The row is a hit target as well as a button: a mouse click anywhere on it still picks the token,
/// which is the path that existed before it had a name.
#[test]
fn a_token_row_still_answers_a_mouse_click() {
   let _turn = shared_gui_turn();
   let ctx = Context::default();
   let rows = picker_rows();

   let nodes = open_picker(&ctx, &rows);

   let bounds = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some(USDC_ROW))
      .and_then(|(_, node)| node.bounds())
      .expect("the row is drawn, so its node has bounds");

   let pos = pos2(
      ((bounds.x0 + bounds.x1) / 2.0) as f32,
      ((bounds.y0 + bounds.y1) / 2.0) as f32,
   );

   let button = |pressed| Event::PointerButton {
      pos,
      button: PointerButton::Primary,
      pressed,
      modifiers: Modifiers::NONE,
   };

   render_picker(&ctx, &rows, vec![button(true)]);
   render_picker(&ctx, &rows, vec![button(false)]);

   assert_eq!(
      picked_symbol().as_deref(),
      Some("USDC"),
      "a mouse click on the row must still pick that token"
   );
}

/// The keyboard path: the row is focusable, a reader's Focus request lands on it, and Enter on it
/// picks the token. The focus has to survive the frame that draws the row — the plate registers
/// itself with an empty sense first, which withdraws it — or Space and Enter would never reach it.
#[test]
fn a_token_row_takes_focus_and_enter_picks_it() {
   let _turn = shared_gui_turn();
   let ctx = Context::default();
   let rows = picker_rows();

   let nodes = open_picker(&ctx, &rows);

   let row = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some(USDC_ROW))
      .map(|(id, _)| *id)
      .expect("the USDC row must be in the tree");

   render_picker(
      &ctx,
      &rows,
      vec![accesskit_action(Action::Focus, row)],
   );

   let focused = ctx.memory(|memory| memory.focused());

   assert_eq!(
      focused.map(|id| id.accesskit_id()),
      Some(row),
      "the row must take the focus a reader asked for, and hold it after the frame drew it"
   );

   render_picker(&ctx, &rows, vec![key_press(Key::Enter)]);

   assert_eq!(
      picked_symbol().as_deref(),
      Some("USDC"),
      "Enter on the focused row must pick that token"
   );
}

/// The row says the token and its balance itself, so the labels inside it are hidden: a reader hears
/// the row once, instead of the row and then "USD Coin USDC", "31.25" and "12.50" after it.
#[test]
fn the_picker_row_is_read_once() {
   let _turn = shared_gui_turn();
   let ctx = Context::default();

   let nodes = open_picker(&ctx, &picker_rows());

   let spoken: Vec<&str> = nodes
      .iter()
      .filter(|(_, node)| !node.is_hidden())
      .filter_map(|(_, node)| node.label().or_else(|| node.value()))
      .collect();

   for covered in ["USD Coin\nUSDC", "$31.25", "12.50"] {
      assert!(
         !spoken.contains(&covered),
         "the row's own name already says {covered:?}, so it is not a node of its own: {spoken:?}"
      );
   }

   assert!(
      spoken.contains(&USDC_ROW),
      "the row's one node carries the words those labels drew: {spoken:?}"
   );
}

/// A row says whether it is the one the picker points at, so a reader can tell where the surface
/// stands without being told the whole list.
#[test]
fn the_picker_row_says_which_token_is_selected() {
   let _turn = shared_gui_turn();
   let ctx = Context::default();
   let rows = picker_rows();

   open_picker(&ctx, &rows);

   crate::gui::SHARED_GUI.write(|gui| {
      gui.token_selection.selected_currency = Some(rows[0].0.clone());
   });

   let nodes = render_picker(&ctx, &rows, Vec::new());

   let selected = |name: &str| {
      nodes
         .iter()
         .find(|(_, node)| node.role() == Role::Button && node.label() == Some(name))
         .map(|(_, node)| node.toggled())
         .expect("the row must be a button in the tree")
   };

   assert_eq!(
      selected(USDC_ROW),
      Some(Toggled::True),
      "the row the picker holds is the selected one"
   );

   assert_eq!(
      selected("ETH, Ethereum"),
      Some(Toggled::False),
      "every other row says it is not the selection"
   );

   let eth = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some("ETH, Ethereum"))
      .map(|(_, node)| node)
      .expect("the empty native row must be in the tree too");

   assert_eq!(
      eth.description(),
      None,
      "a token with no balance draws no fiat value, so it has none to describe"
   );
}

/// The "..." button drew three dots and said nothing: its accessible name was the empty string. It
/// names the row it opens now, and the search box — which painted only a hint — names itself too.
#[test]
fn the_picker_names_its_menu_button_and_its_search_field() {
   let _turn = shared_gui_turn();
   let ctx = Context::default();
   let rows = picker_rows();

   let nodes = open_picker(&ctx, &rows);

   assert!(
      nodes
         .iter()
         .any(|(_, node)| node.role() == Role::Button && node.label() == Some(USDC_OPTIONS)),
      "the menu button must say whose options it opens"
   );

   let search = nodes
      .iter()
      .find(|(_, node)| is_text_input(node.role()))
      .map(|(_, node)| node)
      .expect("the picker must draw its search field");

   assert_eq!(
      node_name(&nodes, search).as_deref(),
      Some("Search tokens"),
      "the field is named for the list it narrows"
   );

   assert_eq!(
      search.description(),
      Some("Search tokens or enter an address"),
      "the hint the field paints is its description"
   );

   crate::gui::SHARED_GUI.write(|gui| gui.token_selection.search_query = "usd".to_string());

   let nodes = render_picker(&ctx, &rows, Vec::new());

   let search = nodes
      .iter()
      .find(|(_, node)| is_text_input(node.role()))
      .map(|(_, node)| node)
      .expect("the picker must still draw its search field");

   assert_eq!(
      search.value(),
      Some("usd"),
      "a search box is not a secret, so what it holds stays its value"
   );
}

/// The announcement rule the picker follows: the count of rows the search left is a sentence when it
/// appears or changes, never for the same list twice, and an empty list says so in words.
#[test]
fn the_picker_says_how_many_rows_the_search_left() {
   use crate::gui::ui::token_selection::{PickerMode, count_announcement};

   for (previous, mode, count, expected) in [
      (None, PickerMode::Fungible, 2, Some("2 tokens")),
      (Some("2 tokens"), PickerMode::Fungible, 2, None),
      (
         Some("2 tokens"),
         PickerMode::Fungible,
         1,
         Some("1 token"),
      ),
      (
         Some("1 token"),
         PickerMode::Fungible,
         0,
         Some("No tokens found"),
      ),
      (None, PickerMode::Nft, 3, Some("3 NFTs")),
      (
         Some("3 NFTs"),
         PickerMode::Nft,
         0,
         Some("No NFTs found"),
      ),
   ] {
      assert_eq!(
         count_announcement(previous, mode, count).as_deref(),
         expected,
         "{previous:?} then {count} rows of {mode:?}"
      );
   }
}

/// The picker has no announcement channel of its own, so it hands its sentence to the frame the way
/// the Send form and the portfolio do. The list it opened on is said once, and the frame that draws
/// the same list again says nothing.
#[test]
fn the_picker_hands_its_count_to_the_frame_once() {
   let _turn = shared_gui_turn();
   let ctx = Context::default();
   let rows = picker_rows();

   open_picker(&ctx, &rows);

   assert_eq!(
      crate::gui::SHARED_GUI.write(|gui| gui.token_selection.take_notice()).as_deref(),
      Some("2 tokens"),
      "the list the picker opened on is said once"
   );

   render_picker(&ctx, &rows, Vec::new());

   assert_eq!(
      crate::gui::SHARED_GUI.write(|gui| gui.token_selection.take_notice()),
      None,
      "the same list a frame later is not a new answer"
   );

   crate::gui::SHARED_GUI.write(|gui| gui.token_selection.search_query = "usdc".to_string());
   render_picker(&ctx, &rows, Vec::new());

   assert_eq!(
      crate::gui::SHARED_GUI.write(|gui| gui.token_selection.take_notice()).as_deref(),
      Some("1 token"),
      "a narrowed list is a new count"
   );

   crate::gui::SHARED_GUI.write(|gui| gui.token_selection.search_query = "zzz".to_string());

   let nodes = render_picker(&ctx, &rows, Vec::new());

   assert_eq!(
      crate::gui::SHARED_GUI.write(|gui| gui.token_selection.take_notice()).as_deref(),
      Some("No tokens found"),
      "an emptied list says so in words"
   );

   assert!(
      !nodes
         .iter()
         .any(|(_, node)| node.role() == Role::Button && node.label() == Some(USDC_ROW)),
      "the emptied list draws no rows"
   );
}

/// The contact the recipient picker lists in these tests.
const PICKER_CONTACT: &str = "Ada Lovelace";

/// Its public address, and the grouped form the picker's row is named with.
const PICKER_CONTACT_ADDRESS: &str = "0x1111111111111111111111111111111111111111";
const PICKER_CONTACT_SPOKEN: &str = "0x1111 1111 1111 1111 1111 1111 1111 1111 1111 1111";

/// A second contact, so a row that is not the recipient in the field has something to compare to.
const OTHER_CONTACT: &str = "Grace Hopper";
const OTHER_CONTACT_ADDRESS: &str = "0x2222222222222222222222222222222222222222";
const OTHER_CONTACT_SPOKEN: &str = "0x2222 2222 2222 2222 2222 2222 2222 2222 2222 2222";

/// The name a contact row is read with: the contact, then its whole address.
fn contact_row_name(name: &str, spoken_address: &str) -> String {
   format!("{name}, {spoken_address}")
}

/// Seed the contacts the recipient picker lists, in the context the shared `GUI` holds.
///
/// Contacts are part of the wallet state, not of the picker, so a real opening reads them from
/// there. The list is replaced whole so one test's contacts cannot decide what the next one draws.
fn seed_contacts(contacts: &[(&str, &str)]) {
   let zeus = crate::gui::SHARED_GUI.read(|gui| gui.ctx.clone());

   zeus.write_wallet_state(|wallet_state| {
      wallet_state.contacts.clear();

      for (name, address) in contacts {
         wallet_state.contacts.push(Contact::new(
            (*name).to_string(),
            (*address).to_string(),
            String::new(),
         ));
      }
   });
}

/// Draw the recipient picker for one frame, out of the `SHARED_GUI` the frame gives it.
///
/// The wallet list a real opening builds is fetched on a worker that writes back into that same
/// global, so a `GUI` of the test's own would draw its spinner for ever. The picker is drawn out of
/// the shared one instead, in the same exclusive step that seeds it.
fn render_recipient_picker(
   ctx: &Context,
   privacy_mode: bool,
   events: Vec<Event>,
) -> Vec<(NodeId, Node)> {
   frame_nodes(ctx, events, |ui| {
      crate::gui::SHARED_GUI.write(|gui| {
         let mut theme = gui.theme.clone();
         theme.install(ctx);

         let icons = gui.icons.clone();
         let zeus = gui.ctx.clone();
         let picker = &mut gui.recipient_selection;
         let contacts_ui = &mut gui.settings.contacts_ui;

         zeus.write(|zeus| {
            picker.show(
               zeus,
               &theme,
               icons,
               privacy_mode,
               ChainId::Ethereum.id(),
               contacts_ui,
               ui,
            )
         });
      });
   })
}

/// Open the recipient picker on `contacts` and draw it until the search field is on screen.
///
/// The opening starts a wallet-value fetch whose writeback ends the loading state the picker begins
/// in: those first frames draw a spinner and nothing else. The drawn search field is the frame where
/// the list itself is drawn.
fn open_recipient_picker(
   ctx: &Context,
   privacy_mode: bool,
   contacts: &[(&str, &str)],
) -> Vec<(NodeId, Node)> {
   seed_contacts(contacts);

   crate::gui::SHARED_GUI.write(|gui| {
      gui.recipient_selection.reset();
      gui.recipient_selection.open();
   });

   for _ in 0..300 {
      let nodes = render_recipient_picker(ctx, privacy_mode, Vec::new());

      if nodes.iter().any(|(_, node)| is_text_input(node.role())) {
         return nodes;
      }

      std::thread::sleep(std::time::Duration::from_millis(10));
   }

   panic!("the recipient picker never finished loading");
}

/// The address in the picker's field, when a row was picked.
fn picked_recipient_address() -> String {
   crate::gui::SHARED_GUI.read(|gui| gui.recipient_selection.get_recipient().evm_address)
}

/// Whether the recipient picker is still up.
fn recipient_picker_open() -> bool {
   crate::gui::SHARED_GUI.read(|gui| gui.recipient_selection.is_open())
}

/// A contact row used to be a plate with a click sense bolted on: a hit target with no role, no
/// name and no way in from a reader. It is one named button now, and an AccessKit Click on it is
/// the same selection hook a mouse click is.
#[test]
fn a_contact_row_can_be_picked_by_a_reader() {
   let _turn = shared_gui_turn();
   let ctx = Context::default();

   let nodes = open_recipient_picker(
      &ctx,
      false,
      &[(PICKER_CONTACT, PICKER_CONTACT_ADDRESS)],
   );

   let name = contact_row_name(PICKER_CONTACT, PICKER_CONTACT_SPOKEN);

   let (row, node) = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some(name.as_str()))
      .expect("the contact row must be one button named after the contact and its address");

   assert!(
      node.supports_action(Action::Click),
      "the row has to answer a Click request, or a reader cannot activate it"
   );

   assert!(
      node.supports_action(Action::Focus),
      "the row has to be reachable by keyboard as well"
   );

   assert_eq!(
      picked_recipient_address(),
      "",
      "nothing is picked before the row is used"
   );

   render_recipient_picker(
      &ctx,
      false,
      vec![accesskit_action(Action::Click, *row)],
   );

   assert_eq!(
      picked_recipient_address(),
      PICKER_CONTACT_ADDRESS,
      "an AccessKit Click on the row must pick that contact"
   );

   assert!(
      !recipient_picker_open(),
      "picking a recipient closes the picker"
   );
}

/// The keyboard path: the row is focusable, a reader's Focus request lands on it, and Enter on it
/// picks the contact. The focus has to survive the frame that draws the row — the plate registers
/// itself with an empty sense first, which withdraws it — or Enter would never reach it.
#[test]
fn a_contact_row_takes_focus_and_enter_picks_it() {
   let _turn = shared_gui_turn();
   let ctx = Context::default();

   let nodes = open_recipient_picker(
      &ctx,
      false,
      &[(PICKER_CONTACT, PICKER_CONTACT_ADDRESS)],
   );

   let name = contact_row_name(PICKER_CONTACT, PICKER_CONTACT_SPOKEN);

   let row = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some(name.as_str()))
      .map(|(id, _)| *id)
      .expect("the contact row must be in the tree");

   render_recipient_picker(
      &ctx,
      false,
      vec![accesskit_action(Action::Focus, row)],
   );

   let focused = ctx.memory(|memory| memory.focused());

   assert_eq!(
      focused.map(|id| id.accesskit_id()),
      Some(row),
      "the row must take the focus a reader asked for, and hold it after the frame drew it"
   );

   render_recipient_picker(&ctx, false, vec![key_press(Key::Enter)]);

   assert_eq!(
      picked_recipient_address(),
      PICKER_CONTACT_ADDRESS,
      "Enter on the focused row must pick that contact"
   );
}

/// The row says the contact and the whole address itself, so the labels inside it are hidden: a
/// reader hears the row once, instead of the row and then "Ada Lovelace" and a shortened address
/// after it.
#[test]
fn the_contact_row_is_read_once() {
   let _turn = shared_gui_turn();
   let ctx = Context::default();

   let nodes = open_recipient_picker(
      &ctx,
      false,
      &[(PICKER_CONTACT, PICKER_CONTACT_ADDRESS)],
   );

   let spoken: Vec<&str> = nodes
      .iter()
      .filter(|(_, node)| !node.is_hidden())
      .filter_map(|(_, node)| node.label().or_else(|| node.value()))
      .collect();

   assert!(
      !spoken.contains(&PICKER_CONTACT),
      "the row's own name label must not also be a label of its own: {spoken:?}"
   );

   assert!(
      !spoken.contains(&"0x1111...111111"),
      "the drawn address is inside the copy button, so it is not a node of its own: {spoken:?}"
   );

   let name = contact_row_name(PICKER_CONTACT, PICKER_CONTACT_SPOKEN);

   assert!(
      spoken.contains(&name.as_str()),
      "the row's one node carries the words those labels drew: {spoken:?}"
   );
}

/// Neither row keeps a selection — picking one sets the recipient and closes the window — but the
/// row for the address already in the field says so, so a reader can tell where the form stands.
#[test]
fn the_contact_row_says_which_recipient_is_in_the_field() {
   let _turn = shared_gui_turn();
   let ctx = Context::default();

   let contacts = [
      (PICKER_CONTACT, PICKER_CONTACT_ADDRESS),
      (OTHER_CONTACT, OTHER_CONTACT_ADDRESS),
   ];

   open_recipient_picker(&ctx, false, &contacts);

   crate::gui::SHARED_GUI.write(|gui| {
      gui.recipient_selection.recipient.evm_address = PICKER_CONTACT_ADDRESS.to_string();
   });

   let nodes = render_recipient_picker(&ctx, false, Vec::new());

   let toggled = |name: &str| {
      nodes
         .iter()
         .find(|(_, node)| node.role() == Role::Button && node.label() == Some(name))
         .map(|(_, node)| node.toggled())
         .expect("the row must be a button in the tree")
   };

   assert_eq!(
      toggled(&contact_row_name(
         PICKER_CONTACT,
         PICKER_CONTACT_SPOKEN
      )),
      Some(Toggled::True),
      "the row whose address is in the field is the selected one"
   );

   assert_eq!(
      toggled(&contact_row_name(
         OTHER_CONTACT,
         OTHER_CONTACT_SPOKEN
      )),
      Some(Toggled::False),
      "every other row says it is not the recipient"
   );
}

/// The copy button is not the row: it copies the address and leaves the window open, so it says
/// what it copies and names the contact where the row's own name already says the address.
#[test]
fn the_copy_button_names_the_address_it_copies() {
   let _turn = shared_gui_turn();
   let ctx = Context::default();

   let nodes = open_recipient_picker(
      &ctx,
      false,
      &[(PICKER_CONTACT, PICKER_CONTACT_ADDRESS)],
   );

   let copy_name = format!("Copy address of {PICKER_CONTACT}");

   let (copy, node) = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some(copy_name.as_str()))
      .expect("the row's address button must say what it copies");

   assert_eq!(
      node.description(),
      Some(PICKER_CONTACT_SPOKEN),
      "the whole address the button hands over is its description"
   );

   assert!(
      node.supports_action(Action::Click) && node.supports_action(Action::Focus),
      "the copy button is a control of its own, so it stays clickable and focusable"
   );

   assert!(
      !node.is_hidden(),
      "the row's name covers its labels, not the button that does something else"
   );

   render_recipient_picker(
      &ctx,
      false,
      vec![accesskit_action(Action::Click, *copy)],
   );

   assert_eq!(
      picked_recipient_address(),
      "",
      "copying the address must not pick the row"
   );

   assert!(
      recipient_picker_open(),
      "copying the address must not close the picker"
   );
}

/// The search box painted only a hint, so a reader landing on it had no word for what it searches.
/// It is named for what it narrows now, its hint is the description, and it — not the first
/// control the layout draws — is where the focus lands when the window opens.
#[test]
fn the_recipient_search_field_is_named_and_takes_the_first_focus() {
   let _turn = shared_gui_turn();
   let ctx = Context::default();

   let nodes = open_recipient_picker(
      &ctx,
      false,
      &[(PICKER_CONTACT, PICKER_CONTACT_ADDRESS)],
   );

   let (search_id, search) = nodes
      .iter()
      .find(|(_, node)| is_text_input(node.role()))
      .map(|(id, node)| (*id, node))
      .expect("the picker must draw its search field");

   assert_eq!(
      node_name(&nodes, search).as_deref(),
      Some("Search contacts or addresses"),
      "the field is named for what it searches"
   );

   assert_eq!(
      search.description(),
      Some("Search contacts, ENS or enter an address"),
      "the hint the field paints is its description"
   );

   assert_eq!(
      ctx.memory(|memory| memory.focused()).map(|id| id.accesskit_id()),
      Some(search_id),
      "the field holds the focus the window opened with, not the first control drawn"
   );

   let nodes = render_recipient_picker(&ctx, true, Vec::new());

   let search = nodes
      .iter()
      .find(|(_, node)| is_text_input(node.role()))
      .map(|(_, node)| node)
      .expect("the picker must still draw its search field");

   assert_eq!(
      node_name(&nodes, search).as_deref(),
      Some("Search contacts or a 0zk address"),
      "privacy mode names the field after what it searches there"
   );
}

/// Write the frame's `TreeUpdate` to `logs/accessibility-tree.txt` for review: one
/// line per node, `role | name | value | description | actions`, which is what a reviewer reads
/// and what a later slice diffs.
///
/// Dev builds only, and only when asked:
/// `ZEUS_ACCESSIBILITY_DUMP=1 cargo test --features dev --bin zeus accessibility`.
///
/// The dump is taken from a headless frame rather than the running app on purpose: eframe builds
/// `platform_output.accesskit_update` in `Context::end_pass`, after `App::ui` has returned, and
/// owns the `FullOutput` it lands in, so the app itself can never see the frame's tree.
#[cfg(feature = "dev")]
#[test]
fn dump_the_unlock_screen_tree() {
   if std::env::var("ZEUS_ACCESSIBILITY_DUMP").as_deref() != Ok("1") {
      return;
   }

   let ctx = Context::default();
   let (locked, mut screen, mut theme) = unlock_screen();
   theme.install(&ctx);

   let nodes = render_unlock_screen(&ctx, &locked, &mut screen, &theme);
   let tree: String = nodes.iter().map(|node| format!("{}\n", node_line(node))).collect();

   if let Err(e) = std::fs::create_dir_all("logs") {
      panic!("could not create logs/: {e}");
   }
   if let Err(e) = std::fs::write("logs/accessibility-tree.txt", tree) {
      panic!("could not write logs/accessibility-tree.txt: {e}");
   }
}

/// One dump line for a node: `role | name | value | description | actions`.
#[cfg(feature = "dev")]
fn node_line(node: &Node) -> String {
   use egui::accesskit::Action;

   let mut actions = String::new();

   for action in [
      Action::Click,
      Action::Focus,
      Action::Expand,
      Action::Collapse,
      Action::SetValue,
      Action::ScrollIntoView,
   ] {
      if !node.supports_action(action) {
         continue;
      }
      if !actions.is_empty() {
         actions.push(',');
      }
      actions.push_str(&format!("{action:?}"));
   }

   format!(
      "{:?} | {} | {} | {} | {}",
      node.role(),
      node.label().unwrap_or_default(),
      node.value().unwrap_or_default(),
      node.description().unwrap_or_default(),
      actions,
   )
}

/// The pieces a prompt window's frame gives it: the installed theme, the icon store its rows
/// draw with, and the appearance settings its hold is read from.
fn prompt_harness() -> (
   Context,
   ZeusCtx,
   Theme,
   Arc<Icons>,
   AppearanceSettings,
) {
   let ctx = Context::default();
   let mut theme = Theme::new(ThemeKind::TokyoNight);
   theme.install(&ctx);
   egui_extras::install_image_loaders(&ctx);

   let icons = Arc::new(Icons::new(&ctx).expect("icons"));
   let handle = ZeusCtx::new(crate::runtime::test_handle());

   (
      ctx,
      handle,
      theme,
      icons,
      AppearanceSettings::new(),
   )
}

/// Draw the confirmation window into a reader's tree.
///
/// A local window stands in for the prompt — nothing claims it and no task prepares it — and the
/// frame draws it out of a local context, which is exactly what `TxConfirmationWindow::show`
/// does in the app.
///
/// Three passes: a shrink-wrapping modal spends two invisible sizing passes before the first
/// frame it actually paints, and egui exposes no node for a widget it considers invisible. The
/// last pass is the one a reader is handed — and the first one the window may take focus on.
fn render_confirmation(
   ctx: &Context,
   handle: &ZeusCtx,
   window: &mut TxConfirmationWindow,
   theme: &Theme,
   icons: &Arc<Icons>,
   appearance: &AppearanceSettings,
   events: Vec<Event>,
) -> Vec<(NodeId, Node)> {
   for pass in 0..3 {
      let events = if pass == 2 {
         events.clone()
      } else {
         Vec::new()
      };

      let nodes = frame_nodes(ctx, events, |ui| {
         handle.write(|c| window.show(c, theme, icons.clone(), appearance, ui));
      });

      if pass == 2 {
         return nodes;
      }
   }

   unreachable!("the third pass returns the tree")
}

/// The node ids in reading order: a depth-first walk from the root, children in the order the
/// accesskit node says they are read.
///
/// The `TreeUpdate` itself is a map and comes out in no particular order, so a tree-order
/// assertion has to walk the children rather than trust the vector.
fn tree_order(nodes: &[(NodeId, Node)]) -> Vec<NodeId> {
   let root = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Window)
      .map(|(id, _)| *id)
      .expect("the tree starts from the root window node");

   let mut order = Vec::new();
   let mut stack = vec![root];

   while let Some(id) = stack.pop() {
      order.push(id);

      if let Some((_, node)) = nodes.iter().find(|(node_id, _)| *node_id == id) {
         for child in node.children().iter().rev() {
            stack.push(*child);
         }
      }
   }

   order
}

/// Where a node whose name starts with `prefix` sits in the tree's reading order.
fn node_position(nodes: &[(NodeId, Node)], prefix: &str) -> usize {
   tree_order(nodes)
      .into_iter()
      .position(|id| {
         nodes
            .iter()
            .find(|(node_id, _)| *node_id == id)
            .and_then(|(_, node)| node.label().or_else(|| node.value()))
            .is_some_and(|text| text.starts_with(prefix))
      })
      .unwrap_or_else(|| panic!("no node named {prefix:?} in the tree"))
}

/// Approvals are the only state change in the window that can cost money after the transaction,
/// so they are drawn — and therefore heard — before the balances, and the fee comes after both.
#[test]
fn the_confirmation_reads_approvals_before_balances_and_the_fee_last() {
   let (ctx, handle, theme, icons, appearance) = prompt_harness();
   let mut window = TxConfirmationWindow::for_test(
      &handle,
      TransactionAnalysis::dummy_with_diffs(),
      "",
      false,
      None,
   );

   let nodes = render_confirmation(
      &ctx,
      &handle,
      &mut window,
      &theme,
      &icons,
      &appearance,
      Vec::new(),
   );

   let approvals = node_position(&nodes, "Approval changes,");
   let balances = node_position(&nodes, "Balance changes,");
   let fee = node_position(&nodes, "Network fee,");

   assert!(
      approvals < balances,
      "the approvals must be heard before the balances: {approvals} vs {balances}"
   );
   assert!(
      balances < fee,
      "the fee must come after the state changes: {balances} vs {fee}"
   );
}

/// A reader hears the action first: the window opens with focus on the action heading, not on
/// the Close button the modal would otherwise leave it on.
#[test]
fn the_confirmation_opens_on_the_action_heading() {
   let (ctx, handle, theme, icons, appearance) = prompt_harness();
   let analysis = TransactionAnalysis::dummy_transfer();
   let action = analysis
      .infer_main_event(handle.clone(), handle.chain().id())
      .name()
      .to_string();

   let mut window = TxConfirmationWindow::for_test(&handle, analysis, "", false, None);
   let nodes = render_confirmation(
      &ctx,
      &handle,
      &mut window,
      &theme,
      &icons,
      &appearance,
      Vec::new(),
   );

   let focused = ctx
      .memory(|memory| memory.focused())
      .expect("the opening frame must leave focus inside the window");
   let node = nodes
      .iter()
      .find(|(node_id, _)| *node_id == focused.accesskit_id())
      .map(|(_, node)| node)
      .expect("the focused node must be in the window's tree");

   assert_eq!(
      node.role(),
      Role::Heading,
      "the first focus is the action heading, not a button: {:?}",
      node.role()
   );
   assert_eq!(
      node.label(),
      Some(action.as_str()),
      "the focused heading must name the action"
   );
   assert_ne!(
      node.label(),
      Some("Close"),
      "focus must not open on Close"
   );

   assert!(
      nodes.iter().any(|(_, node)| node.label() == Some("Requested by Zeus")),
      "an in-app transaction must say Zeus asked for it"
   );
}

/// A wallet that cannot pay hears why Confirm is disabled above the button, and the disabled
/// button itself carries the same sentence: the reason never lives only in a label inside it.
#[test]
fn a_short_balance_says_why_confirm_is_disabled() {
   let (ctx, handle, theme, icons, appearance) = prompt_harness();

   let mut analysis = TransactionAnalysis::dummy_transfer();
   analysis.value = U256::from(1_000_000_000_000_000_000u64);

   let mut window = TxConfirmationWindow::for_test(&handle, analysis, "", false, None);
   let nodes = render_confirmation(
      &ctx,
      &handle,
      &mut window,
      &theme,
      &icons,
      &appearance,
      Vec::new(),
   );

   let confirm = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some("Confirm"))
      .map(|(_, node)| node)
      .expect("the Confirm button must be in the tree");

   let description = confirm.description().unwrap_or_default();
   let sentence = "Insufficient ETH. This wallet cannot cover the amount plus the network fee.";

   assert_eq!(
      confirm.label(),
      Some("Confirm"),
      "the reason must not replace the button's name"
   );
   assert!(
      description.contains(sentence),
      "the disabled button must carry the reason: {description:?}"
   );
   assert!(
      nodes.iter().any(|(_, node)| {
         node.label().or_else(|| node.value()).is_some_and(|text| text == sentence)
      }),
      "the reason must also be drawn as a line above the buttons"
   );
}

/// Escape is a rejection: the dismissal sticks, the awaiting flow gets its answer, and the
/// sentence the flow's caller reads says what happened.
#[test]
fn escape_rejects_the_confirmation_and_closes_it() {
   let (ctx, handle, theme, icons, appearance) = prompt_harness();
   let mut window = TxConfirmationWindow::for_test(
      &handle,
      TransactionAnalysis::dummy_transfer(),
      "",
      false,
      None,
   );

   render_confirmation(
      &ctx,
      &handle,
      &mut window,
      &theme,
      &icons,
      &appearance,
      Vec::new(),
   );
   render_confirmation(
      &ctx,
      &handle,
      &mut window,
      &theme,
      &icons,
      &appearance,
      vec![key_press(Key::Escape)],
   );

   assert!(!window.is_open(), "Escape must close the window");
   assert_eq!(
      window.take_confirmed_or_rejected(),
      Some(false),
      "Escape must answer the awaiting flow with a rejection"
   );
   assert_eq!(
      window.take_notice().as_deref(),
      Some("Transaction rejected."),
      "the rejection is one of the window's sentences"
   );
}

/// The countdown lives in the button's description, never its name: a reader hears "Confirm"
/// throughout, the wait while it runs, and one sentence when it ends.
#[test]
fn the_confirm_countdown_keeps_its_name_and_says_when_it_opens() {
   let (ctx, handle, theme, icons, appearance) = prompt_harness();

   let mut waiting = TxConfirmationWindow::for_test(
      &handle,
      TransactionAnalysis::dummy_transfer(),
      "",
      false,
      Some(Instant::now()),
   );
   let nodes = render_confirmation(
      &ctx,
      &handle,
      &mut waiting,
      &theme,
      &icons,
      &appearance,
      Vec::new(),
   );
   let confirm = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some("Confirm"))
      .map(|(_, node)| node)
      .expect("the Confirm button must be in the tree");

   assert_eq!(
      confirm.label(),
      Some("Confirm"),
      "the name must not become the countdown"
   );
   assert!(
      confirm.description().unwrap_or_default().starts_with("Available in"),
      "the wait belongs in the description: {:?}",
      confirm.description()
   );
   assert_eq!(
      waiting.take_notice(),
      None,
      "nothing is said while the wait is still running"
   );

   let mut ready = TxConfirmationWindow::for_test(
      &handle,
      TransactionAnalysis::dummy_transfer(),
      "",
      false,
      Some(Instant::now() - Duration::from_secs(3)),
   );
   let nodes = render_confirmation(
      &ctx,
      &handle,
      &mut ready,
      &theme,
      &icons,
      &appearance,
      Vec::new(),
   );
   let confirm = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some("Confirm"))
      .map(|(_, node)| node)
      .expect("the Confirm button must still be named Confirm once it opens");

   assert_eq!(confirm.label(), Some("Confirm"));

   assert_eq!(
      ready.take_notice().as_deref(),
      Some("Confirm is now available."),
      "the end of the wait is said exactly once"
   );

   render_confirmation(
      &ctx,
      &handle,
      &mut ready,
      &theme,
      &icons,
      &appearance,
      Vec::new(),
   );
   assert_eq!(
      ready.take_notice(),
      None,
      "the same wait must not be announced again on the next frame"
   );
}

/// An unlimited allowance is not only red: the spender and the grant are an Alert node before the
/// main event, so the sentence survives a reader that cannot see the heading's colour.
#[test]
fn an_unlimited_approval_is_spoken_not_only_coloured() {
   let (ctx, handle, theme, icons, appearance) = prompt_harness();
   let mut window = TxConfirmationWindow::for_test(
      &handle,
      TransactionAnalysis::dummy_permit(),
      "",
      false,
      None,
   );

   let nodes = render_confirmation(
      &ctx,
      &handle,
      &mut window,
      &theme,
      &icons,
      &appearance,
      Vec::new(),
   );

   let alert = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Alert)
      .map(|(_, node)| node)
      .expect("an unlimited approval must warn with an Alert node");

   let spoken = alert.label().or_else(|| alert.description()).unwrap_or_default();

   assert!(
      spoken.contains("Unlimited"),
      "the warning must say the allowance is unlimited: {spoken:?}"
   );
}

/// Draw the sign-message window into a reader's tree.
///
/// A local window stands in for the prompt — nothing claims it and no flow prepares it — and the
/// frame draws it out of a local context, which is exactly what `SignMsgWindow::show` does in the
/// app.
///
/// Three passes, for the same reason as the confirmation: a shrink-wrapping modal spends two
/// invisible sizing passes before the first frame it actually paints, and egui exposes no node
/// for a widget it considers invisible. The last pass is the one a reader is handed — and the
/// first one the window may take focus on.
fn render_sign(
   ctx: &Context,
   handle: &ZeusCtx,
   window: &mut SignMsgWindow,
   theme: &Theme,
   icons: &Arc<Icons>,
   appearance: &AppearanceSettings,
   events: Vec<Event>,
) -> Vec<(NodeId, Node)> {
   for pass in 0..3 {
      let events = if pass == 2 {
         events.clone()
      } else {
         Vec::new()
      };

      let nodes = frame_nodes(ctx, events, |ui| {
         handle.write(|c| window.show(c, theme, icons.clone(), appearance, ui));
      });

      if pass == 2 {
         return nodes;
      }
   }

   unreachable!("the third pass returns the tree")
}

/// An unlimited permit, as the prompt receives one: the crate's own fixture carrying the amount a
/// permit that never runs out grants.
fn unlimited_permit() -> SignMsgType {
   let mut details = Permit2612Details::dummy();
   details.amount = NumericValue::format_wei(U256::MAX, details.token.decimals);

   SignMsgType::Permit2612(details)
}

/// The window is about one signature, so it opens on a heading that names what is being signed —
/// not on the Cancel button the modal would otherwise leave the focus on.
#[test]
fn the_sign_window_opens_on_the_message_heading() {
   let (ctx, handle, theme, icons, appearance) = prompt_harness();
   let msg = SignMsgType::dummy_permit2612();
   let title = msg.title().to_string();
   let mut window = SignMsgWindow::for_test(
      "app.example.org",
      ChainId::default(),
      Address::repeat_byte(0x11),
      msg,
      None,
   );

   let nodes = render_sign(
      &ctx,
      &handle,
      &mut window,
      &theme,
      &icons,
      &appearance,
      Vec::new(),
   );

   let focused = ctx
      .memory(|memory| memory.focused())
      .expect("the opening frame must leave focus inside the window");
   let node = nodes
      .iter()
      .find(|(node_id, _)| *node_id == focused.accesskit_id())
      .map(|(_, node)| node)
      .expect("the focused node must be in the window's tree");

   assert_eq!(
      node.role(),
      Role::Heading,
      "the first focus is the message heading, not a field or a button: {:?}",
      node.role()
   );
   assert_eq!(
      node.label(),
      Some(title.as_str()),
      "the heading must name what is being signed"
   );

   // The action is read first: the origin, the message body and the buttons all follow it.
   let order = tree_order(&nodes);
   let heading_at = order
      .iter()
      .position(|id| *id == focused.accesskit_id())
      .expect("the heading is in the reading order");

   assert!(
      heading_at < node_position(&nodes, "Requested by"),
      "the action must come before who asked for it"
   );
   assert!(
      heading_at < node_position(&nodes, "Message"),
      "the action must come before the message body"
   );
}

/// Who asked is named, not just drawn: the dapp's line says "Requested by <dapp>", and a prompt
/// with no origin line at all still answers the question with Zeus.
#[test]
fn the_sign_window_says_who_requested_it() {
   let (ctx, handle, theme, icons, appearance) = prompt_harness();

   let mut dapp = SignMsgWindow::for_test(
      "app.example.org",
      ChainId::default(),
      Address::repeat_byte(0x11),
      SignMsgType::dummy_permit2612(),
      None,
   );
   let nodes = render_sign(
      &ctx,
      &handle,
      &mut dapp,
      &theme,
      &icons,
      &appearance,
      Vec::new(),
   );

   assert!(
      nodes
         .iter()
         .any(|(_, node)| node.label() == Some("Requested by app.example.org")),
      "the dapp's line must say who asked, not just show its name"
   );

   let mut in_app = SignMsgWindow::for_test(
      "",
      ChainId::default(),
      Address::repeat_byte(0x11),
      SignMsgType::dummy_permit2612(),
      None,
   );
   let nodes = render_sign(
      &ctx,
      &handle,
      &mut in_app,
      &theme,
      &icons,
      &appearance,
      Vec::new(),
   );

   assert!(
      nodes.iter().any(|(_, node)| node.label() == Some("Requested by Zeus")),
      "a request with no origin line must still say Zeus asked for it"
   );
}

/// The raw message is a viewer, not an editor: AT is given one named, read-only field, and the
/// drawn "Message" label does not say the word a second time.
#[test]
fn the_message_body_is_a_named_read_only_field() {
   let (ctx, handle, theme, icons, appearance) = prompt_harness();
   let mut window = SignMsgWindow::for_test(
      "app.example.org",
      ChainId::default(),
      Address::repeat_byte(0x11),
      SignMsgType::dummy_permit2612(),
      None,
   );

   let nodes = render_sign(
      &ctx,
      &handle,
      &mut window,
      &theme,
      &icons,
      &appearance,
      Vec::new(),
   );

   let field = nodes
      .iter()
      .find(|(_, node)| is_text_input(node.role()) && node.label() == Some("Message"))
      .map(|(_, node)| node)
      .expect("the message body must be a text field named Message");

   assert!(
      field.is_read_only(),
      "the field is a viewer: it must say it cannot be typed into"
   );
   assert_eq!(
      field.description(),
      Some("The raw message Zeus will sign."),
      "the field must say what it holds"
   );

   let named_message = nodes.iter().filter(|(_, node)| node.label() == Some("Message")).count();
   assert_eq!(
      named_message, 1,
      "exactly one node may be named Message: the field"
   );

   // The drawn label the field replaced is hidden, so the word is not said twice. Its text-run
   // children stay under that hidden node, which is what takes them out of the tree a reader is
   // handed too.
   let drawn: Vec<&Node> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Label && node.value() == Some("Message"))
      .map(|(_, node)| node)
      .collect();

   assert_eq!(
      drawn.len(),
      1,
      "the drawn Message label is the one node the field replaced"
   );
   assert!(
      drawn[0].is_hidden(),
      "the drawn Message label must be hidden so a reader does not hear the word twice"
   );
}

/// The countdown lives in the button's description, never its name: a reader hears "Sign"
/// throughout, the wait while it runs, and one sentence when it ends.
#[test]
fn the_sign_countdown_keeps_its_name_and_says_when_it_opens() {
   let (ctx, handle, theme, icons, appearance) = prompt_harness();

   let mut waiting = SignMsgWindow::for_test(
      "app.example.org",
      ChainId::default(),
      Address::repeat_byte(0x11),
      SignMsgType::dummy_permit2612(),
      Some(Instant::now()),
   );
   let nodes = render_sign(
      &ctx,
      &handle,
      &mut waiting,
      &theme,
      &icons,
      &appearance,
      Vec::new(),
   );
   let sign = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some("Sign"))
      .map(|(_, node)| node)
      .expect("the Sign button must be in the tree");

   assert_eq!(
      sign.label(),
      Some("Sign"),
      "the name must not become the countdown"
   );
   assert!(
      sign.description().unwrap_or_default().starts_with("Available in"),
      "the wait belongs in the description: {:?}",
      sign.description()
   );
   assert_eq!(
      waiting.take_notice(),
      None,
      "nothing is said while the wait is still running"
   );

   let mut ready = SignMsgWindow::for_test(
      "app.example.org",
      ChainId::default(),
      Address::repeat_byte(0x11),
      SignMsgType::dummy_permit2612(),
      Some(Instant::now() - Duration::from_secs(3)),
   );
   let nodes = render_sign(
      &ctx,
      &handle,
      &mut ready,
      &theme,
      &icons,
      &appearance,
      Vec::new(),
   );
   let sign = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some("Sign"))
      .map(|(_, node)| node)
      .expect("the Sign button must still be named Sign once it opens");

   assert_eq!(sign.label(), Some("Sign"));
   assert!(
      !sign.description().unwrap_or_default().starts_with("Available in"),
      "a wait that is over must not still be described: {:?}",
      sign.description()
   );

   assert_eq!(
      ready.take_notice().as_deref(),
      Some("Sign is now available."),
      "the end of the wait is said exactly once"
   );

   render_sign(
      &ctx,
      &handle,
      &mut ready,
      &theme,
      &icons,
      &appearance,
      Vec::new(),
   );
   assert_eq!(
      ready.take_notice(),
      None,
      "the same wait must not be announced again on the next frame"
   );
}

/// Escape is the window's way out: it cancels the signature, closes the prompt, and says what
/// happened to the flow waiting for an answer.
#[test]
fn escape_cancels_the_signature_and_closes_it() {
   let (ctx, handle, theme, icons, appearance) = prompt_harness();
   let mut window = SignMsgWindow::for_test(
      "app.example.org",
      ChainId::default(),
      Address::repeat_byte(0x11),
      SignMsgType::dummy_permit2612(),
      None,
   );

   render_sign(
      &ctx,
      &handle,
      &mut window,
      &theme,
      &icons,
      &appearance,
      Vec::new(),
   );
   render_sign(
      &ctx,
      &handle,
      &mut window,
      &theme,
      &icons,
      &appearance,
      vec![key_press(Key::Escape)],
   );

   assert!(!window.is_open(), "Escape must close the window");
   assert_eq!(
      window.take_signed(),
      Some(false),
      "Escape must answer the awaiting flow with a cancel"
   );
   assert_eq!(
      window.take_notice().as_deref(),
      Some("Message not signed."),
      "the cancel is one of the window's sentences"
   );
}

/// An unlimited allowance is not only red: the spender and the grant are an Alert node before the
/// permit rows, so the sentence survives a reader that cannot see the title's colour.
#[test]
fn an_unlimited_permit_is_spoken_not_only_coloured() {
   let (ctx, handle, theme, icons, appearance) = prompt_harness();
   let mut window = SignMsgWindow::for_test(
      "app.example.org",
      ChainId::default(),
      Address::repeat_byte(0x11),
      unlimited_permit(),
      None,
   );

   let nodes = render_sign(
      &ctx,
      &handle,
      &mut window,
      &theme,
      &icons,
      &appearance,
      Vec::new(),
   );

   let alert = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Alert)
      .map(|(_, node)| node)
      .expect("an unlimited allowance must warn with an Alert node");

   let spoken = alert.label().or_else(|| alert.description()).unwrap_or_default();

   assert!(
      spoken.contains("Unlimited"),
      "the warning must say the allowance is unlimited: {spoken:?}"
   );
   assert!(
      spoken.contains("take all of this token"),
      "the warning must say what the signature gives away: {spoken:?}"
   );
   assert!(
      node_position(&nodes, "Unlimited allowance.") < node_position(&nodes, "Approve Token"),
      "the warning must be read before the permit rows it warns about"
   );
}

/// Draw a modal the way the window hosting it draws it, into a reader's tree.
///
/// Three passes, for the same reason as the confirmation and the sign window: a
/// shrink-wrapping modal spends two invisible sizing passes before the first frame it actually
/// paints, and egui exposes no node for a widget it considers invisible. The events go to the
/// last pass only, so a key is not pressed three times.
fn render_modal(
   ctx: &Context,
   events: Vec<Event>,
   mut add: impl FnMut(&mut Ui),
) -> Vec<(NodeId, Node)> {
   let mut nodes = Vec::new();

   for pass in 0..3 {
      let events = if pass == 2 {
         events.clone()
      } else {
         Vec::new()
      };

      nodes = frame_nodes(ctx, events, &mut add);
   }

   nodes
}

/// The text field a reader reaches under `name`, however the tree spells that name: the field's
/// own label, or the value of the label it points at.
fn text_field<'a>(nodes: &'a [(NodeId, Node)], name: &str) -> Option<&'a Node> {
   nodes
      .iter()
      .map(|(_, node)| node)
      .find(|node| is_text_input(node.role()) && node_name(nodes, node).as_deref() == Some(name))
}

/// The Events and State buttons paint their count in a badge that reserves no layout space, so
/// the number is in neither the label nor anything else a reader can reach: each button's node
/// says it, while the two buttons without a count keep the word they draw.
#[test]
fn the_inspector_buttons_name_the_counts_they_paint() {
   let (ctx, _handle, theme, _icons, _appearance) = prompt_harness();
   let analysis = TransactionAnalysis::dummy_with_diffs();

   let events = analysis.decoded_events.len();
   let changes = analysis.balance_diff.len() + analysis.approval_diff.len();

   let nodes = frame(&ctx, |ui| {
      show_analysis_buttons(&analysis, &theme, 600.0, true, ui);
   });

   let names: Vec<&str> = nodes
      .iter()
      .filter(|node| node.role() == Role::Button)
      .filter_map(|node| node.label())
      .collect();

   assert!(
      names.contains(&format!("Events, {events}").as_str()),
      "the Events button must say how many events the transaction carries: {names:?}"
   );
   assert!(
      names.contains(&format!("State, {changes}").as_str()),
      "the State button must say how many changes it holds: {names:?}"
   );
   assert!(
      names.contains(&"Calldata") && names.contains(&"Tx Options"),
      "the two buttons without a count keep the words they draw: {names:?}"
   );
   assert!(
      !names.contains(&"Events") && !names.contains(&"State"),
      "a count left out of the name is a count a reader never hears: {names:?}"
   );
}

/// Approvals are the half of a state report that can cost money after the transaction, so the
/// modal reads them before the balances — the order the confirmation window keeps — and each
/// section says how many changes it holds.
#[test]
fn the_state_changes_modal_reads_approvals_before_balances() {
   let (ctx, handle, theme, icons, _appearance) = prompt_harness();
   let analysis = TransactionAnalysis::dummy_with_diffs();
   let chain = ChainId::from(analysis.chain);
   let mut open = true;

   let nodes = render_modal(&ctx, Vec::new(), |ui| {
      handle.write(|zeus| {
         show_tx_diffs_modal(
            &mut open,
            &theme,
            zeus,
            chain,
            icons.clone(),
            &analysis.balance_diff,
            &analysis.approval_diff,
            ui,
         );
      });
   });

   // Each section is named with the count it holds, so a name that lost its count is not
   // found here at all: this is the order and the words together.
   let approvals = node_position(
      &nodes,
      &format!(
         "Approval changes, {}",
         analysis.approval_diff.len()
      ),
   );
   let balances = node_position(
      &nodes,
      &format!("Balance changes, {}", analysis.balance_diff.len()),
   );

   assert!(
      approvals < balances,
      "the approval section must be read before the balance section"
   );
}

/// The fee and gas fields used to be nameless text inputs, and a tier row's gwei and cost are
/// painted beside its label rather than into it: each field is named, and each row's name says
/// the three things its line draws.
#[test]
fn the_transaction_options_name_their_fields_and_their_tiers() {
   let (ctx, handle, theme, _icons, _appearance) = prompt_harness();
   // The custom field slides open; without a still animation the clip it is drawn in has no
   // height yet on the frame a reader is handed.
   ctx.all_styles_mut(|style| style.animation_time = 0.0);

   let tiers = FeeTiers {
      normal: U256::from(12_500_000_000u64),
      fast: U256::from(15_000_000_000u64),
      very_fast: U256::from(18_000_000_000u64),
   };
   let costs = vec![NumericValue::currency_price(0.42); 3];

   let nodes = {
      let mut tier = PriorityTier::Normal;
      let mut custom = true;
      let mut custom_value = "12.5".to_string();
      let mut gas_limit = "45000".to_string();
      let mut open = true;
      let mut fee = FeeSelection {
         tiers: Some(tiers),
         tier: &mut tier,
         custom: &mut custom,
         custom_value: &mut custom_value,
         costs_usd: &costs,
      };

      render_modal(&ctx, Vec::new(), |ui| {
         handle.write(|_zeus| {
            show_tx_options_modal(
               &mut open,
               &theme,
               ChainId::Ethereum,
               &mut fee,
               &mut gas_limit,
               45_000,
               false,
               false,
               ui,
            );
         });
      })
   };

   assert_eq!(
      text_field(&nodes, "Custom priority fee in gwei").map(|node| node.role()),
      Some(Role::TextInput),
      "the custom fee is a text input and must be named"
   );
   assert!(
      text_field(&nodes, "Gas limit").is_some(),
      "the gas limit is a text input and must be named"
   );

   let rows: Vec<&str> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Button)
      .filter_map(|(_, node)| node.label())
      .collect();

   for tier in PriorityTier::ALL {
      let row = rows.iter().find(|row| row.starts_with(tier.label())).unwrap_or_else(|| {
         panic!(
            "the {} row must be a button in the tree",
            tier.label()
         )
      });

      assert!(
         row.contains(" gwei"),
         "a tier row must say the gwei it sets: {row:?}"
      );
   }

   let normal = NumericValue::format_to_gwei(tiers.normal).formatted;
   let row = rows
      .iter()
      .find(|row| row.starts_with("Normal"))
      .expect("the Normal row must be a button in the tree");

   assert!(
      row.contains(&format!("{normal} gwei")),
      "the Normal row must name the tip it sets: {row:?}"
   );
   assert!(
      row.contains("about $"),
      "a row that carries a cost must say what it costs: {row:?}"
   );

   // A chain with no recommendation draws the same two fields on one line, and both need the
   // same names.
   let nodes = {
      let mut tier = PriorityTier::Normal;
      let mut custom = false;
      let mut custom_value = String::new();
      let mut gas_limit = "45000".to_string();
      let mut open = true;
      let mut fee = FeeSelection {
         tiers: None,
         tier: &mut tier,
         custom: &mut custom,
         custom_value: &mut custom_value,
         costs_usd: &costs,
      };

      render_modal(&ctx, Vec::new(), |ui| {
         handle.write(|_zeus| {
            show_tx_options_modal(
               &mut open,
               &theme,
               ChainId::Ethereum,
               &mut fee,
               &mut gas_limit,
               45_000,
               false,
               false,
               ui,
            );
         });
      })
   };

   assert!(
      text_field(&nodes, "Priority fee in gwei").is_some(),
      "the free-text priority fee must be named"
   );
   assert!(
      text_field(&nodes, "Gas limit").is_some(),
      "the gas limit must be named in the two-column layout as well"
   );
}

/// The raw calldata was an editable field whose edits are thrown away, and nothing named it: it
/// is a read-only multiline input called Raw calldata, and the drawn word it replaces is hidden
/// so it is not heard on its own.
#[test]
fn the_raw_calldata_is_one_named_read_only_view() {
   let (ctx, handle, theme, icons, _appearance) = prompt_harness();
   let display = ClearDisplay::dummy_calldata();
   let mut open = true;

   let nodes = render_modal(&ctx, Vec::new(), |ui| {
      handle.write(|zeus| {
         show_calldata_modal(
            &mut open,
            &theme,
            zeus,
            ChainId::Base,
            icons.clone(),
            Some(&display),
            "0x617ba037".to_string(),
            ui,
         );
      });
   });

   let field = text_field(&nodes, "Raw calldata").expect("the raw viewer must be named");

   assert_eq!(
      field.role(),
      Role::MultilineTextInput,
      "the viewer is the multiline input it draws"
   );
   assert_eq!(
      field.description(),
      Some("The raw calldata of this transaction."),
      "the field must say what it holds"
   );
   assert!(
      field.is_read_only(),
      "a viewer that throws every edit away must not offer itself for typing"
   );
   assert!(
      !field.supports_action(Action::SetValue),
      "a read-only field takes no value from a reader"
   );

   // The drawn word is a label node of its own; the text run it paints stays under that node
   // and is hidden with it, which is what takes the word out of the tree a reader is handed.
   let raw: Vec<&Node> = nodes
      .iter()
      .map(|(_, node)| node)
      .filter(|node| node.role() == Role::Label && node.value() == Some("Raw"))
      .collect();

   assert_eq!(
      raw.len(),
      1,
      "the drawn Raw label is the one node the field replaces"
   );
   assert!(
      raw[0].is_hidden(),
      "the word must be hidden, or a reader hears it on its own"
   );
}

/// Escape is the window's way out: the modal ignores it while it is not closable and the local
/// open flag was never written back, so the only exit used to be a button after the whole
/// description.
#[test]
fn escape_closes_the_transaction_details_window() {
   let (ctx, handle, theme, icons, _appearance) = prompt_harness();
   let mut window = TxWindow::for_test(Some(TransactionRich::dummy_clear_signed()), false);

   render_modal(&ctx, Vec::new(), |ui| {
      handle.write(|zeus| window.show(zeus, &theme, icons.clone(), ui));
   });

   assert!(
      window.is_open(),
      "the window must be drawn before the key is sent"
   );

   render_modal(&ctx, vec![key_press(Key::Escape)], |ui| {
      handle.write(|zeus| window.show(zeus, &theme, icons.clone(), ui));
   });

   assert!(
      !window.is_open(),
      "Escape must close the transaction details window"
   );
}

/// A failed transaction says so in the error colour and in nothing else: the same sentence is an
/// Alert node, which is the one thing a reader cannot miss.
#[test]
fn a_failed_transaction_is_an_alert() {
   let (ctx, handle, theme, icons, _appearance) = prompt_harness();

   let mut tx = TransactionRich::dummy_clear_signed();
   tx.success = false;

   let mut window = TxWindow::for_test(Some(tx), false);

   let nodes = render_modal(&ctx, Vec::new(), |ui| {
      handle.write(|zeus| window.show(zeus, &theme, icons.clone(), ui));
   });

   let alert = nodes
      .iter()
      .map(|(_, node)| node)
      .find(|node| {
         node.role() == Role::Alert
            && node.label().unwrap_or_default().to_lowercase().contains("failed")
      })
      .expect("a failed transaction must warn with an Alert node named for it");

   assert_eq!(
      alert.label(),
      Some("Transaction failed"),
      "the alert keeps the sentence the window draws"
   );
}

/// A spinner is drawn, not spoken, so the window's only content while the descriptor loads is a
/// picture: the state it is in is a text node of its own.
#[test]
fn the_loading_details_window_says_it_is_loading() {
   let (ctx, handle, theme, icons, _appearance) = prompt_harness();
   let mut window = TxWindow::for_test(None, true);

   let nodes = render_modal(&ctx, Vec::new(), |ui| {
      handle.write(|zeus| window.show(zeus, &theme, icons.clone(), ui));
   });

   assert!(
      nodes
         .iter()
         .any(|(_, node)| node.value() == Some("Loading transaction details")),
      "the loading state must say what it is waiting for"
   );
}

/// A wallet named `name` holding a random key: imported until a test says otherwise.
fn wallet_named(name: &str) -> WalletInfo {
   WalletInfo::from_wallet(
      &zeus_wallet::Wallet::new_rng(name.to_string()),
      false,
   )
}

/// The master wallet a vault starts with.
fn master_wallet(name: &str) -> WalletInfo {
   let mut wallet = wallet_named(name);
   wallet.is_master = true;
   wallet.is_imported = false;
   wallet
}

/// Stand the Wallets page up on `current`, with `others` under it, each wallet holding something
/// on `chains`.
///
/// The page reads the wallet it is on from the cache unlock fills, and the panel draws it only
/// while the vault is unlocked; the rows and their values are seeded here instead of fetched, since
/// the fetch runs on a worker that reads the frame's own global.
fn seed_wallet_page(
   gui: &mut GUI,
   zeus: &ZeusCtx,
   current: &WalletInfo,
   others: &[WalletInfo],
   chains: &[u64],
) {
   let all: Vec<WalletInfo> =
      std::iter::once(current.clone()).chain(others.iter().cloned()).collect();

   zeus.write(|zeus| {
      zeus.vault_unlocked = true;
      // Which chains are turned off comes out of the user's data directory, so the test sets that
      // set rather than inheriting a machine's configuration.
      zeus.disabled_chains =
         crate::core::types::DisabledChains::new(std::collections::HashSet::new());
      zeus.current_wallet = current.clone();
      zeus.wallet_info_cache = crate::core::types::WalletInfoCache::new(
         all.iter().map(|wallet| (wallet.address, wallet.clone())).collect(),
         all,
      );
   });

   let page = &mut gui.wallet_ui;
   page.for_test(current, others.to_vec());

   for wallet in std::iter::once(current).chain(others) {
      page.set_value_for_test(
         wallet.address,
         WalletValue::default(),
         chains.to_vec(),
      );
   }
}

/// Draw the central panel and then the announcer, which is the order the app draws them in and the
/// only way a page's sentence is said: a page cannot announce from inside its own draw.
fn render_wallet_page(
   ctx: &Context,
   gui: &mut GUI,
   zeus: &ZeusCtx,
   events: Vec<Event>,
) -> Vec<(NodeId, Node)> {
   frame_nodes(ctx, events, |ui| {
      zeus.write(|zeus| crate::gui::ui::panels::central_panel::show(gui, zeus, ui));
      gui.show_announcer(ui);
   })
}

/// Draw the Wallets page twice: the first pass lays the card out, and the second is the frame a
/// reader is handed.
fn draw_wallet_page(ctx: &Context, gui: &mut GUI, zeus: &ZeusCtx) -> Vec<(NodeId, Node)> {
   render_wallet_page(ctx, gui, zeus, Vec::new());
   render_wallet_page(ctx, gui, zeus, Vec::new())
}

/// Every name in the tree, for the failure message of a missing node.
fn drawn_names(nodes: &[(NodeId, Node)]) -> Vec<&str> {
   nodes
      .iter()
      .filter_map(|(_, node)| node.label().or_else(|| node.value()))
      .collect()
}

/// The text of this frame's announcement node, in a tree that keeps node ids.
fn announced(nodes: &[(NodeId, Node)]) -> Option<String> {
   nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Status)
      .and_then(|(_, node)| node.label().map(str::to_string))
}

/// The wallet Zeus is on is the card above the search field: one group node that says the wallet
/// and its value, with the two labels that draw those words left out of the reading.
#[test]
fn the_current_wallet_is_one_card_named_after_it() {
   let (ctx, mut gui, zeus) = main_window();
   let master = master_wallet("Main");
   let savings = wallet_named("Savings");

   seed_wallet_page(&mut gui, &zeus, &master, &[savings], &[1, 42161]);

   let nodes = draw_wallet_page(&ctx, &mut gui, &zeus);
   let card = "Main, master wallet, $0.00";

   let node = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Group && node.label() == Some(card))
      .map(|(_, node)| node)
      .unwrap_or_else(|| {
         panic!(
            "the current wallet must be one group named {card:?}: {:?}",
            drawn_names(&nodes)
         )
      });

   assert_eq!(
      node.description(),
      Some("On Ethereum, Arbitrum"),
      "the chains the card draws are said once, as its description"
   );

   for text in ["Main (Master)", "$0.00"] {
      assert!(
         nodes.iter().any(|(_, node)| node.role() == Role::Label
            && node.is_hidden()
            && node.value() == Some(text)),
         "the label drawing {text:?} must be hidden: the card already says it"
      );
   }
}

/// Every other wallet is one row that is a button, so a reader can pick it: the row carries the
/// wallet and its value, and the chains it holds something on are its description.
#[test]
fn every_other_wallet_is_one_row_that_says_who_it_is() {
   let (ctx, mut gui, zeus) = main_window();
   let master = master_wallet("Main");
   let savings = wallet_named("Savings");

   seed_wallet_page(&mut gui, &zeus, &master, &[savings], &[1, 42161]);

   let nodes = draw_wallet_page(&ctx, &mut gui, &zeus);
   let row = "Savings, imported wallet, $0.00";

   let node = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some(row))
      .map(|(_, node)| node)
      .unwrap_or_else(|| {
         panic!(
            "every other wallet must be one button row named {row:?}: {:?}",
            drawn_names(&nodes)
         )
      });

   assert_eq!(
      node.description(),
      Some("On Ethereum, Arbitrum"),
      "the row says which chains it holds something on"
   );

   assert!(
      node.supports_action(Action::Click),
      "the row has to answer a Click request, or a reader cannot switch to it"
   );

   assert!(
      node.supports_action(Action::Focus),
      "the row has to be reachable by keyboard as well"
   );

   assert!(
      nodes.iter().any(|(_, node)| node.role() == Role::Label
         && node.is_hidden()
         && node.value() == Some("Savings (Imported)")),
      "the drawn name is inside the row's own name, so its label is hidden"
   );
}

/// An AccessKit Click on a row is the same switch a mouse click is, and the page says which wallet
/// it moved to.
#[test]
fn a_wallet_row_click_switches_and_the_page_says_so() {
   let _turn = shared_gui_turn();
   let (ctx, mut gui, zeus) = main_window();
   let master = master_wallet("Main");
   let savings = wallet_named("Savings");

   seed_wallet_page(&mut gui, &zeus, &master, &[savings.clone()], &[1]);

   let nodes = draw_wallet_page(&ctx, &mut gui, &zeus);
   let row = "Savings, imported wallet, $0.00";

   let row = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some(row))
      .map(|(id, _)| *id)
      .unwrap_or_else(|| {
         panic!(
            "the row must be in the tree before it is clicked: {:?}",
            drawn_names(&nodes)
         )
      });

   let nodes = render_wallet_page(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, row)],
   );

   assert_eq!(
      announced(&nodes).as_deref(),
      Some("Switched to Savings, imported wallet."),
      "the page's sentence must reach the announcement node"
   );

   assert_eq!(
      zeus.read(|zeus| zeus.current_wallet.address),
      savings.address,
      "the click must move Zeus to that wallet"
   );

   assert!(
      gui.wallet_ui.take_notice().is_none(),
      "the panel drains the page's sentence, so it is not said again next frame"
   );
}

/// The address button names the wallet it copies for, keeps the whole address as its description
/// and says when the copy is done.
#[test]
fn the_address_button_says_whose_address_it_copied() {
   let (ctx, mut gui, zeus) = main_window();
   let master = master_wallet("Main");

   seed_wallet_page(&mut gui, &zeus, &master, &[], &[1]);

   let nodes = draw_wallet_page(&ctx, &mut gui, &zeus);
   let button = "Copy address of Main, master wallet";

   let (button, node) = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some(button))
      .unwrap_or_else(|| {
         panic!(
            "the card's address button must say what it copies: {:?}",
            drawn_names(&nodes)
         )
      });

   let full = crate::gui::ui::address_for_speech(None, &master.address.to_string()).name;

   assert_eq!(
      node.description(),
      Some(full.as_str()),
      "the whole address a listener cannot see is the button's description"
   );

   assert!(
      !node.description().unwrap_or_default().contains("..."),
      "a shortened address must never stand in for the address in speech"
   );

   let nodes = render_wallet_page(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, *button)],
   );

   assert_eq!(
      announced(&nodes).as_deref(),
      Some("Address copied."),
      "the copy says it happened, since nothing on screen changes"
   );
}

/// The search field says what it narrows, and the count says what the search left — once, when it
/// changes.
#[test]
fn the_search_field_is_named_and_the_count_says_what_it_left() {
   let (ctx, mut gui, zeus) = main_window();
   let master = master_wallet("Main");
   let savings = wallet_named("Savings");
   let trading = wallet_named("Trading");

   seed_wallet_page(
      &mut gui,
      &zeus,
      &master,
      &[savings, trading],
      &[1],
   );

   // The first frame the page draws is the one that says how many wallets it holds.
   let nodes = render_wallet_page(&ctx, &mut gui, &zeus, Vec::new());

   assert!(
      nodes.iter().any(|(_, node)| is_text_input(node.role())
         && node_name(&nodes, node).as_deref() == Some("Search wallets")),
      "the search field must be named for what it narrows: {:?}",
      drawn_names(&nodes)
   );

   assert_eq!(
      announced(&nodes).as_deref(),
      Some("2 wallets"),
      "the page says how many wallets the list holds when it arrives"
   );

   gui.wallet_ui.set_query_for_test("sav");
   let nodes = render_wallet_page(&ctx, &mut gui, &zeus, Vec::new());

   assert_eq!(
      announced(&nodes).as_deref(),
      Some("1 wallet"),
      "narrowing the list to one wallet says so"
   );
}

/// The rename dialog opens on the name in use and puts the keyboard in its field, and Escape
/// leaves both the dialog and the wallet alone.
#[test]
fn the_rename_dialog_opens_on_the_name_in_use_and_escape_leaves_it_alone() {
   let (ctx, mut gui, zeus) = main_window();
   let master = master_wallet("Main");

   seed_wallet_page(&mut gui, &zeus, &master, &[], &[1]);
   gui.wallet_ui.open_rename_wallet(Some(master.clone()));

   // A shrink-wrapping dialog spends two invisible sizing passes before it paints, and egui
   // exposes no node for a widget it considers invisible.
   let mut nodes = Vec::new();

   for _ in 0..3 {
      nodes = render_wallet_page(&ctx, &mut gui, &zeus, Vec::new());
   }

   let (field, node) = nodes
      .iter()
      .find(|(_, node)| {
         is_text_input(node.role()) && node_name(&nodes, node).as_deref() == Some("New wallet name")
      })
      .unwrap_or_else(|| {
         panic!(
            "the dialog must name its field: {:?}",
            drawn_names(&nodes)
         )
      });

   assert_eq!(
      node.value(),
      Some("Main"),
      "the field opens on the name in use, so a reader edits it instead of retyping it"
   );

   assert_eq!(
      ctx.memory(|memory| memory.focused()).map(|id| id.accesskit_id()),
      Some(*field),
      "the field must take the opening focus the dialog gave its Close button"
   );

   let nodes = render_wallet_page(
      &ctx,
      &mut gui,
      &zeus,
      vec![key_press(Key::Escape)],
   );

   assert!(
      !gui.wallet_ui.rename_is_open_for_test(),
      "Escape must close the dialog: {:?}",
      drawn_names(&nodes)
   );

   assert_eq!(
      zeus.read(|zeus| zeus.current_wallet.name().to_string()),
      "Main",
      "closing the dialog must leave the wallet named as it was"
   );
}

/// The options button opens a menu whose items say what each one does, and the master's Delete
/// says why it cannot be used.
#[test]
fn the_wallet_menu_says_what_each_item_does() {
   let (ctx, mut gui, zeus) = main_window();
   let master = master_wallet("Main");

   seed_wallet_page(&mut gui, &zeus, &master, &[], &[1]);

   let nodes = draw_wallet_page(&ctx, &mut gui, &zeus);
   let options = "Options for Main, master wallet";

   let options = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some(options))
      .map(|(id, _)| *id)
      .unwrap_or_else(|| {
         panic!(
            "the card's dots button must say whose options it opens: {:?}",
            drawn_names(&nodes)
         )
      });

   render_wallet_page(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, options)],
   );

   let nodes = render_wallet_page(&ctx, &mut gui, &zeus, Vec::new());

   for label in ["Export key", "Rename", "Show address QR code"] {
      assert!(
         nodes
            .iter()
            .any(|(_, node)| node.role() == Role::Button && node.label() == Some(label)),
         "the menu must say {label:?}: {:?}",
         drawn_names(&nodes)
      );
   }

   let delete = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some("Delete"))
      .map(|(_, node)| node)
      .unwrap_or_else(|| {
         panic!(
            "the master's menu must keep a Delete item: {:?}",
            drawn_names(&nodes)
         )
      });

   assert!(
      delete.is_disabled(),
      "the master wallet is the one wallet Zeus will not delete"
   );

   assert_eq!(
      delete.description(),
      Some("The master wallet cannot be deleted."),
      "the disabled item says why"
   );
}
