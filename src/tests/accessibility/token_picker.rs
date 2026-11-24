use super::app::shared_gui_turn;
use super::harness::{
   accesskit_action, focused_id, frame_nodes, is_text_input, key_combo, key_press, named,
   node_name, parent_of,
};
use egui::accesskit::{Action, Node, NodeId, Role, Toggled};
use egui::{Context, Event, Id, Key, Modifiers, PointerButton, pos2};
use zeus_eth::alloy_primitives::Address;
use zeus_eth::currency::{Currency, ERC20Token, NativeCurrency};
use zeus_eth::utils::NumericValue;

/// The USDC row's name: the full name Zeus knows for the token, and the balance the row draws
/// beside it.
const USDC_ROW: &str = "USD Coin, 12.50";

/// What the menu button must say it opens.
const USDC_OPTIONS: &str = "Options for USD Coin, 12.50";

/// Two rows for the picker: a USDC balance worth a figure, and the empty native currency, which
/// draws no balance and so says nothing about one.
fn picker_rows() -> Vec<(Currency, NumericValue, NumericValue)> {
   use zeus_eth::alloy_primitives::U256;

   let usdc = ERC20Token::from_components(
      1,
      Address::from([0xaa; 20]),
      "USDC",
      "USD Coin",
      6,
      U256::ZERO,
   );

   vec![
      (
         Currency::from(usdc),
         NumericValue::from_f64(12.5),
         NumericValue::from_f64(31.25),
      ),
      (
         Currency::from(NativeCurrency::eth()),
         NumericValue::from_f64(0.0),
         NumericValue::from_f64(0.0),
      ),
   ]
}

/// Draw the picker for one frame, out of the `SHARED_GUI` the frame gives it.
///
/// The list a real opening shows is fetched on a worker that writes it — and the end of the loading
/// state — back into that same global, so a `GUI` of the test's own would draw its spinner for ever.
/// The rows are seeded here instead, in the same exclusive step that draws them, so nothing else can
/// seed over them between the two.
fn render_picker(
   ctx: &Context,
   rows: &[(Currency, NumericValue, NumericValue)],
   events: Vec<Event>,
) -> Vec<(NodeId, Node)> {
   let rows = rows.to_vec();

   frame_nodes(ctx, events, |ui| {
      crate::gui::SHARED_GUI.write(|gui| {
         gui.token_selection.set_processed_currencies(rows.clone());

         let theme = gui.theme.clone();
         let icons = gui.icons.clone();
         let zeus = gui.ctx.clone();
         let picker = &mut gui.token_selection;

         zeus.write(|zeus| {
            picker.show(
               zeus,
               &theme,
               icons,
               1,
               Address::from([0x00; 20]),
               ui,
            )
         });
      });
   })
}

/// Open the picker on `rows` and draw it until the list is on screen.
///
/// The opening starts a balance fetch, and that fetch's writeback is what ends the loading state the
/// picker begins in: the first frames draw a spinner and nothing else. A search box in the tree is
/// the frame where the list itself is drawn.
fn open_picker(
   ctx: &Context,
   rows: &[(Currency, NumericValue, NumericValue)],
) -> Vec<(NodeId, Node)> {
   crate::gui::SHARED_GUI.write(|gui| {
      let mut theme = gui.theme.clone();
      theme.install(ctx);
      // The picker keeps its query across openings, so an earlier opening's search would decide
      // what this one lists.
      gui.token_selection.search_query.clear();
      gui.token_selection.open(false, 1, Address::from([0x00; 20]));
   });

   for _ in 0..300 {
      let nodes = render_picker(ctx, rows, Vec::new());

      if nodes.iter().any(|(_, node)| is_text_input(node.role())) {
         return nodes;
      }

      std::thread::sleep(std::time::Duration::from_millis(10));
   }

   panic!("the picker never finished loading its list");
}

/// The token the picker holds as its selection, by symbol.
fn picked_symbol() -> Option<String> {
   crate::gui::SHARED_GUI
      .write(|gui| gui.token_selection.get_selected_currency().cloned())
      .map(|currency| currency.symbol().to_string())
}

