use super::app::{
   main_window, master_wallet, prompt_harness, render_central, render_settings, settings_window,
   shared_gui_turn, wallet_in_cache, wallet_named,
};
#[cfg(feature = "dev")]
use super::app::{render_unlock_screen, unlock_screen};
use super::harness::{
   accesskit_action, announced, button_named, clipboard_text, copy_said, drawn_names, focused_id,
   focused_node, focused_text, frame, frame_nodes, frame_nodes_and_output, is_control,
   is_text_input, key_combo, key_press, key_release, label_texts, label_words, node_name,
   node_position, node_texts, opener_button, spoken_text, status_text, tree_order,
};
use super::settings_pages::host_of;
use crate::assets::icons::Icons;
use crate::core::clear_signing::ClearDisplay;
use crate::core::signature::msg::Permit2612Details;
use crate::core::tx::{FeeTiers, PriorityTier};
use crate::core::types::Recipient;
use crate::core::{
   SignMsgType, TransactionAnalysis, TransactionRich, WalletInfo, WalletValue, ZeusCtx,
};
use crate::gui::appearance::AppearanceSettings;
use crate::gui::ui::common::spoken_row;
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
use crate::gui::ui::sign_msg_window::SignMsgWindow;
use crate::gui::ui::tx::{
   FeeSelection, TxConfirmationWindow, TxWindow, confrim_window::PromptSheet,
   events::DecodedEvents, show_analysis_buttons, show_calldata_modal, show_tx_diffs_modal,
   show_tx_options_modal, tx_window::DetailsSheet,
};
use crate::gui::ui::wallet::import::ImportWalletType;
use crate::gui::{GUI, Page};
use crate::utils::swap_quoter::SwapStep;
use egui::accesskit::{Action, Node, NodeId, Role, Toggled};
use egui::{Context, Event, Id, Key, Modifiers, RichText, Ui, vec2};
use egui_elements::{Theme, ThemeKind};
use secure_types::SecureString;
use std::sync::Arc;
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

/// Write the frame's `TreeUpdate` to `logs/accessibility-tree.txt` for review: one
/// line per node, `role | name | value | description | actions`, which is what a reviewer reads
/// and what a later slice diffs.
///
/// Dev builds only, and only when asked:
/// `ZEUS_ACCESSIBILITY_DUMP=1 cargo test --features dev --bin zeus accessibility`.
///
/// The dump is taken from a headless frame rather than the running app on purpose: eframe builds
/// `platform_output.accesskit_update` in `Context::end_pass`, after `App::ui` has returned, and
/// owns the `FullOutput` it lands in, so the app itself can never see the frame's tree.
#[cfg(feature = "dev")]
#[test]
fn dump_the_unlock_screen_tree() {
   if std::env::var("ZEUS_ACCESSIBILITY_DUMP").as_deref() != Ok("1") {
      return;
   }

   let ctx = Context::default();
   let (locked, mut screen, mut theme) = unlock_screen();
   theme.install(&ctx);

   let nodes = render_unlock_screen(&ctx, &locked, &mut screen, &theme);
   let tree: String = nodes.iter().map(|node| format!("{}\n", node_line(node))).collect();

   if let Err(e) = std::fs::create_dir_all("logs") {
      panic!("could not create logs/: {e}");
   }
   if let Err(e) = std::fs::write("logs/accessibility-tree.txt", tree) {
      panic!("could not write logs/accessibility-tree.txt: {e}");
   }
}

/// One dump line for a node: `role | name | value | description | actions`.
#[cfg(feature = "dev")]
fn node_line(node: &Node) -> String {
   use egui::accesskit::Action;

   let mut actions = String::new();

   for action in [
      Action::Click,
      Action::Focus,
      Action::Expand,
      Action::Collapse,
      Action::SetValue,
      Action::ScrollIntoView,
   ] {
      if !node.supports_action(action) {
         continue;
      }
      if !actions.is_empty() {
         actions.push(',');
      }
      actions.push_str(&format!("{action:?}"));
   }

   format!(
      "{:?} | {} | {} | {} | {}",
      node.role(),
      node.label().unwrap_or_default(),
      node.value().unwrap_or_default(),
      node.description().unwrap_or_default(),
      actions,
   )
}

/// Draw the confirmation window into a reader's tree.
///
/// A local window stands in for the prompt — nothing claims it and no task prepares it — and the
/// frame draws it out of a local context, which is exactly what `TxConfirmationWindow::show`
/// does in the app.
///
/// Three passes: a shrink-wrapping modal spends two invisible sizing passes before the first
/// frame it actually paints, and egui exposes no node for a widget it considers invisible. The
/// last pass is the one a reader is handed — and the first one the window may take focus on.
fn render_confirmation(
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

/// The dialogs a window's tree is made of, by the headings they are named with.
fn dialogs(nodes: &[(NodeId, Node)]) -> Vec<String> {
   nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Dialog)
      .filter_map(|(_, node)| node.label())
      .map(str::to_string)
      .collect()
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
fn longest_hex_run(text: &str) -> usize {
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

/// Draw a modal the way the window hosting it draws it, into a reader's tree.
///
/// Three passes, for the same reason as the confirmation and the sign window: a
/// shrink-wrapping modal spends two invisible sizing passes before the first frame it actually
/// paints, and egui exposes no node for a widget it considers invisible. The events go to the
/// last pass only, so a key is not pressed three times.
fn render_modal(
   ctx: &Context,
   events: Vec<Event>,
   mut add: impl FnMut(&mut Ui),
) -> Vec<(NodeId, Node)> {
   let mut nodes = Vec::new();

   for pass in 0..3 {
      let events = if pass == 2 {
         events.clone()
      } else {
         Vec::new()
      };

      nodes = frame_nodes(ctx, events, &mut add);
   }

   nodes
}

/// The text field a reader reaches under `name`, however the tree spells that name: the field's
/// own label, or the value of the label it points at.
fn text_field<'a>(nodes: &'a [(NodeId, Node)], name: &str) -> Option<&'a Node> {
   nodes
      .iter()
      .map(|(_, node)| node)
      .find(|node| is_text_input(node.role()) && node_name(nodes, node).as_deref() == Some(name))
}

