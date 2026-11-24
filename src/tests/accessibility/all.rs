use super::app::{
   main_window, master_wallet, render_settings, settings_window, wallet_in_cache, wallet_named,
};
use super::harness::{
   accesskit_action, announced, clipboard_text, copy_said, drawn_names, focused_node, frame,
   frame_nodes, frame_nodes_and_output, is_control, is_text_input, key_combo, key_press,
   key_release, label_texts, label_words, node_texts, spoken_text,
};
use super::settings_pages::host_of;
use super::tx::prompt::text_field;
use crate::core::types::Recipient;
use crate::core::{WalletInfo, ZeusCtx};
use crate::gui::ui::dapps::across::{
   ACROSS_RISK_TIP, ACROSS_RISK_WARNING, ApiResCache, ClientResponse, SuggestedFeesResponse,
};
use crate::gui::ui::dapps::connect::{ConnectDappWindow, DappAccountChoice, DappConnectResult};
use crate::gui::ui::dapps::railgun::{
   RailgunMode,
   merge_notes::MERGE_STARTED,
   shield::{
      PAYMASTER_TIP, SELF_BROADCAST_TIP, SHIELD_DIRECTION, UNSHIELD_DIRECTION, UNWRAP_TO_ETH_TIP,
   },
};
use crate::gui::ui::dapps::uniswap::settings::{DEADLINE_TIP, SLIPPAGE_TIP, UniswapSettingsUi};
use crate::gui::ui::settings::SettingsPage;
use crate::gui::ui::shortcuts::{Action as ShortcutAction, Binding, Chord};
use crate::gui::{GUI, Page};
use crate::utils::swap_quoter::SwapStep;
use egui::accesskit::{Action, Node, NodeId, Role, Toggled};
use egui::{Context, Event, Id, Key, Modifiers, vec2};
use egui_elements::{Theme, ThemeKind};
use std::time::{Duration, Instant};
use zeus_eth::alloy_primitives::{Address, U256};
use zeus_eth::amm::uniswap::{AnyUniswapPool, DexKind, UniswapPool, UniswapV3Pool};
use zeus_eth::currency::{Currency, ERC20Token};
use zeus_eth::types::ChainId;
use zeus_eth::utils::NumericValue;
use zeus_railgun::privacy::RiskBand;

/// The money rows draw an address shortened, and that shortening is what the row is called: a name
/// the glyphs below it do not say is heard twice, and the whole address spelled out one character
/// at a time is a wall nobody can follow. The whole address stays as the description, which is
/// where two rows that agree on both ends still differ.
#[test]
fn the_recipient_row_names_the_address_by_its_ends() {
   let ctx = Context::default();
   let zeus = ZeusCtx::new(crate::runtime::test_handle());
   let theme = Theme::new(ThemeKind::TokyoNight);

   let chain = ChainId::Ethereum;
   let account = Address::from([0x11; 20]);

   // An unnamed address asks for a name off-frame the first time it is drawn. Claiming that
   // once-per-session request first keeps the test away from the network.
   zeus.write(|zeus| {
      zeus.address_name_requested(chain.id(), account);
   });

   let nodes = frame(&ctx, |ui| {
      zeus.write(|zeus| {
         crate::gui::ui::tx::rows::address_with_picked_name(
            zeus,
            chain,
            "Recipient",
            account,
            None,
            &theme,
            ui,
         );
      });
   });

   let link = nodes
      .iter()
      .find(|node| node.role() == Role::Link)
      .expect("the recipient row draws its address as a link");

   let name = link.label().expect("the link must be named");

   assert_eq!(
      name, "0x1111...111111",
      "the link is named by the shortening the row draws, so it is heard once"
   );
   assert!(
      name.chars().count() < 20,
      "the whole address, one character at a time, is not something a listener can follow"
   );
   assert_eq!(
      link.description(),
      Some("0x1111 1111 1111 1111 1111 1111 1111 1111 1111 1111"),
      "the whole address stays on the node, for whoever asks for it"
   );
}

/// A themed label is named by the sentence the row means rather than by the glyphs it draws, and a
/// widget that reported no node at all is given one. Either way the words land in both the value a
/// reader reads a label from and the label it falls back to.
#[test]
fn a_themed_label_is_named_with_its_sentence() {
   let ctx = Context::default();

   let nodes = frame(&ctx, |ui| {
      let label = ui.add(egui_elements::Label::new("ETH", None));
      crate::gui::ui::common::name_text(ui, &label, "Ethereum");

      // The node-less case: a widget that never reported anything has nothing to rename.
      let silent = ui.allocate_response(vec2(10.0, 10.0), egui::Sense::hover());
      assert!(
         !ui.ctx().has_accesskit_node(silent.id),
         "the stand-in must start without a node, or this asserts nothing"
      );
      crate::gui::ui::common::name_text(ui, &silent, "Balance 0.00");
   });

   let names: Vec<&str> = nodes
      .iter()
      .filter(|node| node.role() == Role::Label)
      .filter_map(|node| node.value())
      .collect();

   assert_eq!(
      names.iter().filter(|name| **name == "Ethereum").count(),
      1,
      "a label that already had a node keeps exactly one, named with its sentence: {names:?}"
   );

   let named = nodes
      .iter()
      .find(|node| node.value() == Some("Ethereum"))
      .expect("the themed label must be a node of its own");

   assert_eq!(
      named.label(),
      Some("Ethereum"),
      "a label node must say the sentence whichever property a consumer reads"
   );

   assert!(
      names.contains(&"Balance 0.00"),
      "a widget with no node of its own must be given one: {names:?}"
   );
}

/// A notice panel draws its title and hint with plain labels, and a plain label already carries its
/// text into the tree. This pins that the dapp panels' prose is reachable without being rewritten,
/// and fails loudly if a future egui stops emitting it.
#[test]
fn prose_in_a_notice_panel_reaches_the_tree() {
   use crate::gui::ui::common::notices::notice_panel;

   let ctx = Context::default();
   let mut theme = Theme::new(ThemeKind::TokyoNight);
   theme.install(&ctx);

   let title = "Railgun is disabled";
   let hint = "Enable it in Settings, on the Railgun page, to shield and unshield.";

   let nodes = frame(&ctx, |ui| {
      notice_panel(&theme, (300.0, 120.0), title, Some(hint), ui);
   });

   let spoken = label_texts(&nodes);

   for sentence in [title, hint] {
      assert!(
         spoken.contains(&sentence),
         "a notice line must reach the tree as a label carrying its own text: {sentence:?} in {spoken:?}"
      );
   }
}

/// A caption is what lets two pickers side by side say which side of the form they are on: the
/// trigger is otherwise named from the chain it draws, which is the same kind of name on both.
#[test]
fn a_chain_picker_says_which_side_it_is() {
   use crate::gui::ui::common::ChainSelect;

   let ctx = Context::default();
   let mut gui = GUI::default();
   gui.theme.install(&ctx);

   let theme = gui.theme.clone();
   let icons = gui.icons.clone();
   let zeus = ZeusCtx::new(crate::runtime::test_handle());
   let mut picker = ChainSelect::new("from_chain_test", 1).caption("From chain");

   let nodes = frame(&ctx, |ui| {
      zeus.write(|zeus| {
         picker.show(zeus, &[], &theme, icons.clone(), ui);
      });
   });

   let named = nodes
      .iter()
      .find(|node| node.label() == Some("From chain: Ethereum"))
      .expect("the picker must say which side of the form it is");

   assert_eq!(
      named.role(),
      Role::Button,
      "the caption must name the picker's trigger, not add a node beside it"
   );
}

/// The amount field's node says how much it holds and what it is worth, never what was typed: the
/// amount is on screen, and a node that repeated it would read the whole field back on every
/// keystroke. Everything the field draws beside itself is folded into that one node.
#[test]
fn the_amount_field_never_reads_out_its_text() {
   use crate::gui::ui::common::{AmountField, AmountFieldParams};
   use zeus_eth::currency::{Currency, NativeCurrency};
   use zeus_eth::utils::NumericValue;

   let ctx = Context::default();
   let mut gui = GUI::default();
   gui.theme.install(&ctx);

   let theme = gui.theme.clone();
   let icons = gui.icons.clone();
   let currency = Currency::from(NativeCurrency::from_chain_id(1).unwrap());

   let mut field = AmountField::new();
   field.amount = "123.45".to_string();

   let mut nodes = Vec::new();

   // The percent row measures its items on one pass and places them on the next.
   for _ in 0..3 {
      nodes = frame(&ctx, |ui| {
         field.show(
            AmountFieldParams::new(
               &theme,
               icons.clone(),
               &currency,
               Address::from([0x11; 20]),
               1,
            )
            .balance(NumericValue::from_f64(10.0))
            .max_amount(NumericValue::from_f64(10.0))
            // What the amount is worth, which is a different figure from the amount itself.
            .value(NumericValue::from_f64(308.62))
            .label("Amount")
            .show_slider(true),
            ui,
         );
      });
   }

   let field = nodes
      .iter()
      .find(|node| is_text_input(node.role()))
      .expect("the amount field must be a text field in the tree");

   assert_eq!(
      field.label(),
      Some("Amount in Ethereum"),
      "the field is named after what it holds and the token it is in"
   );

   assert_eq!(
      field.value(),
      Some("6 characters"),
      "the value says how much the field holds, not what it holds"
   );

   let value = field.value().unwrap_or_default();
   let description = field.description().unwrap_or_default();

   assert!(
      !value.contains("123.45") && !description.contains("123.45"),
      "the typed amount must never be read out: value {value:?}, description {description:?}"
   );

   assert_eq!(
      description, "Value $308.62",
      "the fiat value the field draws under itself is its description"
   );

   let spoken: Vec<&str> = nodes
      .iter()
      .filter(|node| node.role() == Role::Label && !node.is_hidden())
      .filter_map(|node| node.value())
      .collect();

   assert!(
      !spoken.contains(&"Amount"),
      "the drawn Amount label must leave the tree, or the field is named twice: {spoken:?}"
   );

   assert!(
      !spoken.iter().any(|text| text.ends_with('%')),
      "the percent readout repeats the slider's own value: {spoken:?}"
   );

   let slider = nodes
      .iter()
      .find(|node| node.role() == Role::Slider)
      .expect("the percent slider must be in the tree");

   assert_eq!(
      slider.label(),
      Some("Amount percent"),
      "the slider is named after the amount share it sets"
   );

   assert!(
      nodes
         .iter()
         .any(|node| node.role() == Role::Button && node.label() == Some("Change token (Ethereum)")),
      "the currency button says what it shows and what pressing it does"
   );

   assert!(
      spoken.contains(&"Balance 10.00"),
      "the balance is named, not left as a bare number: {spoken:?}"
   );
}

/// The eye beside the privacy switch is decoration: the switch itself is named and says whether it
/// is on, so the picture contributes no asset name and no texture URI.
#[test]
fn the_privacy_switch_picture_is_not_read_out() {
   let (ctx, mut gui, zeus) = settings_window();
   let nodes = render_settings(&ctx, &mut gui, &zeus, Vec::new());

   for (_, node) in nodes.iter().filter(|(_, node)| node.role() == Role::Image) {
      for text in node_texts(node) {
         assert!(
            !text.contains("bytes://") && text != "eye" && text != "eye-off",
            "a decorative picture must not read out its asset: {text:?}"
         );
      }
   }
}

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

/// The Uniswap window as the central panel draws it, on the page it opens on.
///
/// Twice: the window fades its page in on the pass that first sees it open, and the second pass is
/// the one a reader is handed.
fn render_uniswap_window(ctx: &Context, gui: &mut GUI, zeus: &ZeusCtx) -> Vec<(NodeId, Node)> {
   let mut nodes = Vec::new();

   for _ in 0..2 {
      nodes = frame_nodes(ctx, Vec::new(), |ui| {
         let icons = gui.icons.clone();
         let theme = gui.theme.clone();
         let uniswap = &mut gui.uniswap;
         let token_selection = &mut gui.token_selection;

         zeus.write(|zeus| uniswap.show(zeus, &theme, icons, token_selection, ui));
      });
   }

   nodes
}

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

/// The swap settings page on its own: no window and no fade around it, so one pass is the whole
/// page.
fn render_swap_settings(
   ctx: &Context,
   settings: &mut UniswapSettingsUi,
   theme: &Theme,
   events: Vec<Event>,
) -> Vec<(NodeId, Node)> {
   frame_nodes(ctx, events, |ui| {
      settings.show(true, false, theme, ui)
   })
}