/// A list is a set, not a bag: each row says its zero-based place and the container egui made for
/// the rows says how many the list holds. The token picker's rows sit directly under the
/// `ScrollArea`'s own `Ui`, so both properties land on real nodes.
#[test]
fn the_token_picker_says_each_rows_place_and_the_lists_size() {
   let _turn = shared_gui_turn();
   let ctx = Context::default();
   let nodes = open_picker(&ctx, &picker_rows());

   for (name, position) in [(USDC_ROW, 0usize), ("Ethereum", 1)] {
      let (row, node) = nodes
         .iter()
         .find(|(_, node)| node.role() == Role::Button && node.label() == Some(name))
         .map(|(id, node)| (*id, node))
         .unwrap_or_else(|| panic!("the {name} row must be in the tree"));

      assert_eq!(
         node.position_in_set(),
         Some(position),
         "the {name} row must say its zero-based place"
      );

      let (container, container_node) = parent_of(&nodes, row);

      assert_eq!(
         container_node.role(),
         Role::GenericContainer,
         "the rows' container is the ScrollArea Ui, not a table"
      );

      assert_eq!(
         container_node.size_of_set(),
         Some(2),
         "the container {container:?} must say how many rows the list holds"
      );
   }
}

/// Tab from the search field lands on the first token's row, not on the options button inside it:
/// the row is the control that picks a token, and its options come after it. Enter on the row takes
/// that token.
#[test]
fn tab_reaches_a_token_row_before_its_options_and_enter_picks_it() {
   let _turn = shared_gui_turn();
   let ctx = Context::default();
   let rows = picker_rows();

   let nodes = open_picker(&ctx, &rows);

   let field = nodes
      .iter()
      .find(|(_, node)| is_text_input(node.role()))
      .map(|(id, _)| *id)
      .expect("the picker must draw its search field");

   render_picker(
      &ctx,
      &rows,
      vec![accesskit_action(Action::Focus, field)],
   );

   assert_eq!(
      focused_id(&ctx),
      Some(field),
      "the search field takes the focus the test gives it"
   );

   let nodes = render_picker(&ctx, &rows, vec![key_press(Key::Tab)]);
   let focused = focused_id(&ctx);

   let row = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some(USDC_ROW))
      .map(|(id, _)| *id)
      .expect("the USDC row must be in the tree");

   assert_eq!(
      focused,
      Some(row),
      "Tab from the search field must reach the row itself, not the options button inside it: \
       {:?}",
      named(&nodes, focused)
   );

   let nodes = render_picker(&ctx, &rows, vec![key_press(Key::Tab)]);
   let focused = focused_id(&ctx);

   let options = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some(USDC_OPTIONS))
      .map(|(id, _)| *id)
      .expect("the USDC options button must be in the tree");

   assert_eq!(
      focused,
      Some(options),
      "the row's options are the control after the row, not before it: {:?}",
      named(&nodes, focused)
   );

   // The two are neighbours, so Shift+Tab walks back onto the row. A backwards move is handed to
   // the next frame, so the press and the frame after it are both part of the same step.
   render_picker(
      &ctx,
      &rows,
      vec![key_combo(Key::Tab, Modifiers::SHIFT)],
   );
   let nodes = render_picker(&ctx, &rows, Vec::new());

   assert_eq!(
      focused_id(&ctx),
      Some(row),
      "Shift+Tab from the options must land back on the row: {:?}",
      named(&nodes, focused_id(&ctx))
   );

   // The row takes the keyboard back, and Enter picks it.
   render_picker(
      &ctx,
      &rows,
      vec![accesskit_action(Action::Focus, row)],
   );
   render_picker(&ctx, &rows, vec![key_press(Key::Enter)]);

   assert_eq!(
      picked_symbol().as_deref(),
      Some("USDC"),
      "Enter on the focused row must pick that token"
   );

   assert!(
      !crate::gui::SHARED_GUI.read(|gui| gui.token_selection.is_open()),
      "picking a token closes the picker"
   );
}

/// Draw a stand-in for the token button these pickers are opened from, then the picker itself.
///
/// The button is drawn first, the way a form draws its own token row before the frame draws the
/// picker, and `open` lets the click handler open the picker on the frame the button is used.
/// Returns the button's id: the control the keyboard comes from and has to go back to.
fn render_picker_from_button(
   ctx: &Context,
   rows: &[(Currency, NumericValue, NumericValue)],
   events: Vec<Event>,
   open: bool,
) -> (Vec<(NodeId, Node)>, Id) {
   let rows = rows.to_vec();
   let mut opener = None;

   let nodes = frame_nodes(ctx, events, |ui| {
      let button = ui.add(egui::Button::new("Select token"));
      opener = Some(button.id);

      let clicked = button.clicked();

      crate::gui::SHARED_GUI.write(|gui| {
         if open && clicked {
            gui.token_selection.open(false, 1, Address::from([0x00; 20]));
         }

         gui.token_selection.set_processed_currencies(rows.clone());

         let theme = gui.theme.clone();
         let icons = gui.icons.clone();
         let zeus = gui.ctx.clone();
         let picker = &mut gui.token_selection;

         zeus.write(|zeus| {
            picker.show(
               zeus,
               &theme,
               icons,
               1,
               Address::from([0x00; 20]),
               ui,
            )
         });
      });
   });

   (
      nodes,
      opener.expect("the button must be in the frame"),
   )
}