/// The Events and State buttons paint their count in a badge that reserves no layout space, so
/// the number is in neither the label nor anything else a reader can reach: each button's node
/// says it, while the two buttons without a count keep the word they draw.
#[test]
fn the_inspector_buttons_name_the_counts_they_paint() {
   let (ctx, _handle, theme, _icons, _appearance) = prompt_harness();
   let analysis = TransactionAnalysis::dummy_with_diffs();

   let events = analysis.decoded_events.len();
   let changes = analysis.balance_diff.len() + analysis.approval_diff.len();

   let nodes = frame(&ctx, |ui| {
      show_analysis_buttons(&analysis, &theme, 600.0, true, ui);
   });

   let names: Vec<&str> = nodes
      .iter()
      .filter(|node| node.role() == Role::Button)
      .filter_map(|node| node.label())
      .collect();

   assert!(
      names.contains(&format!("Events, {events}").as_str()),
      "the Events button must say how many events the transaction carries: {names:?}"
   );
   assert!(
      names.contains(&format!("State, {changes}").as_str()),
      "the State button must say how many changes it holds: {names:?}"
   );
   assert!(
      names.contains(&"Calldata") && names.contains(&"Tx Options"),
      "the two buttons without a count keep the words they draw: {names:?}"
   );
   assert!(
      !names.contains(&"Events") && !names.contains(&"State"),
      "a count left out of the name is a count a reader never hears: {names:?}"
   );
}

/// The events window's counter is a sentence a reader can repeat: it says how many of the
/// transaction's events could be read, and the order of the two numbers says which is which.
#[test]
fn the_events_window_says_how_many_events_were_read() {
   let (ctx, handle, theme, icons, _appearance) = prompt_harness();
   let mut events = DecodedEvents::new();
   events.open();

   // Two different numbers, so a sentence with the arguments the other way round is not this one.
   let mut analysis = TransactionAnalysis::dummy_with_diffs();
   analysis.logs_len = 5;
   analysis.known_events = 3;

   let nodes = render_modal(&ctx, Vec::new(), |ui| {
      handle.write(|zeus| {
         events.show(
            zeus,
            ChainId::from(analysis.chain),
            &theme,
            icons.clone(),
            &analysis,
            vec2(400.0, 45.0),
            theme.frame2,
            (600.0, 400.0),
            ui,
         );
      });
   });

   let sentence = "3 of 5 events could be read";

   assert!(
      nodes
         .iter()
         .any(|(_, node)| node.role() == Role::Label && node.value() == Some(sentence)),
      "the counter must say how many events could be read: {:?}",
      drawn_names(&nodes)
   );
}

/// Every amount in the events window is drawn as the token's ticker and read as its name. The row
/// paints its words, so the glyphs make no node at all and the one node the row makes is the
/// sentence: a reader is never handed a ticker to spell out.
#[test]
fn the_events_window_says_what_each_amount_is() {
   let (ctx, handle, theme, icons, _appearance) = prompt_harness();

   // Each decoded action, and the token names its amount rows must be read in. A name with a
   // leading space is a suffix, so no figure is pinned to one formatting.
   let cases: Vec<(TransactionAnalysis, Vec<&str>)> = vec![
      (
         TransactionAnalysis::dummy_transfer(),
         vec![" Ethereum"],
      ),
      (
         TransactionAnalysis::dummy_erc20_transfer(),
         vec![" Wrapped Ether"],
      ),
      (
         TransactionAnalysis::dummy_swap(),
         vec![" Wrapped Ether", " Dai Stablecoin"],
      ),
      // A bridge out of WETH is drawn as the native coin the EOA receives.
      (
         TransactionAnalysis::dummy_bridge(),
         vec![" Ethereum"],
      ),
      (
         TransactionAnalysis::dummy_wrap_eth(),
         vec![" Wrapped Ether"],
      ),
      (
         TransactionAnalysis::dummy_unwrap_weth(),
         vec![" Ethereum"],
      ),
      (
         TransactionAnalysis::dummy_uniswap_position_operation(),
         vec![" Wrapped Ether", " Dai Stablecoin"],
      ),
   ];

   for (analysis, names) in cases {
      let mut events = DecodedEvents::new();
      events.open();

      let nodes = render_modal(&ctx, Vec::new(), |ui| {
         handle.write(|zeus| {
            events.show(
               zeus,
               ChainId::from(analysis.chain),
               &theme,
               icons.clone(),
               &analysis,
               vec2(400.0, 60.0),
               theme.frame2,
               (600.0, 900.0),
               ui,
            );
         });
      });

      let spoken = spoken_text(&nodes);
      let action = analysis.decoded_selector.as_str();

      for name in names {
         let row = nodes
            .iter()
            .find(|(_, node)| {
               node.role() == Role::Label && node.value().is_some_and(|text| text.ends_with(name))
            })
            .map(|(_, node)| node)
            .unwrap_or_else(|| {
               panic!("a {action} row must say the token's name ({name:?}): {spoken:?}")
            });

         assert_eq!(
            row.label(),
            row.value(),
            "the row says one sentence, in the words it is read in: {action}"
         );
      }

      // The ticker is what the row paints, and a painted word makes no node: no figure a reader
      // meets is spelled with one, so nothing has to be read out letter by letter.
      for text in drawn_names(&nodes).into_iter().filter(|text| {
         text.starts_with(['+', '-']) || text.starts_with(|c: char| c.is_ascii_digit())
      }) {
         assert!(
            ![" WETH", " ETH", " DAI", " USDC"].iter().any(|ticker| text.ends_with(ticker)),
            "an amount row still hands the reader its ticker: {text:?} ({action})"
         );
      }
   }
}

