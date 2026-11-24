use super::app::{master_wallet, shared_gui_turn};
use super::harness::{
   accesskit_action, copy_said, drawn_names, focused_id, frame_nodes, is_text_input, key_press,
   named, node_name, parent_of,
};
use crate::core::types::Contact;
use egui::accesskit::{Action, Node, NodeId, Role, Toggled};
use egui::{Context, Event, Id, Key};
use std::time::Duration;
use zeus_eth::types::ChainId;

/// The recipient picker's contacts are a `ScrollArea` list as well: each row says its zero-based
/// place and the scroll's own `Ui` says how many rows the list holds.
#[test]
fn a_contact_picker_row_says_its_place_in_the_list() {
   let _turn = shared_gui_turn();
   let ctx = Context::default();
   let nodes = open_recipient_picker(
      &ctx,
      false,
      &[
         (PICKER_CONTACT, PICKER_CONTACT_ADDRESS),
         (OTHER_CONTACT, OTHER_CONTACT_ADDRESS),
      ],
   );

   for (name, spoken, position) in [
      (PICKER_CONTACT, PICKER_CONTACT_SPOKEN, 0usize),
      (OTHER_CONTACT, OTHER_CONTACT_SPOKEN, 1),
   ] {
      let row_name = contact_row_name(name, spoken);
      let (row, node) = nodes
         .iter()
         .find(|(_, node)| node.role() == Role::Button && node.label() == Some(row_name.as_str()))
         .map(|(id, node)| (*id, node))
         .unwrap_or_else(|| panic!("the {name} row must be in the tree"));

      assert_eq!(
         node.position_in_set(),
         Some(position),
         "the {name} row must say its zero-based place"
      );

      let (_, container) = parent_of(&nodes, row);

      assert_eq!(
         container.size_of_set(),
         Some(2),
         "the picker's scroll Ui must say how many contacts the list holds"
      );
   }
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
      // The shared surface keeps the tab another test left open; this one opens on Contacts.
      gui.recipient_selection.reset_tab_for_test();
      // No field is drawn by this harness, so the picker is opened with no home to hand back to.
      gui.recipient_selection.open(Id::unique("recipient_picker_without_a_field"));
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

/// Tab walks the contact row before the copy button inside it: the row is the control that picks
/// the recipient, and the button only hands its address over. Enter on the row picks it.
#[test]
fn tab_reaches_a_contact_row_before_its_copy_button_and_enter_picks_it() {
   let _turn = shared_gui_turn();
   let ctx = Context::default();

   let nodes = open_recipient_picker(
      &ctx,
      false,
      &[(PICKER_CONTACT, PICKER_CONTACT_ADDRESS)],
   );

   let field = nodes
      .iter()
      .find(|(_, node)| is_text_input(node.role()))
      .map(|(id, _)| *id)
      .expect("the picker must draw its search field");

   render_recipient_picker(
      &ctx,
      false,
      vec![accesskit_action(Action::Focus, field)],
   );

   assert_eq!(
      focused_id(&ctx),
      Some(field),
      "the search field takes the focus the test gives it"
   );

   let row_name = contact_row_name(PICKER_CONTACT, PICKER_CONTACT_SPOKEN);
   let copy_name = format!("Copy address of {PICKER_CONTACT}");

   // The row and its copy button are both buttons in the list, so whichever Tab reaches first is
   // the one a reader can use without a detour.
   let mut first_list_control = None;

   for _ in 0..8 {
      let nodes = render_recipient_picker(&ctx, false, vec![key_press(Key::Tab)]);
      let Some(focused) = focused_id(&ctx) else {
         break;
      };

      let label = named(&nodes, Some(focused));

      if label.as_deref() == Some(row_name.as_str()) || label.as_deref() == Some(copy_name.as_str())
      {
         first_list_control = Some((focused, label));
         break;
      }
   }

   let (_, label) = first_list_control.expect("Tab must reach the contact list");

   assert_eq!(
      label.as_deref(),
      Some(row_name.as_str()),
      "Tab must reach the row itself, before the copy button inside it"
   );

   render_recipient_picker(&ctx, false, vec![key_press(Key::Enter)]);

   assert_eq!(
      picked_recipient_address(),
      PICKER_CONTACT_ADDRESS,
      "Enter on the row must pick that contact"
   );
}

/// Tab walks the list: row, its copy button, the next row, its copy button. A row that asked for
/// the focus a second time when its own frame closed takes the hand-off meant for the row after it,
/// and the list becomes a two-stop loop between one row and its own button.
#[test]
fn tab_walks_a_recipient_list_row_by_row() {
   let _turn = shared_gui_turn();
   let ctx = Context::default();

   let nodes = open_recipient_picker(
      &ctx,
      false,
      &[
         (PICKER_CONTACT, PICKER_CONTACT_ADDRESS),
         (OTHER_CONTACT, OTHER_CONTACT_ADDRESS),
      ],
   );

   let first_name = contact_row_name(PICKER_CONTACT, PICKER_CONTACT_SPOKEN);
   let second_name = contact_row_name(OTHER_CONTACT, OTHER_CONTACT_SPOKEN);
   let first_copy = format!("Copy address of {PICKER_CONTACT}");
   let second_copy = format!("Copy address of {OTHER_CONTACT}");

   let first = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some(first_name.as_str()))
      .map(|(id, _)| *id)
      .expect("the first contact row must be in the tree");

   render_recipient_picker(
      &ctx,
      false,
      vec![accesskit_action(Action::Focus, first)],
   );

   assert_eq!(
      focused_id(&ctx),
      Some(first),
      "the first row takes the keyboard the test gives it"
   );

   let mut walked = Vec::new();

   for _ in 0..3 {
      let nodes = render_recipient_picker(&ctx, false, vec![key_press(Key::Tab)]);
      walked.push(named(&nodes, focused_id(&ctx)));
   }

   assert_eq!(
      walked,
      vec![Some(first_copy), Some(second_name), Some(second_copy)],
      "Tab must walk row, copy button, next row, copy button"
   );
}

