use super::app::{
   main_window, render_central, render_central_with_settings, render_settings, settings_window,
};
use super::harness::{
   accesskit_action, drawn_names, frame, frame_nodes, is_text_input, key_press, label_texts,
   node_texts, status_text,
};
use crate::core::ZeusCtx;
use crate::gui::GUI;
use egui::accesskit::{Action, Node, Role};
use egui::{Context, Key, vec2};
use egui_elements::{Theme, ThemeKind};
use zeus_eth::alloy_primitives::Address;
use zeus_eth::types::ChainId;

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

/// The money rows draw an address shortened, and that shortening is what the row is called: a name
/// the glyphs below it do not say is heard twice, and the whole address spelled out one character
/// at a time is a wall nobody can follow. The whole address stays as the description, which is
/// where two rows that agree on both ends still differ.
#[test]
fn the_recipient_row_names_the_address_by_its_ends() {
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
      name, "0x1111...111111",
      "the link is named by the shortening the row draws, so it is heard once"
   );
   assert!(
      name.chars().count() < 20,
      "the whole address, one character at a time, is not something a listener can follow"
   );
   assert_eq!(
      link.description(),
      Some("0x1111 1111 1111 1111 1111 1111 1111 1111 1111 1111"),
      "the whole address stays on the node, for whoever asks for it"
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

/// A notice panel draws its title and hint with plain labels, and a plain label already carries its
/// text into the tree. This pins that the dapp panels' prose is reachable without being rewritten,
/// and fails loudly if a future egui stops emitting it.
#[test]
fn prose_in_a_notice_panel_reaches_the_tree() {
   use crate::gui::ui::common::notices::notice_panel;

   let ctx = Context::default();
   let mut theme = Theme::new(ThemeKind::TokyoNight);
   theme.install(&ctx);

   let title = "Railgun is disabled";
   let hint = "Enable it in Settings, on the Railgun page, to shield and unshield.";

   let nodes = frame(&ctx, |ui| {
      notice_panel(&theme, (300.0, 120.0), title, Some(hint), ui);
   });

   let spoken = label_texts(&nodes);

   for sentence in [title, hint] {
      assert!(
         spoken.contains(&sentence),
         "a notice line must reach the tree as a label carrying its own text: {sentence:?} in {spoken:?}"
      );
   }
}

/// A caption is what lets two pickers side by side say which side of the form they are on: the
/// trigger is otherwise named from the chain it draws, which is the same kind of name on both.
#[test]
fn a_chain_picker_says_which_side_it_is() {
   use crate::gui::ui::common::ChainSelect;

   let ctx = Context::default();
   let mut gui = GUI::default();
   gui.theme.install(&ctx);

   let theme = gui.theme.clone();
   let icons = gui.icons.clone();
   let zeus = ZeusCtx::new(crate::runtime::test_handle());
   let mut picker = ChainSelect::new("from_chain_test", 1).caption("From chain");

   let nodes = frame(&ctx, |ui| {
      zeus.write(|zeus| {
         picker.show(zeus, &[], &theme, icons.clone(), ui);
      });
   });

   let named = nodes
      .iter()
      .find(|node| node.label() == Some("From chain: Ethereum"))
      .expect("the picker must say which side of the form it is");

   assert_eq!(
      named.role(),
      Role::Button,
      "the caption must name the picker's trigger, not add a node beside it"
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
      Some("Amount in Ethereum"),
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
         .any(|node| node.role() == Role::Button && node.label() == Some("Change token (Ethereum)")),
      "the currency button says what it shows and what pressing it does"
   );

   assert!(
      spoken.contains(&"Balance 10.00"),
      "the balance is named, not left as a bare number: {spoken:?}"
   );
}

/// The eye beside the privacy switch is decoration: the switch itself is named and says whether it
/// is on, so the picture contributes no asset name and no texture URI.
#[test]
fn the_privacy_switch_picture_is_not_read_out() {
   let (ctx, mut gui, zeus) = settings_window();
   let nodes = render_settings(&ctx, &mut gui, &zeus, Vec::new());

   for (_, node) in nodes.iter().filter(|(_, node)| node.role() == Role::Image) {
      for text in node_texts(node) {
         assert!(
            !text.contains("bytes://") && text != "eye" && text != "eye-off",
            "a decorative picture must not read out its asset: {text:?}"
         );
      }
   }
}