/// An amount row is a stop of its own: `Tab` lands on it, and what it says is the words, not the
/// ticker painted under them.
#[test]
fn tab_reaches_a_painted_amount_row() {
   let (ctx, _handle, theme, _icons, _appearance) = prompt_harness();

   let mut rows = |ui: &mut Ui| {
      let text = RichText::new("- 1.00 WETH").size(theme.typography.large);

      spoken_row(
         &theme,
         ui,
         "- 1.00 Wrapped Ether",
         &[(text, None)],
         0.0,
      );
   };

   // The first frame lays the row out and asks for the focus; the second delivers the Tab.
   frame_nodes(&ctx, Vec::new(), &mut rows);
   let nodes = frame_nodes(&ctx, vec![key_press(Key::Tab)], &mut rows);

   assert_eq!(
      focused_node(&ctx, &nodes).value(),
      Some("- 1.00 Wrapped Ether"),
      "Tab lands on the words the row is read in"
   );
}

/// Approvals are the half of a state report that can cost money after the transaction, so the
/// modal reads them before the balances — the order the confirmation window keeps — and each
/// section says how many changes it holds.
#[test]
fn the_state_changes_modal_reads_approvals_before_balances() {
   let (ctx, handle, theme, icons, _appearance) = prompt_harness();
   let analysis = TransactionAnalysis::dummy_with_diffs();
   let chain = ChainId::from(analysis.chain);
   let mut open = true;

   let nodes = render_modal(&ctx, Vec::new(), |ui| {
      handle.write(|zeus| {
         show_tx_diffs_modal(
            &mut open,
            &theme,
            zeus,
            chain,
            icons.clone(),
            &analysis.balance_diff,
            &analysis.approval_diff,
            ui,
         );
      });
   });

   // Each section draws the count it holds, and the drawn title is the whole node, so a title
   // that lost its count is not found here at all: this is the order and the words together.
   let approvals = node_position(
      &nodes,
      &format!(
         "Approval changes ({})",
         analysis.approval_diff.len()
      ),
   );
   let balances = node_position(
      &nodes,
      &format!(
         "Balance changes ({})",
         analysis.balance_diff.len()
      ),
   );

   assert!(
      approvals < balances,
      "the approval section must be read before the balance section"
   );
}

/// The fee and gas fields used to be nameless text inputs, and a tier row's gwei and cost are
/// painted beside its label rather than into it: each field is named, and each row's name says
/// the three things its line draws.
#[test]
fn the_transaction_options_name_their_fields_and_their_tiers() {
   let (ctx, handle, theme, _icons, _appearance) = prompt_harness();
   // The custom field slides open; without a still animation the clip it is drawn in has no
   // height yet on the frame a reader is handed.
   ctx.all_styles_mut(|style| style.animation_time = 0.0);

   let tiers = FeeTiers {
      normal: U256::from(12_500_000_000u64),
      fast: U256::from(15_000_000_000u64),
      very_fast: U256::from(18_000_000_000u64),
   };
   let costs = vec![NumericValue::currency_price(0.42); 3];

   let nodes = {
      let mut tier = PriorityTier::Normal;
      let mut custom = true;
      let mut custom_value = "12.5".to_string();
      let mut gas_limit = "45000".to_string();
      let mut open = true;
      let mut fee = FeeSelection {
         tiers: Some(tiers),
         tier: &mut tier,
         custom: &mut custom,
         custom_value: &mut custom_value,
         costs_usd: &costs,
      };

      render_modal(&ctx, Vec::new(), |ui| {
         handle.write(|_zeus| {
            show_tx_options_modal(
               &mut open,
               &theme,
               ChainId::Ethereum,
               &mut fee,
               &mut gas_limit,
               45_000,
               false,
               false,
               ui,
            );
         });
      })
   };

   assert_eq!(
      text_field(&nodes, "Custom priority fee in gwei").map(|node| node.role()),
      Some(Role::TextInput),
      "the custom fee is a text input and must be named"
   );
   assert!(
      text_field(&nodes, "Gas limit").is_some(),
      "the gas limit is a text input and must be named"
   );

   let rows: Vec<&str> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Button)
      .filter_map(|(_, node)| node.label())
      .collect();

   for tier in PriorityTier::ALL {
      let row = rows.iter().find(|row| row.starts_with(tier.label())).unwrap_or_else(|| {
         panic!(
            "the {} row must be a button in the tree",
            tier.label()
         )
      });

      assert!(
         row.contains(" gwei"),
         "a tier row must say the gwei it sets: {row:?}"
      );
   }

   let normal = NumericValue::format_to_gwei(tiers.normal).formatted;
   let row = rows
      .iter()
      .find(|row| row.starts_with("Normal"))
      .expect("the Normal row must be a button in the tree");

   assert!(
      row.contains(&format!("{normal} gwei")),
      "the Normal row must name the tip it sets: {row:?}"
   );
   assert!(
      row.contains("about $"),
      "a row that carries a cost must say what it costs: {row:?}"
   );

   // A chain with no recommendation draws the same two fields on one line, and both need the
   // same names.
   let nodes = {
      let mut tier = PriorityTier::Normal;
      let mut custom = false;
      let mut custom_value = String::new();
      let mut gas_limit = "45000".to_string();
      let mut open = true;
      let mut fee = FeeSelection {
         tiers: None,
         tier: &mut tier,
         custom: &mut custom,
         custom_value: &mut custom_value,
         costs_usd: &costs,
      };

      render_modal(&ctx, Vec::new(), |ui| {
         handle.write(|_zeus| {
            show_tx_options_modal(
               &mut open,
               &theme,
               ChainId::Ethereum,
               &mut fee,
               &mut gas_limit,
               45_000,
               false,
               false,
               ui,
            );
         });
      })
   };

   assert!(
      text_field(&nodes, "Priority fee in gwei").is_some(),
      "the free-text priority fee must be named"
   );
   assert!(
      text_field(&nodes, "Gas limit").is_some(),
      "the gas limit must be named in the two-column layout as well"
   );
}

