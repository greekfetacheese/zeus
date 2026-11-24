use super::super::app::{main_window, wallet_in_cache};
use super::super::harness::{accesskit_action, drawn_names, frame_nodes};
use super::super::tx::prompt::text_field;
use super::{render_uniswap_window, weth_usdc};
use crate::core::ZeusCtx;
use crate::gui::GUI;
use egui::accesskit::{Action, Node, NodeId, Role, Toggled};
use egui::{Context, Event};
use zeus_eth::currency::Currency;
use zeus_eth::types::ChainId;

/// The Uniswap pool page on its own, the way the window draws it.
///
/// More than once: the table measures itself against the width on the first pass, and the Add Pool
/// dialog is an overlay that is laid out before it is painted.
///
/// `events` go to the first pass only, so a click that opens a list is not delivered once per pass.
fn render_pools_page(
   ctx: &Context,
   gui: &mut GUI,
   zeus: &ZeusCtx,
   events: Vec<Event>,
) -> Vec<(NodeId, Node)> {
   let mut nodes = Vec::new();

   for pass in 0..3 {
      let events = if pass == 0 {
         events.clone()
      } else {
         Vec::new()
      };

      nodes = frame_nodes(ctx, events, |ui| {
         let icons = gui.icons.clone();
         let theme = gui.theme.clone();
         let pools = &mut gui.uniswap.pools_ui;

         zeus.write(|zeus| pools.show(zeus, &theme, icons, ui));
      });
   }

   nodes
}

/// Stand the pool table up with rows already built, keyed for the page the test is about to draw.
///
/// The real page asks a worker for its rows, and that worker writes into the global `GUI` rather than
/// into the one a test holds — the table would spin for ever. A keyed seed is the rows it would have
/// been handed.
fn seed_pool_rows(
   gui: &mut GUI,
   zeus: &ZeusCtx,
   rows: Vec<(Currency, Currency, &'static str, String)>,
) {
   let (chain, pools) = zeus.read(|ctx| {
      (
         ctx.chain.id(),
         ctx.pool_manager.read(|m| m.pools.len()),
      )
   });

   gui.uniswap.pools_ui.seed_rows_for_test(
      chain,
      pools,
      rows,
      &gui.icons,
      gui.theme.image_tint_recommended,
   );
}

/// The header's refresh control is a picture, so it is named for what it refreshes — and the spinner
/// that stands in its place while the pools and the quote are fetched says the same.
#[test]
fn the_uniswap_header_says_what_refresh_does() {
   let (ctx, mut gui, zeus) = main_window();
   wallet_in_cache(&zeus);
   gui.uniswap.open();

   let nodes = render_uniswap_window(&ctx, &mut gui, &zeus);

   assert!(
      nodes
         .iter()
         .any(|(_, node)| node.role() == Role::Button
            && node.label() == Some("Refresh pools and quote")),
      "the refresh button must be named: {:?}",
      drawn_names(&nodes)
   );

   gui.uniswap.swap_ui.syncing_pools = true;

   let nodes = render_uniswap_window(&ctx, &mut gui, &zeus);

   assert!(
      nodes.iter().any(|(_, node)| node.role() == Role::ProgressIndicator
         && node.label() == Some("Refreshing pools and quote")),
      "the spinner that replaces the button must say what is being fetched: {:?}",
      drawn_names(&nodes)
   );
}

/// One seeded row: the pair cell is one node named after both tokens — by name, not by the tickers
/// it draws — with the three words it draws left out of the reading, while the protocol, fee and
/// header cells say their own text.
#[test]
fn the_pools_table_names_each_row_once() {
   let (ctx, mut gui, zeus) = main_window();
   let (weth, usdc) = weth_usdc();

   seed_pool_rows(
      &mut gui,
      &zeus,
      vec![(weth, usdc, "V3", "0.05%".to_string())],
   );

   let nodes = render_pools_page(&ctx, &mut gui, &zeus, Vec::new());
   let spoken = drawn_names(&nodes);

   assert!(
      spoken.contains(&"Wrapped Ether / USD Coin"),
      "the pair cell must be one node named after both tokens: {spoken:?}"
   );

   for text in ["WETH", "/", "USDC"] {
      assert!(
         !nodes.iter().any(|(_, node)| node.role() == Role::Label
            && !node.is_hidden()
            && node.value() == Some(text)),
         "the {text:?} the pair cell draws must leave the tree: {spoken:?}"
      );

      assert!(
         nodes.iter().any(|(_, node)| node.role() == Role::Label
            && node.is_hidden()
            && node.value() == Some(text)),
         "the drawn {text:?} must be hidden rather than gone"
      );
   }

   // The protocol and the fee cells are plain labels, which already reach the tree with their words.
   for text in ["V3", "0.05%"] {
      assert!(
         nodes
            .iter()
            .any(|(_, node)| node.role() == Role::Label && node.value() == Some(text)),
         "the {text:?} cell must reach the tree: {spoken:?}"
      );
   }

   // So do the header words: they are drawn with plain labels, which arrive as a label's value, and
   // they need no name of their own.
   for text in ["Pool", "Protocol", "Fee"] {
      assert!(
         nodes
            .iter()
            .any(|(_, node)| node.role() == Role::Label && node.value() == Some(text)),
         "the {text:?} header must reach the tree as a label: {spoken:?}"
      );
   }
}

/// The Add Pool form names each field after the caption the grid draws beside it, and the version
/// picker after the caption above it: the placeholders stay hints.
#[test]
fn the_add_pool_form_names_its_fields() {
   let (ctx, mut gui, zeus) = main_window();

   seed_pool_rows(&mut gui, &zeus, Vec::new());
   gui.uniswap.pools_ui.open_add_pool_for_test();

   let nodes = render_pools_page(&ctx, &mut gui, &zeus, Vec::new());

   for name in ["Token A", "Token B"] {
      assert!(
         text_field(&nodes, name).is_some(),
         "the {name} field must be named after the caption beside it: {:?}",
         drawn_names(&nodes)
      );
   }

   assert!(
      nodes.iter().any(|(_, node)| node.label() == Some("Pool version: V3")),
      "the version picker must say which version it shows: {:?}",
      drawn_names(&nodes)
   );
}

/// The fee list says which tier is in force, the way the version list does: the row the trigger draws
/// is the chosen one and every other row says it is not.
#[test]
fn the_add_pool_fee_tiers_say_which_one_is_chosen() {
   let (ctx, mut gui, zeus) = main_window();

   seed_pool_rows(&mut gui, &zeus, Vec::new());
   gui.uniswap.pools_ui.open_add_pool_for_test();

   let nodes = render_pools_page(&ctx, &mut gui, &zeus, Vec::new());

   let trigger = nodes
      .iter()
      .find(|(_, node)| node.label() == Some("Fee tier: 0.3%"))
      .map(|(id, _)| *id)
      .expect("the fee picker must say which tier it shows");

   let nodes = render_pools_page(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, trigger)],
   );

   let row = |label: &str| {
      nodes
         .iter()
         .find(|(_, node)| node.role() == Role::RadioButton && node.label() == Some(label))
         .map(|(_, node)| node)
         .unwrap_or_else(|| {
            panic!(
               "the {label} tier must be a row of the open list: {:?}",
               drawn_names(&nodes)
            )
         })
   };

   assert_eq!(
      row("0.3%").toggled(),
      Some(Toggled::True),
      "the tier the trigger draws is the chosen row"
   );

   assert_eq!(
      row("0.05%").toggled(),
      Some(Toggled::False),
      "a tier that is not on the trigger must not claim to be chosen"
   );
}

