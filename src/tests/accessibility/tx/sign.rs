use super::super::app::prompt_harness;
use super::super::harness::{
   drawn_names, frame_nodes, is_text_input, key_press, node_position, tree_order,
};
use super::confirmation::render_confirmation;
use crate::assets::icons::Icons;
use crate::core::signature::msg::Permit2612Details;
use crate::core::{SignMsgType, TransactionAnalysis, ZeusCtx};
use crate::gui::appearance::AppearanceSettings;
use crate::gui::ui::sign_msg_window::SignMsgWindow;
use crate::gui::ui::tx::TxConfirmationWindow;
use egui::accesskit::{Node, NodeId, Role};
use egui::{Context, Event, Key};
use egui_elements::Theme;
use std::sync::Arc;
use std::time::{Duration, Instant};
use zeus_eth::alloy_primitives::{Address, U256};
use zeus_eth::types::ChainId;
use zeus_eth::utils::NumericValue;

/// Draw the sign-message window into a reader's tree.
///
/// A local window stands in for the prompt — nothing claims it and no flow prepares it — and the
/// frame draws it out of a local context, which is exactly what `SignMsgWindow::show` does in the
/// app.
///
/// Three passes, for the same reason as the confirmation: a shrink-wrapping modal spends two
/// invisible sizing passes before the first frame it actually paints, and egui exposes no node
/// for a widget it considers invisible. The last pass is the one a reader is handed — and the
/// first one the window may take focus on.
fn render_sign(
   ctx: &Context,
   handle: &ZeusCtx,
   window: &mut SignMsgWindow,
   theme: &Theme,
   icons: &Arc<Icons>,
   appearance: &AppearanceSettings,
   events: Vec<Event>,
) -> Vec<(NodeId, Node)> {
   for pass in 0..3 {
      let events = if pass == 2 {
         events.clone()
      } else {
         Vec::new()
      };

      let nodes = frame_nodes(ctx, events, |ui| {
         handle.write(|c| window.show(c, theme, icons.clone(), appearance, ui));
      });

      if pass == 2 {
         return nodes;
      }
   }

   unreachable!("the third pass returns the tree")
}

/// An unlimited permit, as the prompt receives one: the crate's own fixture carrying the amount a
/// permit that never runs out grants.
fn unlimited_permit() -> SignMsgType {
   let mut details = Permit2612Details::dummy();
   details.amount = NumericValue::format_wei(U256::MAX, details.token.decimals);

   SignMsgType::Permit2612(details)
}

/// The window is about one signature, so it opens on a heading that names what is being signed —
/// not on the Cancel button the modal would otherwise leave the focus on.
#[test]
fn the_sign_window_opens_on_the_message_heading() {
   let (ctx, handle, theme, icons, appearance) = prompt_harness();
   let msg = SignMsgType::dummy_permit2612();
   let title = msg.title().to_string();
   let mut window = SignMsgWindow::for_test(
      "app.example.org",
      ChainId::default(),
      Address::repeat_byte(0x11),
      msg,
      None,
   );

   let nodes = render_sign(
      &ctx,
      &handle,
      &mut window,
      &theme,
      &icons,
      &appearance,
      Vec::new(),
   );

   let focused = ctx
      .memory(|memory| memory.focused())
      .expect("the opening frame must leave focus inside the window");
   let node = nodes
      .iter()
      .find(|(node_id, _)| *node_id == focused.accesskit_id())
      .map(|(_, node)| node)
      .expect("the focused node must be in the window's tree");

   assert_eq!(
      node.role(),
      Role::Heading,
      "the first focus is the message heading, not a field or a button: {:?}",
      node.role()
   );
   assert_eq!(
      node.label(),
      Some(title.as_str()),
      "the heading must name what is being signed"
   );

   // The action is read first: the origin, the message body and the buttons all follow it.
   let order = tree_order(&nodes);
   let heading_at = order
      .iter()
      .position(|id| *id == focused.accesskit_id())
      .expect("the heading is in the reading order");

   assert!(
      heading_at < node_position(&nodes, "Requested by"),
      "the action must come before who asked for it"
   );
   assert!(
      heading_at < node_position(&nodes, "Message"),
      "the action must come before the message body"
   );
}