/// The two currencies a seeded pool row draws: the symbols the pair cell prints.
fn weth_usdc() -> (Currency, Currency) {
   let weth = ERC20Token::from_components(
      1,
      Address::from([0xbb; 20]),
      "WETH",
      "Wrapped Ether",
      18,
      U256::ZERO,
   );

   let usdc = ERC20Token::from_components(
      1,
      Address::from([0xcc; 20]),
      "USDC",
      "USD Coin",
      6,
      U256::ZERO,
   );

   (Currency::from(weth), Currency::from(usdc))
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

/// The swap knobs read as knobs: the slippage field carries its own name and the tip the drawn word
/// held, and each slider says what its number counts instead of leaving a bare number beside it.
#[test]
fn the_swap_settings_name_their_controls() {
   let ctx = Context::default();
   let mut theme = Theme::new(ThemeKind::TokyoNight);
   theme.install(&ctx);

   let mut settings = UniswapSettingsUi::new();
   let nodes = render_swap_settings(&ctx, &mut settings, &theme, Vec::new());

   let slippage = nodes
      .iter()
      .find(|(_, node)| is_text_input(node.role()) && node.label() == Some("Slippage percent"))
      .map(|(_, node)| node)
      .unwrap_or_else(|| {
         panic!(
            "the slippage field must be named: {:?}",
            drawn_names(&nodes)
         )
      });

   assert_eq!(
      slippage.description(),
      Some(SLIPPAGE_TIP),
      "the tip the drawn word held belongs to the field"
   );

   let deadline = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Slider && node.label() == Some("Deadline in minutes"))
      .map(|(_, node)| node)
      .unwrap_or_else(|| {
         panic!(
            "the deadline slider must be named: {:?}",
            drawn_names(&nodes)
         )
      });

   let description = deadline.description().unwrap_or_default();

   assert!(
      description.starts_with("Now 5 minutes"),
      "the deadline slider must say the number it counts: {description:?}"
   );

   assert!(
      description.contains(DEADLINE_TIP),
      "the tip the drawn word holds must reach the slider, not only a mouse: {description:?}"
   );

   for (name, value) in [("Max hops", "Now 2 hops"), ("Max routes", "Now 3 routes")] {
      let slider = nodes
         .iter()
         .find(|(_, node)| node.role() == Role::Slider && node.label() == Some(name))
         .map(|(_, node)| node)
         .unwrap_or_else(|| {
            panic!(
               "the {name} slider must be named: {:?}",
               drawn_names(&nodes)
            )
         });

      assert_eq!(
         slider.description(),
         Some(value),
         "the {name} slider must say the number it counts"
      );
   }

   // Each slider holds its number as a value with no words of its own, so the unit lives in the
   // description above — and the drawn captions and numbers are gone from the tree as items of their
   // own, which is what kept a bare "5" being read with nothing saying what it counts.
   let spoken: Vec<&str> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Label && !node.is_hidden())
      .filter_map(|(_, node)| node.value())
      .collect();

   for text in [
      "Slippage",
      "Deadline (minutes)",
      "Max Hops",
      "Max Routes",
      "5",
      "2",
      "3",
   ] {
      assert!(
         !spoken.contains(&text),
         "the drawn {text:?} must leave the tree, the control says it: {spoken:?}"
      );
   }
}

/// The sanitiser rewrites what was typed, and the correction is said: "-1" becomes "0" and the page
/// says so, while a value the field keeps as typed says nothing.
#[test]
fn a_corrected_slippage_is_said() {
   let ctx = Context::default();
   let mut theme = Theme::new(ThemeKind::TokyoNight);
   theme.install(&ctx);

   let mut settings = UniswapSettingsUi::new();
   settings.slippage.clear();

   let nodes = render_swap_settings(&ctx, &mut settings, &theme, Vec::new());
   let field = nodes
      .iter()
      .find(|(_, node)| is_text_input(node.role()) && node.label() == Some("Slippage percent"))
      .map(|(id, _)| *id)
      .expect("the slippage field must be in the tree before it is typed into");

   render_swap_settings(
      &ctx,
      &mut settings,
      &theme,
      vec![accesskit_action(Action::Focus, field)],
   );
   render_swap_settings(
      &ctx,
      &mut settings,
      &theme,
      vec![Event::Text("-1".to_owned())],
   );

   assert_eq!(
      settings.take_notice().as_deref(),
      Some("Slippage set to 0 percent."),
      "a field that rewrites what was typed must say what it holds now"
   );

   let mut kept = UniswapSettingsUi::new();
   kept.slippage.clear();

   let nodes = render_swap_settings(&ctx, &mut kept, &theme, Vec::new());
   let field = nodes
      .iter()
      .find(|(_, node)| is_text_input(node.role()) && node.label() == Some("Slippage percent"))
      .map(|(id, _)| *id)
      .expect("the slippage field must be in the tree before it is typed into");

   render_swap_settings(
      &ctx,
      &mut kept,
      &theme,
      vec![accesskit_action(Action::Focus, field)],
   );
   render_swap_settings(
      &ctx,
      &mut kept,
      &theme,
      vec![Event::Text("1".to_owned())],
   );

   assert_eq!(
      kept.take_notice(),
      None,
      "a value the field keeps as typed is not a correction"
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

/// The pool the simulate page lists a fee for: one V3 WETH/USDC pool at 0.05%.
fn weth_usdc_pool() -> UniswapV3Pool {
   let (weth, usdc) = weth_usdc();

   UniswapV3Pool::new(
      1,
      Address::from([0xaa; 20]),
      500,
      weth.to_erc20().into_owned(),
      usdc.to_erc20().into_owned(),
      DexKind::UniswapV3,
   )
}

/// One hop of a seeded quote: a whole WETH through the WETH/USDC pool and out as USDC.
fn weth_usdc_step(pool: &UniswapV3Pool) -> SwapStep<AnyUniswapPool> {
   let (weth, usdc) = weth_usdc();

   SwapStep::new(
      pool.clone().into(),
      NumericValue::parse_to_wei("1", 18),
      NumericValue::parse_to_wei("2500", 6),
      weth,
      usdc,
   )
}

/// Type a new slippage into the settings page, which is the only way the swap flow is handed one:
/// the raw text is sanitised there and written into the number the form enforces.
fn type_slippage(settings: &mut UniswapSettingsUi, ctx: &Context, theme: &Theme, text: &str) {
   settings.slippage.clear();

   let nodes = render_swap_settings(ctx, settings, theme, Vec::new());
   let field = nodes
      .iter()
      .find(|(_, node)| is_text_input(node.role()) && node.label() == Some("Slippage percent"))
      .map(|(id, _)| *id)
      .expect("the slippage field must be in the tree before it is typed into");

   render_swap_settings(
      ctx,
      settings,
      theme,
      vec![accesskit_action(Action::Focus, field)],
   );
   render_swap_settings(
      ctx,
      settings,
      theme,
      vec![Event::Text(text.to_owned())],
   );
}

/// The swap form on a real pair with both amounts typed, so the button and the details have
/// something to work from.
fn swap_form_on_weth_usdc(gui: &mut GUI) {
   let (weth, usdc) = weth_usdc();

   gui.uniswap.open();
   gui.uniswap.swap_ui.currency_in = weth;
   gui.uniswap.swap_ui.currency_out = usdc;
   gui.uniswap.swap_ui.amount_in_field.amount = "1".to_string();
   gui.uniswap.swap_ui.amount_in_field.amount_wei = U256::from(10).pow(U256::from(18));
   gui.uniswap.swap_ui.amount_out_field.amount = "2500".to_string();
   gui.uniswap.swap_ui.amount_out_field.amount_wei = U256::from(2_500_000_000u64);
}

/// The version picker is a combobox whose trigger draws the version alone, so the caption has to
/// reach its node — and the fee tiers are buttons that draw a number and show their state with a
/// fill no reader can see.
#[test]
fn the_swap_form_says_which_pool_version_is_in_force() {
   let (ctx, mut gui, zeus) = main_window();
   wallet_in_cache(&zeus);

   let pool = weth_usdc_pool();
   let (weth, usdc) = weth_usdc();

   zeus.write(|ctx| ctx.pool_manager.add_pool(pool.clone()));

   // The version picker and the fee tiers belong to the simulate page, which is the page that has to
   // be told which pool it is simulating.
   gui.uniswap.open();
   gui.uniswap.settings.simulate_mode = true;
   gui.uniswap.swap_ui.currency_in = weth;
   gui.uniswap.swap_ui.currency_out = usdc;
   gui.uniswap.swap_ui.pool = Some(pool.clone().into());

   let nodes = render_uniswap_window(&ctx, &mut gui, &zeus);

   assert!(
      nodes.iter().any(|(_, node)| node.label() == Some("Pool version: V3")),
      "the version picker must say which version it shows: {:?}",
      drawn_names(&nodes)
   );

   let tier = nodes
      .iter()
      .find(|(_, node)| node.label() == Some("Fee tier 0.05%"))
      .map(|(_, node)| node)
      .unwrap_or_else(|| {
         panic!(
            "the fee tier button must be named after its tier: {:?}",
            drawn_names(&nodes)
         )
      });

   assert_eq!(
      tier.toggled(),
      Some(Toggled::True),
      "the tier the form is on must say it is the chosen one"
   );
}

/// The reason a swap is blocked used to be the disabled button's label, which no reader can reach.
/// It is a line of its own now, said once when it changes, and the button keeps its name.
#[test]
fn the_swap_button_keeps_its_name_and_says_why_it_is_blocked() {
   let (ctx, mut gui, zeus) = main_window();
   wallet_in_cache(&zeus);

   let (weth, usdc) = weth_usdc();
   gui.uniswap.open();
   gui.uniswap.swap_ui.currency_in = weth;
   gui.uniswap.swap_ui.currency_out = usdc;

   let nodes = render_uniswap_window(&ctx, &mut gui, &zeus);

   let swap = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some("Swap"))
      .map(|(_, node)| node)
      .unwrap_or_else(|| {
         panic!(
            "the action button must be named for what it does: {:?}",
            drawn_names(&nodes)
         )
      });

   assert!(
      swap.is_disabled(),
      "an empty amount leaves nothing to send, which is the state this test is about"
   );

   let spoken: Vec<&str> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Label && !node.is_hidden())
      .filter_map(|(_, node)| node.value())
      .collect();

   assert!(
      spoken.contains(&"Enter Amount"),
      "the reason must be a node of its own, reachable while the button is not: {spoken:?}"
   );

   assert_eq!(
      gui.uniswap.take_notice().as_deref(),
      Some("Enter Amount"),
      "the reason is said once when it appears"
   );

   render_uniswap_window(&ctx, &mut gui, &zeus);

   assert_eq!(
      gui.uniswap.take_notice(),
      None,
      "the same reason a frame later is not said again"
   );

   // More than the wallet holds. The balance was the last check the old label made, so it wins.
   gui.uniswap.swap_ui.amount_in_field.amount = "1".to_string();
   gui.uniswap.swap_ui.amount_in_field.amount_wei = U256::from(10).pow(U256::from(18));

   // The reason is recomputed the way a frame recomputes it: the sentence is the recompute's, not
   // the draw's, so a form that changed between two draws still says what now blocks it.
   zeus.write(|ctx| gui.uniswap.swap_ui.refresh_blocked_for_test(ctx));

   let nodes = render_uniswap_window(&ctx, &mut gui, &zeus);

   assert!(
      nodes.iter().any(|(_, node)| node.role() == Role::Button
         && node.label() == Some("Swap")
         && node.is_disabled()),
      "the button keeps its name whatever is blocking it: {:?}",
      drawn_names(&nodes)
   );

   assert_eq!(
      gui.uniswap.take_notice().as_deref(),
      Some("Insufficient WETH Balance"),
      "a reason that changes is a new sentence"
   );
}

