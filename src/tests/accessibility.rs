//! Accessibility tree tests.
//!
//! A screen reader is handed a `TreeUpdate` by the platform, and egui builds the same value on
//! every AccessKit-enabled frame, on `FullOutput::platform_output`. Asserting on that value is how
//! a role, a name or an announcement is proven without a reader: the check fails when the node
//! stops being emitted, not when a pixel moves.
//!
//! The helpers below are the shared harness; every screen test renders a real screen headlessly.

use crate::core::ZeusCtx;
use crate::core::types::{Contact, RailgunConfig};
use crate::gui::{GUI, LockScreen};
use egui::accesskit::{Action, ActionRequest, Live, Node, NodeId, Role, Toggled, TreeId};
use egui::{Context, Event, Key, Modifiers, RawInput, Rect, Ui, pos2, vec2};
use egui_elements::{Theme, ThemeKind};
use zeus_eth::alloy_primitives::Address;
use zeus_eth::types::ChainId;

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