/// The raw calldata was an editable field whose edits are thrown away, and nothing named it: it
/// is a read-only multiline input called Raw calldata, and the drawn word it replaces is hidden
/// so it is not heard on its own.
#[test]
fn the_raw_calldata_is_one_named_read_only_view() {
   let (ctx, handle, theme, icons, _appearance) = prompt_harness();
   let display = ClearDisplay::dummy_calldata();
   let mut open = true;

   let nodes = render_modal(&ctx, Vec::new(), |ui| {
      handle.write(|zeus| {
         show_calldata_modal(
            &mut open,
            &theme,
            zeus,
            ChainId::Base,
            icons.clone(),
            Some(&display),
            "0x617ba037".to_string(),
            ui,
         );
      });
   });

   let field = text_field(&nodes, "Raw calldata").expect("the raw viewer must be named");

   assert_eq!(
      field.role(),
      Role::MultilineTextInput,
      "the viewer is the multiline input it draws"
   );
   assert_eq!(
      field.description(),
      Some("The raw calldata of this transaction."),
      "the field must say what it holds"
   );
   assert!(
      field.is_read_only(),
      "a viewer that throws every edit away must not offer itself for typing"
   );
   assert!(
      !field.supports_action(Action::SetValue),
      "a read-only field takes no value from a reader"
   );

   // The drawn word is a label node of its own; the text run it paints stays under that node
   // and is hidden with it, which is what takes the word out of the tree a reader is handed.
   let raw: Vec<&Node> = nodes
      .iter()
      .map(|(_, node)| node)
      .filter(|node| node.role() == Role::Label && node.value() == Some("Raw"))
      .collect();

   assert_eq!(
      raw.len(),
      1,
      "the drawn Raw label is the one node the field replaces"
   );
   assert!(
      raw[0].is_hidden(),
      "the word must be hidden, or a reader hears it on its own"
   );
}

/// Escape is the window's way out: the modal ignores it while it is not closable and the local
/// open flag was never written back, so the only exit used to be a button after the whole
/// description.
#[test]
fn escape_closes_the_transaction_details_window() {
   let (ctx, handle, theme, icons, _appearance) = prompt_harness();
   let mut window = TxWindow::for_test(Some(TransactionRich::dummy_clear_signed()), false);

   render_modal(&ctx, Vec::new(), |ui| {
      handle.write(|zeus| window.show(zeus, &theme, icons.clone(), ui));
   });

   assert!(
      window.is_open(),
      "the window must be drawn before the key is sent"
   );

   render_modal(&ctx, vec![key_press(Key::Escape)], |ui| {
      handle.write(|zeus| window.show(zeus, &theme, icons.clone(), ui));
   });

   assert!(
      !window.is_open(),
      "Escape must close the transaction details window"
   );
}

/// The same key owns the same sheet here: Escape closes the events, the calldata or the state
/// changes and leaves the window it was opened from. The sheet is the one dialog that is there
/// before the key and gone after it, which is how this reads without naming every heading the
/// window's sheets resolve to.
#[test]
fn an_escape_closes_a_details_sheet_and_not_the_window() {
   let (ctx, handle, theme, icons, _appearance) = prompt_harness();

   for sheet in [
      DetailsSheet::Events,
      DetailsSheet::Calldata,
      DetailsSheet::StateChanges,
   ] {
      let mut window =
         TxWindow::for_test(Some(TransactionRich::dummy_clear_signed()), false).with_sheet(sheet);

      let nodes = render_modal(&ctx, Vec::new(), |ui| {
         handle.write(|zeus| window.show(zeus, &theme, icons.clone(), ui));
      });

      assert_eq!(
         dialogs(&nodes).len(),
         2,
         "the sheet must be drawn over the window before the key is sent: {:?}",
         dialogs(&nodes),
      );

      render_modal(&ctx, vec![key_press(Key::Escape)], |ui| {
         handle.write(|zeus| window.show(zeus, &theme, icons.clone(), ui));
      });

      assert!(
         window.is_open(),
         "the Escape that closed the sheet must not close the details window"
      );

      let nodes = render_modal(&ctx, Vec::new(), |ui| {
         handle.write(|zeus| window.show(zeus, &theme, icons.clone(), ui));
      });

      assert_eq!(
         dialogs(&nodes),
         vec!["Transaction Details".to_string()],
         "Escape must close the sheet and leave the window behind it"
      );
   }
}

/// A failed transaction says so in the error colour and in nothing else: the same sentence is an
/// Alert node, which is the one thing a reader cannot miss.
#[test]
fn a_failed_transaction_is_an_alert() {
   let (ctx, handle, theme, icons, _appearance) = prompt_harness();

   let mut tx = TransactionRich::dummy_clear_signed();
   tx.success = false;

   let mut window = TxWindow::for_test(Some(tx), false);

   let nodes = render_modal(&ctx, Vec::new(), |ui| {
      handle.write(|zeus| window.show(zeus, &theme, icons.clone(), ui));
   });

   let alert = nodes
      .iter()
      .map(|(_, node)| node)
      .find(|node| {
         node.role() == Role::Alert
            && node.label().unwrap_or_default().to_lowercase().contains("failed")
      })
      .expect("a failed transaction must warn with an Alert node named for it");

   assert_eq!(
      alert.label(),
      Some("Transaction failed"),
      "the alert keeps the sentence the window draws"
   );
}