/// The details pane drew its labels as words and its severity as colour: the rows say what they
/// count now, and the slippage says how bad it is in words too.
#[test]
fn the_swap_details_say_the_slippage_the_floor_and_the_impact() {
   let (ctx, mut gui, zeus) = main_window();
   wallet_in_cache(&zeus);

   let pool = weth_usdc_pool();
   swap_form_on_weth_usdc(&mut gui);

   // Half a percent: low enough that the value is plain, and the number the form was handed.
   let theme = gui.theme.clone();
   type_slippage(&mut gui.uniswap.settings, &ctx, &theme, "0.5");

   let settings = gui.uniswap.settings.clone();
   let step = weth_usdc_step(&pool);
   let min_received = NumericValue::format_wei(U256::from(2_498_750_000u64), 6);

   zeus.write(|ctx| {
      gui.uniswap
         .swap_ui
         .seed_quote_for_test(ctx, &settings, vec![step], min_received, 0.42);
   });

   let nodes = render_uniswap_window(&ctx, &mut gui, &zeus);

   let spoken = drawn_names(&nodes);

   assert!(
      nodes.iter().any(|(_, node)| node.label() == Some("Slippage 0.50%")),
      "the slippage value must say what it counts: {spoken:?}"
   );

   let floor = nodes
      .iter()
      .find(|(_, node)| node.label().is_some_and(|label| label.starts_with("Minimum Received ")))
      .map(|(_, node)| node)
      .unwrap_or_else(|| panic!("the floor row must name its amount: {spoken:?}"));

   assert!(
      floor.label().is_some_and(|label| label.ends_with("USDC")),
      "the floor must be named in the currency being bought: {:?}",
      floor.label()
   );

   assert!(
      nodes.iter().any(|(_, node)| node.label() == Some("Price impact 0.42%")),
      "the impact must say what it counts: {spoken:?}"
   );

   // The words the rows drew are on their values now, not in the tree a second time.
   for word in ["Slippage", "Minimum Received", "Price Impact"] {
      let copies: Vec<(bool, Option<String>)> = nodes
         .iter()
         .filter(|(_, node)| node.role() == Role::Label && node.value() == Some(word))
         .map(|(_, node)| (node.is_hidden(), node.label().map(str::to_string)))
         .collect();

      assert!(
         !copies.iter().any(|(hidden, _)| !hidden),
         "the drawn {word:?} must leave the tree, its value says it: {copies:?}"
      );
   }

   // The severity is colour only, so the value has to carry it: at 2.5 percent the colour is the
   // error one and the words say "risky".
   type_slippage(&mut gui.uniswap.settings, &ctx, &theme, "2.5");

   let nodes = render_uniswap_window(&ctx, &mut gui, &zeus);

   assert!(
      nodes.iter().any(|(_, node)| node.label() == Some("Slippage 2.50% (risky)")),
      "the severity the colour carries must be words: {:?}",
      drawn_names(&nodes)
   );
}

/// The route was drawn in a popup only a mouse could open. The count goes on the row's name now and
/// the hops on its description, so a reader gets the trade without hovering.
#[test]
fn the_route_is_spoken_without_hovering() {
   let (ctx, mut gui, zeus) = main_window();
   wallet_in_cache(&zeus);

   let pool = weth_usdc_pool();
   swap_form_on_weth_usdc(&mut gui);

   let settings = gui.uniswap.settings.clone();
   let step = weth_usdc_step(&pool);

   let hop = format!(
      "{} {} to {} {} in the {}/{} pool, fee {} percent",
      step.amount_in.abbreviated(),
      step.currency_in.symbol(),
      step.amount_out.abbreviated(),
      step.currency_out.symbol(),
      step.pool.currency0().symbol(),
      step.pool.currency1().symbol(),
      step.pool.fee().fee_percent(),
   );

   zeus.write(|ctx| {
      gui.uniswap.swap_ui.seed_quote_for_test(
         ctx,
         &settings,
         vec![step],
         NumericValue::default(),
         0.0,
      );
   });

   let nodes = render_uniswap_window(&ctx, &mut gui, &zeus);

   let routing = nodes
      .iter()
      .find(|(_, node)| node.label() == Some("Routing, 1 swap"))
      .map(|(_, node)| node)
      .unwrap_or_else(|| {
         panic!(
            "the routing row must say how many swaps it routes: {:?}",
            drawn_names(&nodes)
         )
      });

   // One hop is one swap: the name has to agree with the number it reads out.
   assert!(
      !nodes.iter().any(|(_, node)| node.label() == Some("Routing, 1 swaps")),
      "a single hop must not be named with the plural"
   );

   let description = routing.description().unwrap_or_default();

   assert!(
      description.starts_with("Total swaps: 1."),
      "the description must count the hops: {description:?}"
   );

   assert!(
      description.contains(&hop),
      "the description must spell the hop out: {description:?}"
   );

   // The popup's lines are not items of their own, and the bare word the row used to say is gone
   // with them: the reader gets the route once, from the row.
   for (_, node) in &nodes {
      if node.role() != Role::Label {
         continue;
      }

      let value = node.value().unwrap_or_default();

      assert!(
         !value.contains("->") && !value.contains("Total swaps"),
         "the hover popup's lines must not be bare labels: {value:?}"
      );

      assert!(
         value != "Routing",
         "the row says the count instead of the bare word"
      );
   }
}

/// A quote that lands is news the form had none of before, and it is said once — not on every
/// refresh of the same quote.
#[test]
fn a_quote_arriving_is_said_once() {
   let (ctx, mut gui, zeus) = main_window();
   wallet_in_cache(&zeus);

   let pool = weth_usdc_pool();
   swap_form_on_weth_usdc(&mut gui);

   render_uniswap_window(&ctx, &mut gui, &zeus);

   assert_eq!(
      gui.uniswap.take_notice().as_deref(),
      Some("Insufficient WETH Balance"),
      "a form with no quote blocks the button, which is not the quote arriving"
   );

   let settings = gui.uniswap.settings.clone();
   let step = weth_usdc_step(&pool);

   zeus.write(|ctx| {
      gui.uniswap.swap_ui.seed_quote_for_test(
         ctx,
         &settings,
         vec![step],
         NumericValue::default(),
         0.0,
      );
   });

   render_uniswap_window(&ctx, &mut gui, &zeus);

   assert_eq!(
      gui.uniswap.take_notice().as_deref(),
      Some("Quote ready: 2500 USDC."),
      "the quote is said with the amount the Buy field holds"
   );

   render_uniswap_window(&ctx, &mut gui, &zeus);

   assert_eq!(
      gui.uniswap.take_notice(),
      None,
      "the same quote a frame later is not said again"
   );
}

/// The button that exchanges the two currencies is a picture of two arrows, so it has no words of
/// its own to read.
#[test]
fn the_direction_button_says_what_it_swaps() {
   let (ctx, mut gui, zeus) = main_window();
   wallet_in_cache(&zeus);
   gui.uniswap.open();

   let nodes = render_uniswap_window(&ctx, &mut gui, &zeus);

   assert!(
      nodes.iter().any(|(_, node)| node.role() == Role::Button
         && node.label() == Some("Swap Sell and Buy tokens")),
      "an image-only button must say what it exchanges: {:?}",
      drawn_names(&nodes)
   );
}

/// Draw the Across bridge page the way the central panel does, and return its tree.
///
/// Twice: the page measures its frame on the first pass and draws into it on the second. `events`
/// go to the first pass only, so a key that closes a dialog is not pressed once per pass.
fn render_bridge_with(
   ctx: &Context,
   gui: &mut GUI,
   zeus: &ZeusCtx,
   events: Vec<Event>,
) -> Vec<(NodeId, Node)> {
   let mut nodes = Vec::new();

   for pass in 0..2 {
      let events = if pass == 0 {
         events.clone()
      } else {
         Vec::new()
      };

      nodes = frame_nodes(ctx, events, |ui| {
         let icons = gui.icons.clone();
         let theme = gui.theme.clone();
         let bridge = &mut gui.across_bridge;
         let recipient_selection = &mut gui.recipient_selection;
         let contacts = &mut gui.settings.contacts_ui;

         zeus.write(|zeus| {
            bridge.show(
               zeus,
               &theme,
               icons,
               recipient_selection,
               contacts,
               ui,
            )
         });
      });
   }

   nodes
}

/// The page on its own, with nothing in its way.
fn render_bridge(ctx: &Context, gui: &mut GUI, zeus: &ZeusCtx) -> Vec<(NodeId, Node)> {
   render_bridge_with(ctx, gui, zeus, Vec::new())
}

/// Open the bridge page for a test that is not about the first-run API notice.
fn open_bridge_for_test(gui: &mut GUI) {
   gui.across_bridge.open();
   gui.across_bridge.set_api_notice_shown_for_test(true);
}

/// Draw the bridge page until its dialogs are painted rather than measured.
///
/// A modal spends its first passes measuring itself, and a key only belongs to a dialog that is
/// on screen.
fn settle_bridge(ctx: &Context, gui: &mut GUI, zeus: &ZeusCtx) {
   render_bridge(ctx, gui, zeus);
   render_bridge(ctx, gui, zeus);
}

/// The fee card figures and wait, seeded the way a landed quote leaves them.
///
/// The wait comes out of the API cache and the three amounts out of the quote cache, which is
/// keyed to the form as it stands — so the fill time goes in first and the cache is seeded after.
fn seed_bridge_fee(
   gui: &mut GUI,
   zeus: &ZeusCtx,
   cost_usd: f64,
   bridge_fee: f64,
   total_fee: f64,
   fill_secs: u32,
) {
   let from_chain = gui.across_bridge.from_chain.chain.id();
   let to_chain = gui.across_bridge.to_chain.chain.id();

   gui.across_bridge.api_res_cache.insert(
      (from_chain, to_chain),
      ApiResCache {
         res: ClientResponse {
            origin_chain: from_chain,
            destination_chain: to_chain,
            suggested_fees: SuggestedFeesResponse {
               estimated_fill_time_sec: fill_secs,
               ..Default::default()
            },
            ..Default::default()
         },
         last_updated: None,
      },
   );

   zeus.write(|ctx| {
      gui.across_bridge.seed_quote_for_test(
         ctx,
         NumericValue::from_f64(cost_usd),
         NumericValue::from_f64(bridge_fee),
         NumericValue::from_f64(total_fee),
      )
   });
}

/// The card total line, as the tree carries it.
fn bridge_total(nodes: &[(NodeId, Node)]) -> String {
   nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Label)
      .filter_map(|(_, node)| node.value())
      .find(|text| text.starts_with("Total about $"))
      .map(str::to_owned)
      .expect("the fee card must print its total")
}

/// The header is the page title and its two warnings: a heading, a link whose name says where it
/// goes, the risk as an alert, and the hover-only tip on the chip that carries it.
#[test]
fn the_bridge_header_says_what_the_risk_is() {
   let (ctx, mut gui, zeus) = main_window();
   wallet_in_cache(&zeus);

   open_bridge_for_test(&mut gui);

   let nodes = render_bridge(&ctx, &mut gui, &zeus);

   assert!(
      nodes
         .iter()
         .any(|(_, node)| node.role() == Role::Heading && node.label() == Some("Bridge")),
      "the page title must be a heading: {:?}",
      drawn_names(&nodes)
   );

   assert!(
      nodes
         .iter()
         .any(|(_, node)| node.role() == Role::Alert && node.label() == Some(ACROSS_RISK_WARNING)),
      "the risk warning must be an alert carrying its own words: {:?}",
      drawn_names(&nodes)
   );

   let chip = nodes
      .iter()
      .find(|(_, node)| node.label() == Some("Bridge risk"))
      .map(|(_, node)| node)
      .unwrap_or_else(|| {
         panic!(
            "the chip must be named for what it explains: {:?}",
            drawn_names(&nodes)
         )
      });

   assert_eq!(
      chip.description(),
      Some(ACROSS_RISK_TIP),
      "the tip the chip only showed on hover belongs to the node"
   );

   assert!(
      nodes
         .iter()
         .any(|(_, node)| node.role() == Role::Button
            && node.label() == Some("Across bridge settings")),
      "the image-only settings button must say what it opens: {:?}",
      drawn_names(&nodes)
   );

   let link = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Link)
      .map(|(_, node)| node)
      .unwrap_or_else(|| {
         panic!(
            "the powered-by line must be a link: {:?}",
            drawn_names(&nodes)
         )
      });

   let name = link.label().unwrap_or_default();

   assert!(
      name.contains("opens across.to in your browser"),
      "the link name must say where pressing it goes: {name:?}"
   );
}

