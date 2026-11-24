use super::super::app::main_window;
use super::super::harness::{accesskit_action, announced, button_named, drawn_names, node_texts};
use super::render_wallet_page;
use crate::core::{WalletInfo, ZeusCtx};
use crate::gui::GUI;
use egui::Context;
use egui::accesskit::{Action, Node, NodeId, Role};
use secure_types::SecureString;

/// The private key the export tests build their wallet on: a public dev key, so the test can look
/// for the exact string the dialog holds without a secret living in the test.
const EXPORT_TEST_KEY: &str = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";

/// A wallet named `Main` around a key the test knows, with the [`WalletInfo`] the page speaks it
/// by: the master wallet, or an imported one when `master` is false.
fn wallet_with_key(key: &str, master: bool) -> (zeus_wallet::Wallet, WalletInfo) {
   let wallet = zeus_wallet::Wallet::new_from_key_str("Main".to_string(), SecureString::from(key))
      .expect("the test key must build a wallet");

   let mut info = WalletInfo::from_wallet(&wallet, false);
   info.is_master = master;
   info.is_imported = !master;

   (wallet, info)
}

/// Open the export dialog on `wallet` without the worker its own `open` runs, and draw the frame
/// until the dialog paints: a shrink-wrapping modal spends two invisible sizing passes.
fn draw_export_dialog(
   ctx: &Context,
   gui: &mut GUI,
   zeus: &ZeusCtx,
   wallet: zeus_wallet::Wallet,
   info: &WalletInfo,
) -> Vec<(NodeId, Node)> {
   // The QR is decoded through the image loaders the app installs at startup.
   egui_extras::install_image_loaders(ctx);

   zeus.write(|zeus| {
      zeus.vault_unlocked = true;
      // The panel asks the cache which wallet Zeus is on before it draws the page that raises the
      // dialog, and nothing else on the page is drawn while it is closed.
      zeus.current_wallet = info.clone();
      zeus.wallet_info_cache = crate::core::types::WalletInfoCache::new(
         std::iter::once((info.address, info.clone())).collect(),
         vec![info.clone()],
      );
   });

   gui.wallet_ui.export_key_ui_for_test().for_test(info, wallet);

   let mut nodes = Vec::new();

   for _ in 0..3 {
      nodes = render_wallet_page(ctx, gui, zeus, Vec::new());
   }

   nodes
}

/// Reveal the key the way a matched credential does, and draw the body the dialog opens on it.
fn draw_export_key(ctx: &Context, gui: &mut GUI, zeus: &ZeusCtx) -> Vec<(NodeId, Node)> {
   gui.wallet_ui.export_key_ui_for_test().reveal_key_for_test();

   let mut nodes = Vec::new();

   for _ in 0..3 {
      nodes = render_wallet_page(ctx, gui, zeus, Vec::new());
   }

   nodes
}

/// Dismiss the master-wallet warning the way a reader does, and draw the body that replaces it.
fn open_export_body(
   ctx: &Context,
   gui: &mut GUI,
   zeus: &ZeusCtx,
   nodes: Vec<(NodeId, Node)>,
) -> Vec<(NodeId, Node)> {
   let understand = button_named(&nodes, "I understand the risks");

   render_wallet_page(
      ctx,
      gui,
      zeus,
      vec![accesskit_action(Action::Click, understand)],
   );

   // The taller body is re-measured, invisibly, before it is painted.
   let mut nodes = Vec::new();

   for _ in 0..3 {
      nodes = render_wallet_page(ctx, gui, zeus, Vec::new());
   }

   nodes
}

/// No node may carry `secret` in any of the forms a reader can reach.
fn assert_no_secret(nodes: &[(NodeId, Node)], secret: &str, what: &str) {
   for (_, node) in nodes {
      for text in node_texts(node) {
         assert!(!text.contains(secret), "no node may carry {what}");
      }
   }
}