/// The transaction's outcome is part of its details, and a reader has no colour: the words are a
/// line of their own whichever way the transaction ended.
#[test]
fn the_details_window_says_the_transaction_status() {
   let (ctx, handle, theme, icons, _appearance) = prompt_harness();

   for (success, expected) in [(true, "Status: Confirmed"), (false, "Status: Failed")] {
      let mut tx = TransactionRich::dummy_clear_signed();
      tx.success = success;

      let mut window = TxWindow::for_test(Some(tx), false);

      let nodes = render_modal(&ctx, Vec::new(), |ui| {
         handle.write(|zeus| window.show(zeus, &theme, icons.clone(), ui));
      });

      assert!(
         nodes
            .iter()
            .any(|(_, node)| node.role() == Role::Label && node.value() == Some(expected)),
         "the details window must say {expected:?}: {:?}",
         nodes.iter().filter_map(|(_, node)| node.value()).collect::<Vec<_>>()
      );
   }
}

/// A spinner is drawn, not spoken, so the window's only content while the descriptor loads is a
/// picture: the state it is in is a text node of its own.
#[test]
fn the_loading_details_window_says_it_is_loading() {
   let (ctx, handle, theme, icons, _appearance) = prompt_harness();
   let mut window = TxWindow::for_test(None, true);

   let nodes = render_modal(&ctx, Vec::new(), |ui| {
      handle.write(|zeus| window.show(zeus, &theme, icons.clone(), ui));
   });

   assert!(
      nodes
         .iter()
         .any(|(_, node)| node.value() == Some("Loading transaction details")),
      "the loading state must say what it is waiting for"
   );
}

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

/// Draw the central panel and then the announcer, which is the order the app draws them in and the
/// only way a page's sentence is said: a page cannot announce from inside its own draw.
fn render_wallet_page(
   ctx: &Context,
   gui: &mut GUI,
   zeus: &ZeusCtx,
   events: Vec<Event>,
) -> Vec<(NodeId, Node)> {
   frame_nodes(ctx, events, |ui| {
      zeus.write(|zeus| crate::gui::ui::panels::central_panel::show(gui, zeus, ui));
      gui.show_announcer(ui);
   })
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

/// The recovery flow, with no vault behind it: these windows are the whole screen when Zeus has
/// nothing to unlock.
fn recovery_flow() -> (Context, GUI, ZeusCtx) {
   let (ctx, gui, zeus) = main_window();

   // Whether a vault exists comes out of the user's data directory, so the test states it rather
   // than inheriting the machine's setup.
   zeus.write(|zeus| zeus.vault_exists = false);

   (ctx, gui, zeus)
}

/// A window node's name: the title it was created with, wherever egui puts it.
fn window_name(node: &Node) -> Option<String> {
   node.label().or_else(|| node.value()).map(str::to_owned)
}

/// The name of every `egui::Window` in the tree, for the windows that are named by a sentence.
fn window_names(nodes: &[(NodeId, Node)]) -> Vec<String> {
   nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Window)
      .filter_map(|(_, node)| window_name(node))
      .collect()
}

/// Draw the recovery windows until they are painted: each is an `egui::Window`, measured on the pass
/// it opens, and egui exposes no node for a widget it considers invisible.
fn draw_recovery_flow(ctx: &Context, gui: &mut GUI, zeus: &ZeusCtx) -> Vec<(NodeId, Node)> {
   let theme = gui.theme.clone();
   let mut nodes = Vec::new();

   for _ in 0..3 {
      let recovery = &mut gui.recover_wallet_ui;
      let import = &mut gui.settings.import;

      nodes = frame_nodes(ctx, Vec::new(), |ui| {
         zeus.write(|zeus| recovery.show(zeus, &theme, import, ui));
      });
   }

   nodes
}

/// Draw the app-wide overlay dialogs and put this frame's announcement in the tree, the way the app
/// draws the announcer right after the panels.
fn draw_overlay_modals(ctx: &Context, gui: &mut GUI) -> Vec<Node> {
   frame(ctx, |ui| {
      gui.show_overlay_modals(ui);
      gui.show_announcer(ui);
   })
}

/// The first window of the recovery flow is named by a sentence a reader can act on, not by the
/// identifier the code gives it: the title bar is hidden, so the name is all a reader gets.
#[test]
fn the_first_run_window_is_named_by_a_sentence() {
   let (ctx, mut gui, zeus) = recovery_flow();

   let nodes = draw_recovery_flow(&ctx, &mut gui, &zeus);

   assert!(
      nodes
         .iter()
         .any(|(_, node)| node.role() == Role::Label && node.value() == Some("No vault was found")),
      "the credentials window must be drawn: {:?}",
      drawn_names(&nodes)
   );

   let names = window_names(&nodes);

   assert!(
      names.iter().any(|name| name == "Set up Zeus"),
      "the window must be named by a sentence, got {names:?}"
   );
}

/// The wallet-name window is named by a sentence too, and the box it opens on says what it is for
/// and takes the focus the credentials form just gave up.
#[test]
fn the_wallet_name_window_opens_on_a_field_that_says_what_it_is() {
   let (ctx, mut gui, zeus) = recovery_flow();
   gui.recover_wallet_ui.begin_wallet_name_for_test();

   let nodes = draw_recovery_flow(&ctx, &mut gui, &zeus);

   let names = window_names(&nodes);

   assert!(
      names.iter().any(|name| name == "Name your master wallet"),
      "the window must be named by a sentence, got {names:?}"
   );

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
      "the field carries that name itself; its hint stays the placeholder"
   );

   assert_eq!(
      ctx.memory(|memory| memory.focused()).map(|id| id.accesskit_id()),
      Some(*field),
      "the field must take the focus the credentials form gave up"
   );
}