/// The card is four figures with no words of their own: each says what it is, the wait says what
/// it counts, and a total that moves is said once.
#[test]
fn the_bridge_fee_card_says_what_each_number_is() {
   let (ctx, mut gui, zeus) = main_window();
   wallet_in_cache(&zeus);

   open_bridge_for_test(&mut gui);

   seed_bridge_fee(&mut gui, &zeus, 0.01, 0.05, 0.06, 42);

   let nodes = render_bridge(&ctx, &mut gui, &zeus);

   let labels: Vec<&str> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Label)
      .filter_map(|(_, node)| node.value())
      .collect();

   for prefix in ["Network fee about $", "Bridge fee about $", "Total about $"] {
      assert!(
         nodes.iter().any(|(_, node)| node.role() == Role::Label
            && node.value().is_some_and(|text| text.starts_with(prefix))),
         "the card must say what the {prefix:?} figure is: {labels:?}",
      );
   }

   assert!(
      nodes.iter().any(|(_, node)| node.role() == Role::Label
         && node.value() == Some("Estimated fill time: 42 seconds")),
      "the wait must say what it counts: {labels:?}"
   );

   let first = bridge_total(&nodes);

   // The first frame belongs to the block above the button, said once; the total it printed was
   // remembered, so the figure that moves next is the page news.
   gui.across_bridge.take_notice();

   seed_bridge_fee(&mut gui, &zeus, 0.01, 0.05, 0.07, 42);

   let nodes = render_bridge(&ctx, &mut gui, &zeus);
   let second = bridge_total(&nodes);

   assert_ne!(
      first, second,
      "the seeded totals must differ, or this asserts nothing"
   );

   let figure = second.trim_start_matches("Total about ");
   let expected = format!("Bridge fee: total about {figure}.");

   assert_eq!(
      gui.across_bridge.take_notice().as_deref(),
      Some(expected.as_str()),
      "a total that moved must be said once"
   );

   render_bridge(&ctx, &mut gui, &zeus);

   assert_eq!(
      gui.across_bridge.take_notice(),
      None,
      "the same total a frame later is not said again"
   );
}

/// The reason a Bridge is blocked used to be the disabled button label, which no reader can
/// reach. It is a line of its own now, said once when it changes, and the button keeps its name.
#[test]
fn the_bridge_button_keeps_its_name_and_says_why_it_cannot_send() {
   let (ctx, mut gui, zeus) = main_window();
   wallet_in_cache(&zeus);

   open_bridge_for_test(&mut gui);

   let nodes = render_bridge(&ctx, &mut gui, &zeus);

   let bridge = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some("Bridge"))
      .map(|(_, node)| node)
      .unwrap_or_else(|| {
         panic!(
            "the action button must be named for what it does: {:?}",
            drawn_names(&nodes)
         )
      });

   assert!(
      bridge.is_disabled(),
      "an empty amount leaves nothing to send, which is the state this test is about"
   );

   let spoken: Vec<&str> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Label && !node.is_hidden())
      .filter_map(|(_, node)| node.value())
      .collect();

   assert!(
      spoken.contains(&"Enter Amount"),
      "the reason must be a node of its own, reachable while the button is not: {spoken:?}"
   );

   assert_eq!(
      gui.across_bridge.take_notice().as_deref(),
      Some("Enter Amount"),
      "the reason is said once when it appears"
   );

   render_bridge(&ctx, &mut gui, &zeus);

   assert_eq!(
      gui.across_bridge.take_notice(),
      None,
      "the same reason a frame later is not said again"
   );

   // A recipient resolved for another chain: the bridge can point the destination at it, and the
   // reason says so instead of only refusing.
   let elsewhere = ChainId::new(1).unwrap();

   gui.recipient_selection.recipient = Recipient {
      evm_address: Address::from([0x42; 20]).to_string(),
      chain: Some(elsewhere.id()),
      ..Default::default()
   };

   let nodes = render_bridge(&ctx, &mut gui, &zeus);

   let reason = format!(
      "Bridge is off: recipient is on {}",
      elsewhere.name()
   );

   let spoken: Vec<&str> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Label && !node.is_hidden())
      .filter_map(|(_, node)| node.value())
      .collect();

   assert!(
      spoken.contains(&reason.as_str()),
      "the wrong-chain reason must be a line of its own: {spoken:?}"
   );

   assert_eq!(
      gui.across_bridge.take_notice().as_deref(),
      Some(reason.as_str()),
      "a reason that changes is a new sentence"
   );

   let fix = format!("Set destination to {}", elsewhere.name());

   assert!(
      nodes
         .iter()
         .any(|(_, node)| node.role() == Role::Button && node.label() == Some(fix.as_str())),
      "the button that fixes the chain must stay where it is: {:?}",
      drawn_names(&nodes)
   );

   assert!(
      nodes.iter().any(|(_, node)| node.role() == Role::Button
         && node.label() == Some("Bridge")
         && node.is_disabled()),
      "the button keeps its name whatever is blocking it: {:?}",
      drawn_names(&nodes)
   );
}

/// The settings dialog names the controls it draws, and the API URL is named after its host: a URL
/// can carry a provider key, so its text never enters the tree.
#[test]
fn the_across_settings_name_their_controls() {
   const API_URL: &str = "https://rpc.example-provider.dev/v3/secret-key-1234";

   let (ctx, mut gui, zeus) = main_window();
   wallet_in_cache(&zeus);

   gui.across_bridge.open();
   gui.across_bridge.set_api_for_test(API_URL, false);
   gui.across_bridge.open_settings_for_test();

   let nodes = render_bridge(&ctx, &mut gui, &zeus);

   let field = nodes
      .iter()
      .find(|(_, node)| {
         is_text_input(node.role())
            && node.label() == Some("Across API URL at rpc.example-provider.dev")
      })
      .map(|(_, node)| node)
      .unwrap_or_else(|| {
         panic!(
            "the URL field must be named after its host: {:?}",
            drawn_names(&nodes)
         )
      });

   assert_eq!(
      field.value(),
      None,
      "the URL text must not be read back to the user"
   );

   for (_, node) in &nodes {
      for text in [node.label(), node.value(), node.description()] {
         let text = text.unwrap_or_default();

         assert!(
            !text.contains(API_URL) && !text.contains("secret-key-1234"),
            "the URL must never reach the tree: {text:?}"
         );
      }
   }

   assert!(
      nodes
         .iter()
         .any(|(_, node)| node.role() == Role::CheckBox && node.label() == Some("Use API")),
      "the box that draws no words must be named: {:?}",
      drawn_names(&nodes)
   );

   let slider = nodes
      .iter()
      .find(|(_, node)| {
         node.role() == Role::Slider && node.label() == Some("Fee to pay, in percent")
      })
      .map(|(_, node)| node)
      .unwrap_or_else(|| {
         panic!(
            "the fee slider must say what its number counts: {:?}",
            drawn_names(&nodes)
         )
      });

   let description = slider.description().unwrap_or_default();

   assert!(
      description.starts_with("Now ") && description.ends_with(" percent"),
      "the slider must carry the number it sets: {description:?}"
   );

   // The two captions the controls now say themselves leave the reading.
   for text in ["API URL", "Use API"] {
      assert!(
         nodes.iter().any(|(_, node)| node.role() == Role::Label
            && node.is_hidden()
            && node.value() == Some(text)),
         "the drawn {text:?} must be hidden rather than read beside its control"
      );
   }
}

/// Escape is the second way out of the settings dialog: it closes without saving, so a keyboard
/// is not trapped into keeping an accidental edit.
#[test]
fn escape_closes_the_across_settings_without_saving() {
   let (ctx, mut gui, zeus) = main_window();
   wallet_in_cache(&zeus);

   gui.across_bridge.open();
   gui.across_bridge.open_settings_for_test();

   // The dialog is measured on its first passes, and the page behind it says its block once.
   settle_bridge(&ctx, &mut gui, &zeus);
   gui.across_bridge.take_notice();

   let drawn = render_bridge(&ctx, &mut gui, &zeus);

   assert!(
      drawn.iter().any(|(_, node)| is_text_input(node.role())
         && node.label().is_some_and(|label| label.starts_with("Across API URL at "))),
      "the settings dialog must be drawn before the key is pressed, or this asserts nothing"
   );

   let before = gui.across_bridge.settings_for_test();

   let nodes = render_bridge_with(
      &ctx,
      &mut gui,
      &zeus,
      vec![key_press(Key::Escape)],
   );

   assert!(
      !gui.across_bridge.settings_open,
      "one Escape must close the dialog: {:?}",
      drawn_names(&nodes)
   );

   assert_eq!(
      gui.across_bridge.settings_for_test(),
      before,
      "closing without saving must leave the settings as they were"
   );

   assert_eq!(
      gui.across_bridge.take_notice(),
      None,
      "leaving without saving is not a save"
   );
}

/// The first-run API notice was `.closable(false)`, so OK was the only exit and a keyboard was
/// trapped in it. Escape gives the same answer, and a dismissed notice does not come back.
#[test]
fn the_across_api_notice_closes_on_escape() {
   let (ctx, mut gui, zeus) = main_window();
   wallet_in_cache(&zeus);

   gui.across_bridge.open();
   gui.across_bridge.set_api_notice_shown_for_test(false);

   let notice = render_bridge(&ctx, &mut gui, &zeus);

   assert!(
      notice
         .iter()
         .any(|(_, node)| node.role() == Role::Dialog && node.label() == Some("Across API")),
      "the notice must be up before the key is pressed, or this asserts nothing: {:?}",
      drawn_names(&notice)
   );

   assert!(
      !gui.across_bridge.api_notice_shown_for_test(),
      "the notice must still be unread before the key is pressed"
   );

   settle_bridge(&ctx, &mut gui, &zeus);

   render_bridge_with(
      &ctx,
      &mut gui,
      &zeus,
      vec![key_press(Key::Escape)],
   );

   assert!(
      gui.across_bridge.api_notice_shown_for_test(),
      "one Escape must dismiss the notice the way OK does"
   );

   let nodes = render_bridge(&ctx, &mut gui, &zeus);

   assert!(
      !nodes
         .iter()
         .any(|(_, node)| node.role() == Role::Dialog && node.label() == Some("Across API")),
      "a dismissed notice must not come back: {:?}",
      drawn_names(&nodes)
   );
}

// ---- Railgun shield --------------------------------------------------------

/// The shield form with Railgun enabled and synced on chain 1, so the card draws instead of the
/// "Railgun is disabled" notice. The currency is the one the frame picks for the mode.
fn shield_form(mode: RailgunMode) -> (Context, GUI, ZeusCtx) {
   let (ctx, mut gui, zeus) = main_window();
   wallet_in_cache(&zeus);

   zeus.write(|ctx| {
      ctx.railgun_config.set_enabled(1, true);
      ctx.railgun_status.set_synced(1, true);
   });

   gui.shield_ui.open(mode);
   gui.shield_ui.default_currency(1);

   (ctx, gui, zeus)
}

