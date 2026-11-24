use super::app::main_window;
use super::harness::{accesskit_action, frame_nodes, is_text_input, key_press};
use crate::core::ZeusCtx;
use crate::core::types::{Contact, Recipient};
use crate::gui::GUI;
use egui::accesskit::{Action, Node, NodeId, Role};
use egui::{Context, Event, Key};
use zeus_eth::alloy_primitives::Address;
use zeus_eth::types::ChainId;

/// Render the recipient row headlessly, with `events` delivered to the first pass.
fn render_recipient(
   ctx: &Context,
   gui: &mut GUI,
   zeus: &ZeusCtx,
   events: Vec<Event>,
) -> Vec<(NodeId, Node)> {
   render_recipient_on(ctx, gui, zeus, events, false)
}

/// Render the recipient row with the field on the `privacy_mode` side: `0zk` when set, `0x`
/// otherwise.
fn render_recipient_on(
   ctx: &Context,
   gui: &mut GUI,
   zeus: &ZeusCtx,
   events: Vec<Event>,
   privacy_mode: bool,
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
            privacy_mode,
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

/// An address the book already knows is named, not called unknown: pasting a wallet or contact
/// address leaves the recipient without a name binding, and the row still says who it is.
#[test]
fn a_known_recipient_address_is_said_by_its_name() {
   const ADDRESS: &str = "0x1111111111111111111111111111111111111111";
   const NAME: &str = "Railgun Test 3";

   let (ctx, mut gui, zeus) = main_window();
   let address = ADDRESS.parse::<Address>().expect("the fixture is an address");
   zeus.address_book().insert_identity(address, NAME);

   gui.recipient_selection.recipient.evm_address = ADDRESS.to_string();

   let nodes = render_recipient(&ctx, &mut gui, &zeus, Vec::new());
   let spoken: Vec<&str> = nodes.iter().filter_map(|(_, node)| node.value()).collect();

   assert!(
      spoken.contains(&NAME),
      "the row must name the address the book already knows: {spoken:?}"
   );
   assert!(
      !spoken.contains(&"Unknown Address"),
      "a known address is not an unknown one: {spoken:?}"
   );

   let field = nodes
      .iter()
      .find(|(_, node)| is_text_input(node.role()))
      .map(|(_, node)| node)
      .expect("the recipient row must draw its address field");

   assert_eq!(
      field.description(),
      Some("Railgun Test 3. Press Enter to choose a recipient."),
      "the known name leads the description, and the way into the picker follows it"
   );
}

/// The private side resolves a typed `0zk` address the same way: a contact that already owns it is
/// named instead of being called unknown.
#[test]
fn a_known_private_recipient_is_said_by_its_name() {
   const ZK_ADDRESS: &str = "0zk1example";
   const NAME: &str = "Railgun Test 3";

   let (ctx, mut gui, zeus) = main_window();
   zeus.write_wallet_state(|wallet_state| {
      wallet_state.contacts.push(Contact::new(
         NAME.to_string(),
         String::new(),
         ZK_ADDRESS.to_string(),
      ));
   });

   gui.recipient_selection.recipient = Recipient {
      zk_address: ZK_ADDRESS.to_string(),
      ..Default::default()
   };

   let nodes = render_recipient_on(&ctx, &mut gui, &zeus, Vec::new(), true);
   let spoken: Vec<&str> = nodes.iter().filter_map(|(_, node)| node.value()).collect();

   assert!(
      spoken.contains(&NAME),
      "the row must name the contact the `0zk` address belongs to: {spoken:?}"
   );
   assert!(
      !spoken.contains(&"Unknown Address"),
      "a known `0zk` address is not an unknown one: {spoken:?}"
   );

   let field = nodes
      .iter()
      .find(|(_, node)| is_text_input(node.role()))
      .map(|(_, node)| node)
      .expect("the recipient row must draw its `0zk` address field");

   assert_eq!(
      field.label(),
      Some("Recipient 0zk address"),
      "the privacy field keeps the name it draws"
   );
   assert_eq!(
      field.description(),
      Some("Railgun Test 3. Press Enter to choose a recipient."),
      "the known name leads the description, and the way into the picker follows it"
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