/// The verify dialog says what it is, why it is asking, and its button is the word it says.
#[test]
fn the_verify_dialog_says_why_it_asks_before_the_key_is_shown() {
   let (ctx, mut gui, zeus) = main_window();
   let (wallet, info) = wallet_with_key(EXPORT_TEST_KEY, true);

   let nodes = draw_export_dialog(&ctx, &mut gui, &zeus, wallet, &info);

   let dialogs: Vec<&Node> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Dialog)
      .map(|(_, node)| node)
      .collect();

   assert_eq!(
      dialogs.len(),
      1,
      "the credentials form must be exactly one dialog: {:?}",
      drawn_names(&nodes)
   );

   assert_eq!(
      dialogs[0].label(),
      Some("Verify Credentials"),
      "the dialog must be named"
   );

   assert!(
      nodes.iter().any(|(_, node)| node.role() == Role::Label
         && node.value() == Some("This verifies your vault credentials before the key is shown.")),
      "the dialog must say why it is asking: {:?}",
      drawn_names(&nodes)
   );

   assert!(
      nodes
         .iter()
         .any(|(_, node)| node.role() == Role::Button && node.label() == Some("Confirm")),
      "the button must be the word the user hears: {:?}",
      drawn_names(&nodes)
   );

   assert!(
      !nodes
         .iter()
         .any(|(_, node)| node_texts(node).into_iter().any(|text| text.contains("Confrim"))),
      "the misspelling must be gone from every node"
   );
}

/// The key's copy button says which wallet's key it hands over.
#[test]
fn the_copy_button_names_the_wallet_it_copies_for() {
   let (ctx, mut gui, zeus) = main_window();
   let (wallet, info) = wallet_with_key(EXPORT_TEST_KEY, true);

   draw_export_dialog(&ctx, &mut gui, &zeus, wallet, &info);
   let nodes = draw_export_key(&ctx, &mut gui, &zeus);
   let nodes = open_export_body(&ctx, &mut gui, &zeus, nodes);

   for name in [
      "Copy key for Main, master wallet",
      "Show QR code for Main, master wallet",
   ] {
      assert!(
         nodes
            .iter()
            .any(|(_, node)| node.role() == Role::Button && node.label() == Some(name)),
         "the button must be named {name:?}: {:?}",
         drawn_names(&nodes)
      );
   }

   assert!(
      nodes.iter().any(|(_, node)| node.role() == Role::Label
         && node.value() == Some("Save this key in a safe place.")),
      "the body must say what to do with the key: {:?}",
      drawn_names(&nodes)
   );
}

/// A copy is said, because nothing else on screen changes when one happens.
#[test]
fn a_copy_from_the_export_dialog_is_said() {
   let (ctx, mut gui, zeus) = main_window();
   let (wallet, info) = wallet_with_key(EXPORT_TEST_KEY, true);

   draw_export_dialog(&ctx, &mut gui, &zeus, wallet, &info);
   let nodes = draw_export_key(&ctx, &mut gui, &zeus);
   let nodes = open_export_body(&ctx, &mut gui, &zeus, nodes);

   let copy = button_named(&nodes, "Copy key for Main, master wallet");

   assert_eq!(
      ctx.memory(|memory| memory.focused()).map(|id| id.accesskit_id()),
      Some(copy),
      "the first copy button must take the focus once the warning is gone"
   );

   let nodes = render_wallet_page(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, copy)],
   );

   assert_eq!(
      announced(&nodes).as_deref(),
      Some("Private key copied."),
      "the copy must reach the announcement node"
   );

   let clear = button_named(&nodes, "Clear Clipboard");
   let nodes = render_wallet_page(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, clear)],
   );

   assert_eq!(
      announced(&nodes).as_deref(),
      Some("Clipboard cleared."),
      "clearing the clipboard must say so"
   );
}