/// Render the shield page headlessly, twice: an egui area is measured on its first pass and a modal
/// spends one before it is painted.
fn render_shield_page(
   ctx: &Context,
   gui: &mut GUI,
   zeus: &ZeusCtx,
   events: Vec<Event>,
) -> Vec<(NodeId, Node)> {
   let mut nodes = Vec::new();

   for pass in 0..2 {
      let events = if pass == 0 {
         events.clone()
      } else {
         Vec::new()
      };

      let icons = gui.icons.clone();
      let theme = gui.theme.clone();
      let shield = &mut gui.shield_ui;
      let token_selection = &mut gui.token_selection;
      let recipient_selection = &mut gui.recipient_selection;
      let contacts = &mut gui.settings.contacts_ui;

      nodes = frame_nodes(ctx, events, |ui| {
         zeus.write(|zeus| {
            shield.show(
               zeus,
               &theme,
               icons.clone(),
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

/// Render the merge card headlessly, twice, for the same reason as the shield page.
fn render_merge_notes(
   ctx: &Context,
   gui: &mut GUI,
   zeus: &ZeusCtx,
   events: Vec<Event>,
) -> Vec<(NodeId, Node)> {
   let mut nodes = Vec::new();

   for pass in 0..2 {
      let events = if pass == 0 {
         events.clone()
      } else {
         Vec::new()
      };

      let theme = gui.theme.clone();
      let window = &mut gui.merge_notes_window;

      nodes = frame_nodes(ctx, events, |ui| {
         zeus.write(|zeus| window.show(zeus, &theme, ui));
      });
   }

   nodes
}

/// Draw the merge card until it is painted rather than measured.
fn settle_merge_notes(ctx: &Context, gui: &mut GUI, zeus: &ZeusCtx) -> Vec<(NodeId, Node)> {
   render_merge_notes(ctx, gui, zeus, Vec::new());
   render_merge_notes(ctx, gui, zeus, Vec::new())
}

/// Draw the shield page until the dialog it was opened on is painted rather than measured.
fn settle_shield_page(ctx: &Context, gui: &mut GUI, zeus: &ZeusCtx) -> Vec<(NodeId, Node)> {
   render_shield_page(ctx, gui, zeus, Vec::new());
   render_shield_page(ctx, gui, zeus, Vec::new())
}

/// The form's title is the page heading, and the one word it draws does not say which way the funds
/// move: the direction is the heading's description.
#[test]
fn the_shield_page_says_which_direction_it_moves() {
   // Case: the_shield_title_says_which_way_the_funds_move
   let (ctx, mut gui, zeus) = shield_form(RailgunMode::Shield);
   let nodes = render_shield_page(&ctx, &mut gui, &zeus, Vec::new());

   let heading = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Heading && node.label() == Some("Shield"))
      .map(|(_, node)| node)
      .unwrap_or_else(|| {
         panic!(
            "the form's title must be a heading named Shield: {:?}",
            drawn_names(&nodes)
         )
      });

   assert_eq!(
      heading.description(),
      Some(SHIELD_DIRECTION),
      "a shield must say that it moves funds into the private pool"
   );

   // Case: the_unshield_title_says_which_way_the_funds_move
   gui.shield_ui.open(RailgunMode::Unshield);
   let nodes = render_shield_page(&ctx, &mut gui, &zeus, Vec::new());

   let heading = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Heading && node.label() == Some("Unshield"))
      .map(|(_, node)| node)
      .unwrap_or_else(|| {
         panic!(
            "the unshield form's title must be a heading: {:?}",
            drawn_names(&nodes)
         )
      });

   assert_eq!(
      heading.description(),
      Some(UNSHIELD_DIRECTION),
      "an unshield must say that it moves funds out of the private pool"
   );
}

/// Both badges on the broadcast row carry their warning only as hover text, which a keyboard never
/// reaches: the name says what the badge costs, and the tip is the node's description.
#[test]
fn the_broadcast_badges_say_what_they_cost() {
   // Case: the_self_broadcast_badge_carries_its_warning
   let (ctx, mut gui, zeus) = shield_form(RailgunMode::Unshield);
   gui.shield_ui.set_switches_for_test(true, false);

   let nodes = render_shield_page(&ctx, &mut gui, &zeus, Vec::new());

   let badge = nodes
      .iter()
      .find(|(_, node)| node.label() == Some("Self-broadcast, breaks anonymity"))
      .map(|(_, node)| node)
      .unwrap_or_else(|| {
         panic!(
            "the self-broadcast badge must say what it costs in its name: {:?}",
            drawn_names(&nodes)
         )
      });

   assert_eq!(
      badge.description(),
      Some(SELF_BROADCAST_TIP),
      "the warning only a hover answered belongs to the node"
   );

   // Case: the_unwrap_badge_carries_its_tip
   gui.shield_ui.set_switches_for_test(false, true);

   let nodes = render_shield_page(&ctx, &mut gui, &zeus, Vec::new());

   let badge = nodes
      .iter()
      .find(|(_, node)| node.label() == Some("Unwrap to ETH"))
      .map(|(_, node)| node)
      .unwrap_or_else(|| {
         panic!(
            "the unwrap badge must be named: {:?}",
            drawn_names(&nodes)
         )
      });

   assert_eq!(
      badge.description(),
      Some(UNWRAP_TO_ETH_TIP),
      "the unwrap tip belongs to the badge, not to a hover"
   );
}

/// The broadcast options dialog names the two boxes it draws, and the bundler URL is named after
/// its host: a URL can carry a provider key, so its text never enters the tree.
#[test]
fn the_broadcast_options_dialog_names_its_fields() {
   const BUNDLER_URL: &str = "https://rpc.example-bundler.dev/v1/secret-key-9";

   let (ctx, mut gui, zeus) = shield_form(RailgunMode::Unshield);
   gui.shield_ui.set_bundler_url(BUNDLER_URL.to_string());
   gui.shield_ui.open_broadcast_options_for_test();

   let nodes = settle_shield_page(&ctx, &mut gui, &zeus);

   let memo = nodes
      .iter()
      .find(|(_, node)| is_text_input(node.role()) && node.label() == Some("Memo, optional"))
      .map(|(_, node)| node)
      .unwrap_or_else(|| {
         panic!(
            "the memo box must be named: {:?}",
            drawn_names(&nodes)
         )
      });

   assert_eq!(
      memo.value(),
      Some(""),
      "the memo is the user's text, and it is empty until they type"
   );

   let host = host_of(BUNDLER_URL);
   let expected = format!("Bundler URL at {host}");
   let field = nodes
      .iter()
      .find(|(_, node)| is_text_input(node.role()) && node.label() == Some(expected.as_str()))
      .map(|(_, node)| node)
      .unwrap_or_else(|| {
         panic!(
            "the bundler field must be named after its host: {:?}",
            drawn_names(&nodes)
         )
      });

   assert_eq!(
      field.value(),
      None,
      "the URL text must not be read back to the user"
   );

   let description = field.description().unwrap_or_default();

   assert!(
      description.contains("provider key"),
      "the field must say why its text is withheld: {description:?}"
   );

   for (_, node) in &nodes {
      for text in [node.label(), node.value(), node.description()] {
         let text = text.unwrap_or_default();

         assert!(
            !text.contains(BUNDLER_URL) && !text.contains("secret-key-9"),
            "the URL must never reach the tree: {text:?}"
         );
      }
   }

   let help = nodes
      .iter()
      .find(|(_, node)| node.label() == Some("Bundler URL help"))
      .map(|(_, node)| node)
      .unwrap_or_else(|| {
         panic!(
            "the help chip must be named: {:?}",
            drawn_names(&nodes)
         )
      });

   assert_eq!(
      help.description(),
      Some(PAYMASTER_TIP),
      "the chip's tip is the node's description, not only a hover"
   );
}

/// The reason a shield is blocked used to be the disabled button's own label, which no reader can
/// reach. It is a line of its own now, said once when it appears, and the button keeps its name.
#[test]
fn the_shield_button_keeps_its_name_and_says_why_it_is_blocked() {
   let (ctx, mut gui, zeus) = shield_form(RailgunMode::Shield);

   // One ETH typed with nothing to spend it: the balance is what is missing, and the private
   // recipient is there so nothing above it is what blocks the button.
   gui.shield_ui.type_amount_for_test("1");
   gui.recipient_selection.recipient = Recipient {
      zk_address: "0zk1qexample".to_string(),
      ..Default::default()
   };

   let nodes = render_shield_page(&ctx, &mut gui, &zeus, Vec::new());

   let reason = "Insufficient ETH Balance";

   assert!(
      nodes.iter().any(|(_, node)| node.role() == Role::Button
         && node.label() == Some("Shield")
         && node.is_disabled()),
      "the button keeps the name of what it does, whatever is blocking it: {:?}",
      drawn_names(&nodes)
   );

   assert!(
      spoken_text(&nodes).contains(&reason),
      "the reason must be a node of its own, reachable while the button is not: {:?}",
      spoken_text(&nodes)
   );

   assert_eq!(
      gui.shield_ui.take_notice().as_deref(),
      Some(reason),
      "the reason is said once when it appears"
   );

   render_shield_page(&ctx, &mut gui, &zeus, Vec::new());

   assert_eq!(
      gui.shield_ui.take_notice(),
      None,
      "the same reason a frame later is not said again"
   );

   // The chain falls out of sync: the reason changes with it, and is said again.
   zeus.write(|ctx| ctx.railgun_status.set_synced(1, false));

   let nodes = render_shield_page(&ctx, &mut gui, &zeus, Vec::new());

   assert!(
      spoken_text(&nodes).contains(&"Waiting for Railgun to sync"),
      "an unsynced chain is said in the form's own words: {:?}",
      spoken_text(&nodes)
   );

   assert_eq!(
      gui.shield_ui.take_notice().as_deref(),
      Some("Waiting for Railgun to sync"),
      "a reason that changes is a new sentence"
   );

   assert!(
      nodes.iter().any(|(_, node)| node.role() == Role::Button
         && node.label() == Some("Shield")
         && node.is_disabled()),
      "and the button still says what it does: {:?}",
      drawn_names(&nodes)
   );
}

/// The merge card is drawn with the sentences it will act on, and it closes on the press that
/// starts the work — so the press is what says the merge began.
#[test]
fn the_merge_window_says_what_it_will_merge() {
   let (ctx, mut gui, zeus) = main_window();
   wallet_in_cache(&zeus);

   gui.merge_notes_window.seed_notes_for_test(&[
      1_000_000_000_000_000_000,
      2_000_000_000_000_000_000,
      3_000_000_000_000_000_000,
   ]);

   let nodes = settle_merge_notes(&ctx, &mut gui, &zeus);
   let texts = spoken_text(&nodes);

   assert!(
      texts.contains(&"Merge 3 notes into 1 note (proof circuit 03x01)"),
      "the card must say how many notes it merges and into what: {texts:?}"
   );

   for leaf in 0..3 {
      let prefix = format!("Note {}: ", leaf + 1);

      assert!(
         texts.iter().any(|text| text.starts_with(&prefix)),
         "each note must be named rather than numbered from the list: {texts:?}"
      );
   }

   assert!(
      texts.iter().any(|text| text.ends_with("(leaf 0)")),
      "a note still names the leaf it sits in: {texts:?}"
   );

   // The tree has nothing to merge: the card says so rather than drawing an empty list.
   gui.merge_notes_window.seed_no_merge_for_test();
   let nodes = settle_merge_notes(&ctx, &mut gui, &zeus);
   let texts = spoken_text(&nodes);

   assert!(
      texts.contains(&"No merge needed. This tree already has at most one note for this token."),
      "the nothing-to-do answer must be a sentence: {texts:?}"
   );

   // Back to a merge, and press it: the card goes and the start is said.
   gui.merge_notes_window
      .seed_notes_for_test(&[1_000_000_000_000_000_000, 2_000_000_000_000_000_000]);

   let nodes = settle_merge_notes(&ctx, &mut gui, &zeus);

   let confirm = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some("Confirm Merge"))
      .map(|(id, _)| *id)
      .unwrap_or_else(|| {
         panic!(
            "the card must offer its confirm button: {:?}",
            drawn_names(&nodes)
         )
      });

   render_merge_notes(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, confirm)],
   );

   assert_eq!(
      gui.merge_notes_window.take_notice().as_deref(),
      Some(MERGE_STARTED),
      "pressing Confirm Merge closes the card, so the press must say the merge began"
   );

   assert!(
      !gui.merge_notes_window.is_open(),
      "the work continues with no card on screen"
   );
}

/// The privacy band lands after the amount has settled, while the reader is still in the amount
/// field: it is said once when it changes, and never again for the same verdict.
#[test]
fn the_privacy_verdict_is_said_when_it_changes() {
   let (ctx, mut gui, zeus) = shield_form(RailgunMode::Unshield);

   let owner = zeus.read(|ctx| ctx.current_wallet_info().address);
   let recipient = Address::from([0x42; 20]);

   gui.recipient_selection.recipient = Recipient {
      evm_address: recipient.to_string(),
      ..Default::default()
   };
   gui.shield_ui.type_amount_for_test("1.02");

   // The form as it stands is blocked from the first frame — there is nothing private to spend —
   // and that sentence is drained first, so the band is the only new thing on the frame it lands.
   render_shield_page(&ctx, &mut gui, &zeus, Vec::new());
   gui.shield_ui.take_notice();

   gui.shield_ui.seed_privacy_band_for_test(
      RiskBand::High,
      "1.02",
      ChainId::new(1).unwrap(),
      owner,
      recipient,
   );

   let nodes = render_shield_page(&ctx, &mut gui, &zeus, Vec::new());

   assert!(
      spoken_text(&nodes).contains(&"Amount privacy risk: High"),
      "the band must still be drawn for a reader who reaches the block: {:?}",
      spoken_text(&nodes)
   );

   assert_eq!(
      gui.shield_ui.take_notice().as_deref(),
      Some("Privacy risk: High."),
      "a band that arrives is said once"
   );

   render_shield_page(&ctx, &mut gui, &zeus, Vec::new());

   assert_eq!(
      gui.shield_ui.take_notice(),
      None,
      "the same band a frame later is not said again"
   );
}

/// The pool is thin: the sentences are drawn under a form the reader never left, so the first of
/// them is said once, and a set that does not change says nothing more.
#[test]
fn the_pool_warnings_are_said_when_they_change() {
   let (ctx, mut gui, zeus) = shield_form(RailgunMode::Shield);

   // The button's own block is the first sentence drained, so the pool's is the only new one.
   render_shield_page(&ctx, &mut gui, &zeus, Vec::new());
   gui.shield_ui.take_notice();

   gui.shield_ui.seed_pool_warnings_for_test(ChainId::new(1).unwrap());

   let nodes = render_shield_page(&ctx, &mut gui, &zeus, Vec::new());
   let spoken = spoken_text(&nodes);

   let notice = gui
      .shield_ui
      .take_notice()
      .expect("the first pool warning is the news of the frame it appears on");

   assert!(
      spoken.contains(&notice.as_str()),
      "the sentence said must be the one the block draws: {notice:?} not in {spoken:?}"
   );

   render_shield_page(&ctx, &mut gui, &zeus, Vec::new());

   assert_eq!(
      gui.shield_ui.take_notice(),
      None,
      "the same set of warnings is not said again"
   );
}

/// The tips are drawn with a literal bullet, which a reader would say out loud: the row keeps the
/// bullet and the node is named with the sentence alone.
#[test]
fn the_priv_tips_have_no_bullet_in_their_names() {
   let (ctx, mut gui, zeus) = shield_form(RailgunMode::Unshield);
   gui.shield_ui.open_privacy_tips_for_test();

   let nodes = settle_shield_page(&ctx, &mut gui, &zeus);

   for (_, node) in &nodes {
      for text in [node.label(), node.value()] {
         let text = text.unwrap_or_default();

         assert!(
            !text.starts_with('•'),
            "a reader must not be read the bullet: {text:?}"
         );
      }
   }

   // The tips are the only long prose this page draws with no verdict: one IP tip and three
   // habits, each of them a sentence.
   let tips: Vec<&str> = nodes
      .iter()
      .filter_map(|(_, node)| node.label())
      .filter(|name| name.chars().count() > 60)
      .collect();

   assert_eq!(
      tips.len(),
      4,
      "the tips dialog must draw four tips: {tips:?}"
   );

   for tip in tips {
      assert!(
         tip.starts_with(char::is_alphabetic),
         "a tip must start with its first word: {tip:?}"
      );
   }
}

/// Draw the shortcut layer and the panels it moves the focus in, in the app's order.
pub(super) fn render_shortcuts(
   ctx: &Context,
   gui: &mut GUI,
   zeus: &ZeusCtx,
   events: Vec<Event>,
) -> Vec<(NodeId, Node)> {
   frame_nodes(ctx, events, |ui| {
      zeus.write(|zeus| {
         crate::gui::ui::shortcuts::handle(gui, zeus, ui.ctx());
         crate::gui::ui::panels::left_panel::show(gui, zeus, ui);
         crate::gui::ui::panels::central_panel::show(gui, zeus, ui);
      });
   })
}

/// Press one chord and let it go across two frames, the way a keyboard sends it.
///
/// A press the window never sees released is a key held down, and egui delivers the next press of
/// that key as an auto-repeat — which the layer ignores on purpose. A test that pressed the same
/// key twice without a release would therefore prove nothing about the second press.
pub(super) fn press_shortcut(
   ctx: &Context,
   gui: &mut GUI,
   zeus: &ZeusCtx,
   key: Key,
   modifiers: Modifiers,
) -> Vec<(NodeId, Node)> {
   render_shortcuts(ctx, gui, zeus, vec![key_combo(key, modifiers)]);

   // The release is a frame of its own, and it draws the same panels: a widget that is not drawn
   // again has its focus given up, so the frame that shows where the press landed is this one.
   render_shortcuts(ctx, gui, zeus, vec![key_release(key)])
}

/// Every `Alt` digit opens the page the nav puts in that position.
#[test]
fn alt_digits_open_the_page_on_their_number() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| zeus.vault_unlocked = true);

   let pages = [
      (Key::Num1, Page::Home),
      (Key::Num2, Page::Send),
      (Key::Num3, Page::Shield),
      (Key::Num4, Page::Swap),
      (Key::Num5, Page::Bridge),
      (Key::Num6, Page::Wallets),
      (Key::Num7, Page::Transactions),
      (Key::Num8, Page::Approvals),
   ];

   for (key, page) in pages {
      frame_nodes(&ctx, vec![key_combo(key, Modifiers::ALT)], |ui| {
         zeus.write(|zeus| crate::gui::ui::shortcuts::handle(&mut gui, zeus, ui.ctx()));
      });

      assert_eq!(
         gui.current_page(),
         page,
         "Alt+{key:?} must open {page:?}"
      );
   }

   frame_nodes(
      &ctx,
      vec![key_combo(Key::Num0, Modifiers::ALT)],
      |ui| {
         zeus.write(|zeus| crate::gui::ui::shortcuts::handle(&mut gui, zeus, ui.ctx()));
      },
   );

   assert!(
      gui.connected_dapps.is_open(),
      "Alt+0 must open Connected Dapps"
   );

   // A dialog keeps the keyboard until it closes, so the next check closes this one first.
   gui.connected_dapps.close();

   frame_nodes(
      &ctx,
      vec![key_combo(Key::Num9, Modifiers::ALT)],
      |ui| {
         zeus.write(|zeus| crate::gui::ui::shortcuts::handle(&mut gui, zeus, ui.ctx()));
      },
   );

   assert!(gui.settings.is_open(), "Alt+9 must open Settings");
}

