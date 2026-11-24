use super::super::app::{
   main_window, master_wallet, render_central, shared_gui_turn, wallet_named,
};
use super::super::harness::{
   accesskit_action, announced, button_named, copy_said, drawn_names, focused_id, focused_text,
   is_text_input, key_press, node_name,
};
use super::render_wallet_page;
use crate::core::{WalletInfo, WalletValue, ZeusCtx};
use crate::gui::GUI;
use crate::gui::ui::wallet::import::ImportWalletType;
use egui::accesskit::{Action, Node, NodeId, Role};
use egui::{Context, Key};

/// Stand the Wallets page up on `current`, with `others` under it, each wallet holding something
/// on `chains`.
///
/// The page reads the wallet it is on from the cache unlock fills, and the panel draws it only
/// while the vault is unlocked; the rows and their values are seeded here instead of fetched, since
/// the fetch runs on a worker that reads the frame's own global.
fn seed_wallet_page(
   gui: &mut GUI,
   zeus: &ZeusCtx,
   current: &WalletInfo,
   others: &[WalletInfo],
   chains: &[u64],
) {
   let all: Vec<WalletInfo> =
      std::iter::once(current.clone()).chain(others.iter().cloned()).collect();

   zeus.write(|zeus| {
      zeus.vault_unlocked = true;
      // Which chains are turned off comes out of the user's data directory, so the test sets that
      // set rather than inheriting a machine's configuration.
      zeus.disabled_chains =
         crate::core::types::DisabledChains::new(std::collections::HashSet::new());
      zeus.current_wallet = current.clone();
      zeus.wallet_info_cache = crate::core::types::WalletInfoCache::new(
         all.iter().map(|wallet| (wallet.address, wallet.clone())).collect(),
         all,
      );
   });

   let page = &mut gui.wallet_ui;
   page.for_test(current, others.to_vec());

   for wallet in std::iter::once(current).chain(others) {
      page.set_value_for_test(
         wallet.address,
         WalletValue::default(),
         chains.to_vec(),
      );
   }
}

/// Draw the Wallets page twice: the first pass lays the card out, and the second is the frame a
/// reader is handed.
fn draw_wallet_page(ctx: &Context, gui: &mut GUI, zeus: &ZeusCtx) -> Vec<(NodeId, Node)> {
   render_wallet_page(ctx, gui, zeus, Vec::new());
   render_wallet_page(ctx, gui, zeus, Vec::new())
}

/// The wallet Zeus is on is the card above the search field: one group node that says the wallet
/// and its value, with the two labels that draw those words left out of the reading.
#[test]
fn the_current_wallet_is_one_card_named_after_it() {
   let (ctx, mut gui, zeus) = main_window();
   let master = master_wallet("Main");
   let savings = wallet_named("Savings");

   seed_wallet_page(&mut gui, &zeus, &master, &[savings], &[1, 42161]);

   let nodes = draw_wallet_page(&ctx, &mut gui, &zeus);
   let card = "Main, master wallet, $0.00";

   let node = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Group && node.label() == Some(card))
      .map(|(_, node)| node)
      .unwrap_or_else(|| {
         panic!(
            "the current wallet must be one group named {card:?}: {:?}",
            drawn_names(&nodes)
         )
      });

   assert_eq!(
      node.description(),
      Some("On Ethereum, Arbitrum"),
      "the chains the card draws are said once, as its description"
   );

   for text in ["Main (Master)", "$0.00"] {
      assert!(
         nodes.iter().any(|(_, node)| node.role() == Role::Label
            && node.is_hidden()
            && node.value() == Some(text)),
         "the label drawing {text:?} must be hidden: the card already says it"
      );
   }
}