/// The visible text runs under the one node a reader reaches under `spoken`.
///
/// A label draws its own text as a run beneath itself, and a reader can be handed that run as well
/// as the name: a row whose two strings differ is heard twice, which is how the fee and the origin
/// line came out doubled. A row that says one thing has one run, and it is the string itself.
fn drawn_runs(nodes: &[(NodeId, Node)], spoken: &str) -> Vec<String> {
   // A label and the run it draws carry the same string, so the role is what picks the node the
   // runs hang from.
   let Some((id, _)) = nodes.iter().find(|(_, node)| {
      node.role() == Role::Label && node.label().or_else(|| node.value()) == Some(spoken)
   }) else {
      return Vec::new();
   };

   runs_below(nodes, *id)
}

/// The longest run of hex characters in `text`: a shortening stays under twenty, while an
/// address spelled out is forty — a wall a listener cannot follow.
pub(super) fn longest_hex_run(text: &str) -> usize {
   let mut longest = 0;
   let mut current = 0;

   for character in text.chars() {
      if character.is_ascii_hexdigit() {
         current += 1;
         longest = longest.max(current);
      } else {
         current = 0;
      }
   }

   longest
}

/// The visible runs drawn beneath the node `id`, whatever role that node has.
///
/// A link draws its text the way a label does, so the runs hang from it just the same — which is
/// how an address link can say its shortening twice.
fn runs_below(nodes: &[(NodeId, Node)], id: NodeId) -> Vec<String> {
   let Some((_, node)) = nodes.iter().find(|(node_id, _)| *node_id == id) else {
      return Vec::new();
   };

   node
      .children()
      .iter()
      .filter_map(|child| nodes.iter().find(|(node_id, _)| node_id == child))
      .filter(|(_, child)| !child.is_hidden())
      .filter_map(|(_, child)| child.value())
      .map(str::to_string)
      .collect()
}

/// Who asked is named, not just drawn: the dapp's line says "Requested by <dapp>", and a prompt
/// with no origin line at all still answers the question with Zeus. The line draws the words it is
/// called, too, because a name written over a label's own glyphs is a second spelling of the row.
#[test]
fn the_sign_window_says_who_requested_it() {
   let (ctx, handle, theme, icons, appearance) = prompt_harness();

   let mut dapp = SignMsgWindow::for_test(
      "app.example.org",
      ChainId::default(),
      Address::repeat_byte(0x11),
      SignMsgType::dummy_permit2612(),
      None,
   );
   let nodes = render_sign(
      &ctx,
      &handle,
      &mut dapp,
      &theme,
      &icons,
      &appearance,
      Vec::new(),
   );

   let spoken = "Requested by app.example.org";

   assert!(
      nodes
         .iter()
         .any(|(_, node)| node.label().or_else(|| node.value()) == Some(spoken)),
      "the dapp's line must say who asked, not just show its name"
   );
   assert_eq!(
      drawn_runs(&nodes, spoken),
      vec![spoken.to_string()],
      "the line must draw the words it is called: a second spelling is read aloud beside it"
   );

   let mut in_app = SignMsgWindow::for_test(
      "",
      ChainId::default(),
      Address::repeat_byte(0x11),
      SignMsgType::dummy_permit2612(),
      None,
   );
   let nodes = render_sign(
      &ctx,
      &handle,
      &mut in_app,
      &theme,
      &icons,
      &appearance,
      Vec::new(),
   );

   assert!(
      nodes.iter().any(|(_, node)| node.label() == Some("Requested by Zeus")),
      "a request with no origin line must still say Zeus asked for it"
   );
}