/// A pick closes the picker from inside its own body, and the keyboard goes back to the control it
/// came from. Without that the focus is left on nothing, so the next Tab starts at the top of the
/// window — the account panel's own tabs — instead of where the user was.
#[test]
fn picking_a_token_returns_the_focus_to_the_control_that_opened_the_picker() {
   let _turn = shared_gui_turn();
   let ctx = Context::default();
   let rows = picker_rows();

   // The shared picker holds whatever another test left it with; this one starts from closed.
   crate::gui::SHARED_GUI.write(|gui| {
      gui.token_selection.close();
      gui.token_selection.search_query.clear();
   });

   let (_, opener) = render_picker_from_button(&ctx, &rows, Vec::new(), false);

   ctx.memory_mut(|memory| memory.request_focus(opener));
   render_picker_from_button(&ctx, &rows, Vec::new(), false);

   assert_eq!(
      focused_id(&ctx),
      Some(opener.accesskit_id()),
      "the stand-in button holds the keyboard the picker is opened from"
   );

   // Enter on the button opens the picker, the way the form's own handler does.
   render_picker_from_button(&ctx, &rows, vec![key_press(Key::Enter)], true);

   // The list lands a frame or two after the opening, because it is fetched.
   let mut row = None;

   for _ in 0..300 {
      let (nodes, _) = render_picker_from_button(&ctx, &rows, Vec::new(), true);

      row = nodes
         .iter()
         .find(|(_, node)| node.role() == Role::Button && node.label() == Some(USDC_ROW))
         .map(|(id, _)| *id);

      if row.is_some() {
         break;
      }

      std::thread::sleep(std::time::Duration::from_millis(10));
   }

   let row = row.expect("the picker must draw its list");

   render_picker_from_button(
      &ctx,
      &rows,
      vec![accesskit_action(Action::Focus, row)],
      true,
   );
   render_picker_from_button(&ctx, &rows, vec![key_press(Key::Enter)], true);

   // A picker closed from inside hands the keyboard back on the first pass the dialog layer no
   // longer covers, which is the second frame after the pick.
   render_picker_from_button(&ctx, &rows, Vec::new(), true);
   render_picker_from_button(&ctx, &rows, Vec::new(), true);

   assert_eq!(
      focused_id(&ctx),
      Some(opener.accesskit_id()),
      "a pick must hand the keyboard back to the control the picker was opened from"
   );
}

/// A picker row used to be a frame with a click sense bolted on: a hit target with no role, no name
/// and no way in from a reader. The row is one named button now, and an AccessKit Click on its node
/// is the same selection hook a mouse click is.
#[test]
fn a_token_row_can_be_picked_by_a_reader() {
   let _turn = shared_gui_turn();
   let ctx = Context::default();
   let rows = picker_rows();

   let nodes = open_picker(&ctx, &rows);

   let (row, node) = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some(USDC_ROW))
      .expect("the USDC row must be one button named after what it holds");

   assert!(
      node.supports_action(Action::Click),
      "the row has to answer a Click request, or a reader cannot activate it"
   );

   assert!(
      node.supports_action(Action::Focus),
      "the row has to be reachable by keyboard as well"
   );

   assert_eq!(
      node.description(),
      Some("$31.25"),
      "the fiat value the row draws is its description"
   );

   assert_eq!(
      picked_symbol(),
      None,
      "nothing is picked before the row is used"
   );

   render_picker(
      &ctx,
      &rows,
      vec![accesskit_action(Action::Click, *row)],
   );

   assert_eq!(
      picked_symbol().as_deref(),
      Some("USDC"),
      "an AccessKit Click on the row must pick that token"
   );
}

