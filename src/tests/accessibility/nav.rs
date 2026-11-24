use super::all::{press_shortcut, render_shortcuts};
use super::app::{main_window, render_central, render_nav, wallet_in_cache};
use super::harness::{
   accesskit_action, drawn_names, focused_node, frame_nodes, is_text_input, key_press,
};
use egui::accesskit::{Action, Node, Role, Toggled};
use egui::{Key, Modifiers};

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

   let shield = nodes
      .iter()
      .find(|node| node.role() == Role::Button && node.label() == Some("Shield"))
      .expect("the Shield nav entry must be in the tree");

   assert_eq!(
      shield.description(),
      Some("Deposit into the private pool"),
      "the Shield plate must say what opening the private pool does"
   );

   // Privacy mode turns the same plate into Unshield, which says what leaving does.
   zeus.write(|ctx| ctx.privacy_mode = true);

   let nodes: Vec<Node> = render_nav(&ctx, &mut gui, &zeus, Vec::new())
      .into_iter()
      .map(|(_, node)| node)
      .collect();

   let unshield = nodes
      .iter()
      .find(|node| node.role() == Role::Button && node.label() == Some("Unshield"))
      .expect("the Unshield nav entry must be in the tree in privacy mode");

   assert_eq!(
      unshield.description(),
      Some("Withdraw from the private pool"),
      "the Unshield plate must say what leaving the private pool does"
   );
}

/// Every connected app draws the same Disconnect word, so each button is named after the app it
/// disconnects, and the origin field puts the origin where a reader can hear it.
#[test]
fn the_connected_dapps_rows_name_their_disconnect_button_and_origin() {
   const ORIGIN: &str = "https://app.example";

   let (ctx, mut gui, zeus) = main_window();
   let theme = gui.theme.clone();
   let icons = gui.icons.clone();

   zeus.connect_dapp(ORIGIN.to_string());
   gui.connected_dapps.open();

   let mut nodes = Vec::new();

   // A modal spends its first passes measuring, and egui exposes no node for a widget it
   // considers invisible.
   for _ in 0..3 {
      nodes = frame_nodes(&ctx, Vec::new(), |ui| {
         zeus.write(|zeus| gui.connected_dapps.show(zeus, &theme, icons.clone(), ui));
      });
   }

   let name = format!("Disconnect {ORIGIN}");

   assert!(
      nodes
         .iter()
         .any(|(_, node)| node.role() == Role::Button && node.label() == Some(name.as_str())),
      "each row's Disconnect button must be named after the app it disconnects: {:?}",
      drawn_names(&nodes)
   );

   let origin = nodes
      .iter()
      .find(|(_, node)| is_text_input(node.role()))
      .map(|(_, node)| node)
      .unwrap_or_else(|| {
         panic!(
            "the row must draw its origin field: {:?}",
            drawn_names(&nodes)
         )
      });

   assert_eq!(
      origin.value(),
      Some(ORIGIN),
      "the origin field must carry the origin as its value, not leave an empty box to a reader"
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

/// A reader who picks the page already in front is still asking for that page: the keyboard lands
/// on the page, not on the plate that was pressed. Without that the next Tab walks the rest of the
/// side menu while the page the reader asked for sits unfocused.
#[test]
fn choosing_the_page_that_is_already_open_lands_on_it() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| zeus.vault_unlocked = true);
   wallet_in_cache(&zeus);

   render_shortcuts(&ctx, &mut gui, &zeus, Vec::new());
   press_shortcut(&ctx, &mut gui, &zeus, Key::Num2, Modifiers::ALT);

   let nodes = render_shortcuts(&ctx, &mut gui, &zeus, Vec::new());
   let plate = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some("Send"))
      .map(|(id, _)| *id)
      .expect("the nav must draw the plate of the page in front");

   render_shortcuts(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Focus, plate)],
   );

   let nodes = render_shortcuts(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, plate)],
   );
   let focused = focused_node(&ctx, &nodes);

   assert_eq!(
      focused.role(),
      Role::Heading,
      "selecting the open page must put the keyboard on the page, not leave it on the plate"
   );

   assert_eq!(focused.label(), Some("Send"));
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
