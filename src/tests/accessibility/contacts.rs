use super::app::{open_settings_page, render_settings, render_settings_speaking, settings_window};
use super::harness::{
   accesskit_action, announced, button_named, copy_said, drawn_names, focused_node, is_text_input,
   node_name, node_texts, parent_of,
};
use crate::core::types::Contact;
use egui::accesskit::{Action, Role};

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

/// Copying a contact row's address changes nothing on screen, so the row hands the sentence — the
/// fact and the address — to the page, and the Settings window says it once.
#[test]
fn a_contact_row_copy_says_it_happened() {
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

   let copy_name = format!("Copy address {address}");
   let copy = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some(copy_name.as_str()))
      .map(|(id, _)| *id)
      .unwrap_or_else(|| {
         panic!(
            "the contact row's address button must be in the tree: {:?}",
            drawn_names(&nodes)
         )
      });

   let nodes = render_settings_speaking(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, copy)],
   );

   assert_eq!(
      announced(&nodes).as_deref(),
      Some(copy_said(address).as_str()),
      "the copy must reach the announcement node the Settings window feeds, naming the address"
   );

   let nodes = render_settings_speaking(&ctx, &mut gui, &zeus, Vec::new());

   assert_eq!(
      announced(&nodes).as_deref().unwrap_or_default(),
      "",
      "the copy is said once, so the next frame says nothing more"
   );
}

/// The contacts page wraps its rows in a `Frame`, so the rows' container is that frame's own
/// `Ui` node. The place and the size still land on the nodes that carry the list.
#[test]
fn a_contact_row_is_placed_in_the_contacts_page_list() {
   let (ctx, mut gui, zeus) = settings_window();

   for (name, address) in [
      (
         "Ada Lovelace",
         "0x1111111111111111111111111111111111111111",
      ),
      (
         "Grace Hopper",
         "0x2222222222222222222222222222222222222222",
      ),
   ] {
      zeus.write_wallet_state(|ws| {
         ws.contacts.push(Contact::new(
            name.to_string(),
            address.to_string(),
            String::new(),
         ));
      });
   }

   let nodes = open_settings_page(&ctx, &mut gui, &zeus, "Contacts");

   for (name, position) in [("Ada Lovelace", 0usize), ("Grace Hopper", 1)] {
      let (row, node) = nodes
         .iter()
         .find(|(_, node)| node.role() == Role::Group && node.label() == Some(name))
         .map(|(id, node)| (*id, node))
         .unwrap_or_else(|| panic!("the {name} row must be in the tree"));

      assert_eq!(
         node.position_in_set(),
         Some(position),
         "the {name} row must say its zero-based place"
      );

      let (_, container_node) = parent_of(&nodes, row);

      assert_eq!(
         container_node.size_of_set(),
         Some(2),
         "the contacts frame must say how many rows the page lists"
      );
   }
}

/// The contacts rows' options menu closes when an item is chosen.
///
/// The explorer item leaves the page as it was, so nothing else takes the menu away: without the
/// close it stays over the card it was chosen from.
#[test]
fn the_contacts_menu_closes_when_an_item_is_chosen() {
   let (ctx, mut gui, zeus) = settings_window();

   zeus.write_wallet_state(|ws| {
      ws.contacts.push(Contact::new(
         "Ada Lovelace".to_string(),
         "0x1111111111111111111111111111111111111111".to_string(),
         String::new(),
      ));
   });

   let nodes = open_settings_page(&ctx, &mut gui, &zeus, "Contacts");
   let options = button_named(&nodes, "Options for Ada Lovelace");

   render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, options)],
   );
   let nodes = render_settings(&ctx, &mut gui, &zeus, Vec::new());
   // The explorer item is the one that leaves the page as it was: the other three switch the view,
   // which removes the button the menu hangs from and takes the menu with it.
   let explorer = button_named(&nodes, "See on Block Explorer");

   render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, explorer)],
   );
   let nodes = render_settings(&ctx, &mut gui, &zeus, Vec::new());

   for label in ["Edit", "Delete", "Show QR Code"] {
      assert!(
         !nodes
            .iter()
            .any(|(_, node)| node.role() == Role::Button && node.label() == Some(label)),
         "the menu must close once an item is chosen: {:?}",
         drawn_names(&nodes)
      );
   }
}

/// The contact QR page's own statement of which address is on screen is a heading a reader can
/// move to, and its copy button is named with the whole address. A click says the copy happened,
/// through the page's notice, which the Settings window drains.
#[test]
fn a_contact_qr_page_says_which_address_it_shows() {
   let address = "0x1111111111111111111111111111111111111111";
   let contact = Contact::new(
      "Ada Lovelace".to_string(),
      address.to_string(),
      String::new(),
   );

   let (ctx, mut gui, zeus) = settings_window();
   gui.settings.contacts_ui.show_contact_qr_for_test(contact);

   let nodes = open_settings_page(&ctx, &mut gui, &zeus, "Contacts");

   assert!(
      nodes
         .iter()
         .any(|(_, node)| node.role() == Role::Heading
            && node.label() == Some("Public Address (EVM)")),
      "the page must say which address is on screen: {:?}",
      drawn_names(&nodes)
   );

   let spoken = crate::gui::ui::address_for_speech(None, address);

   let button = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some(spoken.name.as_str()))
      .map(|(id, _)| *id)
      .unwrap_or_else(|| {
         panic!(
            "the address button must be named by the shortening the row draws: {:?}",
            drawn_names(&nodes)
         )
      });

   render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, button)],
   );

   assert_eq!(
      gui.settings.take_notice().as_deref(),
      Some(copy_said(address).as_str()),
      "the page hands the copy up to the window, which says it in the frame it was drawn"
   );

   let qr = nodes
      .iter()
      .find(|(_, node)| {
         node.role() == Role::Image
            && node.label() == Some("QR code of the address above, for another device to scan.")
      })
      .map(|(_, node)| node)
      .expect("the page's QR must be named with what it is for");

   assert!(
      !node_texts(qr).iter().any(|text| text.contains("bytes://")),
      "the QR must not read out its texture URI"
   );
}

/// Opening the add-contact form puts the focus on the field the user came to fill.
#[test]
fn the_add_contact_form_opens_on_the_name_field() {
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

   let focused = focused_node(&ctx, &nodes);

   assert_eq!(
      node_name(&nodes, focused).as_deref(),
      Some("Contact name"),
      "the form must open on the field it exists to fill: {:?}",
      focused.role()
   );
}

/// Back leaves the user on the list, at the button that opened the form.
#[test]
fn back_from_the_add_contact_form_gives_the_focus_back() {
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

   let back = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some("Back"))
      .map(|(id, _)| *id)
      .expect("the add form must offer its Back button");

   render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, back)],
   );

   let nodes = render_settings(&ctx, &mut gui, &zeus, Vec::new());
   let focused = focused_node(&ctx, &nodes);

   assert_eq!(
      focused.role(),
      Role::Button,
      "Back must put the focus on the button that opened the form: {:?}",
      focused.role()
   );
   assert_eq!(focused.label(), Some("Add Contact"));
}