/// Draw the recipient picker the way the form does — the picker first, its own button after — and
/// return the button's id: the control a keyboard comes from and has to go back to.
fn render_recipient_from_field(
   ctx: &Context,
   events: Vec<Event>,
   open: bool,
) -> (Vec<(NodeId, Node)>, Id) {
   let mut opener = None;

   let nodes = frame_nodes(ctx, events, |ui| {
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
               false,
               ChainId::Ethereum.id(),
               contacts_ui,
               ui,
            )
         });
      });

      let button = ui.add(egui::Button::new("Whom to"));
      opener = Some(button.id);

      if open && button.clicked() {
         crate::gui::SHARED_GUI.write(|gui| gui.recipient_selection.open(button.id));
      }
   });

   (
      nodes,
      opener.expect("the button must be in the frame"),
   )
}

/// Draw the shared recipient row: the picker and the address field that opens it.
fn render_recipient_field(ctx: &Context, events: Vec<Event>) -> Vec<(NodeId, Node)> {
   frame_nodes(ctx, events, |ui| {
      crate::gui::SHARED_GUI.write(|gui| {
         let mut theme = gui.theme.clone();
         theme.install(ctx);

         let icons = gui.icons.clone();
         let zeus = gui.ctx.clone();
         let picker = &mut gui.recipient_selection;
         let contacts_ui = &mut gui.settings.contacts_ui;

         zeus.write(|zeus| {
            crate::gui::ui::common::recipient_field(
               zeus,
               &theme,
               icons,
               picker,
               contacts_ui,
               false,
               ChainId::Ethereum.id(),
               ChainId::Ethereum,
               ui,
            )
         });
      });
   })
}

