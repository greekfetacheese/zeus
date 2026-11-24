use super::app::{main_window, master_wallet};
use super::harness::{
   accesskit_action, announced, copy_said, drawn_names, focused_node, frame_nodes, node_texts,
   opener_button,
};
use crate::core::ZeusCtx;
use crate::gui::GUI;
use egui::accesskit::{Action, Node, NodeId, Role};
use egui::{Context, Event, Id};

/// Draw the receive window and say what it left behind, the way the left panel does: the window
/// is drawn from the panel, and the panel hands its words to the app's one channel.
///
/// Three passes for the same reason as the other shrink-wrapping dialogs: the first two size it,
/// and egui exposes no node for a widget it considers invisible.
fn render_receive_window(
   ctx: &Context,
   gui: &mut GUI,
   zeus: &ZeusCtx,
   events: Vec<Event>,
) -> Vec<(NodeId, Node)> {
   let mut nodes = Vec::new();

   for pass in 0..3 {
      let events = if pass == 2 {
         events.clone()
      } else {
         Vec::new()
      };
      let theme = gui.theme.clone();

      nodes = frame_nodes(ctx, events, |ui| {
         zeus.write(|zeus| gui.account_panel.qr_window.show(zeus, &theme, ui));

         if let Some(text) = gui.account_panel.take_notice() {
            gui.announce(text);
         }

         gui.show_announcer(ui);
      });
   }

   nodes
}

/// The receive window draws no title, so its dialog node had no name at all. The mode the window
/// shows names it, the wallet is said as its name and kind, and the wide copy button is named with
/// the whole address grouped in fours — the drawn text repeats the address, but only the grouped
/// form lets a listener check it by ear. The shortening drawn on screen stays as the description.
#[test]
fn the_receive_window_says_which_address_it_shows() {
   let (ctx, mut gui, zeus) = main_window();
   let wallet = master_wallet("Main");
   let address = wallet.address.to_string();

   gui.account_panel.qr_window.open_for_test(Some(wallet));

   let nodes = render_receive_window(&ctx, &mut gui, &zeus, Vec::new());

   assert!(
      nodes.iter().any(
         |(_, node)| node.role() == Role::Dialog && node.label() == Some("Public address (EVM)")
      ),
      "the window must be a dialog named after the address it shows: {:?}",
      drawn_names(&nodes)
   );

   assert!(
      nodes.iter().any(|(_, node)| node.role() == Role::Label
         && node.value() == Some("Main, master wallet")),
      "the wallet must be said as its name and kind: {:?}",
      drawn_names(&nodes)
   );

   let spoken = crate::gui::ui::address_for_speech(None, &address);

   let button = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some(spoken.name.as_str()))
      .map(|(_, node)| node)
      .unwrap_or_else(|| {
         panic!(
            "the address button must be named by the shortening the row draws: {:?}",
            drawn_names(&nodes)
         )
      });

   assert_eq!(
      button.description(),
      spoken.description.as_deref(),
      "the whole address is the button's description"
   );
}

/// Copying the address changes nothing on screen, so the window says it happened. The sentence
/// goes through the panel to the app's one channel, and the next frame says nothing more.
#[test]
fn the_receive_window_says_when_an_address_is_copied() {
   let (ctx, mut gui, zeus) = main_window();
   let wallet = master_wallet("Main");
   let address = wallet.address.to_string();

   gui.account_panel.qr_window.open_for_test(Some(wallet));

   let nodes = render_receive_window(&ctx, &mut gui, &zeus, Vec::new());
   let spoken = crate::gui::ui::address_for_speech(None, &address);

   let copy = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some(spoken.name.as_str()))
      .map(|(id, _)| *id)
      .unwrap_or_else(|| {
         panic!(
            "the address button must be in the tree before it is clicked: {:?}",
            drawn_names(&nodes)
         )
      });

   let nodes = render_receive_window(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, copy)],
   );

   assert_eq!(
      announced(&nodes).as_deref(),
      Some(copy_said(&address).as_str()),
      "the copy must reach the announcement node the panel feeds, naming the address"
   );

   let nodes = render_receive_window(&ctx, &mut gui, &zeus, Vec::new());

   assert_eq!(
      announced(&nodes).as_deref().unwrap_or_default(),
      "",
      "the copy is said once, so the next frame says nothing more"
   );
}