/// The page a shortcut opens takes the focus, exactly as a nav click does.
#[test]
fn a_shortcut_opened_page_puts_the_focus_on_its_heading() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| zeus.vault_unlocked = true);
   wallet_in_cache(&zeus);

   let nodes = press_shortcut(&ctx, &mut gui, &zeus, Key::Num2, Modifiers::ALT);
   let focused = focused_node(&ctx, &nodes);

   assert_eq!(focused.role(), Role::Heading);
   assert_eq!(focused.label(), Some("Send"));
}

/// F6 walks the regions clockwise and skips the ones with nothing to focus.
#[test]
fn f6_cycles_from_the_page_to_the_nav() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| zeus.vault_unlocked = true);
   wallet_in_cache(&zeus);
   gui.send_crypto.open();

   // The page change puts the focus on the heading and records the page.
   render_shortcuts(&ctx, &mut gui, &zeus, Vec::new());
   // Main -> Status (closed) -> Account (closed) -> Navigation.
   let nodes = press_shortcut(&ctx, &mut gui, &zeus, Key::F6, Modifiers::NONE);

   let focused = focused_node(&ctx, &nodes);

   assert_eq!(focused.role(), Role::Button);
   assert_eq!(focused.label(), Some("Send"));

   // Navigation -> Main: the same key comes back to the page heading — the heading node, not the
   // nav plate that carries the same word.
   let nodes = press_shortcut(&ctx, &mut gui, &zeus, Key::F6, Modifiers::NONE);
   let focused = focused_node(&ctx, &nodes);

   assert_eq!(focused.role(), Role::Heading);
   assert_eq!(focused.label(), Some("Send"));
}

/// The account card is a region too, and F6 reaches its address row when a wallet is shown.
#[test]
fn f6_reaches_the_account_card_when_a_wallet_is_shown() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| zeus.vault_unlocked = true);
   wallet_in_cache(&zeus);
   gui.account_panel.open();

   render_shortcuts(&ctx, &mut gui, &zeus, Vec::new());
   // Main -> Status (closed) -> Account.
   let nodes = press_shortcut(&ctx, &mut gui, &zeus, Key::F6, Modifiers::NONE);

   let focused = focused_node(&ctx, &nodes);

   assert_eq!(focused.role(), Role::Button);
   assert!(
      focused.label().is_some_and(|label| label.starts_with("0x")),
      "the account card's address row takes the focus"
   );
}

/// The lock screen owns the keyboard: a shortcut must not open a page behind it.
#[test]
fn shortcuts_are_inert_behind_the_lock_screen() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| {
      zeus.vault_unlocked = true;
      zeus.locked = true;
   });

   frame_nodes(
      &ctx,
      vec![key_combo(Key::Num2, Modifiers::ALT)],
      |ui| {
         zeus.write(|zeus| crate::gui::ui::shortcuts::handle(&mut gui, zeus, ui.ctx()));
      },
   );

   assert!(!gui.send_crypto.is_open());
}

/// A dialog keeps the keyboard until it closes: a shortcut must not move the page under it.
#[test]
fn shortcuts_do_not_reach_behind_a_dialog() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| {
      zeus.vault_unlocked = true;
      gui.settings.open(zeus, &mut gui.appearance_settings);
   });

   frame_nodes(
      &ctx,
      vec![key_combo(Key::Num2, Modifiers::ALT)],
      |ui| {
         zeus.write(|zeus| crate::gui::ui::shortcuts::handle(&mut gui, zeus, ui.ctx()));
      },
   );

   assert!(gui.settings.is_open());
   assert!(!gui.send_crypto.is_open());
}

/// Alt+N is the one way to reach a notification from any page.
#[test]
fn alt_n_moves_the_focus_to_the_notification() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| zeus.vault_unlocked = true);
   gui.notification.open_with_spinner(
      "Transfer".to_string(),
      crate::gui::ui::NotificationType::Other(String::new()),
   );

   let mut nodes = Vec::new();

   for pass in 0..2 {
      let events = if pass == 1 {
         vec![key_combo(Key::N, Modifiers::ALT)]
      } else {
         Vec::new()
      };
      let theme = gui.theme.clone();
      let icons = gui.icons.clone();

      nodes = frame_nodes(&ctx, events, |ui| {
         zeus.write(|zeus| {
            crate::gui::ui::shortcuts::handle(&mut gui, zeus, ui.ctx());
            gui.notification.show(zeus, &theme, icons.clone(), ui);
         });
      });
   }

   assert_eq!(
      focused_node(&ctx, &nodes).label(),
      Some("Dismiss")
   );
}

/// The character an Alt chord can produce on X11 goes with the key, not into the field.
#[test]
fn an_alt_digit_does_not_type_into_a_focused_field() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| zeus.vault_unlocked = true);

   let id = Id::unique("shortcut_probe");
   let mut text = String::new();

   frame_nodes(&ctx, Vec::new(), |ui| {
      ui.add(egui::TextEdit::singleline(&mut text).id(id));
      ui.memory_mut(|memory| memory.request_focus(id));
   });

   frame_nodes(
      &ctx,
      vec![
         key_combo(Key::Num2, Modifiers::ALT),
         Event::Text("2".to_string()),
      ],
      |ui| {
         zeus.write(|zeus| crate::gui::ui::shortcuts::handle(&mut gui, zeus, ui.ctx()));
         ui.add(egui::TextEdit::singleline(&mut text).id(id));
      },
   );

   assert_eq!(text, "");
   assert!(gui.send_crypto.is_open());
}

/// Draw the shortcut layer, the panels it moves the focus in and the app's live region, in the
/// The shortcut layer over the account card, with the app's live region after it.
///
/// The copy tests draw the card the shortcut copies from and not the pages behind it: those read a
/// wallet cache and an RPC this test does not stand up.
fn render_account_card(
   ctx: &Context,
   gui: &mut GUI,
   zeus: &ZeusCtx,
   events: Vec<Event>,
) -> (Vec<(NodeId, Node)>, Vec<egui::OutputCommand>) {
   frame_nodes_and_output(ctx, events, |ui| {
      zeus.write(|zeus| {
         crate::gui::ui::shortcuts::handle(gui, zeus, ui.ctx());
         crate::gui::ui::panels::left_panel::show(gui, zeus, ui);
      });

      gui.show_announcer(ui);
   })
}

/// `Ctrl+Tab` asks for the next control. `Tab` alone stops on every line of text — that is what puts
/// the page's words on the keyboard path — so the shortcut hops in a pass where a line is not a tab
/// stop at all.
#[test]
fn ctrl_tab_lands_on_the_next_control() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| zeus.vault_unlocked = true);
   wallet_in_cache(&zeus);

   // The reader model: every `Label` is focusable, which is what gives this key work to do.
   ctx.options_mut(|options| options.screen_reader = true);

   // Alt+2 opens Send and leaves the focus on its heading, so the walk starts on prose.
   press_shortcut(&ctx, &mut gui, &zeus, Key::Num2, Modifiers::ALT);

   let nodes = press_shortcut(&ctx, &mut gui, &zeus, Key::Tab, Modifiers::NONE);

   assert!(
      !is_control(focused_node(&ctx, &nodes).role()),
      "Tab alone must stop on the next line of text"
   );

   let nodes = press_shortcut(&ctx, &mut gui, &zeus, Key::Tab, Modifiers::CTRL);
   let focused = focused_node(&ctx, &nodes);

   assert!(
      is_control(focused.role()),
      "Ctrl+Tab must land on a control, not on another line: {:?}",
      focused.role()
   );

   assert!(
      ctx.options(|options| options.screen_reader),
      "the pass after the hop must have the reader's model back, or prose would leave the keyboard"
   );
}

