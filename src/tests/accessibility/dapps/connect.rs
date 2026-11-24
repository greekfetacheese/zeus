use super::super::app::{main_window, master_wallet, wallet_named};
use super::super::harness::{accesskit_action, drawn_names, frame, frame_nodes, key_press};
use crate::core::WalletInfo;
use crate::gui::GUI;
use crate::gui::ui::dapps::connect::{ConnectDappWindow, DappAccountChoice, DappConnectResult};
use egui::accesskit::{Action, Node, NodeId, Role, Toggled};
use egui::{Context, Event, Key};
use egui_elements::Theme;
use std::time::{Duration, Instant};
use zeus_eth::alloy_primitives::Address;

/// Draw the connect prompt into a reader's tree.
///
/// Two passes: the prompt is a `Modal`, which is measured on its first one and draws on the
/// second, and the events go to the last pass so a click is not sent twice.
fn render_connect(
   ctx: &Context,
   window: &mut ConnectDappWindow,
   theme: &Theme,
   events: Vec<Event>,
) -> Vec<(NodeId, Node)> {
   let mut nodes = Vec::new();

   for pass in 0..2 {
      let events = if pass == 1 {
         events.clone()
      } else {
         Vec::new()
      };

      nodes = frame_nodes(ctx, events, |ui| {
         window.show(theme, ui);
      });
   }

   nodes
}

/// The connect prompt, opened for `origin` on a frame of its own.
fn connect_prompt(
   origin: &str,
   remembered: Option<Address>,
   seen: Vec<Address>,
   wallets: Vec<WalletInfo>,
) -> (Context, GUI, Theme) {
   let (ctx, mut gui, _) = main_window();

   gui.connect_dapp_window.open(origin.to_string(), remembered, seen, wallets);

   let theme = gui.theme.clone();

   (ctx, gui, theme)
}

/// The node a reader hears as `name` among the prompt's radio options.
fn connect_option(nodes: &[(NodeId, Node)], name: &str) -> NodeId {
   nodes
      .iter()
      .find(|(_, node)| node.role() == Role::RadioButton && node.label() == Some(name))
      .map(|(id, _)| *id)
      .unwrap_or_else(|| {
         panic!(
            "the prompt must offer {name:?} as a radio button: {:?}",
            drawn_names(nodes)
         )
      })
}

/// The prompt says which app is asking before it says anything else, and what answering costs: the
/// origin is a sentence of its own under the heading, and the paragraph says the app will see the
/// chosen account's address.
#[test]
fn the_connect_prompt_names_the_app_that_is_asking() {
   let (ctx, mut gui, theme) = connect_prompt("app.example", None, Vec::new(), Vec::new());

   let nodes = render_connect(
      &ctx,
      &mut gui.connect_dapp_window,
      &theme,
      Vec::new(),
   );

   assert!(
      nodes
         .iter()
         .any(|(_, node)| node.role() == Role::Dialog && node.label() == Some("Connect to App")),
      "the prompt must be a dialog named for what it is: {:?}",
      drawn_names(&nodes)
   );

   let labels: Vec<&str> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Label)
      .filter_map(|(_, node)| node.value())
      .collect();

   assert!(
      labels.contains(&"app.example is asking for one account."),
      "the app that is asking must be a sentence, not a bare origin: {labels:?}"
   );

   assert!(
      labels
         .iter()
         .any(|text| text.contains("It will see the address of the account you choose here.")),
      "the paragraph must say what the app is given: {labels:?}"
   );
}

/// Both choices are options a reader can pick and hear as chosen: two radio buttons named by the
/// words they draw, one of them saying it is the selected one.
#[test]
fn the_two_choices_are_options_with_a_chosen_one() {
   let (ctx, mut gui, theme) = connect_prompt(
      "app.example",
      None,
      Vec::new(),
      vec![master_wallet("Main")],
   );

   let nodes = render_connect(
      &ctx,
      &mut gui.connect_dapp_window,
      &theme,
      Vec::new(),
   );

   let options: Vec<&Node> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::RadioButton)
      .map(|(_, node)| node)
      .collect();

   assert_eq!(
      options.len(),
      2,
      "the prompt offers exactly two choices: {:?}",
      drawn_names(&nodes)
   );

   let toggled = |name: &str| {
      options
         .iter()
         .find(|node| node.label() == Some(name))
         .and_then(|node| node.toggled())
   };

   assert_eq!(
      toggled("Create a new account for this app"),
      Some(Toggled::True),
      "a new app opens on the account made for it"
   );

   assert_eq!(
      toggled("Use an existing account"),
      Some(Toggled::False),
      "the option that is not chosen must say so"
   );
}