/// The count belongs to the filter, so it is said when it moves — and only then: the same empty table
/// a frame later says nothing.
#[test]
fn the_pools_search_says_how_many_rows_it_left() {
   let (ctx, mut gui, zeus) = main_window();

   seed_pool_rows(&mut gui, &zeus, Vec::new());

   render_pools_page(&ctx, &mut gui, &zeus, Vec::new());

   assert_eq!(
      gui.uniswap.pools_ui.take_notice().as_deref(),
      Some("No pools match this filter."),
      "an empty table must say how many rows the filter left"
   );

   render_pools_page(&ctx, &mut gui, &zeus, Vec::new());

   assert_eq!(
      gui.uniswap.pools_ui.take_notice(),
      None,
      "the same count is not said again"
   );
}

/// The window's own notices are plain labels, so they already reach the tree: this is the premise the
/// rest of the window's names rest on.
#[test]
fn the_uniswap_notices_are_in_the_tree() {
   let (ctx, mut gui, zeus) = main_window();
   wallet_in_cache(&zeus);
   gui.uniswap.open();

   zeus.write(|ctx| ctx.privacy_mode = true);

   let nodes = render_uniswap_window(&ctx, &mut gui, &zeus);

   assert!(
      nodes.iter().any(|(_, node)| node.role() == Role::Label
         && node.value() == Some("Private swaps are not supported")),
      "the private-swaps line must reach the tree as a label: {:?}",
      drawn_names(&nodes)
   );

   zeus.write(|ctx| {
      ctx.privacy_mode = false;
      ctx.chain = ChainId::new(56).unwrap();
   });

   let nodes = render_uniswap_window(&ctx, &mut gui, &zeus);

   assert!(
      nodes.iter().any(|(_, node)| node.role() == Role::Label
         && node.value() == Some("Swap feature is not available on Binance Smart Chain")),
      "the BSC line must reach the tree as a label: {:?}",
      drawn_names(&nodes)
   );
}

/// The pool table is a list of pools to hunt through and import from by eye — a search field, a
/// filter and a table of pairs. A reader is offered the two pages that are not that, and a window
/// left on the table when a reader attaches falls back to Swap instead of drawing a page whose tab
/// is gone.
#[test]
fn a_reader_is_offered_the_two_swap_pages_that_are_not_a_table() {
   let (ctx, mut gui, zeus) = main_window();
   wallet_in_cache(&zeus);
   gui.uniswap.open();

   let tabs = |nodes: &[(NodeId, Node)]| -> Vec<String> {
      nodes
         .iter()
         .filter(|(_, node)| node.role() == Role::Tab)
         .filter_map(|(_, node)| node.label())
         .map(str::to_string)
         .collect()
   };

   let nodes = render_uniswap_window(&ctx, &mut gui, &zeus);
   let mut sighted = tabs(&nodes);
   sighted.sort();

   assert_eq!(
      sighted,
      vec!["Pools", "Settings", "Swap"],
      "a sighted user keeps the pool table"
   );

   // The page the reader is not offered is the one in front when the reader attaches.
   gui.uniswap.open_pools_for_test();
   gui.uniswap.set_reader_attached(true);

   let nodes = render_uniswap_window(&ctx, &mut gui, &zeus);
   let mut reader_tabs = tabs(&nodes);
   reader_tabs.sort();

   assert_eq!(
      reader_tabs,
      vec!["Settings", "Swap"],
      "a reader must not be handed a table of pools to hunt through"
   );

   assert!(
      !nodes
         .iter()
         .any(|(_, node)| node.role() == Role::Button && node.label() == Some("Add Pool")),
      "the pool table must not stay in front of a reader: {:?}",
      drawn_names(&nodes)
   );
}
