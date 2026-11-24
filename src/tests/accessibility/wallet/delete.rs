use super::super::app::{main_window, master_wallet};
use super::super::harness::{announced, button_named, drawn_names, key_press};
use super::render_wallet_page;
use crate::core::{WalletInfo, ZeusCtx};
use crate::gui::GUI;
use egui::accesskit::{Node, NodeId, Role};
use egui::{Context, Key};

/// Seed the context the Wallets page draws from, so the dialog over it draws without a vault.
fn seed_delete_context(zeus: &ZeusCtx, wallet: &WalletInfo) {
   zeus.write(|zeus| {
      zeus.vault_unlocked = true;
      zeus.current_wallet = wallet.clone();
      zeus.wallet_info_cache = crate::core::types::WalletInfoCache::new(
         std::iter::once((wallet.address, wallet.clone())).collect(),
         vec![wallet.clone()],
      );
   });
}

/// Open the delete confirmation on `wallet` with its credentials already verified, and draw the
/// frame until the dialog paints: a shrink-wrapping modal spends two invisible sizing passes.
fn draw_delete_dialog(
   ctx: &Context,
   gui: &mut GUI,
   zeus: &ZeusCtx,
   wallet: &WalletInfo,
) -> Vec<(NodeId, Node)> {
   seed_delete_context(zeus, wallet);
   gui.wallet_ui.delete_wallet_ui_for_test().for_test(wallet.clone());

   let mut nodes = Vec::new();

   for _ in 0..3 {
      nodes = render_wallet_page(ctx, gui, zeus, Vec::new());
   }

   nodes
}

/// The confirmation is a dialog named after the wallet it removes, and the lines it drew before are
/// still in the reading under that name.
#[test]
fn the_delete_confirmation_is_named_after_the_wallet_it_removes() {
   let (ctx, mut gui, zeus) = main_window();
   let wallet = master_wallet("Main");

   let nodes = draw_delete_dialog(&ctx, &mut gui, &zeus, &wallet);

   let dialog = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Dialog)
      .map(|(_, node)| node)
      .unwrap_or_else(|| {
         panic!(
            "the confirmation must be a named dialog: {:?}",
            drawn_names(&nodes)
         )
      });

   assert_eq!(
      dialog.label(),
      Some("Delete Main, master wallet?"),
      "the dialog is named by the wallet it removes and the question it asks"
   );

   let drawn: Vec<&Node> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Label && node.value() == Some("Main (Master)"))
      .map(|(_, node)| node)
      .collect();

   assert_eq!(
      drawn.len(),
      1,
      "the dialog draws the wallet's own name once: {:?}",
      drawn_names(&nodes)
   );

   assert!(
      drawn[0].is_hidden(),
      "the drawn name repeats the heading, so its label is hidden"
   );

   let address = wallet.address.to_string();

   for text in [address.as_str(), "Are you sure you want to continue?"] {
      assert!(
         nodes.iter().any(|(_, node)| node.role() == Role::Label
            && !node.is_hidden()
            && node.value() == Some(text)),
         "the dialog must still draw {text:?}: {:?}",
         drawn_names(&nodes)
      );
   }

   for plate in ["Public $", "Railgun $"] {
      assert!(
         nodes.iter().any(|(_, node)| node.role() == Role::Label
            && !node.is_hidden()
            && node.value().is_some_and(|value| value.starts_with(plate))),
         "the value plate stays where it was: {:?}",
         drawn_names(&nodes)
      );
   }
}

/// The consequence is an alert in one sentence, so a reader is told what the delete costs instead of
/// walking past a coloured paragraph.
#[test]
fn the_delete_warning_is_spoken_as_an_alert() {
   let (ctx, mut gui, zeus) = main_window();
   let wallet = master_wallet("Main");

   let nodes = draw_delete_dialog(&ctx, &mut gui, &zeus, &wallet);

   let sentence = "Deleting this wallet will also delete all its transaction history and token approval data next time Zeus starts.";

   let alert = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Alert)
      .map(|(_, node)| node)
      .unwrap_or_else(|| {
         panic!(
            "the consequence must be an alert a reader is told: {:?}",
            drawn_names(&nodes)
         )
      });

   assert_eq!(
      alert.label(),
      Some(sentence),
      "the alert carries the sentence the warning paragraph draws"
   );
}