/// Every other wallet is one row that is a button, so a reader can pick it: the row carries the
/// wallet and its value, and the chains it holds something on are its description.
#[test]
fn every_other_wallet_is_one_row_that_says_who_it_is() {
   let (ctx, mut gui, zeus) = main_window();
   let master = master_wallet("Main");
   let savings = wallet_named("Savings");

   seed_wallet_page(&mut gui, &zeus, &master, &[savings], &[1, 42161]);

   let nodes = draw_wallet_page(&ctx, &mut gui, &zeus);
   let row = "Savings, imported wallet, $0.00";

   let node = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some(row))
      .map(|(_, node)| node)
      .unwrap_or_else(|| {
         panic!(
            "every other wallet must be one button row named {row:?}: {:?}",
            drawn_names(&nodes)
         )
      });

   assert_eq!(
      node.description(),
      Some("On Ethereum, Arbitrum"),
      "the row says which chains it holds something on"
   );

   assert!(
      node.supports_action(Action::Click),
      "the row has to answer a Click request, or a reader cannot switch to it"
   );

   assert!(
      node.supports_action(Action::Focus),
      "the row has to be reachable by keyboard as well"
   );

   assert!(
      nodes.iter().any(|(_, node)| node.role() == Role::Label
         && node.is_hidden()
         && node.value() == Some("Savings (Imported)")),
      "the drawn name is inside the row's own name, so its label is hidden"
   );
}

/// An AccessKit Click on a row is the same switch a mouse click is, and the page says which wallet
/// it moved to.
#[test]
fn a_wallet_row_click_switches_and_the_page_says_so() {
   let _turn = shared_gui_turn();
   let (ctx, mut gui, zeus) = main_window();
   let master = master_wallet("Main");
   let savings = wallet_named("Savings");

   seed_wallet_page(
      &mut gui,
      &zeus,
      &master,
      std::slice::from_ref(&savings),
      &[1],
   );

   let nodes = draw_wallet_page(&ctx, &mut gui, &zeus);
   let row = "Savings, imported wallet, $0.00";

   let row = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some(row))
      .map(|(id, _)| *id)
      .unwrap_or_else(|| {
         panic!(
            "the row must be in the tree before it is clicked: {:?}",
            drawn_names(&nodes)
         )
      });

   let nodes = render_wallet_page(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, row)],
   );

   assert_eq!(
      announced(&nodes).as_deref(),
      Some("Switched to Savings, imported wallet."),
      "the page's sentence must reach the announcement node"
   );

   assert_eq!(
      zeus.read(|zeus| zeus.current_wallet.address),
      savings.address,
      "the click must move Zeus to that wallet"
   );

   assert!(
      gui.wallet_ui.take_notice().is_none(),
      "the panel drains the page's sentence, so it is not said again next frame"
   );
}

/// The address button names the wallet it copies for, keeps the whole address as its description
/// and says when the copy is done.
#[test]
fn the_address_button_says_whose_address_it_copied() {
   let (ctx, mut gui, zeus) = main_window();
   let master = master_wallet("Main");

   seed_wallet_page(&mut gui, &zeus, &master, &[], &[1]);

   let nodes = draw_wallet_page(&ctx, &mut gui, &zeus);
   let button = "Copy address of Main, master wallet";

   let (button, node) = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some(button))
      .unwrap_or_else(|| {
         panic!(
            "the card's address button must say what it copies: {:?}",
            drawn_names(&nodes)
         )
      });

   let full = crate::gui::ui::address_for_speech(None, &master.address.to_string())
      .description
      .unwrap_or_default();

   assert_eq!(
      node.description(),
      Some(full.as_str()),
      "the whole address a listener cannot see is the button's description"
   );

   assert!(
      !node.description().unwrap_or_default().contains("..."),
      "a shortened address must never stand in for the address in speech"
   );

   let nodes = render_wallet_page(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, *button)],
   );

   assert_eq!(
      announced(&nodes).as_deref(),
      Some(copy_said(&master.address.to_string()).as_str()),
      "the copy says it happened, and which address, since nothing on screen changes"
   );
}