/// The prompt says the same thing the eye sees: the origin line draws "Requested by <dapp>" and
/// the state-change titles draw their counts, so no row carries a name that is a second spelling of
/// its own glyphs. That second spelling is what a reader hears twice when a window opens.
#[test]
fn the_confirmation_draws_the_origin_and_the_titles_it_says() {
   let (ctx, handle, theme, icons, appearance) = prompt_harness();
   let analysis = TransactionAnalysis::dummy_with_diffs();
   let approvals = format!(
      "Approval Changes ({})",
      analysis.approval_diff.len()
   );
   let balances = format!(
      "Balance Changes ({})",
      analysis.balance_diff.len()
   );

   let mut window =
      TxConfirmationWindow::for_test(&handle, analysis, "app.uniswap.org", false, None);
   let nodes = render_confirmation(
      &ctx,
      &handle,
      &mut window,
      &theme,
      &icons,
      &appearance,
      Vec::new(),
   );

   for spoken in [
      "Requested by app.uniswap.org",
      approvals.as_str(),
      balances.as_str(),
   ] {
      assert_eq!(
         drawn_runs(&nodes, spoken),
         vec![spoken.to_string()],
         "{spoken:?} must be the one string the row shows and the one it is called"
      );
   }
}

/// The raw message is a viewer, not an editor: AT is given one named, read-only field, and the
/// drawn "Message" label does not say the word a second time.
#[test]
fn the_message_body_is_a_named_read_only_field() {
   let (ctx, handle, theme, icons, appearance) = prompt_harness();
   let mut window = SignMsgWindow::for_test(
      "app.example.org",
      ChainId::default(),
      Address::repeat_byte(0x11),
      SignMsgType::dummy_permit2612(),
      None,
   );

   let nodes = render_sign(
      &ctx,
      &handle,
      &mut window,
      &theme,
      &icons,
      &appearance,
      Vec::new(),
   );

   let field = nodes
      .iter()
      .find(|(_, node)| is_text_input(node.role()) && node.label() == Some("Message"))
      .map(|(_, node)| node)
      .expect("the message body must be a text field named Message");

   assert!(
      field.is_read_only(),
      "the field is a viewer: it must say it cannot be typed into"
   );
   assert_eq!(
      field.description(),
      Some("The raw message Zeus will sign."),
      "the field must say what it holds"
   );

   let named_message = nodes.iter().filter(|(_, node)| node.label() == Some("Message")).count();
   assert_eq!(
      named_message, 1,
      "exactly one node may be named Message: the field"
   );

   // The drawn label the field replaced is hidden, so the word is not said twice. Its text-run
   // children stay under that hidden node, which is what takes them out of the tree a reader is
   // handed too.
   let drawn: Vec<&Node> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Label && node.value() == Some("Message"))
      .map(|(_, node)| node)
      .collect();

   assert_eq!(
      drawn.len(),
      1,
      "the drawn Message label is the one node the field replaced"
   );
   assert!(
      drawn[0].is_hidden(),
      "the drawn Message label must be hidden so a reader does not hear the word twice"
   );
}

