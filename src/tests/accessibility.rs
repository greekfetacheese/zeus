//! Accessibility tree tests.
//!
//! A screen reader is handed a `TreeUpdate` by the platform, and egui builds the same value on
//! every AccessKit-enabled frame, on `FullOutput::platform_output`. Asserting on that value is how
//! a role, a name or an announcement is proven without a reader: the check fails when the node
//! stops being emitted, not when a pixel moves.
//!
//! The helpers below are the shared harness; every screen test renders a real screen headlessly.

use crate::core::ZeusCtx;
use crate::gui::{GUI, LockScreen};
use egui::accesskit::{Action, ActionRequest, Live, Node, NodeId, Role, Toggled, TreeId};
use egui::{Context, Event, Key, Modifiers, RawInput, Rect, Ui, pos2, vec2};
use egui_elements::{Theme, ThemeKind};

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