/// The page is given an address, not a name, so every row says which account it is in speech and
/// carries the whole grouped address as its description: a listener can tell the rows apart and
/// knows which one the app would see.
#[test]
fn a_connect_wallet_row_says_which_account_the_app_would_see() {
   let master = master_wallet("Main");
   let mut child = wallet_named("Spending");
   child.is_child = true;
   child.is_imported = false;

   let (ctx, mut gui, theme) = connect_prompt(
      "app.example",
      Some(child.address),
      vec![child.address],
      vec![master, child.clone()],
   );

   let nodes = render_connect(
      &ctx,
      &mut gui.connect_dapp_window,
      &theme,
      Vec::new(),
   );

   let (_, row) = nodes
      .iter()
      .find(|(_, node)| {
         node.role() == Role::RadioButton
            && node.label() == Some("Spending, child wallet, previously connected")
      })
      .unwrap_or_else(|| {
         panic!(
            "the row must say which account and that the app knows it: {:?}",
            drawn_names(&nodes)
         )
      });

   let description = row.description().expect("a row's address must reach a listener");

   assert!(
      description.contains("0x"),
      "the description must be the whole address: {description:?}"
   );

   assert!(
      description.split(' ').count() > 1,
      "the address must be grouped so it can be followed by ear: {description:?}"
   );

   assert_eq!(
      row.toggled(),
      Some(Toggled::True),
      "the account this app was last connected with is the chosen row"
   );

   let (_, other) = nodes
      .iter()
      .find(|(_, node)| {
         node.role() == Role::RadioButton && node.label() == Some("Main, master wallet")
      })
      .expect("every other wallet must be a row of its own");

   assert_eq!(
      other.toggled(),
      Some(Toggled::False),
      "the row that is not remembered must not claim to be chosen"
   );
}

/// A reader can make the whole decision: choose an existing account, choose the wallet, and
/// connect with it — and the answer names the address the page would see.
#[test]
fn connecting_with_an_existing_account_exposes_that_address() {
   let master = master_wallet("Main");
   let mut child = wallet_named("Spending");
   child.is_child = true;
   child.is_imported = false;

   let (ctx, mut gui, theme) = connect_prompt(
      "app.example",
      None,
      Vec::new(),
      vec![master, child.clone()],
   );

   gui.connect_dapp_window
      .set_opened_at_for_test(Some(Instant::now() - Duration::from_secs(3)));

   let nodes = render_connect(
      &ctx,
      &mut gui.connect_dapp_window,
      &theme,
      Vec::new(),
   );
   let existing = connect_option(&nodes, "Use an existing account");

   let nodes = render_connect(
      &ctx,
      &mut gui.connect_dapp_window,
      &theme,
      vec![accesskit_action(Action::Click, existing)],
   );
   let wallet = connect_option(&nodes, "Spending, child wallet");

   render_connect(
      &ctx,
      &mut gui.connect_dapp_window,
      &theme,
      vec![accesskit_action(Action::Click, wallet)],
   );

   let nodes = render_connect(
      &ctx,
      &mut gui.connect_dapp_window,
      &theme,
      Vec::new(),
   );

   let connect = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some("Connect"))
      .map(|(id, _)| *id)
      .expect("the Connect button must be in the tree");

   render_connect(
      &ctx,
      &mut gui.connect_dapp_window,
      &theme,
      vec![accesskit_action(Action::Click, connect)],
   );

   assert_eq!(
      gui.connect_dapp_window.take_result(),
      Some(DappConnectResult::Approved(
         DappAccountChoice::Existing(child.address)
      )),
      "the answer must name the account the user picked"
   );
}

/// Escape answers the prompt the way Reject does, which is the way out a keyboard had without
/// reaching for the mouse.
#[test]
fn escape_rejects_the_connection() {
   let (ctx, mut gui, theme) = connect_prompt("app.example", None, Vec::new(), Vec::new());

   render_connect(
      &ctx,
      &mut gui.connect_dapp_window,
      &theme,
      Vec::new(),
   );

   assert!(
      gui.connect_dapp_window.is_open(),
      "the prompt must be drawn before the key is sent"
   );

   render_connect(
      &ctx,
      &mut gui.connect_dapp_window,
      &theme,
      vec![key_press(Key::Escape)],
   );

   assert!(
      !gui.connect_dapp_window.is_open(),
      "Escape must close the connect prompt"
   );

   assert_eq!(
      gui.connect_dapp_window.take_result(),
      Some(DappConnectResult::Rejected),
      "closing the prompt must answer the page, or its flow waits forever"
   );
}

/// The countdown lives in the button's description, never its name: a reader hears "Connect"
/// throughout, the wait while it runs, and one sentence when it opens.
#[test]
fn the_connect_countdown_keeps_its_name_and_says_when_it_opens() {
   let (ctx, mut gui, theme) = connect_prompt("app.example", None, Vec::new(), Vec::new());

   let nodes = render_connect(
      &ctx,
      &mut gui.connect_dapp_window,
      &theme,
      Vec::new(),
   );

   let (_, waiting) = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some("Connect"))
      .expect("the countdown must keep the button's name");

   assert!(
      waiting.description().unwrap_or_default().starts_with("Available in"),
      "the wait belongs in the description: {:?}",
      waiting.description()
   );

   assert_eq!(
      gui.connect_dapp_window.take_notice(),
      None,
      "nothing is said while the wait is still running"
   );

   gui.connect_dapp_window
      .set_opened_at_for_test(Some(Instant::now() - Duration::from_secs(3)));

   let nodes = render_connect(
      &ctx,
      &mut gui.connect_dapp_window,
      &theme,
      Vec::new(),
   );

   let (_, ready) = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some("Connect"))
      .expect("the button must still be named Connect once it opens");

   assert!(
      !ready.is_disabled(),
      "the wait has run out, so the button must be enabled"
   );

   assert_eq!(
      gui.connect_dapp_window.take_notice().as_deref(),
      Some("Connect is now available."),
      "the end of the wait is said exactly once"
   );

   render_connect(
      &ctx,
      &mut gui.connect_dapp_window,
      &theme,
      Vec::new(),
   );

   assert_eq!(
      gui.connect_dapp_window.take_notice(),
      None,
      "the same sentence is not said again"
   );
}