/// The row is a hit target as well as a button: a mouse click anywhere on it still picks the token,
/// which is the path that existed before it had a name.
#[test]
fn a_token_row_still_answers_a_mouse_click() {
   let _turn = shared_gui_turn();
   let ctx = Context::default();
   let rows = picker_rows();

   let nodes = open_picker(&ctx, &rows);

   let bounds = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some(USDC_ROW))
      .and_then(|(_, node)| node.bounds())
      .expect("the row is drawn, so its node has bounds");

   let pos = pos2(
      ((bounds.x0 + bounds.x1) / 2.0) as f32,
      ((bounds.y0 + bounds.y1) / 2.0) as f32,
   );

   let button = |pressed| Event::PointerButton {
      pos,
      button: PointerButton::Primary,
      pressed,
      modifiers: Modifiers::NONE,
   };

   render_picker(&ctx, &rows, vec![button(true)]);
   render_picker(&ctx, &rows, vec![button(false)]);

   assert_eq!(
      picked_symbol().as_deref(),
      Some("USDC"),
      "a mouse click on the row must still pick that token"
   );
}

/// The keyboard path: the row is focusable, a reader's Focus request lands on it, and Enter on it
/// picks the token. The focus has to survive the frame that draws the row — the plate registers
/// itself with an empty sense first, which withdraws it — or Space and Enter would never reach it.
#[test]
fn a_token_row_takes_focus_and_enter_picks_it() {
   let _turn = shared_gui_turn();
   let ctx = Context::default();
   let rows = picker_rows();

   let nodes = open_picker(&ctx, &rows);

   let row = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some(USDC_ROW))
      .map(|(id, _)| *id)
      .expect("the USDC row must be in the tree");

   render_picker(
      &ctx,
      &rows,
      vec![accesskit_action(Action::Focus, row)],
   );

   let focused = ctx.memory(|memory| memory.focused());

   assert_eq!(
      focused.map(|id| id.accesskit_id()),
      Some(row),
      "the row must take the focus a reader asked for, and hold it after the frame drew it"
   );

   render_picker(&ctx, &rows, vec![key_press(Key::Enter)]);

   assert_eq!(
      picked_symbol().as_deref(),
      Some("USDC"),
      "Enter on the focused row must pick that token"
   );
}

/// The row says the token and its balance itself, so the labels inside it are hidden: a reader hears
/// the row once, instead of the row and then "USD Coin USDC", "31.25" and "12.50" after it.
#[test]
fn the_picker_row_is_read_once() {
   let _turn = shared_gui_turn();
   let ctx = Context::default();

   let nodes = open_picker(&ctx, &picker_rows());

   let spoken: Vec<&str> = nodes
      .iter()
      .filter(|(_, node)| !node.is_hidden())
      .filter_map(|(_, node)| node.label().or_else(|| node.value()))
      .collect();

   for covered in ["USD Coin\nUSDC", "$31.25", "12.50"] {
      assert!(
         !spoken.contains(&covered),
         "the row's own name already says {covered:?}, so it is not a node of its own: {spoken:?}"
      );
   }

   assert!(
      spoken.contains(&USDC_ROW),
      "the row's one node carries the words those labels drew: {spoken:?}"
   );
}

/// A row says whether it is the one the picker points at, so a reader can tell where the surface
/// stands without being told the whole list.
#[test]
fn the_picker_row_says_which_token_is_selected() {
   let _turn = shared_gui_turn();
   let ctx = Context::default();
   let rows = picker_rows();

   open_picker(&ctx, &rows);

   crate::gui::SHARED_GUI.write(|gui| {
      gui.token_selection.selected_currency = Some(rows[0].0.clone());
   });

   let nodes = render_picker(&ctx, &rows, Vec::new());

   let selected = |name: &str| {
      nodes
         .iter()
         .find(|(_, node)| node.role() == Role::Button && node.label() == Some(name))
         .map(|(_, node)| node.toggled())
         .expect("the row must be a button in the tree")
   };

   assert_eq!(
      selected(USDC_ROW),
      Some(Toggled::True),
      "the row the picker holds is the selected one"
   );

   assert_eq!(
      selected("Ethereum"),
      Some(Toggled::False),
      "every other row says it is not the selection"
   );

   let eth = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some("Ethereum"))
      .map(|(_, node)| node)
      .expect("the empty native row must be in the tree too");

   assert_eq!(
      eth.description(),
      None,
      "a token with no balance draws no fiat value, so it has none to describe"
   );
}