/// The countdown lives in the button's description, never its name: a reader hears "Sign"
/// throughout, the wait while it runs, and one sentence when it ends.
#[test]
fn the_sign_countdown_keeps_its_name_and_says_when_it_opens() {
   let (ctx, handle, theme, icons, appearance) = prompt_harness();

   let mut waiting = SignMsgWindow::for_test(
      "app.example.org",
      ChainId::default(),
      Address::repeat_byte(0x11),
      SignMsgType::dummy_permit2612(),
      Some(Instant::now()),
   );
   let nodes = render_sign(
      &ctx,
      &handle,
      &mut waiting,
      &theme,
      &icons,
      &appearance,
      Vec::new(),
   );
   let sign = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some("Sign"))
      .map(|(_, node)| node)
      .expect("the Sign button must be in the tree");

   assert_eq!(
      sign.label(),
      Some("Sign"),
      "the name must not become the countdown"
   );
   assert!(
      sign.description().unwrap_or_default().starts_with("Available in"),
      "the wait belongs in the description: {:?}",
      sign.description()
   );
   assert_eq!(
      waiting.take_notice(),
      None,
      "nothing is said while the wait is still running"
   );

   let mut ready = SignMsgWindow::for_test(
      "app.example.org",
      ChainId::default(),
      Address::repeat_byte(0x11),
      SignMsgType::dummy_permit2612(),
      Some(Instant::now() - Duration::from_secs(3)),
   );
   let nodes = render_sign(
      &ctx,
      &handle,
      &mut ready,
      &theme,
      &icons,
      &appearance,
      Vec::new(),
   );
   let sign = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some("Sign"))
      .map(|(_, node)| node)
      .expect("the Sign button must still be named Sign once it opens");

   assert_eq!(sign.label(), Some("Sign"));
   assert!(
      !sign.description().unwrap_or_default().starts_with("Available in"),
      "a wait that is over must not still be described: {:?}",
      sign.description()
   );

   assert_eq!(
      ready.take_notice().as_deref(),
      Some("Sign is now available."),
      "the end of the wait is said exactly once"
   );

   render_sign(
      &ctx,
      &handle,
      &mut ready,
      &theme,
      &icons,
      &appearance,
      Vec::new(),
   );
   assert_eq!(
      ready.take_notice(),
      None,
      "the same wait must not be announced again on the next frame"
   );
}

/// Escape is the window's way out: it cancels the signature, closes the prompt, and says what
/// happened to the flow waiting for an answer.
#[test]
fn escape_cancels_the_signature_and_closes_it() {
   let (ctx, handle, theme, icons, appearance) = prompt_harness();
   let mut window = SignMsgWindow::for_test(
      "app.example.org",
      ChainId::default(),
      Address::repeat_byte(0x11),
      SignMsgType::dummy_permit2612(),
      None,
   );

   render_sign(
      &ctx,
      &handle,
      &mut window,
      &theme,
      &icons,
      &appearance,
      Vec::new(),
   );
   render_sign(
      &ctx,
      &handle,
      &mut window,
      &theme,
      &icons,
      &appearance,
      vec![key_press(Key::Escape)],
   );

   assert!(!window.is_open(), "Escape must close the window");
   assert_eq!(
      window.take_signed(),
      Some(false),
      "Escape must answer the awaiting flow with a cancel"
   );
   assert_eq!(
      window.take_notice().as_deref(),
      Some("Message not signed."),
      "the cancel is one of the window's sentences"
   );
}

/// An unlimited allowance is not only red: the spender and the grant are an Alert node before the
/// permit rows, so the sentence survives a reader that cannot see the title's colour.
#[test]
fn an_unlimited_permit_is_spoken_not_only_coloured() {
   let (ctx, handle, theme, icons, appearance) = prompt_harness();
   let mut window = SignMsgWindow::for_test(
      "app.example.org",
      ChainId::default(),
      Address::repeat_byte(0x11),
      unlimited_permit(),
      None,
   );

   let nodes = render_sign(
      &ctx,
      &handle,
      &mut window,
      &theme,
      &icons,
      &appearance,
      Vec::new(),
   );

   let alert = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Alert)
      .map(|(_, node)| node)
      .expect("an unlimited allowance must warn with an Alert node");

   let spoken = alert.label().or_else(|| alert.description()).unwrap_or_default();

   assert!(
      spoken.contains("Unlimited"),
      "the warning must say the allowance is unlimited: {spoken:?}"
   );
   assert!(
      spoken.contains("take all of this token"),
      "the warning must say what the signature gives away: {spoken:?}"
   );
   assert!(
      node_position(&nodes, "Unlimited allowance.") < node_position(&nodes, "Approve Token"),
      "the warning must be read before the permit rows it warns about"
   );
}

