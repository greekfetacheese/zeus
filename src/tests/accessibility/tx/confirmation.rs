use super::super::app::prompt_harness;
use super::super::harness::{drawn_names, frame_nodes, key_press, node_position, spoken_text};
use super::prompt::dialogs;
use super::sign::longest_hex_run;
use crate::assets::icons::Icons;
use crate::core::{TransactionAnalysis, ZeusCtx};
use crate::gui::appearance::AppearanceSettings;
use crate::gui::ui::tx::{TxConfirmationWindow, confrim_window::PromptSheet};
use egui::accesskit::{Node, NodeId, Role};
use egui::{Context, Event, Key};
use egui_elements::Theme;
use std::sync::Arc;
use std::time::{Duration, Instant};
use zeus_eth::alloy_primitives::U256;

/// Draw the confirmation window into a reader's tree.
///
/// A local window stands in for the prompt — nothing claims it and no task prepares it — and the
/// frame draws it out of a local context, which is exactly what `TxConfirmationWindow::show`
/// does in the app.
///
/// Three passes: a shrink-wrapping modal spends two invisible sizing passes before the first
/// frame it actually paints, and egui exposes no node for a widget it considers invisible. The
/// last pass is the one a reader is handed — and the first one the window may take focus on.
pub(super) fn render_confirmation(
   ctx: &Context,
   handle: &ZeusCtx,
   window: &mut TxConfirmationWindow,
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

/// Approvals are the only state change in the window that can cost money after the transaction,
/// so they are drawn — and therefore heard — before the balances, and the fee comes after both.
#[test]
fn the_confirmation_reads_approvals_before_balances_and_the_fee_last() {
   let (ctx, handle, theme, icons, appearance) = prompt_harness();
   let mut window = TxConfirmationWindow::for_test(
      &handle,
      TransactionAnalysis::dummy_with_diffs(),
      "",
      false,
      None,
   );

   let nodes = render_confirmation(
      &ctx,
      &handle,
      &mut window,
      &theme,
      &icons,
      &appearance,
      Vec::new(),
   );

   let approvals = node_position(&nodes, "Approval Changes (");
   let balances = node_position(&nodes, "Balance Changes (");
   let fee = node_position(&nodes, "Network fee");

   assert!(
      approvals < balances,
      "the approvals must be heard before the balances: {approvals} vs {balances}"
   );
   assert!(
      balances < fee,
      "the fee must come after the state changes: {balances} vs {fee}"
   );
}

/// The fee row says the amount once. Its title names the figure beside it, and the figure is drawn
/// as the ticker the page is laid out around and said as the token's name, with the hedge the words
/// need when they are heard: "about" is spoken where the other rows draw a tilde. The glyphs are
/// painted, so the tree holds the sentence once and the ticker nowhere.
#[test]
fn the_fee_is_said_once() {
   let (ctx, handle, theme, icons, appearance) = prompt_harness();
   let mut window = TxConfirmationWindow::for_test(
      &handle,
      TransactionAnalysis::dummy_with_diffs(),
      "",
      false,
      None,
   );

   let nodes = render_confirmation(
      &ctx,
      &handle,
      &mut window,
      &theme,
      &icons,
      &appearance,
      Vec::new(),
   );

   let spoken = spoken_text(&nodes);
   let titles = spoken.iter().filter(|text| **text == "Network fee").count();
   let figures = spoken.iter().filter(|text| text.starts_with("0.00 Ethereum, about $")).count();

   assert_eq!(
      titles, 1,
      "the row's title must name the figure: {spoken:?}"
   );
   assert_eq!(
      figures, 1,
      "the figure must be said once: {spoken:?}"
   );
   assert!(
      !spoken.contains(&"Cost"),
      "a title that only says \"cost\" was the row's second spelling: {spoken:?}"
   );

   let (_, figure) = nodes
      .iter()
      .find(|(_, node)| {
         node.role() == Role::Label
            && node.value().is_some_and(|text| text.starts_with("0.00 Ethereum, about $"))
      })
      .expect("the figure must be a label of its own");

   assert_eq!(
      figure.label(),
      figure.value(),
      "the row is named with the words it is read in"
   );
   assert!(
      figure.children().is_empty(),
      "the glyphs are painted, so the row draws nothing under it: {:?}",
      figure.children()
   );
   assert!(
      !drawn_names(&nodes).iter().any(|text| text.starts_with("0.00 ETH, $")),
      "the fee's ticker is painted, so no node spells it: {:?}",
      drawn_names(&nodes)
   );
}

/// A reader hears the action first: the window opens with focus on the action heading, not on
/// the Close button the modal would otherwise leave it on.
#[test]
fn the_confirmation_opens_on_the_action_heading() {
   let (ctx, handle, theme, icons, appearance) = prompt_harness();
   let analysis = TransactionAnalysis::dummy_transfer();
   let action = analysis
      .infer_main_event(handle.clone(), handle.chain().id())
      .name()
      .to_string();

   let mut window = TxConfirmationWindow::for_test(&handle, analysis, "", false, None);
   let nodes = render_confirmation(
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
      "the first focus is the action heading, not a button: {:?}",
      node.role()
   );
   assert_eq!(
      node.label(),
      Some(action.as_str()),
      "the focused heading must name the action"
   );
   assert_ne!(
      node.label(),
      Some("Close"),
      "focus must not open on Close"
   );

   assert!(
      nodes.iter().any(|(_, node)| node.label() == Some("Requested by Zeus")),
      "an in-app transaction must say Zeus asked for it"
   );
}

/// A wallet that cannot pay hears why Confirm is disabled above the button, and the disabled
/// button itself carries the same sentence: the reason never lives only in a label inside it.
#[test]
fn a_short_balance_says_why_confirm_is_disabled() {
   let (ctx, handle, theme, icons, appearance) = prompt_harness();

   let mut analysis = TransactionAnalysis::dummy_transfer();
   analysis.value = U256::from(1_000_000_000_000_000_000u64);

   let mut window = TxConfirmationWindow::for_test(&handle, analysis, "", false, None);
   let nodes = render_confirmation(
      &ctx,
      &handle,
      &mut window,
      &theme,
      &icons,
      &appearance,
      Vec::new(),
   );

   let confirm = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some("Confirm"))
      .map(|(_, node)| node)
      .expect("the Confirm button must be in the tree");

   let description = confirm.description().unwrap_or_default();
   let sentence = "Insufficient ETH. This wallet cannot cover the amount plus the network fee.";

   assert_eq!(
      confirm.label(),
      Some("Confirm"),
      "the reason must not replace the button's name"
   );
   assert!(
      description.contains(sentence),
      "the disabled button must carry the reason: {description:?}"
   );
   assert!(
      nodes.iter().any(|(_, node)| {
         node.label().or_else(|| node.value()).is_some_and(|text| text == sentence)
      }),
      "the reason must also be drawn as a line above the buttons"
   );
}