/// `Ctrl+Shift+Tab` is the same hop backwards. From the first control it stops on the page's own
/// heading — the landmark the walk started at, which the page makes a tab stop on purpose — and a
/// hop forward is the control it came from.
#[test]
fn ctrl_shift_tab_is_the_same_hop_backwards() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| zeus.vault_unlocked = true);
   wallet_in_cache(&zeus);
   ctx.options_mut(|options| options.screen_reader = true);

   press_shortcut(&ctx, &mut gui, &zeus, Key::Num2, Modifiers::ALT);

   let nodes = press_shortcut(&ctx, &mut gui, &zeus, Key::Tab, Modifiers::CTRL);
   let forward = focused_node(&ctx, &nodes).label().map(str::to_string);

   let nodes = press_shortcut(
      &ctx,
      &mut gui,
      &zeus,
      Key::Tab,
      Modifiers::CTRL | Modifiers::SHIFT,
   );
   let back = focused_node(&ctx, &nodes);

   assert_eq!(
      back.role(),
      Role::Heading,
      "the hop back must not stop on a line of text"
   );
   assert_eq!(
      back.label(),
      Some("Send"),
      "it stops on the page's own heading, which is the landmark before the first control"
   );

   let nodes = press_shortcut(&ctx, &mut gui, &zeus, Key::Tab, Modifiers::CTRL);

   assert_eq!(
      focused_node(&ctx, &nodes).label().map(str::to_string),
      forward,
      "forward again is the control the walk came from"
   );
}

/// `Ctrl+Shift+C` copies the address the account card shows, and says which one: the clipboard is
/// invisible, so the words are the only way to check what landed there.
#[test]
fn ctrl_shift_c_copies_the_address_and_says_which_one() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| zeus.vault_unlocked = true);

   let wallet = master_wallet("Main");
   let address = wallet.address.to_string();

   gui.account_panel.open();
   gui.account_panel.set_current_wallet(wallet);

   // Quiet passes first: the node a message is written to has to exist before the one that says it.
   render_account_card(&ctx, &mut gui, &zeus, Vec::new());
   render_account_card(&ctx, &mut gui, &zeus, Vec::new());

   let (nodes, commands) = render_account_card(
      &ctx,
      &mut gui,
      &zeus,
      vec![key_combo(Key::C, Modifiers::COMMAND | Modifiers::SHIFT)],
   );

   assert_eq!(
      clipboard_text(&commands),
      Some(address.clone()),
      "the whole address the card shows must land on the clipboard"
   );

   assert_eq!(
      announced(&nodes).as_deref(),
      Some(copy_said(&address).as_str()),
      "the copy must say which address it copied"
   );
}

/// In privacy mode the card shows a zk address. A wallet without one has nothing to copy, and the
/// shortcut must say so rather than hand over the public address the card is hiding.
#[test]
fn ctrl_shift_c_says_when_there_is_no_privacy_address() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| {
      zeus.vault_unlocked = true;
      zeus.privacy_mode = true;
   });

   gui.account_panel.open();
   gui.account_panel.set_current_wallet(master_wallet("Main"));

   render_account_card(&ctx, &mut gui, &zeus, Vec::new());
   render_account_card(&ctx, &mut gui, &zeus, Vec::new());

   let (nodes, commands) = render_account_card(
      &ctx,
      &mut gui,
      &zeus,
      vec![key_combo(Key::C, Modifiers::COMMAND | Modifiers::SHIFT)],
   );

   assert_eq!(
      clipboard_text(&commands),
      None,
      "the public address the card is hiding must not be handed over"
   );

   assert_eq!(
      announced(&nodes).as_deref(),
      Some("This wallet has no privacy address to copy."),
      "the shortcut must say why nothing was copied"
   );
}

/// Ctrl+Enter asks the Send page to press its own button; it never signs anything itself.
#[test]
fn ctrl_enter_asks_the_send_page_to_press_its_own_button() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| zeus.vault_unlocked = true);
   gui.send_crypto.open();

   frame_nodes(
      &ctx,
      vec![key_combo(Key::Enter, Modifiers::COMMAND)],
      |ui| {
         zeus.write(|zeus| crate::gui::ui::shortcuts::handle(&mut gui, zeus, ui.ctx()));
      },
   );

   assert!(gui.send_crypto.send_requested());
}

/// On any other page the same key is not a Send.
#[test]
fn ctrl_enter_on_another_page_does_not_press_send() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| zeus.vault_unlocked = true);

   frame_nodes(
      &ctx,
      vec![key_combo(Key::Enter, Modifiers::COMMAND)],
      |ui| {
         zeus.write(|zeus| crate::gui::ui::shortcuts::handle(&mut gui, zeus, ui.ctx()));
      },
   );

   assert!(!gui.send_crypto.send_requested());
}

/// Draw the app-wide overlays twice: the sheet is an `egui::Area` measured on the pass it opens,
/// and egui exposes no node for a widget it considers invisible.
fn draw_overlays(ctx: &Context, gui: &mut GUI) -> Vec<(NodeId, Node)> {
   frame_nodes(ctx, Vec::new(), |ui| gui.show_overlay_modals(ui));

   frame_nodes(ctx, Vec::new(), |ui| gui.show_overlay_modals(ui))
}

/// F1 puts the sheet up, and it lists the keys the app answers: the page rows come from the same
/// table the `Alt` digits are read from, so the sheet cannot promise a key that does nothing.
#[test]
fn f1_opens_the_sheet_of_every_binding() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| zeus.vault_unlocked = true);
   wallet_in_cache(&zeus);

   press_shortcut(&ctx, &mut gui, &zeus, Key::F1, Modifiers::NONE);

   assert!(
      gui.shortcut_sheet.is_open(),
      "F1 must put the sheet up"
   );

   let nodes = draw_overlays(&ctx, &mut gui);

   let dialog = nodes
      .iter()
      .map(|(_, node)| node)
      .find(|node| node.role() == Role::Dialog)
      .expect("the sheet is a dialog");

   assert_eq!(
      dialog.label(),
      Some("Keyboard shortcuts"),
      "the sheet names itself the way its heading says"
   );

   let labels = label_words(&nodes);

   for expected in [
      "Alt+1: Home",
      "Alt+2: Send",
      "Alt+3: Shield",
      "Alt+4: Swap",
      "Alt+5: Bridge",
      "Alt+6: Wallets",
      "Alt+7: Transactions",
      "Alt+8: Approvals",
      "Alt+9: Settings",
      "Alt+0: Connected Dapps",
      "Tab / Shift+Tab: the next or previous control",
      "F6: go to the next part: the account, the navigation, the page, the notification",
      "Shift+F6: go back to the previous part",
      "Ctrl+Tab: the next control, skipping the text between them",
      "Ctrl+Shift+Tab: the previous control, skipping the text between them",
      "Ctrl+Enter: press Send on the Send page",
      "Alt+N: go to the open notification",
      "Ctrl+Shift+C: copy your address",
      "F1: show or hide this list",
      "Ctrl+K: list every command, and run one by name",
   ] {
      assert!(
         labels.iter().any(|label| label == expected),
         "the sheet must list {expected:?}: {labels:?}"
      );
   }
}

/// The sheet takes the keyboard when it opens: its own close button has the focus, so Tab walks
/// the list from inside the dialog and Escape has something to act on.
#[test]
fn the_sheet_takes_the_focus_when_it_opens() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| zeus.vault_unlocked = true);
   wallet_in_cache(&zeus);

   press_shortcut(&ctx, &mut gui, &zeus, Key::F1, Modifiers::NONE);

   let nodes = draw_overlays(&ctx, &mut gui);
   let focused = focused_node(&ctx, &nodes);

   assert_eq!(focused.role(), Role::Button);
   assert_eq!(focused.label(), Some("Close"));
}

/// The sheet closes the way a dialog does — Escape — and the key that opened it puts it away too.
#[test]
fn escape_and_f1_put_the_sheet_away() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| zeus.vault_unlocked = true);
   wallet_in_cache(&zeus);

   press_shortcut(&ctx, &mut gui, &zeus, Key::F1, Modifiers::NONE);
   assert!(
      gui.shortcut_sheet.is_open(),
      "F1 must put the sheet up"
   );

   draw_overlays(&ctx, &mut gui);

   frame_nodes(
      &ctx,
      vec![key_combo(Key::Escape, Modifiers::NONE)],
      |ui| gui.show_overlay_modals(ui),
   );

   assert!(
      !gui.shortcut_sheet.is_open(),
      "Escape must put the sheet away"
   );

   press_shortcut(&ctx, &mut gui, &zeus, Key::F1, Modifiers::NONE);
   assert!(gui.shortcut_sheet.is_open(), "F1 opens it again");

   press_shortcut(&ctx, &mut gui, &zeus, Key::F1, Modifiers::NONE);
   assert!(
      !gui.shortcut_sheet.is_open(),
      "a second F1 puts it away"
   );
}

/// The sheet holds the keyboard while it is up: a page key pressed behind it changes nothing.
#[test]
fn a_page_key_does_not_reach_behind_the_sheet() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| zeus.vault_unlocked = true);
   wallet_in_cache(&zeus);

   press_shortcut(&ctx, &mut gui, &zeus, Key::F1, Modifiers::NONE);
   render_shortcuts(
      &ctx,
      &mut gui,
      &zeus,
      vec![key_combo(Key::Num2, Modifiers::ALT)],
   );

   assert_eq!(
      gui.current_page(),
      Page::Home,
      "the sheet keeps the page where it was"
   );
}

/// The lock screen and a dialog both own the keyboard, so F1 reaches neither of them.
#[test]
fn f1_does_not_reach_past_the_lock_screen_or_a_dialog() {
   let (ctx, mut gui, zeus) = main_window();

   press_shortcut(&ctx, &mut gui, &zeus, Key::F1, Modifiers::NONE);

   assert!(
      !gui.shortcut_sheet.is_open(),
      "the lock screen owns the keyboard"
   );

   zeus.write(|zeus| zeus.vault_unlocked = true);
   wallet_in_cache(&zeus);
   gui.msg_window.open("A message over the page");

   press_shortcut(&ctx, &mut gui, &zeus, Key::F1, Modifiers::NONE);

   assert!(
      !gui.shortcut_sheet.is_open(),
      "a dialog keeps the keyboard until it closes"
   );
}

/// The same list is behind a button on the Settings page, for a user who never tries F1: the press
/// is handed up to the frame, which is the only thing that can draw a window over another one.
#[test]
fn the_general_page_opens_the_sheet_with_a_button() {
   let (ctx, mut gui, zeus) = settings_window();

   let nodes = render_settings(&ctx, &mut gui, &zeus, Vec::new());
   let button = nodes
      .iter()
      .find(|(_, node)| {
         node.role() == Role::Button && node.label() == Some("Show Keyboard Shortcuts")
      })
      .map(|(id, _)| *id)
      .expect("the General page draws the button that puts the sheet up");

   render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, button)],
   );

   // What the app does with the request, on the pass the page raised it.
   if gui.settings.take_shortcuts_request() {
      gui.shortcut_sheet.open();
   }

   assert!(
      gui.shortcut_sheet.is_open(),
      "the button must put the sheet up"
   );
}

/// The palette lists every chord the app answers and every page the nav lists: a command a reader
/// cannot find is a command that does not exist.
#[test]
fn ctrl_k_lists_every_command() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| zeus.vault_unlocked = true);
   wallet_in_cache(&zeus);

   press_shortcut(&ctx, &mut gui, &zeus, Key::K, Modifiers::COMMAND);

   assert!(
      gui.palette.is_open(),
      "Ctrl+K must put the palette up"
   );

   let nodes = draw_overlays(&ctx, &mut gui);

   let dialog = nodes
      .iter()
      .map(|(_, node)| node)
      .find(|node| node.role() == Role::Dialog)
      .expect("the palette is a dialog");

   assert_eq!(
      dialog.label(),
      Some("Commands"),
      "the palette names itself the way its heading says"
   );

   let names: Vec<&str> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Button)
      .filter_map(|(_, node)| node.label())
      .collect();

   for action in ShortcutAction::ALL {
      let expected = format!(
         "{}: {}",
         action.label(),
         gui.keymap.binding(action).label()
      );

      assert!(
         names.contains(&expected.as_str()),
         "the palette must list {expected:?}: {names:?}"
      );
   }

   for page in [
      Page::Home,
      Page::Send,
      Page::Shield,
      Page::Swap,
      Page::Bridge,
      Page::Wallets,
      Page::Transactions,
      Page::Approvals,
      Page::Settings,
      Page::ConnectedDapps,
   ] {
      let prefix = format!("Go to {}:", page.name());

      assert!(
         names.iter().any(|name| name.starts_with(prefix.as_str())),
         "the palette must list {prefix:?}: {names:?}"
      );
   }

   assert!(
      names.contains(&"Change keyboard shortcuts"),
      "the page that changes the chords is a command too: {names:?}"
   );

   let words = label_words(&nodes);
   let count = ShortcutAction::ALL.len() + 10 + 1;

   assert!(
      words.iter().any(|word| *word == format!("{count} commands")),
      "the palette must say how many commands it holds: {words:?}"
   );
}

