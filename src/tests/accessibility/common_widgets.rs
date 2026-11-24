use super::app::{main_window, render_central, render_central_with_settings, settings_window};
use super::harness::{accesskit_action, drawn_names, frame, frame_nodes, key_press, status_text};
use crate::gui::GUI;
use egui::accesskit::{Action, Node, Role};
use egui::{Context, Key};

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

/// A label that paints its own galley has nothing under it in the tree, so a name written on it is
/// the only string a reader gets. A plain `ui.label` owns a `TextRun` child holding the same
/// words it draws, so naming one over its glyphs says the line twice — once in the name, once in
/// the text under it. That is the rule the amount lines follow: an `egui-elements` label is named
/// over its ticker, a plain one is drawn in the words a reader needs.
#[test]
fn a_named_label_is_read_once_only_when_it_paints_its_own_galley() {
   let ctx = Context::default();

   let nodes = frame_nodes(&ctx, Vec::new(), |ui| {
      let plain = ui.label(egui::RichText::new("0.00 ETH"));
      crate::gui::ui::common::name_text(ui, &plain, "0.00 Ethereum");

      let themed = egui_elements::Label::new(egui::RichText::new("0.50 USDC"), None);
      let response = ui.add(themed);
      crate::gui::ui::common::name_text(ui, &response, "0.50 USD Coin");
   });

   let named = |text: &str| {
      nodes
         .iter()
         .find(|(_, node)| node.role() == Role::Label && node.label() == Some(text))
         .map(|(_, node)| node)
         .unwrap_or_else(|| {
            panic!(
               "the label must be named by the words it was given: {:?}",
               drawn_names(&nodes)
            )
         })
   };

   let plain = named("0.00 Ethereum");
   let under: Vec<&str> = plain
      .children()
      .iter()
      .filter_map(|child| nodes.iter().find(|(id, _)| id == child))
      .filter(|(_, node)| !node.is_hidden())
      .filter_map(|(_, node)| node.value())
      .collect();

   assert_eq!(
      under,
      vec!["0.00 ETH"],
      "a plain label still carries the glyphs it drew, which a reader would hear as well"
   );

   let themed = named("0.50 USD Coin");

   assert!(
      themed
         .children()
         .iter()
         .all(|child| nodes.iter().any(|(id, node)| id == child && node.is_hidden())),
      "a themed label paints its own galley, so the name is the only string under it"
   );
}