/// Escape is a rejection: the dismissal sticks, the awaiting flow gets its answer, and the
/// sentence the flow's caller reads says what happened.
#[test]
fn escape_rejects_the_confirmation_and_closes_it() {
   let (ctx, handle, theme, icons, appearance) = prompt_harness();
   let mut window = TxConfirmationWindow::for_test(
      &handle,
      TransactionAnalysis::dummy_transfer(),
      "",
      false,
      None,
   );

   render_confirmation(
      &ctx,
      &handle,
      &mut window,
      &theme,
      &icons,
      &appearance,
      Vec::new(),
   );
   render_confirmation(
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
      window.take_confirmed_or_rejected(),
      Some(false),
      "Escape must answer the awaiting flow with a rejection"
   );
   assert_eq!(
      window.take_notice().as_deref(),
      Some("Transaction rejected."),
      "the rejection is one of the window's sentences"
   );
}

/// The first Escape belongs to the sheet drawn over the prompt: it closes the events, the
/// calldata, the state changes or the options and leaves the transaction still waiting for an
/// answer. A prompt that went with the sheet rejected a transaction the reader never got to read.
#[test]
fn an_escape_closes_the_sheet_and_not_the_confirmation() {
   for (sheet, title) in [
      (PromptSheet::Events, "Decoded Events"),
      (PromptSheet::Calldata, "Calldata"),
      (PromptSheet::StateChanges, "State Changes"),
      (PromptSheet::Options, "Transaction Options"),
   ] {
      let (ctx, handle, theme, icons, appearance) = prompt_harness();
      let mut window = TxConfirmationWindow::for_test(
         &handle,
         TransactionAnalysis::dummy_with_diffs(),
         "",
         false,
         None,
      )
      .with_sheet(sheet);

      let nodes = render_confirmation(
         &ctx,
         &handle,
         &mut window,
         &theme,
         &icons,
         &appearance,
         Vec::new(),
      );

      assert!(
         dialogs(&nodes).iter().any(|name| name == title),
         "the {title} sheet must be drawn over the prompt before the key is sent: {:?}",
         dialogs(&nodes),
      );

      render_confirmation(
         &ctx,
         &handle,
         &mut window,
         &theme,
         &icons,
         &appearance,
         vec![key_press(Key::Escape)],
      );

      assert!(
         window.is_open(),
         "the Escape that closed the {title} sheet must not close the prompt"
      );
      assert_eq!(
         window.take_confirmed_or_rejected(),
         None,
         "a sheet that closes answers nothing: the transaction is still awaiting its answer"
      );

      // The frame the key lands in still paints the sheet it closed: the tree is read one frame
      // later, where the sheet is gone.
      let nodes = render_confirmation(
         &ctx,
         &handle,
         &mut window,
         &theme,
         &icons,
         &appearance,
         Vec::new(),
      );

      assert!(
         !dialogs(&nodes).iter().any(|name| name == title),
         "Escape must close the {title} sheet: {:?}",
         dialogs(&nodes),
      );
   }
}

