use super::super::app::prompt_harness;
use super::super::harness::{
   accesskit_action, drawn_names, focused_node, frame, frame_nodes, key_press, node_position,
   opener_button, spoken_text,
};
use super::prompt::{dialogs, render_modal, text_field};
use crate::core::clear_signing::ClearDisplay;
use crate::core::tx::{FeeTiers, PriorityTier};
use crate::core::{TransactionAnalysis, TransactionRich};
use crate::gui::ui::common::spoken_row;
use crate::gui::ui::tx::{
   FeeSelection, TxConfirmationWindow, TxWindow, events::DecodedEvents, show_analysis_buttons,
   show_calldata_modal, show_tx_diffs_modal, show_tx_options_modal, tx_window::DetailsSheet,
};
use egui::accesskit::{Action, Node, Role};
use egui::{Event, Id, Key, RichText, Ui, vec2};
use zeus_eth::alloy_primitives::U256;
use zeus_eth::types::ChainId;
use zeus_eth::utils::NumericValue;

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
