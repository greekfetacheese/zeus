use super::app::{main_window, master_wallet, render_left_panel};
use super::harness::{
   accesskit_action, announced, copy_said, drawn_names, focused_text, frame, frame_nodes,
   is_text_input, status_text,
};
use crate::core::ZeusCtx;
use crate::gui::GUI;
use egui::accesskit::{Action, Node, NodeId, Role};
use egui::{Context, Event};

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

/// The Delegate window asks for a smart-contract address in a field with no drawn label, so the
/// field has to say what it is for.
#[test]
fn the_delegate_window_names_its_address_field() {
   let (ctx, mut gui, zeus) = main_window();
   gui.account_panel.open();
   gui.account_panel.delegate.open();

   render_account_panel(&ctx, &mut gui, &zeus, Vec::new());
   let nodes = render_account_panel(&ctx, &mut gui, &zeus, Vec::new());

   assert!(
      nodes
         .iter()
         .any(|(_, node)| is_text_input(node.role()) && node.label() == Some("Delegate address")),
      "the address field must be named after what it takes: {:?}",
      drawn_names(&nodes)
   );
}

/// The panel's three dots buttons used to be unnamed: the header's menu, the two service rows'
/// menus and the vitals card's. Each says whose options it opens.
#[test]
fn the_account_panel_menus_are_named() {
   let (ctx, mut gui, zeus) = main_window();
   gui.account_panel.open();

   let nodes = render_account_panel(&ctx, &mut gui, &zeus, Vec::new());

   assert!(
      nodes
         .iter()
         .any(|(_, node)| node.role() == Role::Button
            && node.label() == Some("Delegation options")),
      "the header's dots must say what they open: {:?}",
      drawn_names(&nodes)
   );

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

   for name in [
      "Railgun options",
      "Wallet Connector options",
      "Vitals options",
   ] {
      assert!(
         nodes
            .iter()
            .any(|(_, node)| node.role() == Role::Button && node.label() == Some(name)),
         "the {name} menu must say whose options it opens: {:?}",
         drawn_names(&nodes)
      );
   }
}

/// A reader is not walked through the parts of the panel that exist only for sight: the tab strip
/// that leads to a page of numbers, the address row — an address truncated so only an eye can
/// finish it, a QR code that is a picture of it, a link that leaves the wallet — and the delegation
/// opener, whose form takes a contract address from the clipboard.
#[test]
fn a_reader_walks_the_account_panel_without_its_sight_only_parts() {
   let (ctx, mut gui, zeus) = main_window();
   let wallet = master_wallet("Main");
   let address = wallet.evm_address_truncated();
   let wallet_name = wallet.name_with_id_short();

   gui.account_panel.open();
   gui.account_panel.set_current_wallet(wallet);
   // What the app reports on every pass once a screen reader is attached.
   gui.account_panel.set_field_controls(false);

   let nodes = render_account_panel(&ctx, &mut gui, &zeus, Vec::new());

   let tabs: Vec<&str> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Tab)
      .filter_map(|(_, node)| node.label())
      .collect();

   assert!(
      tabs.is_empty(),
      "a reader must not be walked through a tab strip: {tabs:?}"
   );

   let buttons: Vec<&str> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Button)
      .filter_map(|(_, node)| node.label())
      .collect();

   for gone in [
      "Show QR Code",
      "Open the address in the block explorer",
      "Delegation options",
   ] {
      assert!(
         !buttons.contains(&gone),
         "{gone} must not be a stop for a reader: {buttons:?}"
      );
   }

   assert!(
      !buttons.contains(&address.as_str()),
      "the truncated address is not a stop either: {buttons:?}"
   );

   // The status the row carried is still said, and the rows that work are still there.
   let spoken: Vec<&str> = nodes
      .iter()
      .map(|(_, node)| node)
      .filter(|node| !node.is_hidden())
      .filter_map(|node| node.value())
      .collect();

   assert!(
      spoken.contains(&"Not Delegated"),
      "the delegation state is still said: {spoken:?}"
   );
   assert!(
      buttons.contains(&wallet_name.as_str()),
      "the wallet row still says which account this is: {buttons:?}"
   );
}

/// The region shortcut used to land on the address row, which a reader does not have. It lands on
/// the wallet selector instead: a region must never be entered on nothing.
#[test]
fn the_account_region_opens_on_the_wallet_row_for_a_reader() {
   let (ctx, mut gui, zeus) = main_window();
   let wallet = master_wallet("Main");
   let wallet_name = wallet.name_with_id_short();

   gui.account_panel.open();
   gui.account_panel.set_current_wallet(wallet);
   gui.account_panel.set_field_controls(false);

   gui.request_region_focus(crate::gui::FocusRegion::Account);

   render_left_panel(&ctx, &mut gui, &zeus, Vec::new());
   let nodes = render_left_panel(&ctx, &mut gui, &zeus, Vec::new());

   assert_eq!(
      focused_text(&ctx, &nodes).as_deref(),
      Some(wallet_name.as_str()),
      "the region must land on the wallet row: {:?}",
      drawn_names(&nodes)
   );
}

/// Copying the header's address changes nothing on screen, so the panel hands the sentence to the
/// left panel, and the announcer says it once.
#[test]
fn the_account_address_copy_says_it_happened() {
   let (ctx, mut gui, zeus) = main_window();
   let wallet = master_wallet("Main");
   let address = wallet.evm_address_truncated();
   let said = copy_said(&wallet.address.to_string());

   gui.account_panel.open();
   gui.account_panel.set_current_wallet(wallet);

   render_left_panel(&ctx, &mut gui, &zeus, Vec::new());
   let nodes = render_left_panel(&ctx, &mut gui, &zeus, Vec::new());

   let copy = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some(address.as_str()))
      .map(|(id, _)| *id)
      .unwrap_or_else(|| {
         panic!(
            "the header address must be a button in the tree: {:?}",
            drawn_names(&nodes)
         )
      });

   let nodes = render_left_panel(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, copy)],
   );

   assert_eq!(
      announced(&nodes).as_deref(),
      Some(said.as_str()),
      "the copy must reach the announcement node the left panel feeds, naming the address"
   );

   let nodes = render_left_panel(&ctx, &mut gui, &zeus, Vec::new());

   assert_eq!(
      announced(&nodes).as_deref().unwrap_or_default(),
      "",
      "the copy is said once, so the next frame says nothing more"
   );
}
