use super::app::{main_window, wallet_in_cache};
use super::harness::{frame, is_text_input};
use crate::core::ZeusCtx;
use crate::gui::GUI;
use egui::Context;
use egui::accesskit::{Node, Role};
use zeus_eth::alloy_primitives::Address;

/// Draw the Send form the way the central panel does, and return its tree.
///
/// The panel gates the pages on the unlocked vault; the form itself does not, so a context with no
/// vault still draws every node a reader has to land on.
fn render_send(ctx: &Context, gui: &mut GUI, zeus: &ZeusCtx) -> Vec<Node> {
   let mut nodes = Vec::new();

   // The form measures its frame on the first pass and draws into it on the second.
   for _ in 0..2 {
      nodes = frame(ctx, |ui| {
         let icons = gui.icons.clone();
         let theme = gui.theme.clone();
         let send = &mut gui.send_crypto;
         let token_selection = &mut gui.token_selection;
         let recipient_selection = &mut gui.recipient_selection;
         let contacts = &mut gui.settings.contacts_ui;

         zeus.write(|zeus| {
            send.show(
               zeus,
               icons,
               &theme,
               token_selection,
               recipient_selection,
               contacts,
               ui,
            )
         });
      });
   }

   nodes
}

/// The reason a Send is blocked used to be the disabled button's label, which no reader can reach.
/// It is a line of its own now, said once when it appears, and the button keeps its name.
#[test]
fn a_blocked_send_says_why_beside_a_button_named_send() {
   let (ctx, mut gui, zeus) = main_window();
   wallet_in_cache(&zeus);

   gui.send_crypto.open();
   gui.send_crypto.amount_field.amount = "0.5".to_string();

   let nodes = render_send(&ctx, &mut gui, &zeus);

   let send = nodes
      .iter()
      .find(|node| node.role() == Role::Button && node.label() == Some("Send"))
      .expect("the form must draw its Send button");

   assert!(
      send.is_disabled(),
      "the button must be blocked, which is the state this test is about"
   );

   assert!(
      nodes
         .iter()
         .any(|node| node.role() == Role::Heading && node.label() == Some("Send Crypto")),
      "the form's own title must be a heading of the page"
   );

   let reason = "Insufficient ETH Balance";

   let spoken: Vec<&str> = nodes
      .iter()
      .filter(|node| node.role() == Role::Label && !node.is_hidden())
      .filter_map(|node| node.value())
      .collect();

   assert!(
      spoken.contains(&reason),
      "the reason must be a node of its own, reachable while the button is not: {spoken:?}"
   );

   let line = nodes
      .iter()
      .find(|node| node.role() == Role::Label && node.value() == Some(reason))
      .and_then(|node| node.bounds())
      .expect("the reason is drawn, so its node has bounds");

   let button = send.bounds().expect("the button is drawn, so its node has bounds");

   assert!(
      ((line.x0 + line.x1) - (button.x0 + button.x1)).abs() < 1.0,
      "the line must sit in the button's own column: {line:?} against {button:?}"
   );

   assert!(
      line.y1 <= button.y0,
      "the line must be drawn above the button: {line:?} against {button:?}"
   );

   assert_eq!(
      gui.send_crypto.take_notice().as_deref(),
      Some(reason),
      "the reason must be said when it appears"
   );

   // The same block a frame later is not a second announcement.
   render_send(&ctx, &mut gui, &zeus);

   assert_eq!(
      gui.send_crypto.take_notice(),
      None,
      "a reason that has not changed says nothing twice"
   );

   // A different reason is a new sentence, drawn and said.
   gui.send_crypto.amount_field.amount = "abc".to_string();

   let nodes = render_send(&ctx, &mut gui, &zeus);

   let reason = "Amount is not valid";

   let spoken: Vec<&str> = nodes
      .iter()
      .filter(|node| node.role() == Role::Label && !node.is_hidden())
      .filter_map(|node| node.value())
      .collect();

   assert!(
      spoken.contains(&reason),
      "the new reason must be the line that is drawn: {spoken:?}"
   );

   assert_eq!(
      gui.send_crypto.take_notice().as_deref(),
      Some(reason),
      "a reason that changed is a new sentence"
   );
}

/// The NFT side of the form is named as well: the selector says what it changes, the quantity field
/// is the amount, and the same line explains why Send is blocked.
#[test]
fn the_nft_send_form_names_its_selector_and_amount() {
   use zeus_eth::alloy_primitives::U256;
   use zeus_eth::nft::{NftStandard, NftToken};

   let (ctx, mut gui, zeus) = main_window();
   wallet_in_cache(&zeus);

   gui.send_crypto.open();
   gui.send_crypto.set_nft(NftToken {
      chain_id: 1,
      collection: Address::from([0xbc; 20]),
      token_id: U256::from(7),
      standard: NftStandard::Erc1155,
      metadata_uri: None,
   });

   let nodes = render_send(&ctx, &mut gui, &zeus);

   assert!(
      nodes
         .iter()
         .any(|node| node.role() == Role::Button && node.label() == Some("Change NFT")),
      "the selector must say what it changes"
   );

   let amount = nodes
      .iter()
      .find(|node| is_text_input(node.role()))
      .expect("the quantity field must be a text field in the tree");

   assert_eq!(
      amount.label(),
      Some("Amount"),
      "the quantity field is the amount of the token picked above"
   );

   let drawn_amount = nodes
      .iter()
      .find(|node| node.role() == Role::Label && node.value() == Some("Amount"))
      .expect("the drawn quantity label must stay in the tree, hidden");

   assert!(
      drawn_amount.is_hidden(),
      "the quantity field already says Amount, so the drawn word must not be read again"
   );

   assert!(
      nodes
         .iter()
         .any(|node| node.role() == Role::Button && node.label() == Some("Send")),
      "the button keeps its name on this path too"
   );

   let spoken: Vec<&str> = nodes
      .iter()
      .filter(|node| node.role() == Role::Label && !node.is_hidden())
      .filter_map(|node| node.value())
      .collect();

   assert!(
      spoken.contains(&"Amount is not valid"),
      "the reason a block is drawn in the same place on this path: {spoken:?}"
   );
}

/// The announcement rule the Send form follows: a reason that appears or changes is a new sentence,
/// the same reason a frame later is not, and a block that lifts says nothing.
#[test]
fn the_send_block_is_announced_when_it_appears_or_changes() {
   use crate::gui::ui::common::blocked_announcement;

   for (previous, current, expected) in [
      (
         None,
         Some("Amount is not valid"),
         Some("Amount is not valid"),
      ),
      (
         Some("Amount is not valid"),
         Some("Amount is not valid"),
         None,
      ),
      (
         Some("Amount is not valid"),
         Some("Insufficient ETH Balance"),
         Some("Insufficient ETH Balance"),
      ),
      (Some("Amount is not valid"), None, None),
      (None, None, None),
   ] {
      assert_eq!(
         blocked_announcement(previous, current).as_deref(),
         expected,
         "{previous:?} then {current:?}"
      );
   }
}