/// The search field says what it narrows, and the count says what the search left — once, when it
/// changes.
#[test]
fn the_search_field_is_named_and_the_count_says_what_it_left() {
   let (ctx, mut gui, zeus) = main_window();
   let master = master_wallet("Main");
   let savings = wallet_named("Savings");
   let trading = wallet_named("Trading");

   seed_wallet_page(
      &mut gui,
      &zeus,
      &master,
      &[savings, trading],
      &[1],
   );

   // The first frame the page draws is the one that says how many wallets it holds.
   let nodes = render_wallet_page(&ctx, &mut gui, &zeus, Vec::new());

   assert!(
      nodes.iter().any(|(_, node)| is_text_input(node.role())
         && node_name(&nodes, node).as_deref() == Some("Search wallets")),
      "the search field must be named for what it narrows: {:?}",
      drawn_names(&nodes)
   );

   assert_eq!(
      announced(&nodes).as_deref(),
      Some("2 wallets"),
      "the page says how many wallets the list holds when it arrives"
   );

   gui.wallet_ui.set_query_for_test("sav");
   let nodes = render_wallet_page(&ctx, &mut gui, &zeus, Vec::new());

   assert_eq!(
      announced(&nodes).as_deref(),
      Some("1 wallet"),
      "narrowing the list to one wallet says so"
   );
}

/// The rename dialog opens on the name in use and puts the keyboard in its field, and Escape
/// leaves both the dialog and the wallet alone.
#[test]
fn the_rename_dialog_opens_on_the_name_in_use_and_escape_leaves_it_alone() {
   let (ctx, mut gui, zeus) = main_window();
   let master = master_wallet("Main");

   seed_wallet_page(&mut gui, &zeus, &master, &[], &[1]);
   gui.wallet_ui.open_rename_wallet(Some(master.clone()), None);

   // A shrink-wrapping dialog spends two invisible sizing passes before it paints, and egui
   // exposes no node for a widget it considers invisible.
   let mut nodes = Vec::new();

   for _ in 0..3 {
      nodes = render_wallet_page(&ctx, &mut gui, &zeus, Vec::new());
   }

   let (field, node) = nodes
      .iter()
      .find(|(_, node)| {
         is_text_input(node.role()) && node_name(&nodes, node).as_deref() == Some("New wallet name")
      })
      .unwrap_or_else(|| {
         panic!(
            "the dialog must name its field: {:?}",
            drawn_names(&nodes)
         )
      });

   assert_eq!(
      node.value(),
      Some("Main"),
      "the field opens on the name in use, so a reader edits it instead of retyping it"
   );

   assert_eq!(
      ctx.memory(|memory| memory.focused()).map(|id| id.accesskit_id()),
      Some(*field),
      "the field must take the opening focus the dialog gave its Close button"
   );

   let nodes = render_wallet_page(
      &ctx,
      &mut gui,
      &zeus,
      vec![key_press(Key::Escape)],
   );

   assert!(
      !gui.wallet_ui.rename_is_open_for_test(),
      "Escape must close the dialog: {:?}",
      drawn_names(&nodes)
   );

   assert_eq!(
      zeus.read(|zeus| zeus.current_wallet.name().to_string()),
      "Main",
      "closing the dialog must leave the wallet named as it was"
   );
}

/// The rename dialog hands the keyboard back to the card it was opened from.
///
/// The menu item that opened it goes with the menu, and the dialog's own field and buttons go with
/// the dialog: without a focus handed back, a reader is left on nothing in the middle of the page.
#[test]
fn the_rename_dialog_gives_the_keyboard_back_to_the_card() {
   let (ctx, mut gui, zeus) = main_window();
   let master = master_wallet("Main");

   seed_wallet_page(&mut gui, &zeus, &master, &[], &[1]);

   let nodes = draw_wallet_page(&ctx, &mut gui, &zeus);
   let options = button_named(&nodes, "Options for Main, master wallet");

   render_wallet_page(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, options)],
   );
   let nodes = render_wallet_page(&ctx, &mut gui, &zeus, Vec::new());
   let rename = button_named(&nodes, "Rename");

   render_wallet_page(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, rename)],
   );
   render_wallet_page(&ctx, &mut gui, &zeus, Vec::new());

   assert!(
      gui.wallet_ui.rename_is_open_for_test(),
      "the dialog must open"
   );

   // The save runs on a worker and closes the dialog from off the frame; the page hands the
   // keyboard back on the next frame it draws.
   gui.wallet_ui.close_rename_wallet();

   let nodes = render_wallet_page(&ctx, &mut gui, &zeus, Vec::new());

   assert_eq!(
      focused_id(&ctx),
      Some(options),
      "the keyboard must go back to the card's options button: {:?}",
      focused_text(&ctx, &nodes)
   );
}

