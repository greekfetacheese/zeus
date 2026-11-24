use super::super::app::{main_window, master_wallet};
use super::super::harness::{
   accesskit_action, announced, drawn_names, focused_id, is_text_input, key_press, node_name,
};
use super::render_wallet_page;
use crate::core::{WalletInfo, ZeusCtx};
use crate::gui::GUI;
use egui::accesskit::{Action, Node, NodeId, Role};
use egui::{Context, Key};
use zeus_eth::alloy_primitives::Address;

/// A discovery list of `count` rows: the n-th row's address repeats `0x11 × n`, and its index is
/// the derivation index the row is named by.
fn discovered_wallets(count: u32) -> crate::core::DiscoveredWallets {
   let mut wallets = crate::core::DiscoveredWallets::new();

   for index in 0..count {
      let address = Address::from([0x11 * (index as u8 + 1); 20]);
      wallets.add_wallet(
         address,
         vec![44, 60, 0, index].into(),
         zeus_bip32::BIP32_HARDEN + index,
      );
   }

   wallets
}

/// Open the discovery flow on `wallets`, the way a finished load leaves it, without the worker its
/// own `open` runs.
fn open_discover_flow(gui: &mut GUI, wallets: crate::core::DiscoveredWallets) {
   gui.wallet_ui.add_wallet_ui.discover_child_wallets_ui.for_test(wallets);
}

/// Stand the vault unlocked on `wallet`, with every chain on: the discovery flow draws over the
/// central panel, and each of its rows asks the context for a value.
fn seed_discover_context(zeus: &ZeusCtx, wallet: &WalletInfo) {
   zeus.write(|zeus| {
      zeus.vault_unlocked = true;
      // Which chains are turned off comes out of the user's data directory, so the test sets that
      // set rather than inheriting a machine's configuration.
      zeus.disabled_chains =
         crate::core::types::DisabledChains::new(std::collections::HashSet::new());
      zeus.current_wallet = wallet.clone();
      zeus.wallet_info_cache = crate::core::types::WalletInfoCache::new(
         std::iter::once((wallet.address, wallet.clone())).collect(),
         vec![wallet.clone()],
      );
   });
}

/// Draw the discovery flow until its modal has painted: a modal spends a sizing pass, and the
/// results table measures on the pass it appears on.
fn draw_discover_flow(ctx: &Context, gui: &mut GUI, zeus: &ZeusCtx) -> Vec<(NodeId, Node)> {
   let mut nodes = Vec::new();

   for _ in 0..3 {
      nodes = render_wallet_page(ctx, gui, zeus, Vec::new());
   }

   nodes
}

/// The count is said when the page or the number of wallets changes, so the list a reader opens on
/// says how much of it is on screen, and an empty one says so in words.
#[test]
fn the_discovery_count_says_which_wallets_the_page_holds() {
   let (ctx, mut gui, zeus) = main_window();
   seed_discover_context(&zeus, &master_wallet("Main"));

   open_discover_flow(&mut gui, discovered_wallets(3));

   let nodes = render_wallet_page(&ctx, &mut gui, &zeus, Vec::new());

   assert_eq!(
      announced(&nodes).as_deref(),
      Some("Wallets 1 to 3 of 3."),
      "the flow says which wallets of the list it found"
   );

   // The same list a frame later is not a new answer: a count is said once per change.
   let nodes = render_wallet_page(&ctx, &mut gui, &zeus, Vec::new());

   assert_ne!(
      announced(&nodes).as_deref(),
      Some("Wallets 1 to 3 of 3."),
      "a count that did not change is not said again"
   );

   open_discover_flow(&mut gui, discovered_wallets(0));

   let nodes = render_wallet_page(&ctx, &mut gui, &zeus, Vec::new());

   assert_eq!(
      announced(&nodes).as_deref(),
      Some("No wallets found."),
      "an emptied list is said, not only drawn"
   );
}

/// Moving to the next page says the page it landed on, for the twelve wallets that fit on two of
/// them.
#[test]
fn the_discovery_count_follows_the_page() {
   let (ctx, mut gui, zeus) = main_window();
   seed_discover_context(&zeus, &master_wallet("Main"));

   open_discover_flow(&mut gui, discovered_wallets(12));
   draw_discover_flow(&ctx, &mut gui, &zeus);

   gui.wallet_ui.add_wallet_ui.discover_child_wallets_ui.next_page_for_test();

   let nodes = render_wallet_page(&ctx, &mut gui, &zeus, Vec::new());

   assert_eq!(
      announced(&nodes).as_deref(),
      Some("Wallets 6 to 10 of 12."),
      "the page change is the answer the Next button gets"
   );
}