/// A master wallet's warning is heard as an alert, and the safe exit holds the focus so Enter backs
/// out of the export instead of revealing the key.
#[test]
fn the_master_warning_is_an_alert_and_the_safe_exit_takes_the_focus() {
   let (ctx, mut gui, zeus) = main_window();
   let (wallet, info) = wallet_with_key(EXPORT_TEST_KEY, true);

   draw_export_dialog(&ctx, &mut gui, &zeus, wallet, &info);
   let nodes = draw_export_key(&ctx, &mut gui, &zeus);

   let sentence = "This is your master wallet. If this key gets exposed, any child wallet you generated with this key will be compromised. You don't have to export this key unless something broke in Zeus and you cannot move your funds to other wallets.";

   let alert = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Alert)
      .map(|(_, node)| node)
      .unwrap_or_else(|| {
         panic!(
            "the warning must be an alert a reader is told: {:?}",
            drawn_names(&nodes)
         )
      });

   assert_eq!(
      alert.label(),
      Some(sentence),
      "the alert must carry all three sentences of the drawn paragraph in one breath"
   );

   let dismiss = button_named(&nodes, "Changed my mind");

   assert_eq!(
      ctx.memory(|memory| memory.focused()).map(|id| id.accesskit_id()),
      Some(dismiss),
      "the safe exit must hold the focus, so Enter backs out"
   );

   assert!(
      nodes
         .iter()
         .any(|(_, node)| node.role() == Role::Label && node.value() == Some("Warning")),
      "the drawn heading stays where it was"
   );
}

/// The key never enters the tree, and the QR that draws it stays a picture with no words.
#[test]
fn the_export_dialog_never_puts_the_key_in_the_tree() {
   let (ctx, mut gui, zeus) = main_window();
   let (wallet, info) = wallet_with_key(EXPORT_TEST_KEY, false);
   let key = wallet.key_string().unlock_str(|key| key.to_string());

   draw_export_dialog(&ctx, &mut gui, &zeus, wallet, &info);
   let nodes = draw_export_key(&ctx, &mut gui, &zeus);

   assert_no_secret(&nodes, &key, "the private key");

   let show_qr = button_named(&nodes, "Show QR code for Main, imported wallet");
   render_wallet_page(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, show_qr)],
   );

   // The picture is added after the button that asked for it, so the frame a reader is handed is
   // the one after the click.
   let nodes = render_wallet_page(&ctx, &mut gui, &zeus, Vec::new());

   assert_no_secret(&nodes, &key, "the private key");

   let images: Vec<&Node> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Image)
      .map(|(_, node)| node)
      .collect();

   assert_eq!(
      images.len(),
      1,
      "the dialog draws one picture, the key's QR"
   );

   // The dialog gives the QR no alt text, so the picture is never named: no word of the dialog's
   // reaches the node, and the only text on it is egui's own account of a picture it cannot decode
   // in a frame that drops the texture upload.
   let picture = images[0];

   assert!(
      node_texts(picture).is_empty(),
      "the key's QR must say nothing at all, not even its texture URI: {:?}",
      node_texts(picture)
   );

   for text in node_texts(picture) {
      assert!(
         !text.contains(&key),
         "the QR must not carry the key"
      );
      assert!(
         !text.contains(info.name()),
         "the QR must not be named after the wallet"
      );
   }
}

/// The seed phrase's button says whose phrase it is, says when the copy is done, and the phrase
/// stays out of the tree.
#[test]
fn a_seed_phrase_copy_is_named_and_said() {
   let (ctx, mut gui, zeus) = main_window();
   let phrase = "legal winner thank year wave sausage worth useful legal winner thank yellow";

   let wallet =
      zeus_wallet::Wallet::new_from_mnemonic("Main".to_string(), SecureString::from(phrase))
         .expect("the test phrase must build a wallet");
   let info = WalletInfo::from_wallet(&wallet, false);

   draw_export_dialog(&ctx, &mut gui, &zeus, wallet, &info);
   let nodes = draw_export_key(&ctx, &mut gui, &zeus);

   assert_no_secret(&nodes, phrase, "the seed phrase");

   let copy = button_named(
      &nodes,
      "Copy seed phrase for Main, imported wallet",
   );
   let nodes = render_wallet_page(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, copy)],
   );

   assert_eq!(
      announced(&nodes).as_deref(),
      Some("Seed phrase copied."),
      "the copy must reach the announcement node"
   );

   assert_no_secret(&nodes, phrase, "the seed phrase");
}