/// The "..." button drew three dots and said nothing: its accessible name was the empty string. It
/// names the row it opens now, and the search box — which painted only a hint — names itself too.
#[test]
fn the_picker_names_its_menu_button_and_its_search_field() {
   let _turn = shared_gui_turn();
   let ctx = Context::default();
   let rows = picker_rows();

   let nodes = open_picker(&ctx, &rows);

   assert!(
      nodes
         .iter()
         .any(|(_, node)| node.role() == Role::Button && node.label() == Some(USDC_OPTIONS)),
      "the menu button must say whose options it opens"
   );

   let search = nodes
      .iter()
      .find(|(_, node)| is_text_input(node.role()))
      .map(|(_, node)| node)
      .expect("the picker must draw its search field");

   assert_eq!(
      node_name(&nodes, search).as_deref(),
      Some("Search tokens"),
      "the field is named for the list it narrows"
   );

   assert_eq!(
      search.description(),
      Some("Search tokens or enter an address"),
      "the hint the field paints is its description"
   );

   crate::gui::SHARED_GUI.write(|gui| gui.token_selection.search_query = "usd".to_string());

   let nodes = render_picker(&ctx, &rows, Vec::new());

   let search = nodes
      .iter()
      .find(|(_, node)| is_text_input(node.role()))
      .map(|(_, node)| node)
      .expect("the picker must still draw its search field");

   assert_eq!(
      search.value(),
      Some("usd"),
      "a search box is not a secret, so what it holds stays its value"
   );
}

/// The announcement rule the picker follows: the count of rows the search left is a sentence when it
/// appears or changes, never for the same list twice, and an empty list says so in words.
#[test]
fn the_picker_says_how_many_rows_the_search_left() {
   use crate::gui::ui::token_selection::{PickerMode, count_announcement};

   for (previous, mode, count, expected) in [
      (None, PickerMode::Fungible, 2, Some("2 tokens")),
      (Some("2 tokens"), PickerMode::Fungible, 2, None),
      (
         Some("2 tokens"),
         PickerMode::Fungible,
         1,
         Some("1 token"),
      ),
      (
         Some("1 token"),
         PickerMode::Fungible,
         0,
         Some("No tokens found"),
      ),
      (None, PickerMode::Nft, 3, Some("3 NFTs")),
      (
         Some("3 NFTs"),
         PickerMode::Nft,
         0,
         Some("No NFTs found"),
      ),
   ] {
      assert_eq!(
         count_announcement(previous, mode, count).as_deref(),
         expected,
         "{previous:?} then {count} rows of {mode:?}"
      );
   }
}

/// The picker has no announcement channel of its own, so it hands its sentence to the frame the way
/// the Send form and the portfolio do. The list it opened on is said once, and the frame that draws
/// the same list again says nothing.
#[test]
fn the_picker_hands_its_count_to_the_frame_once() {
   let _turn = shared_gui_turn();
   let ctx = Context::default();
   let rows = picker_rows();

   open_picker(&ctx, &rows);

   assert_eq!(
      crate::gui::SHARED_GUI.write(|gui| gui.token_selection.take_notice()).as_deref(),
      Some("2 tokens"),
      "the list the picker opened on is said once"
   );

   render_picker(&ctx, &rows, Vec::new());

   assert_eq!(
      crate::gui::SHARED_GUI.write(|gui| gui.token_selection.take_notice()),
      None,
      "the same list a frame later is not a new answer"
   );

   crate::gui::SHARED_GUI.write(|gui| gui.token_selection.search_query = "usdc".to_string());
   render_picker(&ctx, &rows, Vec::new());

   assert_eq!(
      crate::gui::SHARED_GUI.write(|gui| gui.token_selection.take_notice()).as_deref(),
      Some("1 token"),
      "a narrowed list is a new count"
   );

   crate::gui::SHARED_GUI.write(|gui| gui.token_selection.search_query = "zzz".to_string());

   let nodes = render_picker(&ctx, &rows, Vec::new());

   assert_eq!(
      crate::gui::SHARED_GUI.write(|gui| gui.token_selection.take_notice()).as_deref(),
      Some("No tokens found"),
      "an emptied list says so in words"
   );

   assert!(
      !nodes
         .iter()
         .any(|(_, node)| node.role() == Role::Button && node.label() == Some(USDC_ROW)),
      "the emptied list draws no rows"
   );
}