/// The picker is drawn before the field's own button — the form reads the recipient the picker
/// wrote before it draws the row — so the keyboard it hands back has to wait for the dialog layer
/// to clear: on the frame after the pick the button's own registration would withdraw it again.
#[test]
fn picking_a_recipient_returns_the_focus_to_the_field_it_was_opened_from() {
   let _turn = shared_gui_turn();
   let ctx = Context::default();

   seed_contacts(&[(PICKER_CONTACT, PICKER_CONTACT_ADDRESS)]);

   crate::gui::SHARED_GUI.write(|gui| {
      gui.recipient_selection.reset();
      // The shared surface keeps the tab another test left open; this one opens on Contacts.
      gui.recipient_selection.reset_tab_for_test();
      gui.recipient_selection.close();
   });

   let (_, opener) = render_recipient_from_field(&ctx, Vec::new(), false);

   ctx.memory_mut(|memory| memory.request_focus(opener));
   render_recipient_from_field(&ctx, Vec::new(), false);

   assert_eq!(
      focused_id(&ctx),
      Some(opener.accesskit_id()),
      "the stand-in button holds the keyboard the picker is opened from"
   );

   render_recipient_from_field(&ctx, vec![key_press(Key::Enter)], true);

   let name = contact_row_name(PICKER_CONTACT, PICKER_CONTACT_SPOKEN);
   let mut row = None;

   for _ in 0..300 {
      let (nodes, _) = render_recipient_from_field(&ctx, Vec::new(), true);

      row = nodes
         .iter()
         .find(|(_, node)| node.role() == Role::Button && node.label() == Some(name.as_str()))
         .map(|(id, _)| *id);

      if row.is_some() {
         break;
      }

      std::thread::sleep(std::time::Duration::from_millis(10));
   }

   let row = row.expect("the picker must draw its contact list");

   render_recipient_from_field(
      &ctx,
      vec![accesskit_action(Action::Focus, row)],
      true,
   );
   render_recipient_from_field(&ctx, vec![key_press(Key::Enter)], true);

   // The hand-back waits for the dialog layer to clear, so it lands two frames after the pick.
   render_recipient_from_field(&ctx, Vec::new(), true);
   render_recipient_from_field(&ctx, Vec::new(), true);

   assert_eq!(
      focused_id(&ctx),
      Some(opener.accesskit_id()),
      "a pick must hand the keyboard back to the field it was opened from"
   );
}