/// The options button opens a menu whose items say what each one does, and the master's Delete
/// says why it cannot be used.
#[test]
fn the_wallet_menu_says_what_each_item_does() {
   let (ctx, mut gui, zeus) = main_window();
   let master = master_wallet("Main");

   seed_wallet_page(&mut gui, &zeus, &master, &[], &[1]);

   let nodes = draw_wallet_page(&ctx, &mut gui, &zeus);
   let options = "Options for Main, master wallet";

   let options = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some(options))
      .map(|(id, _)| *id)
      .unwrap_or_else(|| {
         panic!(
            "the card's dots button must say whose options it opens: {:?}",
            drawn_names(&nodes)
         )
      });

   render_wallet_page(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, options)],
   );

   let nodes = render_wallet_page(&ctx, &mut gui, &zeus, Vec::new());

   for label in ["Export key", "Rename", "Show address QR code"] {
      assert!(
         nodes
            .iter()
            .any(|(_, node)| node.role() == Role::Button && node.label() == Some(label)),
         "the menu must say {label:?}: {:?}",
         drawn_names(&nodes)
      );
   }

   let delete = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some("Delete"))
      .map(|(_, node)| node)
      .unwrap_or_else(|| {
         panic!(
            "the master's menu must keep a Delete item: {:?}",
            drawn_names(&nodes)
         )
      });

   assert!(
      delete.is_disabled(),
      "the master wallet is the one wallet Zeus will not delete"
   );

   assert_eq!(
      delete.description(),
      Some("The master wallet cannot be deleted."),
      "the disabled item says why"
   );
}

/// A chosen item closes the menu it was chosen from.
///
/// A popup closes itself only on a pointer click, and a reader activates an item with Enter, which
/// presses no pointer at all: the menu would stay open over the card, above the dialog the item
/// opened, and be the next thing the keyboard could reach.
#[test]
fn the_wallet_menu_closes_when_an_item_is_chosen() {
   let (ctx, mut gui, zeus) = main_window();
   let master = master_wallet("Main");

   seed_wallet_page(&mut gui, &zeus, &master, &[], &[1]);

   let nodes = draw_wallet_page(&ctx, &mut gui, &zeus);
   let options = button_named(&nodes, "Options for Main, master wallet");

   render_wallet_page(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, options)],
   );
   let nodes = render_wallet_page(&ctx, &mut gui, &zeus, Vec::new());
   let rename = button_named(&nodes, "Rename");

   render_wallet_page(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, rename)],
   );
   let nodes = render_wallet_page(&ctx, &mut gui, &zeus, Vec::new());

   // Rename is the item chosen above, and the dialog it opens draws a button of that name too, so
   // the words that only the menu says are the ones that must be gone.
   for label in ["Export key", "Show address QR code", "Delete"] {
      assert!(
         !nodes
            .iter()
            .any(|(_, node)| node.role() == Role::Button && node.label() == Some(label)),
         "the menu must close once an item is chosen: {:?}",
         drawn_names(&nodes)
      );
   }

   assert!(
      gui.wallet_ui.rename_is_open_for_test(),
      "the chosen item must still do its work: {:?}",
      drawn_names(&nodes)
   );
}

/// Open the import dialog over the Wallets page and draw it until it paints: a shrink-wrapping
/// modal spends two invisible sizing passes, and egui exposes no node for a widget it considers
/// invisible.
fn draw_import_dialog(
   ctx: &Context,
   gui: &mut GUI,
   zeus: &ZeusCtx,
   kind: ImportWalletType,
) -> Vec<(NodeId, Node)> {
   gui.wallet_ui.add_wallet_ui.open();
   gui.wallet_ui.add_wallet_ui.import_wallet.open(kind);

   let mut nodes = Vec::new();

   for _ in 0..3 {
      nodes = render_central(ctx, gui, zeus, Vec::new());
   }

   nodes
}