/// The loading window is a dialog with no heading, so it says what it is doing twice over: its
/// message on the frame's announcement node, and a spinner named for the wait.
#[test]
fn the_loading_window_says_what_it_is_doing() {
   let (ctx, mut gui, _) = main_window();
   let message = "Recovering your wallet. This takes 10 to 15 minutes.";

   gui.loading_window.new_size((300.0, 150.0));
   gui.loading_window.open(message);

   let announcing = draw_overlay_modals(&ctx, &mut gui);

   assert_eq!(
      status_text(&announcing).as_deref(),
      Some(message),
      "the message must be said on the frame the window appears"
   );

   let mut painted = Vec::new();

   for _ in 0..3 {
      painted = draw_overlay_modals(&ctx, &mut gui);
   }

   assert!(
      painted
         .iter()
         .any(|node| node.role() == Role::ProgressIndicator && node.label() == Some("Working")),
      "the spinner must say what it is waiting for: {:?}",
      painted.iter().filter_map(|node| node.label()).collect::<Vec<_>>()
   );

   assert_ne!(
      status_text(&painted).as_deref(),
      Some(message),
      "a message that did not change is not said again"
   );

   // The same wait continuing is not a new phase: opening the window again with the sentence it
   // already carries must not interrupt the reader with it.
   gui.loading_window.open(message);

   let repeated = draw_overlay_modals(&ctx, &mut gui);

   assert_ne!(
      status_text(&repeated).as_deref(),
      Some(message),
      "a sentence the window already shows is not announced again"
   );

   // Progress text moves while the wait runs: it is drawn on the window, never said.
   let progress = "Download progress: 42.0%";

   gui.loading_window.progress(progress);

   let progressing = draw_overlay_modals(&ctx, &mut gui);

   assert_ne!(
      status_text(&progressing).as_deref(),
      Some(progress),
      "a percentage that changes is visible, not narrated"
   );
   assert!(
      progressing
         .iter()
         .any(|node| node.role() == Role::Label && node.value() == Some(progress)),
      "the progress must be drawn on the window: {:?}",
      progressing.iter().filter_map(|node| node.value()).collect::<Vec<_>>()
   );

   // A new sentence is a new phase, and that one is said once.
   let revoking = "Revoking the approval…";

   gui.loading_window.open(revoking);

   let changed = draw_overlay_modals(&ctx, &mut gui);

   assert_eq!(
      status_text(&changed).as_deref(),
      Some(revoking),
      "a different sentence opens a new announcement"
   );
}

/// The requirements warning is drawn only once the memory sample lands, so it can appear after the
/// user has started typing: each line is an alert carrying the words it draws, instead of a
/// coloured line nobody hears.
#[test]
fn the_memory_warning_is_heard_when_it_appears() {
   let (ctx, mut gui, zeus) = recovery_flow();

   // 16 GB in the machine and 4 GB free, against the 8.39 GB the recovery needs: both lines of the
   // warning are due.
   gui.recover_wallet_ui
      .set_memory_for_test(16 * 1024 * 1024 * 1024, 4 * 1024 * 1024 * 1024);

   let nodes = draw_recovery_flow(&ctx, &mut gui, &zeus);

   let alerts: Vec<&str> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Alert)
      .filter_map(|(_, node)| node.label())
      .collect();

   assert_eq!(
      alerts.len(),
      2,
      "both lines of the warning must be alerts: {alerts:?}"
   );

   for sentence in [
      "You need at least 8.39 GB of free RAM to recover your wallet",
      "You currently have 4.00 GB of free RAM",
   ] {
      assert!(
         alerts.contains(&sentence),
         "the warning line must be an alert carrying what it draws: {sentence:?} missing from \
          {alerts:?}"
      );
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

/// Draw the receive window and say what it left behind, the way the left panel does: the window
/// is drawn from the panel, and the panel hands its words to the app's one channel.
///
/// Three passes for the same reason as the other shrink-wrapping dialogs: the first two size it,
/// and egui exposes no node for a widget it considers invisible.
fn render_receive_window(
   ctx: &Context,
   gui: &mut GUI,
   zeus: &ZeusCtx,
   events: Vec<Event>,
) -> Vec<(NodeId, Node)> {
   let mut nodes = Vec::new();

   for pass in 0..3 {
      let events = if pass == 2 {
         events.clone()
      } else {
         Vec::new()
      };
      let theme = gui.theme.clone();

      nodes = frame_nodes(ctx, events, |ui| {
         zeus.write(|zeus| gui.account_panel.qr_window.show(zeus, &theme, ui));

         if let Some(text) = gui.account_panel.take_notice() {
            gui.announce(text);
         }

         gui.show_announcer(ui);
      });
   }

   nodes
}

/// The receive window draws no title, so its dialog node had no name at all. The mode the window
/// shows names it, the wallet is said as its name and kind, and the wide copy button is named with
/// the whole address grouped in fours — the drawn text repeats the address, but only the grouped
/// form lets a listener check it by ear. The shortening drawn on screen stays as the description.
#[test]
fn the_receive_window_says_which_address_it_shows() {
   let (ctx, mut gui, zeus) = main_window();
   let wallet = master_wallet("Main");
   let address = wallet.address.to_string();

   gui.account_panel.qr_window.open_for_test(Some(wallet));

   let nodes = render_receive_window(&ctx, &mut gui, &zeus, Vec::new());

   assert!(
      nodes.iter().any(
         |(_, node)| node.role() == Role::Dialog && node.label() == Some("Public address (EVM)")
      ),
      "the window must be a dialog named after the address it shows: {:?}",
      drawn_names(&nodes)
   );

   assert!(
      nodes.iter().any(|(_, node)| node.role() == Role::Label
         && node.value() == Some("Main, master wallet")),
      "the wallet must be said as its name and kind: {:?}",
      drawn_names(&nodes)
   );

   let spoken = crate::gui::ui::address_for_speech(None, &address);

   let button = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some(spoken.name.as_str()))
      .map(|(_, node)| node)
      .unwrap_or_else(|| {
         panic!(
            "the address button must be named by the shortening the row draws: {:?}",
            drawn_names(&nodes)
         )
      });

   assert_eq!(
      button.description(),
      spoken.description.as_deref(),
      "the whole address is the button's description"
   );
}