/// A receive window with no wallet says what happened and what to do about it, in place of the
/// sentence that blamed the build.
#[test]
fn a_receive_window_without_a_wallet_says_what_to_do() {
   let (ctx, mut gui, zeus) = main_window();
   gui.account_panel.qr_window.open_for_test(None);

   let nodes = render_receive_window(&ctx, &mut gui, &zeus, Vec::new());
   let sentence = "No wallet selected. Close this window and open it from the account menu.";

   assert!(
      nodes
         .iter()
         .any(|(_, node)| node.role() == Role::Label && node.value() == Some(sentence)),
      "a window with no wallet must say what to do: {:?}",
      drawn_names(&nodes)
   );

   assert!(
      !nodes
         .iter()
         .any(|(_, node)| node_texts(node).into_iter().any(|text| text.contains("this is a bug"))),
      "no node may blame the build"
   );
}

/// Draw the receive window with the button that opened it beside it, in the same frame, so the
/// focus the window gives back can be looked up.
fn render_receive_window_after(
   ctx: &Context,
   gui: &mut GUI,
   zeus: &ZeusCtx,
   opener: Id,
   events: Vec<Event>,
) -> Vec<(NodeId, Node)> {
   let mut nodes = Vec::new();

   for pass in 0..3 {
      let events = if pass == 2 {
         events.clone()
      } else {
         Vec::new()
      };
      let theme = gui.theme.clone();

      nodes = frame_nodes(ctx, events, |ui| {
         opener_button(ui, opener, "Show QR Code");
         zeus.write(|zeus| gui.account_panel.qr_window.show(zeus, &theme, ui));

         if let Some(text) = gui.account_panel.take_notice() {
            gui.announce(text);
         }

         gui.show_announcer(ui);
      });
   }

   nodes
}

/// The receive window is about one address, so it opens on that address's copy button — not on
/// the Close button the modal would otherwise leave the focus on.
#[test]
fn the_receive_window_opens_on_its_address_button() {
   let (ctx, mut gui, zeus) = main_window();
   let wallet = master_wallet("Main");
   let address = wallet.address.to_string();

   gui.account_panel.qr_window.open_for_test(Some(wallet));

   let nodes = render_receive_window(&ctx, &mut gui, &zeus, Vec::new());
   let spoken = crate::gui::ui::address_for_speech(None, &address);
   let focused = focused_node(&ctx, &nodes);

   assert_eq!(
      focused.role(),
      Role::Button,
      "the first focus is the address button, not a picture or the Close button: {:?}",
      focused.role()
   );
   assert_eq!(
      focused.label(),
      Some(spoken.name.as_str()),
      "the focused button must be named by the shortening the row draws"
   );
}

/// A window with no wallet has nothing to copy: its one control, Close, is where a reader lands.
#[test]
fn a_receive_window_without_a_wallet_opens_on_close() {
   let (ctx, mut gui, zeus) = main_window();
   gui.account_panel.qr_window.open_for_test(None);

   let nodes = render_receive_window(&ctx, &mut gui, &zeus, Vec::new());
   let focused = focused_node(&ctx, &nodes);

   assert_eq!(
      focused.label(),
      Some("Close"),
      "a window with nothing to copy opens on its one control"
   );
}

/// Closing the receive window is not the end of the interaction: the control that opened it takes
/// the focus back, so the reader is where they were.
#[test]
fn closing_the_receive_window_gives_the_focus_back() {
   let (ctx, mut gui, zeus) = main_window();
   let wallet = master_wallet("Main");
   let opener = Id::unique("receive_window_opener");

   gui.account_panel.qr_window.open_for_test_with_opener(Some(wallet), opener);

   let nodes = render_receive_window_after(&ctx, &mut gui, &zeus, opener, Vec::new());
   let close = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some("Close"))
      .map(|(id, _)| *id)
      .expect("the window must offer its Close button");

   let nodes = render_receive_window_after(
      &ctx,
      &mut gui,
      &zeus,
      opener,
      vec![accesskit_action(Action::Click, close)],
   );

   assert!(
      !gui.account_panel.qr_window.is_open(),
      "the Close button must close the window"
   );

   let focused = focused_node(&ctx, &nodes);

   assert_eq!(
      focused.label(),
      Some("Show QR Code"),
      "the control that opened the window takes the focus back"
   );
}