/// The two buttons say what they act on, and the safe one holds the focus when the dialog opens, so
/// Enter keeps the wallet instead of deleting it.
#[test]
fn the_delete_confirmation_names_its_buttons_and_focuses_the_safe_one() {
   let (ctx, mut gui, zeus) = main_window();
   let wallet = master_wallet("Main");

   let nodes = draw_delete_dialog(&ctx, &mut gui, &zeus, &wallet);

   let keep = button_named(&nodes, "Keep this wallet");

   assert!(
      nodes.iter().any(|(_, node)| node.role() == Role::Button
         && node.label() == Some("Delete Main, master wallet")),
      "the destructive button is named for the wallet it removes: {:?}",
      drawn_names(&nodes)
   );

   assert!(
      !nodes
         .iter()
         .any(|(_, node)| node.role() == Role::Button && node.label() == Some("Delete")),
      "the drawn word is not the name the button is reached by"
   );

   assert_eq!(
      ctx.memory(|memory| memory.focused()).map(|id| id.accesskit_id()),
      Some(keep),
      "the safe button must hold the focus, so Enter keeps the wallet"
   );
}

/// Escape is the dialog's own way out: the wallet is kept, the flow is reset, and the frame says
/// which of the two ways out was taken.
#[test]
fn escape_closes_the_delete_confirmation_without_deleting() {
   let (ctx, mut gui, zeus) = main_window();
   let wallet = master_wallet("Main");

   draw_delete_dialog(&ctx, &mut gui, &zeus, &wallet);

   let nodes = render_wallet_page(
      &ctx,
      &mut gui,
      &zeus,
      vec![key_press(Key::Escape)],
   );

   let delete = gui.wallet_ui.delete_wallet_ui_for_test();

   assert!(!delete.is_open(), "Escape must close the dialog");

   assert!(
      !delete.holds_authorization_for_test(),
      "Escape must reset the flow, not leave the delete authorized"
   );

   assert_eq!(
      announced(&nodes).as_deref(),
      Some("Wallet not deleted."),
      "the dialog going away must say the wallet was kept"
   );

   // The dialog draws before it reads the key, so the frame the key landed on still carries it; the
   // next frame is the one a reader is handed after the close.
   let nodes = render_wallet_page(&ctx, &mut gui, &zeus, Vec::new());

   assert!(
      !nodes.iter().any(|(_, node)| node.role() == Role::Dialog
         || (node.role() == Role::Button && node.label() == Some("Delete Main, master wallet"))),
      "the dialog must be gone from the tree: {:?}",
      drawn_names(&nodes)
   );
}

/// The same form opens before an export, so the dialog says here that it verifies the vault before a
/// wallet is deleted.
#[test]
fn the_verify_dialog_says_why_it_asks_before_a_wallet_is_deleted() {
   let (ctx, mut gui, zeus) = main_window();
   let wallet = master_wallet("Main");

   seed_delete_context(&zeus, &wallet);
   gui.wallet_ui.delete_wallet_ui_for_test().open(wallet);

   let mut nodes = Vec::new();

   for _ in 0..3 {
      nodes = render_wallet_page(&ctx, &mut gui, &zeus, Vec::new());
   }

   let dialog = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Dialog)
      .map(|(_, node)| node)
      .unwrap_or_else(|| {
         panic!(
            "the credentials form must be a dialog: {:?}",
            drawn_names(&nodes)
         )
      });

   assert_eq!(
      dialog.label(),
      Some("Verify Credentials"),
      "the dialog must be named"
   );

   assert!(
      nodes.iter().any(|(_, node)| node.role() == Role::Label
         && !node.is_hidden()
         && node.value()
            == Some("This verifies your vault credentials before the wallet is deleted.")),
      "the dialog must say why it is asking: {:?}",
      drawn_names(&nodes)
   );
}