/// The chord that opened the palette is the chord that closes it, so a user who moves that key is
/// not left with a dialog only the old key leaves.
#[test]
fn the_chord_that_opened_the_palette_closes_it() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| zeus.vault_unlocked = true);
   wallet_in_cache(&zeus);

   gui.keymap
      .set(
         ShortcutAction::OpenPalette,
         Binding::new(Key::P, Chord::Alt),
      )
      .expect("Alt+P is a free chord");

   press_shortcut(&ctx, &mut gui, &zeus, Key::P, Modifiers::ALT);

   assert!(
      gui.palette.is_open(),
      "the chord the user chose must open it"
   );

   press_shortcut(&ctx, &mut gui, &zeus, Key::P, Modifiers::ALT);

   assert!(
      !gui.palette.is_open(),
      "the same chord must close it"
   );
}

/// Escape closes the palette the way it closes every other dialog.
#[test]
fn escape_puts_the_palette_away() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| zeus.vault_unlocked = true);
   wallet_in_cache(&zeus);

   press_shortcut(&ctx, &mut gui, &zeus, Key::K, Modifiers::COMMAND);
   draw_overlays(&ctx, &mut gui);

   frame_nodes(
      &ctx,
      vec![key_combo(Key::Escape, Modifiers::NONE)],
      |ui| gui.show_overlay_modals(ui),
   );

   assert!(
      !gui.palette.is_open(),
      "Escape must put the palette away"
   );
}

/// Enter runs the row the query leaves first, and the command runs on the pass that has the context
/// to run it: the palette itself draws in a pass without one.
#[test]
fn enter_runs_the_command_the_query_found() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| zeus.vault_unlocked = true);
   wallet_in_cache(&zeus);

   press_shortcut(&ctx, &mut gui, &zeus, Key::K, Modifiers::COMMAND);

   // The field asks for the focus on the pass the palette first paints in.
   draw_overlays(&ctx, &mut gui);

   gui.palette.set_query("transactions");

   frame_nodes(
      &ctx,
      vec![key_combo(Key::Enter, Modifiers::NONE)],
      |ui| {
         zeus.write(|zeus| crate::gui::ui::shortcuts::handle(&mut gui, zeus, ui.ctx()));
      },
   );

   assert!(
      !gui.palette.is_open(),
      "Enter runs the command and puts the palette away"
   );

   frame_nodes(&ctx, Vec::new(), |ui| {
      zeus.write(|zeus| crate::gui::ui::shortcuts::handle(&mut gui, zeus, ui.ctx()));
   });

   assert_eq!(
      gui.current_page(),
      Page::Transactions,
      "the command must open the page the query named"
   );
}

/// The palette owns the keyboard while it is up: a page key pressed behind it changes nothing.
#[test]
fn a_page_key_does_not_reach_behind_the_palette() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| zeus.vault_unlocked = true);
   wallet_in_cache(&zeus);

   press_shortcut(&ctx, &mut gui, &zeus, Key::K, Modifiers::COMMAND);
   render_shortcuts(
      &ctx,
      &mut gui,
      &zeus,
      vec![key_combo(Key::Num2, Modifiers::ALT)],
   );

   assert_eq!(
      gui.current_page(),
      Page::Home,
      "the palette keeps the page where it was"
   );
}

/// The lock screen owns the keyboard, so the palette cannot open over it.
#[test]
fn the_palette_is_inert_behind_the_lock_screen() {
   let (ctx, mut gui, zeus) = main_window();

   press_shortcut(&ctx, &mut gui, &zeus, Key::K, Modifiers::COMMAND);

   assert!(
      !gui.palette.is_open(),
      "the lock screen owns the keyboard"
   );
}

/// A chord the user moved takes its action with it, and the chord it left does nothing: the layer
/// reads the table, not a default written into the handler.
#[test]
fn a_remapped_chord_copies_the_address_and_the_old_one_stops() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| zeus.vault_unlocked = true);

   let wallet = master_wallet("Main");
   let address = wallet.address.to_string();

   gui.account_panel.open();
   gui.account_panel.set_current_wallet(wallet);

   gui.keymap
      .set(
         ShortcutAction::CopyAddress,
         Binding::new(Key::L, Chord::Alt),
      )
      .expect("Alt+L is a free chord");

   render_account_card(&ctx, &mut gui, &zeus, Vec::new());
   render_account_card(&ctx, &mut gui, &zeus, Vec::new());

   let (_nodes, commands) = render_account_card(
      &ctx,
      &mut gui,
      &zeus,
      vec![key_combo(Key::L, Modifiers::ALT)],
   );

   assert_eq!(
      clipboard_text(&commands),
      Some(address),
      "the chord the user chose must copy the address"
   );

   let (_nodes, commands) = render_account_card(
      &ctx,
      &mut gui,
      &zeus,
      vec![key_combo(Key::C, Modifiers::COMMAND | Modifiers::SHIFT)],
   );

   assert_eq!(
      clipboard_text(&commands),
      None,
      "the chord the action was moved off must do nothing"
   );
}

/// The sheet prints the chord in the table, so a remapped key is described by the key that runs it.
#[test]
fn the_sheet_shows_the_chord_the_user_chose() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| zeus.vault_unlocked = true);
   wallet_in_cache(&zeus);

   gui.keymap
      .set(
         ShortcutAction::CopyAddress,
         Binding::new(Key::L, Chord::Alt),
      )
      .expect("Alt+L is a free chord");

   press_shortcut(&ctx, &mut gui, &zeus, Key::F1, Modifiers::NONE);

   let words = label_words(&draw_overlays(&ctx, &mut gui));

   assert!(
      words.iter().any(|word| *word == "Alt+L: copy your address"),
      "the sheet must describe the chord that answers: {words:?}"
   );
   assert!(
      !words.iter().any(|word| *word == "Ctrl+Shift+C: copy your address"),
      "the sheet must not keep describing the chord the action left: {words:?}"
   );
}

/// The Keyboard page offers a recorder for every action, each named after the action it changes.
#[test]
fn the_keyboard_page_names_every_action() {
   let (ctx, mut gui, zeus) = settings_window();
   zeus.write(|zeus| {
      gui.settings.open_page(
         SettingsPage::Keyboard,
         zeus,
         &mut gui.appearance_settings,
      );
   });

   let nodes = render_settings(&ctx, &mut gui, &zeus, Vec::new());

   let names: Vec<&str> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Button)
      .filter_map(|(_, node)| node.label())
      .collect();

   for action in ShortcutAction::ALL {
      let expected = format!("Change {}", action.label());

      assert!(
         names.contains(&expected.as_str()),
         "the page must offer {expected:?}: {names:?}"
      );
   }
}

/// One frame with the Settings window up, the shortcut layer in front of it and one key in the
/// frame's input, which is the order the app runs them in.
fn record_chord(
   ctx: &Context,
   gui: &mut GUI,
   zeus: &ZeusCtx,
   events: Vec<Event>,
) -> Vec<(NodeId, Node)> {
   let icons = gui.icons.clone();
   let theme = gui.theme.clone();

   frame_nodes(ctx, events, |ui| {
      zeus.write(|zeus| {
         crate::gui::ui::shortcuts::handle(gui, zeus, ui.ctx());
         gui.settings.show(
            zeus,
            icons.clone(),
            &theme,
            &mut gui.appearance_settings,
            &mut gui.keymap,
            ui,
         );
      });
   })
}

/// The button that records Copy address, in the tree the page drew.
fn copy_address_recorder(nodes: &[(NodeId, Node)]) -> NodeId {
   nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some("Change Copy address"))
      .map(|(id, _)| *id)
      .expect("the page draws the button that records Copy address")
}

/// A refused chord is said out loud and the recording stays open for the next attempt.
///
/// The chord chosen here is the one Previous control answers, which is what proves the shortcut
/// layer stood down: were it still reading the keyboard, it would have taken the key and the page
/// would never have seen it.
#[test]
fn a_refused_chord_keeps_the_recording_open() {
   let (ctx, mut gui, zeus) = settings_window();
   zeus.write(|zeus| {
      gui.settings.open_page(
         SettingsPage::Keyboard,
         zeus,
         &mut gui.appearance_settings,
      );
   });

   let nodes = render_settings(&ctx, &mut gui, &zeus, Vec::new());
   let recorder = copy_address_recorder(&nodes);

   render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, recorder)],
   );

   assert!(
      gui.settings.is_capturing(),
      "the button starts recording"
   );

   record_chord(
      &ctx,
      &mut gui,
      &zeus,
      vec![key_combo(Key::Tab, Modifiers::CTRL | Modifiers::SHIFT)],
   );

   let notice = gui.settings.take_notice().expect("a refused chord must be said");

   assert!(
      notice.contains("Previous control"),
      "the refusal must name the action that already answers the chord: {notice:?}"
   );
   assert!(
      gui.settings.is_capturing(),
      "a refused chord keeps the recording open for the next attempt"
   );
}

/// A text field's own chord cannot be recorded: the layer takes keys before a field draws, so the
/// binding would break copy, paste and undo in every text field in the app.
#[test]
fn a_text_fields_chord_cannot_be_recorded() {
   let (ctx, mut gui, zeus) = settings_window();
   zeus.write(|zeus| {
      gui.settings.open_page(
         SettingsPage::Keyboard,
         zeus,
         &mut gui.appearance_settings,
      );
   });

   let nodes = render_settings(&ctx, &mut gui, &zeus, Vec::new());
   let recorder = copy_address_recorder(&nodes);

   render_settings(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, recorder)],
   );

   record_chord(
      &ctx,
      &mut gui,
      &zeus,
      vec![key_combo(Key::C, Modifiers::COMMAND)],
   );

   let notice = gui.settings.take_notice().expect("a refused chord must be said");

   assert!(
      notice.contains("text fields"),
      "the refusal must say why the chord cannot be used: {notice:?}"
   );
   assert_eq!(
      gui.keymap.binding(ShortcutAction::CopyAddress),
      ShortcutAction::CopyAddress.default_binding(),
      "a refused chord changes nothing"
   );
}

/// The palette can reach the page that changes the chords, so a user who never opens Settings still
/// finds it.
#[test]
fn the_palette_opens_the_keyboard_settings() {
   let (ctx, mut gui, zeus) = main_window();
   zeus.write(|zeus| zeus.vault_unlocked = true);
   wallet_in_cache(&zeus);

   press_shortcut(&ctx, &mut gui, &zeus, Key::K, Modifiers::COMMAND);
   draw_overlays(&ctx, &mut gui);

   gui.palette.set_query("change keyboard");

   frame_nodes(
      &ctx,
      vec![key_combo(Key::Enter, Modifiers::NONE)],
      |ui| {
         zeus.write(|zeus| crate::gui::ui::shortcuts::handle(&mut gui, zeus, ui.ctx()));
      },
   );

   frame_nodes(&ctx, Vec::new(), |ui| {
      zeus.write(|zeus| crate::gui::ui::shortcuts::handle(&mut gui, zeus, ui.ctx()));
   });

   assert!(
      gui.settings.is_open(),
      "the command must open Settings"
   );

   let words = label_words(&render_settings(
      &ctx,
      &mut gui,
      &zeus,
      Vec::new(),
   ));

   assert!(
      words.iter().any(|word| *word == "Keyboard"),
      "Settings must land on the Keyboard page: {words:?}"
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

/// A reader's window never hides in the tray: the icon behind it is a pointer target, so a hidden
/// window leaves the keyboard and alt-tab both, with nothing but a mouse to bring it back.
#[test]
fn a_reader_window_never_minimizes_into_the_tray() {
   use crate::gui::tray::minimize_hides_window;

   assert!(
      minimize_hides_window(true, false),
      "a sighted user's preference still hides the window"
   );

   assert!(
      !minimize_hides_window(true, true),
      "a reader's window must stay where the keyboard and alt-tab can reach it"
   );

   assert!(
      !minimize_hides_window(false, false),
      "the preference off is the preference off"
   );
}