/// Every row's button says which wallet it adds, so ten identical drawn words become ten names,
/// and each row's link says where it leads instead of reading out the address it draws.
#[test]
fn every_discovered_row_names_the_wallet_it_adds() {
   let (ctx, mut gui, zeus) = main_window();
   seed_discover_context(&zeus, &master_wallet("Main"));

   open_discover_flow(&mut gui, discovered_wallets(2));

   let nodes = draw_discover_flow(&ctx, &mut gui, &zeus);

   for index in 0..2 {
      let name = format!("Add wallet {index}");

      let node = nodes
         .iter()
         .find(|(_, node)| node.role() == Role::Button && node.label() == Some(name.as_str()))
         .map(|(_, node)| node)
         .unwrap_or_else(|| {
            panic!(
               "each row's button must say which wallet it adds ({name}): {:?}",
               drawn_names(&nodes)
            )
         });

      assert_eq!(
         node.description(),
         None,
         "the button must not spell the address out: {name}"
      );
   }

   let mut add_names: Vec<&str> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Button)
      .filter_map(|(_, node)| node.label())
      .filter(|label| label.starts_with("Add wallet "))
      .collect();

   add_names.sort_unstable();

   assert_eq!(
      add_names,
      ["Add wallet 0", "Add wallet 1"],
      "no two rows are reached by the same name"
   );

   // The figure is one wallet's holdings, and a bare amount does not say whose.
   assert!(
      nodes.iter().any(|(_, node)| node.role() == Role::Label
         && node.value().is_some_and(|value| value.starts_with("Value $"))),
      "the value plate must say what the figure is: {:?}",
      drawn_names(&nodes)
   );

   let mut links: Vec<&str> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Link)
      .filter_map(|(_, node)| node.label())
      .collect();

   links.sort_unstable();

   assert_eq!(
      links,
      [
         "Wallet 0 address on the block explorer",
         "Wallet 1 address on the block explorer",
      ],
      "each row's link must say where it leads, not read the address out: {:?}",
      drawn_names(&nodes)
   );
}

/// The row is named by the index the derivation path ends with, and nothing on it reads the path
/// or an address out: a path of digits and forty hex characters are walls a listener cannot follow.
#[test]
fn a_discovered_row_is_not_read_out_as_a_path_or_an_address() {
   let (ctx, mut gui, zeus) = main_window();
   seed_discover_context(&zeus, &master_wallet("Main"));

   open_discover_flow(&mut gui, discovered_wallets(2));

   let nodes = draw_discover_flow(&ctx, &mut gui, &zeus);

   for index in 0..2 {
      let number = index.to_string();

      assert!(
         nodes
            .iter()
            .any(|(_, node)| node.role() == Role::Label && node.value() == Some(number.as_str())),
         "the row whose path ends in {number} is named by that number: {:?}",
         drawn_names(&nodes)
      );
   }

   // Nothing in the tree spells the glyphs out: a selectable label hands its drawn text to the
   // reader as a `TextRun` under its own node, so the row said its index and the whole path after
   // it, and every row of the page spelled the address it draws.
   for (_, node) in &nodes {
      for spoken in [node.label(), node.value(), node.description()].into_iter().flatten() {
         assert!(
            !spoken.contains("m/44"),
            "a row must not read its derivation path out: {spoken:?}"
         );

         assert!(
            !spoken.contains("0x"),
            "a row must not read an address out: {spoken:?}"
         );
      }
   }
}

/// The page's rows are a keyboard path: `Tab` walks one wallet at a time to the page buttons,
/// with the reader model on, because a screen reader is the pass that walks labels at all.
#[test]
fn tab_walks_every_discovered_wallet_on_the_page_and_reaches_the_page_buttons() {
   let (ctx, mut gui, zeus) = main_window();
   seed_discover_context(&zeus, &master_wallet("Main"));

   // The reader model: every label is a tab stop, which is the page a screen reader arrives on.
   ctx.options_mut(|options| options.screen_reader = true);

   open_discover_flow(&mut gui, discovered_wallets(12));
   draw_discover_flow(&ctx, &mut gui, &zeus);

   let mut stops: Vec<String> = Vec::new();

   for _ in 0..80 {
      let nodes = render_wallet_page(&ctx, &mut gui, &zeus, vec![key_press(Key::Tab)]);

      // The pass after the dialog's close button hands the focus on has no stop of its own.
      let Some(focused) = focused_id(&ctx) else {
         continue;
      };

      let spoken = nodes
         .iter()
         .find(|(node_id, _)| *node_id == focused)
         .map(|(_, node)| node.label().or_else(|| node.value()).unwrap_or_default())
         .unwrap_or_default()
         .to_string();

      assert!(
         !spoken.is_empty(),
         "a tab stop with nothing to say strands the keyboard: {:?}",
         drawn_names(&nodes)
      );

      let done = spoken == "Next";

      stops.push(spoken);

      if done {
         break;
      }
   }

   let adds: Vec<&str> = stops
      .iter()
      .map(String::as_str)
      .filter(|stop| stop.starts_with("Add wallet "))
      .collect();

   let expected: Vec<String> = (0..5).map(|index| format!("Add wallet {index}")).collect();
   let expected: Vec<&str> = expected.iter().map(String::as_str).collect();

   assert_eq!(
      adds, expected,
      "Tab must walk the page one wallet at a time, in the order the rows are drawn"
   );

   assert_eq!(
      stops.last().map(String::as_str),
      Some("Next"),
      "Tab must reach the page buttons, or a list of more than ten wallets cannot be read"
   );

   // The button the walk landed on is the one that turns the page.
   render_wallet_page(&ctx, &mut gui, &zeus, vec![key_press(Key::Enter)]);

   assert_eq!(
      gui.wallet_ui.add_wallet_ui.discover_child_wallets_ui.current_page, 1,
      "Enter on the focused Next button must turn the page"
   );
}