/// The countdown lives in the button's description, never its name: a reader hears "Confirm"
/// throughout, the wait while it runs, and one sentence when it ends.
#[test]
fn the_confirm_countdown_keeps_its_name_and_says_when_it_opens() {
   let (ctx, handle, theme, icons, appearance) = prompt_harness();

   let mut waiting = TxConfirmationWindow::for_test(
      &handle,
      TransactionAnalysis::dummy_transfer(),
      "",
      false,
      Some(Instant::now()),
   );
   let nodes = render_confirmation(
      &ctx,
      &handle,
      &mut waiting,
      &theme,
      &icons,
      &appearance,
      Vec::new(),
   );
   let confirm = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some("Confirm"))
      .map(|(_, node)| node)
      .expect("the Confirm button must be in the tree");

   assert_eq!(
      confirm.label(),
      Some("Confirm"),
      "the name must not become the countdown"
   );
   assert!(
      confirm.description().unwrap_or_default().starts_with("Available in"),
      "the wait belongs in the description: {:?}",
      confirm.description()
   );
   assert_eq!(
      waiting.take_notice(),
      None,
      "nothing is said while the wait is still running"
   );

   let mut ready = TxConfirmationWindow::for_test(
      &handle,
      TransactionAnalysis::dummy_transfer(),
      "",
      false,
      Some(Instant::now() - Duration::from_secs(3)),
   );
   let nodes = render_confirmation(
      &ctx,
      &handle,
      &mut ready,
      &theme,
      &icons,
      &appearance,
      Vec::new(),
   );
   let confirm = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some("Confirm"))
      .map(|(_, node)| node)
      .expect("the Confirm button must still be named Confirm once it opens");

   assert_eq!(confirm.label(), Some("Confirm"));

   assert_eq!(
      ready.take_notice().as_deref(),
      Some("Confirm is now available."),
      "the end of the wait is said exactly once"
   );

   render_confirmation(
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

/// An unlimited allowance is not only red: the spender and the grant are an Alert node before the
/// main event, so the sentence survives a reader that cannot see the heading's colour.
#[test]
fn an_unlimited_approval_is_spoken_not_only_coloured() {
   let (ctx, handle, theme, icons, appearance) = prompt_harness();
   let mut window = TxConfirmationWindow::for_test(
      &handle,
      TransactionAnalysis::dummy_permit(),
      "",
      false,
      None,
   );

   let nodes = render_confirmation(
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
      .expect("an unlimited approval must warn with an Alert node");

   let spoken = alert.label().or_else(|| alert.description()).unwrap_or_default();

   assert!(
      spoken.contains("Unlimited"),
      "the warning must say the allowance is unlimited: {spoken:?}"
   );
   assert!(
      longest_hex_run(spoken) < 20,
      "the warning must name the spender by its ends, not spell its address out: {spoken:?}"
   );
}