/// The address field gives its focus up on the Enter that opens the picker, so the picker cannot
/// read its home off the keyboard the way a button's can. The pick still has to end on the field
/// it was opened from: a focus lost here sends the next Tab back to the top of the window.
#[test]
fn a_pick_from_the_address_field_returns_the_keyboard_to_it() {
   let _turn = shared_gui_turn();
   let ctx = Context::default();

   seed_contacts(&[(PICKER_CONTACT, PICKER_CONTACT_ADDRESS)]);

   crate::gui::SHARED_GUI.write(|gui| {
      gui.recipient_selection.reset();
      gui.recipient_selection.reset_tab_for_test();
      gui.recipient_selection.close();
   });

   let nodes = render_recipient_field(&ctx, Vec::new());
   let field = nodes
      .iter()
      .find(|(_, node)| is_text_input(node.role()))
      .map(|(id, _)| *id)
      .expect("the row must draw the address field");

   render_recipient_field(&ctx, vec![accesskit_action(Action::Focus, field)]);
   assert_eq!(
      focused_id(&ctx),
      Some(field),
      "the address field takes the keyboard"
   );

   render_recipient_field(&ctx, vec![key_press(Key::Enter)]);
   assert!(
      recipient_picker_open(),
      "Enter on the field opens the picker"
   );

   let name = contact_row_name(PICKER_CONTACT, PICKER_CONTACT_SPOKEN);
   let mut row = None;

   for _ in 0..300 {
      let nodes = render_recipient_field(&ctx, Vec::new());

      row = nodes
         .iter()
         .find(|(_, node)| node.role() == Role::Button && node.label() == Some(name.as_str()))
         .map(|(id, _)| *id);

      if row.is_some() {
         break;
      }

      std::thread::sleep(Duration::from_millis(10));
   }

   let row = row.expect("the picker must draw its contact list");

   render_recipient_field(&ctx, vec![accesskit_action(Action::Focus, row)]);
   render_recipient_field(&ctx, vec![key_press(Key::Enter)]);

   assert_eq!(
      picked_recipient_address(),
      PICKER_CONTACT_ADDRESS,
      "Enter on the focused row picks that contact"
   );

   // The hand-back waits for the dialog layer to clear, so it lands two frames after the pick.
   render_recipient_field(&ctx, Vec::new());
   render_recipient_field(&ctx, Vec::new());

   assert_eq!(
      focused_id(&ctx),
      Some(field),
      "a pick must give the keyboard back to the address field it was opened from"
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

/// Copying a recipient contact's address changes nothing on screen, so the picker hands the
/// sentence to the frame, which says it once.
#[test]
fn a_recipient_contact_copy_says_it_happened() {
   let _turn = shared_gui_turn();
   let ctx = Context::default();

   let nodes = open_recipient_picker(
      &ctx,
      false,
      &[(PICKER_CONTACT, PICKER_CONTACT_ADDRESS)],
   );

   let copy_name = format!("Copy address of {PICKER_CONTACT}");
   let copy = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some(copy_name.as_str()))
      .map(|(id, _)| *id)
      .expect("the row's address button must say what it copies");

   crate::gui::SHARED_GUI.write(|gui| gui.recipient_selection.take_notice());

   render_recipient_picker(
      &ctx,
      false,
      vec![accesskit_action(Action::Click, copy)],
   );

   assert_eq!(
      crate::gui::SHARED_GUI
         .write(|gui| gui.recipient_selection.take_notice())
         .as_deref(),
      Some(copy_said(PICKER_CONTACT_ADDRESS).as_str()),
      "the copy must reach the frame's announcement channel, naming the address"
   );

   render_recipient_picker(&ctx, false, Vec::new());

   assert_eq!(
      crate::gui::SHARED_GUI.write(|gui| gui.recipient_selection.take_notice()),
      None,
      "the copy is said once, so the next frame says nothing more"
   );
}

/// The same for a wallet row: its address copies like a contact's, and the picker says so once.
#[test]
fn a_recipient_wallet_copy_says_it_happened() {
   let _turn = shared_gui_turn();
   let ctx = Context::default();

   let nodes = open_recipient_picker(&ctx, false, &[]);

   let wallets = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Tab && node.label() == Some("Wallets"))
      .map(|(id, _)| *id)
      .expect("the picker must offer a Wallets tab");

   render_recipient_picker(
      &ctx,
      false,
      vec![accesskit_action(Action::Click, wallets)],
   );

   let wallet = master_wallet("Main");
   let copy_name = format!(
      "Copy address of {}",
      crate::gui::ui::common::spoken_wallet_name(&wallet)
   );

   let said = copy_said(&wallet.address.to_string());

   crate::gui::SHARED_GUI.write(|gui| {
      gui.recipient_selection.set_wallets_for_test(vec![wallet]);
      gui.recipient_selection.take_notice();
   });

   let nodes = render_recipient_picker(&ctx, false, Vec::new());

   let copy = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some(copy_name.as_str()))
      .map(|(id, _)| *id)
      .unwrap_or_else(|| {
         panic!(
            "the wallet row's address button must be in the tree: {:?}",
            drawn_names(&nodes)
         )
      });

   render_recipient_picker(
      &ctx,
      false,
      vec![accesskit_action(Action::Click, copy)],
   );

   assert_eq!(
      crate::gui::SHARED_GUI
         .write(|gui| gui.recipient_selection.take_notice())
         .as_deref(),
      Some(said.as_str()),
      "the copy must reach the frame's announcement channel, naming the address"
   );

   render_recipient_picker(&ctx, false, Vec::new());

   assert_eq!(
      crate::gui::SHARED_GUI.write(|gui| gui.recipient_selection.take_notice()),
      None,
      "the copy is said once, so the next frame says nothing more"
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