/// A load and a sync each say what they are doing: the spinner is named for the work behind it,
/// and the count is not an answer while the list is still loading.
#[test]
fn the_discovery_says_what_it_is_loading_and_syncing() {
   let (ctx, mut gui, zeus) = main_window();
   seed_discover_context(&zeus, &master_wallet("Main"));

   open_discover_flow(&mut gui, discovered_wallets(3));
   gui.wallet_ui.add_wallet_ui.discover_child_wallets_ui.loading = true;

   let nodes = draw_discover_flow(&ctx, &mut gui, &zeus);

   assert!(
      nodes.iter().any(|(_, node)| node.role() == Role::ProgressIndicator
         && node.label() == Some("Loading discovered wallets")),
      "the spinner must say what it is waiting for: {:?}",
      drawn_names(&nodes)
   );

   assert_ne!(
      announced(&nodes).as_deref(),
      Some("Wallets 1 to 3 of 3."),
      "the count is not an answer while the list is still loading"
   );

   gui.wallet_ui.add_wallet_ui.discover_child_wallets_ui.loading = false;
   gui.wallet_ui.add_wallet_ui.discover_child_wallets_ui.set_syncing_for_test();

   let nodes = draw_discover_flow(&ctx, &mut gui, &zeus);

   assert!(
      nodes.iter().any(|(_, node)| node.role() == Role::ProgressIndicator
         && node.label() == Some("Syncing balances")),
      "the spinner replacing the sync button must say what is running: {:?}",
      drawn_names(&nodes)
   );
}

/// The window that names a child wallet opens on its name field, which says what it is for
/// without the parentheses of the drawn instruction.
#[test]
fn the_add_wallet_window_opens_on_its_name_field() {
   let (ctx, mut gui, zeus) = main_window();
   seed_discover_context(&zeus, &master_wallet("Main"));

   open_discover_flow(&mut gui, discovered_wallets(1));

   let nodes = draw_discover_flow(&ctx, &mut gui, &zeus);

   let add = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some("Add wallet 0"))
      .map(|(id, _)| *id)
      .unwrap_or_else(|| {
         panic!(
            "the row's Add button must be in the tree: {:?}",
            drawn_names(&nodes)
         )
      });

   // The window opens on the row's click, and spends sizing passes of its own before it paints.
   let mut nodes = render_wallet_page(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, add)],
   );

   for _ in 0..3 {
      nodes = render_wallet_page(&ctx, &mut gui, &zeus, Vec::new());
   }

   let (field, node) = nodes
      .iter()
      .find(|(_, node)| {
         is_text_input(node.role())
            && node_name(&nodes, node).as_deref() == Some("Wallet name, optional")
      })
      .unwrap_or_else(|| {
         panic!(
            "the window must name its field: {:?}",
            drawn_names(&nodes)
         )
      });

   assert_eq!(
      node.label(),
      Some("Wallet name, optional"),
      "the field carries that name itself, without the parentheses of the drawn label"
   );

   assert!(
      nodes.iter().any(|(_, node)| node.role() == Role::Label
         && node.is_hidden()
         && node.value() == Some("Wallet Name (Optional)")),
      "the drawn instruction must be hidden, or the row is read twice: {:?}",
      drawn_names(&nodes)
   );

   assert_eq!(
      ctx.memory(|memory| memory.focused()).map(|id| id.accesskit_id()),
      Some(*field),
      "the field must take the opening focus the window gave its close button"
   );
}