/// An address the wallet has no name for is read by the ends Zeus draws, never spelled out in hex.
///
/// Forty characters, one at a time, is a spelling nobody can follow, and the row that draws a
/// shortening would say it twice — once as the node's name and once as the glyphs under it. The
/// name is the shortening and the drawn run is the same string, so the row is heard once; the whole
/// address stays on the node as its description, for whoever asks for it.
#[test]
fn an_unnamed_spender_is_read_by_its_ends_and_not_spelled_out() {
   let (ctx, handle, theme, icons, appearance) = prompt_harness();
   let mut details = Permit2612Details::dummy();
   details.amount = NumericValue::format_wei(U256::MAX, details.token.decimals);

   let spender = details.spender.to_string();
   let mut window = SignMsgWindow::for_test(
      "app.example.org",
      ChainId::default(),
      Address::repeat_byte(0x11),
      SignMsgType::Permit2612(details),
      None,
   );

   let nodes = render_sign(
      &ctx,
      &handle,
      &mut window,
      &theme,
      &icons,
      &appearance,
      Vec::new(),
   );

   // The fixture's spender is unnamed, so the ends are all a listener can be given.
   let drawn = "0x2222...222222";
   let whole = "0x2222 2222 2222 2222 2222 2222 2222 2222 2222 2222";

   let alert = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Alert)
      .map(|(_, node)| node)
      .expect("an unlimited permit must warn with an Alert node");

   let sentence = alert.label().or_else(|| alert.description()).unwrap_or_default();

   assert!(
      !sentence.replace(' ', "").contains(&spender),
      "the warning must not spell the spender's hex, which is a wall of characters by ear: \
{sentence:?}"
   );
   assert!(
      sentence.contains(drawn),
      "the warning must name the spender the way the row below it does: {sentence:?}"
   );

   let (id, row) =
      nodes.iter().find(|(_, node)| node.label() == Some(drawn)).unwrap_or_else(|| {
         panic!(
            "the spender row must be named by its ends, not by the whole address: {:?}",
            drawn_names(&nodes)
         )
      });

   assert!(
      !row.is_hidden(),
      "the row a reader hears must be the one on screen"
   );
   assert_eq!(
      row.description(),
      Some(whole),
      "the whole address stays on the node, for whoever asks for it"
   );
   assert_eq!(
      runs_below(&nodes, *id),
      vec![drawn.to_string()],
      "the row draws the one string it is called, so it is not heard twice"
   );
}

/// An alert says its sentence once.
///
/// A label's words sit in its node's value, and a widget given the alert role needs them as the
/// name too: naming them over the value left the sentence in both places, and the Windows reader
/// said both — which is how the unlimited-allowance warning came out doubled for a second time. The
/// name is what an alert is announced by, so the value is let go.
#[test]
fn an_alert_does_not_say_its_sentence_twice() {
   let (ctx, handle, theme, icons, appearance) = prompt_harness();

   let mut signing = SignMsgWindow::for_test(
      "app.example.org",
      ChainId::default(),
      Address::repeat_byte(0x11),
      unlimited_permit(),
      None,
   );

   let signed = render_sign(
      &ctx,
      &handle,
      &mut signing,
      &theme,
      &icons,
      &appearance,
      Vec::new(),
   );

   let mut confirming = TxConfirmationWindow::for_test(
      &handle,
      TransactionAnalysis::dummy_permit(),
      "app.example.org",
      false,
      None,
   );

   let confirmed = render_confirmation(
      &ctx,
      &handle,
      &mut confirming,
      &theme,
      &icons,
      &appearance,
      Vec::new(),
   );

   for (prompt, nodes) in [
      ("the signing prompt", signed),
      ("the confirmation", confirmed),
   ] {
      let alert = nodes
         .iter()
         .find(|(_, node)| node.role() == Role::Alert)
         .map(|(_, node)| node)
         .unwrap_or_else(|| panic!("{prompt} must warn with an Alert node"));

      let sentence = alert.label().unwrap_or_default();

      assert!(
         sentence.starts_with("Unlimited allowance."),
         "{prompt}'s alert must be named by its sentence: {sentence:?}"
      );
      assert!(
         alert.value().unwrap_or_default().is_empty(),
         "{prompt} says its sentence as its name and as its value, which a reader hears twice: {:?}",
         alert.value()
      );
   }
}