/// Copying the address changes nothing on screen, so the window says it happened. The sentence
/// goes through the panel to the app's one channel, and the next frame says nothing more.
#[test]
fn the_receive_window_says_when_an_address_is_copied() {
   let (ctx, mut gui, zeus) = main_window();
   let wallet = master_wallet("Main");
   let address = wallet.address.to_string();

   gui.account_panel.qr_window.open_for_test(Some(wallet));

   let nodes = render_receive_window(&ctx, &mut gui, &zeus, Vec::new());
   let spoken = crate::gui::ui::address_for_speech(None, &address);

   let copy = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some(spoken.name.as_str()))
      .map(|(id, _)| *id)
      .unwrap_or_else(|| {
         panic!(
            "the address button must be in the tree before it is clicked: {:?}",
            drawn_names(&nodes)
         )
      });

   let nodes = render_receive_window(
      &ctx,
      &mut gui,
      &zeus,
      vec![accesskit_action(Action::Click, copy)],
   );

   assert_eq!(
      announced(&nodes).as_deref(),
      Some(copy_said(&address).as_str()),
      "the copy must reach the announcement node the panel feeds, naming the address"
   );

   let nodes = render_receive_window(&ctx, &mut gui, &zeus, Vec::new());

   assert_eq!(
      announced(&nodes).as_deref().unwrap_or_default(),
      "",
      "the copy is said once, so the next frame says nothing more"
   );
}

/// A receive window with no wallet says what happened and what to do about it, in place of the
/// sentence that blamed the build.
#[test]
fn a_receive_window_without_a_wallet_says_what_to_do() {
   let (ctx, mut gui, zeus) = main_window();
   gui.account_panel.qr_window.open_for_test(None);

   let nodes = render_receive_window(&ctx, &mut gui, &zeus, Vec::new());
   let sentence = "No wallet selected. Close this window and open it from the account menu.";

   assert!(
      nodes
         .iter()
         .any(|(_, node)| node.role() == Role::Label && node.value() == Some(sentence)),
      "a window with no wallet must say what to do: {:?}",
      drawn_names(&nodes)
   );

   assert!(
      !nodes
         .iter()
         .any(|(_, node)| node_texts(node).into_iter().any(|text| text.contains("this is a bug"))),
      "no node may blame the build"
   );
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

/// Draw the receive window with the button that opened it beside it, in the same frame, so the
/// focus the window gives back can be looked up.
fn render_receive_window_after(
   ctx: &Context,
   gui: &mut GUI,
   zeus: &ZeusCtx,
   opener: Id,
   events: Vec<Event>,
) -> Vec<(NodeId, Node)> {
   let mut nodes = Vec::new();

   for pass in 0..3 {
      let events = if pass == 2 {
         events.clone()
      } else {
         Vec::new()
      };
      let theme = gui.theme.clone();

      nodes = frame_nodes(ctx, events, |ui| {
         opener_button(ui, opener, "Show QR Code");
         zeus.write(|zeus| gui.account_panel.qr_window.show(zeus, &theme, ui));

         if let Some(text) = gui.account_panel.take_notice() {
            gui.announce(text);
         }

         gui.show_announcer(ui);
      });
   }

   nodes
}

/// The receive window is about one address, so it opens on that address's copy button — not on
/// the Close button the modal would otherwise leave the focus on.
#[test]
fn the_receive_window_opens_on_its_address_button() {
   let (ctx, mut gui, zeus) = main_window();
   let wallet = master_wallet("Main");
   let address = wallet.address.to_string();

   gui.account_panel.qr_window.open_for_test(Some(wallet));

   let nodes = render_receive_window(&ctx, &mut gui, &zeus, Vec::new());
   let spoken = crate::gui::ui::address_for_speech(None, &address);
   let focused = focused_node(&ctx, &nodes);

   assert_eq!(
      focused.role(),
      Role::Button,
      "the first focus is the address button, not a picture or the Close button: {:?}",
      focused.role()
   );
   assert_eq!(
      focused.label(),
      Some(spoken.name.as_str()),
      "the focused button must be named by the shortening the row draws"
   );
}

/// A window with no wallet has nothing to copy: its one control, Close, is where a reader lands.
#[test]
fn a_receive_window_without_a_wallet_opens_on_close() {
   let (ctx, mut gui, zeus) = main_window();
   gui.account_panel.qr_window.open_for_test(None);

   let nodes = render_receive_window(&ctx, &mut gui, &zeus, Vec::new());
   let focused = focused_node(&ctx, &nodes);

   assert_eq!(
      focused.label(),
      Some("Close"),
      "a window with nothing to copy opens on its one control"
   );
}

/// Closing the receive window is not the end of the interaction: the control that opened it takes
/// the focus back, so the reader is where they were.
#[test]
fn closing_the_receive_window_gives_the_focus_back() {
   let (ctx, mut gui, zeus) = main_window();
   let wallet = master_wallet("Main");
   let opener = Id::unique("receive_window_opener");

   gui.account_panel.qr_window.open_for_test_with_opener(Some(wallet), opener);

   let nodes = render_receive_window_after(&ctx, &mut gui, &zeus, opener, Vec::new());
   let close = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some("Close"))
      .map(|(id, _)| *id)
      .expect("the window must offer its Close button");

   let nodes = render_receive_window_after(
      &ctx,
      &mut gui,
      &zeus,
      opener,
      vec![accesskit_action(Action::Click, close)],
   );

   assert!(
      !gui.account_panel.qr_window.is_open(),
      "the Close button must close the window"
   );

   let focused = focused_node(&ctx, &nodes);

   assert_eq!(
      focused.label(),
      Some("Show QR Code"),
      "the control that opened the window takes the focus back"
   );
}