/// The import form opens on its name field: one text input named without the parentheses of the
/// drawn instruction, which leaves the tree, and the keyboard in the field instead of on the
/// dialog's Close button.
#[test]
fn the_import_dialog_opens_on_its_name_field() {
   let (ctx, mut gui, zeus) = main_window();
   seed_wallet_page(&mut gui, &zeus, &master_wallet("Main"), &[], &[1]);

   let nodes = draw_import_dialog(
      &ctx,
      &mut gui,
      &zeus,
      ImportWalletType::MnemonicPhrase,
   );

   let (field, node) = nodes
      .iter()
      .find(|(_, node)| {
         is_text_input(node.role())
            && node_name(&nodes, node).as_deref() == Some("Wallet name, optional")
      })
      .unwrap_or_else(|| {
         panic!(
            "the dialog must name its name field: {:?}",
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
      "the field must take the opening focus the dialog gave its Close button"
   );
}

/// The secret field is named by the label the crate draws above it, and says how much has been
/// typed, so a phrase can be checked by ear while the secret itself stays out of the tree.
#[test]
fn the_secret_field_says_how_much_it_holds() {
   let (ctx, mut gui, zeus) = main_window();
   seed_wallet_page(&mut gui, &zeus, &master_wallet("Main"), &[], &[1]);

   let nodes = draw_import_dialog(
      &ctx,
      &mut gui,
      &zeus,
      ImportWalletType::MnemonicPhrase,
   );

   assert!(
      nodes
         .iter()
         .any(|(_, node)| node.role() == Role::Label && node.value() == Some("Seed Phrase")),
      "the crate must draw the label the field is named by: {:?}",
      drawn_names(&nodes)
   );

   assert!(
      nodes.iter().any(|(_, node)| is_text_input(node.role())
         && node_name(&nodes, node).as_deref() == Some("Seed Phrase")),
      "the secret field's name is the drawn label, resolved through labelled_by: {:?}",
      drawn_names(&nodes)
   );

   gui.wallet_ui
      .add_wallet_ui
      .import_wallet
      .set_secret_for_test("winner thank year");

   let nodes = render_central(&ctx, &mut gui, &zeus, Vec::new());

   let field = nodes
      .iter()
      .find(|(_, node)| {
         is_text_input(node.role()) && node_name(&nodes, node).as_deref() == Some("Seed Phrase")
      })
      .map(|(_, node)| node)
      .unwrap_or_else(|| {
         panic!(
            "the secret field must still be in the tree: {:?}",
            drawn_names(&nodes)
         )
      });

   assert_eq!(
      field.description(),
      Some("3 words"),
      "a three-word phrase must say how many words it holds"
   );
}

/// The same field in a private-key dialog counts characters instead of words.
#[test]
fn a_private_key_dialog_says_how_many_characters_it_holds() {
   let (ctx, mut gui, zeus) = main_window();
   seed_wallet_page(&mut gui, &zeus, &master_wallet("Main"), &[], &[1]);

   draw_import_dialog(
      &ctx,
      &mut gui,
      &zeus,
      ImportWalletType::PrivateKey,
   );

   gui.wallet_ui.add_wallet_ui.import_wallet.set_secret_for_test("abcd1234");

   let nodes = render_central(&ctx, &mut gui, &zeus, Vec::new());

   let field = nodes
      .iter()
      .find(|(_, node)| {
         is_text_input(node.role()) && node_name(&nodes, node).as_deref() == Some("Private Key")
      })
      .map(|(_, node)| node)
      .unwrap_or_else(|| {
         panic!(
            "the private-key dialog must name its field: {:?}",
            drawn_names(&nodes)
         )
      });

   assert_eq!(
      field.description(),
      Some("8 characters"),
      "a private key must say how many characters it holds"
   );
}