/// The details window opens with the focus inside it: the reader lands on the window's heading,
/// not on the root window.
#[test]
fn the_transaction_details_window_opens_on_its_heading() {
   let (ctx, handle, theme, icons, _appearance) = prompt_harness();
   let opener = Id::unique("transaction_details_opener");
   let mut window = TxWindow::for_test_with_opener(
      Some(TransactionRich::dummy_clear_signed()),
      false,
      opener,
   );

   let nodes = render_modal(&ctx, Vec::new(), |ui| {
      opener_button(ui, opener, "Open transaction details");
      handle.write(|zeus| window.show(zeus, &theme, icons.clone(), ui));
      window.give_focus_back(ui.ctx());
   });

   let focused = focused_node(&ctx, &nodes);

   assert_eq!(
      focused.role(),
      Role::Heading,
      "the first focus is the window's heading, not the root window: {:?}",
      focused.role()
   );
   assert_eq!(
      focused.label(),
      Some("Transaction Details"),
      "the focused heading must name the window"
   );
}

/// Both exits hand the focus back to the control that opened the details window.
///
/// The request is made by the frame at the end of its pass, not by the window: the page under it
/// draws after the window, and a request aimed at that page from inside the window is cleared by
/// the page's own widgets while the modal layer is still up (`hand_focus_back`).
#[test]
fn closing_the_transaction_details_window_gives_the_focus_back() {
   let (ctx, handle, theme, icons, _appearance) = prompt_harness();
   let opener = Id::unique("transaction_details_opener");

   let mut window = TxWindow::for_test_with_opener(
      Some(TransactionRich::dummy_clear_signed()),
      false,
      opener,
   );

   let nodes = render_modal(&ctx, vec![key_press(Key::Escape)], |ui| {
      opener_button(ui, opener, "Open transaction details");
      handle.write(|zeus| window.show(zeus, &theme, icons.clone(), ui));
      window.give_focus_back(ui.ctx());
   });

   assert!(!window.is_open(), "Escape must close the window");
   assert_eq!(
      focused_node(&ctx, &nodes).label(),
      Some("Open transaction details"),
      "Escape must hand the focus back to the control that opened the window"
   );

   let mut window = TxWindow::for_test_with_opener(
      Some(TransactionRich::dummy_clear_signed()),
      false,
      opener,
   );

   let nodes = render_modal(&ctx, Vec::new(), |ui| {
      opener_button(ui, opener, "Open transaction details");
      handle.write(|zeus| window.show(zeus, &theme, icons.clone(), ui));
      window.give_focus_back(ui.ctx());
   });

   let close = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some("Close"))
      .map(|(id, _)| *id)
      .expect("the window must offer its Close button");

   let nodes = render_modal(
      &ctx,
      vec![accesskit_action(Action::Click, close)],
      |ui| {
         opener_button(ui, opener, "Open transaction details");
         handle.write(|zeus| window.show(zeus, &theme, icons.clone(), ui));
         window.give_focus_back(ui.ctx());
      },
   );

   assert!(
      !window.is_open(),
      "the Close button must close the window"
   );
   assert_eq!(
      focused_node(&ctx, &nodes).label(),
      Some("Open transaction details"),
      "the Close button must hand the focus back the same way"
   );
}

/// The keyboard comes back to the page when the details window closes.
///
/// The app draws a dialog before the page it covers, so on the frame a dialog closes the page's
/// widgets still draw last — and egui clears the focus of a widget drawn behind a modal layer,
/// which is what that page is while the layer is still on the stack. A request made from inside
/// the window is therefore wiped by the page that draws after it.
#[test]
fn closing_the_details_window_gives_the_page_under_it_its_focus_back() {
   let (ctx, handle, theme, icons, _appearance) = prompt_harness();
   let opener = Id::unique("transaction_details_opener");
   let mut window = TxWindow::for_test_with_opener(
      Some(TransactionRich::dummy_clear_signed()),
      false,
      opener,
   );

   // The app's order: the window first, then the page it was opened from, then the hand-back the
   // frame ends its pass with (`central_panel`).
   let draw = |events: Vec<Event>, window: &mut TxWindow| {
      render_modal(&ctx, events, |ui| {
         handle.write(|zeus| window.show(zeus, &theme, icons.clone(), ui));
         opener_button(ui, opener, "Details for Supply");
         window.give_focus_back(ui.ctx());
      })
   };

   draw(Vec::new(), &mut window);
   let nodes = draw(vec![key_press(Key::Escape)], &mut window);

   assert!(!window.is_open(), "Escape must close the window");
   assert_eq!(
      focused_node(&ctx, &nodes).label(),
      Some("Details for Supply"),
      "the row's own control must take the keyboard back"
   );
}

/// The prompt a revoke raises gives the keyboard back to the button that raised it.
///
/// Same frame shape as the details window: the prompt takes the screen over from a page that draws
/// under it, so the control the reader was on has to be given the focus again once the page has
/// drawn — otherwise the keyboard is dropped at the top of the page and the walk starts over.
#[test]
fn closing_the_confirmation_window_gives_the_button_its_focus_back() {
   let (ctx, handle, theme, icons, appearance) = prompt_harness();
   let mut window = TxConfirmationWindow::for_test(
      &handle,
      TransactionAnalysis::dummy_with_diffs(),
      "",
      false,
      None,
   );
   let opener = Id::unique("revoke_opener");

   // Where the reader was when the prompt took the screen over: the row's own Revoke button.
   ctx.memory_mut(|memory| memory.request_focus(opener));

   let mut nodes = Vec::new();

   for pass in 0..3 {
      let events = if pass == 2 {
         vec![key_press(Key::Escape)]
      } else {
         Vec::new()
      };

      // The page the prompt was raised from draws under it, and the frame hands the keyboard back
      // once everything has drawn — the order `central_panel` uses.
      nodes = frame_nodes(&ctx, events, |ui| {
         handle.write(|c| window.show(c, &theme, icons.clone(), &appearance, ui));
         opener_button(ui, opener, "Revoke USDC approval");
         window.give_focus_back(ui.ctx());
      });
   }

   assert!(!window.is_open(), "Escape must close the prompt");
   assert_eq!(
      focused_node(&ctx, &nodes).label(),
      Some("Revoke USDC approval"),
      "the button that raised the prompt must take the keyboard back"
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
