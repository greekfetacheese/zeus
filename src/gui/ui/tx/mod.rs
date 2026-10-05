//! This module contains the UI components for showing a transaction
//!
//! - The TxConfirmationWindow contains as much information as possible about the transaction before the user confirms it.
//! - The TxWindow is what we show to the user for a transaction that has been confirmed.

use egui::{
   Align, CursorIcon, FontId, Layout, Margin, OpenUrl, Order, RichText, ScrollArea, Sense,
   TextEdit, TextWrapMode, Ui, scroll_area::ScrollBarVisibility, vec2,
};
use egui_elements::{
   Button, Label, Modal, MultiLabel, Theme,
   widgets::{Badge as CornerBadge, BadgeCorner},
};
use egui_lucide::Lucide;
use elegance::{Badge, BadgeTone};
use zeus_eth::alloy_primitives::TxHash;

use crate::assets::icons::Icons;
use crate::core::clear_signing::{ClearDisplay, FormattedValue};
use crate::core::tx::{
   ApprovalChange, ApprovalDiff, ApprovalKind, BalanceChange, BalanceDiff, NftApprovalChange,
   NftApprovalTarget, NftApprovalValue, NftBalanceChange,
};
use crate::core::{TransactionAnalysis, ZeusContext};
use crate::gui::SHARED_GUI;
use crate::utils::{RT, truncate_address, truncate_hash};
use zeus_eth::{
   alloy_primitives::{Address, U256},
   currency::{Currency, NativeCurrency},
   types::ChainId,
   utils::NumericValue,
};

use std::sync::Arc;

pub mod confrim_window;
pub mod events;
pub mod spent_note_window;
pub mod tx_window;

pub use confrim_window::TxConfirmationWindow;
pub use spent_note_window::{SpentHistoryRow, SpentNoteWindow};
pub use tx_window::TxWindow;

const NULL_ADDRESS_TIP: &str =
   "Recipient is null, any tokens sent to this address will be lost forever.";

/// Show the transaction cost in a horizontal layout from left to right
pub fn tx_cost(
   chain: ChainId,
   eth_cost: &NumericValue,
   eth_cost_usd: &NumericValue,
   theme: &Theme,
   ui: &mut Ui,
) {
   let eth = NativeCurrency::from(chain.id());

   ui.horizontal(|ui| {
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         ui.label(RichText::new("Cost").size(theme.typography.large));
      });

      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         let cost = eth_cost.abbreviated();
         let text = format!(
            "{:.10} {} ~ ${}",
            cost,
            eth.symbol,
            eth_cost_usd.abbreviated()
         );
         ui.label(RichText::new(text).size(theme.typography.large));
      });
   });
}

/// Show the trasnsaction hash with a hyperlink to the block explorer
/// in a horizontal layout from left to right
pub fn tx_hash(chain: ChainId, tx_hash: &TxHash, theme: &Theme, ui: &mut Ui) {
   ui.horizontal(|ui| {
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         let text = "Transaction hash";
         ui.label(RichText::new(text).size(theme.typography.large));
      });

      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         let hash_str = truncate_hash(tx_hash.to_string());
         let explorer = chain.block_explorer();
         let link = format!("{}/tx/{}", explorer, tx_hash);
         ui.hyperlink_to(
            RichText::new(hash_str).size(theme.typography.large).color(theme.colors.info),
            link,
         );
      });
   });
}

/// Show the value of a transaction in a horizontal layout from left to right
pub fn value(
   ctx: &mut ZeusContext,
   chain: ChainId,
   value: NumericValue,
   theme: &Theme,
   ui: &mut Ui,
) {
   let eth = Currency::from(NativeCurrency::from(chain.id()));

   ui.horizontal(|ui| {
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         ui.label(RichText::new("Value").size(theme.typography.large));
      });

      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         let value_usd = ctx.get_currency_value_for_amount(value.f64(), &eth);
         let text = format!(
            "{:.12} {} ~ ${:5}",
            value.abbreviated(),
            eth.symbol(),
            value_usd.abbreviated()
         );
         ui.label(RichText::new(text).size(theme.typography.large));
      });
   });
}

/// Show a label with a hyperlink to the block explorer
/// in a horizontal layout from left to right
pub fn address(
   ctx: &mut ZeusContext,
   chain: ChainId,
   label: &str,
   address: Address,
   theme: &Theme,
   ui: &mut Ui,
) {
   let is_recipient = label.contains("Recipient");

   let q_mark = RichText::new("?").size(theme.typography.normal);
   let danger = Badge::new(q_mark, BadgeTone::Danger);
   let tip_text = RichText::new(NULL_ADDRESS_TIP).size(theme.typography.large);

   ui.horizontal(|ui| {
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         let mut text = RichText::new(label).size(theme.typography.large);
         if is_recipient && address.is_zero() {
            text = text.color(theme.colors.error);
            ui.horizontal(|ui| {
               ui.label(text);
               ui.add_space(5.0);
               ui.add(danger).on_hover_text(tip_text);
            });
         } else {
            ui.label(text);
         }
      });

      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         // Empty/whitespace names (failed Sourcify/ERC-7730 inserts) must
         // not hide the truncated address
         let address_name = match ctx.get_address_name(chain.id(), address) {
            Some(name) if !name.trim().is_empty() => name.to_string(),
            _ => {
               if !ctx.address_name_requested(chain.id(), address) {
                  request_address_name(chain.id(), address);
               }
               truncate_address(address.to_string())
            }
         };

         let explorer = chain.block_explorer();
         let link = format!("{}/address/{}", explorer, address.to_string());
         ui.hyperlink_to(
            RichText::new(address_name)
               .size(theme.typography.large)
               .color(theme.colors.info),
            link,
         );
      });
   });
}

/// Show the chain name with an icon in a horizontal layout from left to right
pub fn chain(chain: ChainId, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
   let tint = theme.image_tint_recommended;
   ui.horizontal(|ui| {
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         ui.label(RichText::new("Chain").size(theme.typography.large));
      });

      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         let icon = icons.chain_icon(chain.id(), tint);
         let text = RichText::new(chain.name()).size(theme.typography.large);
         let label = Label::new(text, Some(icon)).image_on_left().interactive(false);
         ui.add(label);
      });
   });
}

/// Show the ETH spent in a horizontal layout from left to right
pub fn eth_spent(
   chain: u64,
   eth_spent: NumericValue,
   eth_spent_usd: NumericValue,
   theme: &Theme,
   icons: Arc<Icons>,
   _text: &str,
   ui: &mut Ui,
) {
   let tint = theme.image_tint_recommended;
   let native = NativeCurrency::from(chain);
   let icon = icons.native_currency_icon(chain, tint).fit_to_exact_size(vec2(24.0, 24.0));
   let text = format!(
      "{:.10} {} ≈ {:.10}",
      eth_spent.abbreviated(),
      native.symbol,
      eth_spent_usd.abbreviated()
   );
   let text = RichText::new(text).size(theme.typography.normal);
   ui.add(Label::new(text, Some(icon)).interactive(false));
}

/// Show the ETH received in a horizontal layout from left to right
pub fn eth_received(
   chain: u64,
   eth_received: NumericValue,
   eth_received_usd: NumericValue,
   theme: &Theme,
   _icons: Arc<Icons>,
   text: &str,
   ui: &mut Ui,
) {
   let native = NativeCurrency::from(chain);
   let text = format!(
      "{text} {:.10} {} ≈ ${:.10}",
      eth_received.abbreviated(),
      native.symbol,
      eth_received_usd.abbreviated()
   );
   let text = RichText::new(text).size(theme.typography.large);
   ui.add(Label::new(text, None).interactive(false));
}

pub fn balance_change_row(
   _ctx: &mut ZeusContext,
   theme: &Theme,
   icons: Arc<Icons>,
   change: &BalanceChange,
   ui: &mut Ui,
) {
   let tint = theme.image_tint_recommended;
   let icon_size = vec2(24.0, 24.0);
   let icon = icons.currency_icon_x32(&change.currency, tint).fit_to_exact_size(icon_size);
   let sign = if change.is_increase() { "+" } else { "−" };

   let color = if change.is_increase() {
      theme.colors.success
   } else {
      theme.colors.error
   };

   let delta = change.abs_delta();
   let value = change.price.f64() * delta.f64();
   let usd_value = NumericValue::from_f64(value);

   ui.horizontal(|ui| {
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         let text = RichText::new(format!(
            "{} {:.10} {}",
            sign,
            delta.abbreviated(),
            change.currency.symbol()
         ))
         .size(theme.typography.large)
         .color(color);
         let label = Label::new(text, Some(icon)).spacing(3.0).interactive(false);
         ui.add(label);
      });
      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         ui.label(
            RichText::new(format!("~ ${:.10}", usd_value.abbreviated()))
               .size(theme.typography.large),
         );
      });
   });
}

pub fn approval_change_row(
   ctx: &mut ZeusContext,
   chain: ChainId,
   theme: &Theme,
   icons: Arc<Icons>,
   change: &ApprovalChange,
   ui: &mut Ui,
) {
   let tint = theme.image_tint_recommended;
   let icon_size = vec2(24.0, 24.0);
   let icon = icons.currency_icon_x32(&change.token, tint).fit_to_exact_size(icon_size);

   let amount = format!("{:.10}", change.after.abbreviated());
   let value = change.price.f64() * change.after.f64();
   let usd_value = NumericValue::from_f64(value);
   let color = if change.is_revoke() {
      theme.colors.success
   } else {
      theme.colors.warning
   };

   let spender_name = address_label(ctx, chain, change.spender);
   let explorer = chain.block_explorer();
   let spender_link = format!("{}/address/{}", explorer, change.spender);

   ui.horizontal(|ui| {
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         let token_text = RichText::new(change.token.symbol()).size(theme.typography.large);
         let token_label = Label::new(token_text, Some(icon)).spacing(6.0).interactive(false);

         let arrow = Lucide::ArrowRight.size(20.0).color(theme.colors.text).image();
         let arrow_label = Label::new("", Some(arrow)).spacing(0.0).interactive(false);

         ui.add(MultiLabel::new(vec![token_label, arrow_label]));

         ui.add_space(6.0);

         ui.hyperlink_to(
            RichText::new(spender_name)
               .size(theme.typography.large)
               .color(theme.colors.info),
            spender_link,
         );
      });
      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         ui.spacing_mut().item_spacing.x = theme.spacing.xs;

         if change.kind == ApprovalKind::Permit2 {
            if let Some(expiration) = change.expiration_after {
               let q_mark = RichText::new("?").size(theme.typography.normal);
               let info_tip = Badge::new(q_mark, BadgeTone::Info);
               let hover = format!("Expires {}", expiration.to_relative());
               ui.add(info_tip).on_hover_text(hover);
            }
         }

         let amount_text = RichText::new(amount).size(theme.typography.large).color(color);
         let amount_label = Label::new(amount_text, None).interactive(false);

         if change.is_unlimited() {
            ui.add(amount_label);
         } else {
            let usd_text = RichText::new(format!("~ ${:.10}", usd_value.abbreviated()))
               .size(theme.typography.large);
            let usd_label = Label::new(usd_text, None).interactive(false);
            ui.add(MultiLabel::new(vec![amount_label, usd_label]));
         }
      });
   });
}

/// The thumbnail every diff row draws, and the height of the slot it is given.
const ROW_ICON_SIZE: f32 = 24.0;

/// The arrow that points from an asset to its operator.
///
/// Its size is also the slot the row must reserve for it: a label that has already wrapped inside its
/// box leaves no room for whatever follows it, which is what kept a "fixed" row overflowing.
const ROW_ARROW_SIZE: f32 = 20.0;

/// How much of a row's left side an operator name may take.
///
/// A cap is what keeps an absurd operator from starving the asset it acts on: past it the name is
/// truncated, with the full one on hover.
const OPERATOR_WIDTH_SHARE: f32 = 0.4;

/// How a diff row divides its width.
struct RowBudget {
   /// The slot for the asset label, which wraps inside it.
   label: f32,
   /// The slot for the operator, which truncates inside it.
   link: f32,
}

/// Split `row_width` between a row's variable parts.
///
/// A `Label` in a horizontal layout **extends** rather than wraps (egui's wrap mode there is `Extend`)
/// and an egui hyperlink cannot wrap at all, so every part that varies in length needs an explicit
/// slot or one long collection or operator name widens the row past the modal it sits in. Measured on
/// the real rows: a 63-character collection name overflowed a 651px row by 197px, a 43-character
/// operator name by 94px.
///
/// Pure, so "the row cannot overflow" is a unit test rather than a hope.
fn row_budget(row_width: f32, gap: f32, amount_width: f32, operator_width: f32) -> RowBudget {
   let left = (row_width - amount_width - gap).max(0.0);
   let link = operator_width.min(left * OPERATOR_WIDTH_SHARE);
   let asset = (left - link - gap).max(0.0);
   let label = (asset - ROW_ARROW_SIZE - gap).max(0.0);

   RowBudget { label, link }
}

/// The width `text` renders at, unwrapped — what a row has to reserve for it.
fn text_width(ui: &Ui, text: &str, size: f32) -> f32 {
   ui.ctx().fonts_mut(|fonts| {
      fonts
         .layout_no_wrap(
            text.to_string(),
            FontId::proportional(size),
            egui::Color32::PLACEHOLDER,
         )
         .size()
         .x
   })
}

/// One NFT whose signer ownership moved across the tx.
///
/// Not a [`balance_change_row`]: an NFT has no USD value to put in the right column, so the row
/// spends its width on what actually moved — which id, of which collection.
pub fn nft_balance_change_row(
   ctx: &mut ZeusContext,
   chain: ChainId,
   theme: &Theme,
   icons: Arc<Icons>,
   change: &NftBalanceChange,
   ui: &mut Ui,
) {
   let tint = theme.image_tint_recommended;
   let icon = icons
      .nft_icon_x64(
         chain.id(),
         change.collection,
         change.token_id,
         tint,
      )
      .fit_to_exact_size(vec2(ROW_ICON_SIZE, ROW_ICON_SIZE));

   let sign = if change.is_received() { "+" } else { "−" };
   let color = if change.is_received() {
      theme.colors.success
   } else {
      theme.colors.error
   };

   // ERC-721 ownership moves one id at a time, so only an ERC-1155 count can be more than one.
   let delta = change.abs_delta();
   let count = if delta > U256::from(1) {
      format!(" × {delta}")
   } else {
      String::new()
   };

   let name = address_label(ctx, chain, change.collection);
   let hover = format!("{}\n{}", name, change.collection);

   ui.horizontal(|ui| {
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         let text = RichText::new(format!(
            "{} #{}{} {}",
            sign, change.token_id, count, name
         ))
         .size(theme.typography.large)
         .color(color);
         // `.wrap()` is the whole guard for this row: the label is its only content, and a wrapped
         // label fits the width it is offered instead of extending past it (a horizontal layout's wrap
         // mode is `Extend`). Nothing follows it, so no slot has to be reserved.
         let label = Label::new(text, Some(icon)).spacing(3.0).interactive(false).wrap();
         ui.add(label).on_hover_text(hover);
      });
   });
}

/// One NFT approval whose state moved across the tx.
pub fn nft_approval_change_row(
   ctx: &mut ZeusContext,
   chain: ChainId,
   theme: &Theme,
   icons: Arc<Icons>,
   change: &NftApprovalChange,
   ui: &mut Ui,
) {
   let tint = theme.image_tint_recommended;

   let token_id = match change.target {
      NftApprovalTarget::Token(id) | NftApprovalTarget::Allowance(id) => Some(id),
      NftApprovalTarget::ForAll => None,
   };

   // A collection-wide approval has no id, so it is identified by whatever art of the collection is
   // cached — asking for a fixed id usually lands on art that was never fetched.
   let icon = match token_id {
      Some(id) => icons.nft_icon_x64(chain.id(), change.collection, id, tint),
      None => icons.nft_collection_icon_x64(chain.id(), change.collection, tint),
   }
   .fit_to_exact_size(vec2(ROW_ICON_SIZE, ROW_ICON_SIZE));

   let scope = match token_id {
      Some(id) => format!("#{id}"),
      None => "All tokens".to_string(),
   };

   let name = address_label(ctx, chain, change.collection);
   let title = format!("{name} {scope}");
   let asset_hover = format!("{}\n{}", name, change.collection);

   // The channel the ERC-20 approval row uses: taking access away is the good outcome, granting it
   // is the one worth noticing.
   let color = if change.is_revoke() {
      theme.colors.success
   } else {
      theme.colors.warning
   };

   let operator_name = address_label(ctx, chain, change.operator);
   let operator_hover = format!("{operator_name}\n{}", change.operator);
   let operator_link = format!(
      "{}/address/{}",
      chain.block_explorer(),
      change.operator
   );
   let amount = nft_approval_amount(change);

   // Every part that varies in length gets an explicit slot, or one long collection or operator name
   // widens the row past this modal. See [`row_budget`].
   let gap = theme.spacing.xs;
   let budget = row_budget(
      ui.available_width(),
      gap,
      text_width(ui, &amount, theme.typography.large),
      text_width(ui, &operator_name, theme.typography.large),
   );

   ui.horizontal(|ui| {
      ui.spacing_mut().item_spacing.x = gap;

      ui.allocate_ui_with_layout(
         vec2(budget.label, ROW_ICON_SIZE),
         Layout::left_to_right(Align::Min),
         |ui| {
            ui.set_max_width(budget.label);
            let asset_text = RichText::new(title).size(theme.typography.large);
            let asset_label =
               Label::new(asset_text, Some(icon)).spacing(6.0).interactive(false).wrap();
            ui.add(asset_label).on_hover_text(asset_hover);
         },
      );

      // Its own widget, and its own reserved slot in the budget: a label that has wrapped inside its
      // box leaves no room for what follows it.
      let arrow = Lucide::ArrowRight.size(ROW_ARROW_SIZE).color(theme.colors.text).image();
      ui.add(Label::new("", Some(arrow)).spacing(0.0).interactive(false));

      // `ui.hyperlink_to` extends whatever width it is offered, so the operator is a clickable label
      // that truncates inside its slot — the same treatment a long dapp origin gets in the sidebar,
      // with the full name and address on hover.
      let link = ui.allocate_ui_with_layout(
         vec2(budget.link, ROW_ICON_SIZE),
         Layout::left_to_right(Align::Min),
         |ui| {
            ui.set_max_width(budget.link);
            let operator_text = RichText::new(operator_name)
               .size(theme.typography.large)
               .color(theme.colors.info);
            ui.add(
               Label::new(operator_text, None)
                  .sense(Sense::click())
                  .wrap_mode(TextWrapMode::Truncate),
            )
         },
      );

      if link.inner.clicked() {
         ui.ctx().open_url(OpenUrl::new_tab(operator_link));
      }

      link
         .inner
         .on_hover_text(operator_hover)
         .on_hover_cursor(CursorIcon::PointingHand);

      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         let text = RichText::new(amount).size(theme.typography.large).color(color);
         ui.add(Label::new(text, None).interactive(false));
      });
   });
}

/// What the amount column of an NFT approval row says.
///
/// The operator has a column of its own, so what is left to report is the state the shape grants:
/// "approved" is not a number, and only ERC-5216 has one to show.
fn nft_approval_amount(change: &NftApprovalChange) -> String {
   match change.after {
      NftApprovalValue::Approved(address) => match address.is_zero() {
         true => "Revoked".to_string(),
         false => "Approved".to_string(),
      },
      NftApprovalValue::ForAll(approved) => match approved {
         true => "Approved".to_string(),
         false => "Revoked".to_string(),
      },
      NftApprovalValue::Allowance(amount) => {
         if amount == U256::MAX {
            "Unlimited".to_string()
         } else if amount.is_zero() {
            "Revoked".to_string()
         } else {
            amount.to_string()
         }
      }
   }
}

pub fn show_balance_diff_rows(
   ctx: &mut ZeusContext,
   chain: ChainId,
   theme: &Theme,
   icons: Arc<Icons>,
   diff: &BalanceDiff,
   ui: &mut Ui,
) {
   ui.spacing_mut().item_spacing = vec2(0.0, theme.spacing.sm);
   let frame = theme.frame2.outer_margin(Margin::ZERO);

   for change in diff.changes() {
      frame.show(ui, |ui| {
         balance_change_row(ctx, theme, icons.clone(), change, ui);
      });
   }

   // NFT rows after the fungible ones, the way they list everywhere else in Zeus — and in the same
   // outflows-first order the fungible rows use.
   for change in diff.nft_changes() {
      frame.show(ui, |ui| {
         nft_balance_change_row(ctx, chain, theme, icons.clone(), change, ui);
      });
   }
}

pub fn show_approval_diff_rows(
   ctx: &mut ZeusContext,
   chain: ChainId,
   theme: &Theme,
   icons: Arc<Icons>,
   diff: &ApprovalDiff,
   ui: &mut Ui,
) {
   ui.spacing_mut().item_spacing = vec2(0.0, theme.spacing.sm);
   let frame = theme.frame2.outer_margin(Margin::ZERO);

   for change in diff.sorted() {
      frame.show(ui, |ui| {
         approval_change_row(ctx, chain, theme, icons.clone(), change, ui);
      });
   }

   // `nft_sorted` puts revokes first, the order the fungible rows use and for the same reason.
   for change in diff.nft_sorted() {
      frame.show(ui, |ui| {
         nft_approval_change_row(ctx, chain, theme, icons.clone(), change, ui);
      });
   }
}

/// Which of the [`show_analysis_buttons`] buttons were clicked
#[derive(Clone, Copy, Debug, Default)]
pub struct AnalysisButtons {
   pub events: bool,
   pub calldata: bool,
   pub balance_and_approvals: bool,
}

/// The Events / Calldata / Balance & Approvals buttons.
///
/// A [`CornerBadge`] is painted on top of the button and reserves no layout
/// space, so the row relies on `button_padding` for the badge to clear the
/// label. Note that `Button::min_size` is only a floor: a button grows past its
/// share of the row as soon as its label plus that padding is wider than the
/// share, which stretches the whole column. "Balance & Approvals" at
/// `theme.typography.large` takes ~202 points that way, so [`width`] must be
/// wide enough for `n * 202 + gap` — the modals hosting this row are sized for
/// it.
pub fn show_analysis_buttons(
   analysis: &TransactionAnalysis,
   theme: &Theme,
   width: f32,
   ui: &mut Ui,
) -> AnalysisButtons {
   let mut clicked = AnalysisButtons::default();

   let has_diffs = !analysis.balance_diff.is_empty() || !analysis.approval_diff.is_empty();

   let n = match has_diffs {
      true => 3.0,
      false => 2.0,
   };

   let height = 30.0;
   let gap = theme.spacing.sm * (n - 1.0);
   let size = vec2((width - gap) / n, height);
   let button_visuals = theme.button_visuals();

   ui.allocate_ui(vec2(width, height), |ui| {
      ui.set_width(width);
      ui.horizontal(|ui| {
         ui.spacing_mut().item_spacing.x = theme.spacing.sm;
         // room for the badge, which is painted over the button
         ui.spacing_mut().button_padding.x = theme.spacing.md;

         let text = RichText::new("Events").size(theme.typography.large);
         let mut button = Button::new(text).visuals(button_visuals).min_size(size);

         let event_count = analysis.decoded_events.len();
         if event_count > 0 {
            let text = RichText::new(event_count.to_string()).size(theme.typography.very_small);
            let badge = CornerBadge::new(text).corner(BadgeCorner::TopRight);
            button = button.badge(badge);
         }

         clicked.events = ui.add(button).clicked();

         let text = RichText::new("Calldata").size(theme.typography.large);
         let button = Button::new(text).visuals(button_visuals).min_size(size);

         clicked.calldata = ui.add(button).clicked();

         if has_diffs {
            let diff_count = analysis.balance_diff.len() + analysis.approval_diff.len();

            let text = RichText::new(diff_count.to_string()).size(theme.typography.very_small);
            let badge = CornerBadge::new(text).corner(BadgeCorner::TopRight);

            let text = RichText::new("State").size(theme.typography.large);
            let button = Button::new(text).badge(badge).visuals(button_visuals).min_size(size);

            clicked.balance_and_approvals = ui.add(button).clicked();
         }
      });
   });

   clicked
}

pub fn show_tx_diffs_modal(
   open: &mut bool,
   theme: &Theme,
   ctx: &mut ZeusContext,
   chain: ChainId,
   icons: Arc<Icons>,
   balance_diff: &BalanceDiff,
   approval_diff: &ApprovalDiff,
   ui: &mut Ui,
) {
   let heading = RichText::new("State Changes").size(theme.typography.heading);
   let modal_frame = theme.window_frame.fill(theme.frame1.fill);
   let modal_width = 720.0;

   Modal::new("tx_diffs", open)
      .backdrop_order(Order::Foreground)
      .content_order(Order::Tooltip)
      .heading(heading)
      .center_header(true)
      .frame(modal_frame)
      .max_width(modal_width)
      .show(ui.ctx(), |ui| {
         ui.set_width(ui.available_width());
         ui.spacing_mut().item_spacing.y = theme.spacing.md;

         ScrollArea::vertical()
            .id_salt("tx_diff_modal_scroll")
            .scroll_bar_visibility(ScrollBarVisibility::AlwaysVisible)
            .content_margin(5)
            .show(ui, |ui| {
               ui.set_min_height(350.0);
               ui.set_min_width(ui.available_width());

               let text = if balance_diff.is_empty() {
                  "No balance changes"
               } else {
                  "Balance changes"
               };

               ui.label(RichText::new(text).size(theme.typography.large));

               if !balance_diff.is_empty() {
                  show_balance_diff_rows(ctx, chain, theme, icons.clone(), balance_diff, ui);
               }

               ui.add_space(10.0);

               let text = if approval_diff.is_empty() {
                  "No approval changes"
               } else {
                  "Approval changes"
               };

               ui.label(RichText::new(text).size(theme.typography.large));

               if !approval_diff.is_empty() {
                  show_approval_diff_rows(
                     ctx,
                     chain,
                     theme,
                     icons.clone(),
                     approval_diff,
                     ui,
                  );
               }
            });
      });
}

pub fn clear_display_ui(
   ctx: &mut ZeusContext,
   chain_id: ChainId,
   display: &ClearDisplay,
   theme: &Theme,
   icons: Arc<Icons>,
   ui: &mut Ui,
) {
   let tint = theme.image_tint_recommended;

   ui.spacing_mut().item_spacing.y = theme.spacing.sm;

   if let Some(owner) = display.owner.as_ref() {
      let name = match &display.contract_name {
         Some(c) => format!("{owner} · {c}"),
         None => owner.clone(),
      };
      ui.label(RichText::new(name).size(theme.typography.normal));
   }

   if let Some(intent) = display.interpolated_intent.as_ref() {
      ui.label(RichText::new(intent).size(theme.typography.large));
   }

   for warning in &display.warnings {
      ui.label(RichText::new(warning).size(theme.typography.normal).color(theme.colors.warning));
   }

   for field in &display.fields {
      match &field.value {
         FormattedValue::Address(addr) => {
            address(ctx, chain_id, &field.label, *addr, theme, ui);
         }
         FormattedValue::TokenAmount {
            amount,
            token,
            unlimited,
         } => {
            ui.horizontal(|ui| {
               ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                  ui.label(RichText::new(&field.label).size(theme.typography.large));
               });
               ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                  let amount_txt = if *unlimited {
                     "Unlimited"
                  } else {
                     amount.abbreviated()
                  };
                  let text = format!("{:.10} {}", amount_txt, token.symbol);
                  let icon = icons
                     .token_icon_x32(token.address, token.chain_id, tint)
                     .fit_to_exact_size(vec2(24.0, 24.0));
                  let text = RichText::new(text).size(theme.typography.large);
                  let label = Label::new(text, Some(icon))
                     .wrap()
                     .visuals(theme.label_visuals())
                     .interactive(false);
                  ui.add(label);
               });
            });
         }
         FormattedValue::Date(ts) => {
            ui.horizontal(|ui| {
               ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                  ui.label(RichText::new(&field.label).size(theme.typography.large));
               });
               ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                  ui.label(RichText::new(ts.to_relative()).size(theme.typography.large));
               });
            });
         }
         FormattedValue::Text(text) | FormattedValue::Bytes(text) => {
            ui.horizontal(|ui| {
               ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                  ui.label(RichText::new(&field.label).size(theme.typography.large));
               });
               ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                  ui.label(RichText::new(text).size(theme.typography.large));
               });
            });
         }
      }
   }
}

pub fn show_calldata_modal(
   open: &mut bool,
   theme: &Theme,
   ctx: &mut ZeusContext,
   chain: ChainId,
   icons: Arc<Icons>,
   display: Option<&ClearDisplay>,
   mut calldata: String,
   ui: &mut Ui,
) {
   let heading = if let Some(d) = display {
      RichText::new(&d.heading).size(theme.typography.heading)
   } else {
      RichText::new("Calldata").size(theme.typography.heading)
   };

   let modal_width = 520.0;
   let edit_height = 260.0;

   Modal::new("Calldata", open)
      .backdrop_order(Order::Foreground)
      .content_order(Order::Tooltip)
      .heading(heading)
      .max_width(modal_width)
      .show(ui.ctx(), |ui| {
         ui.set_width(ui.available_width());

         ui.vertical_centered(|ui| {
            if let Some(display) = display {
               ScrollArea::vertical()
                  .id_salt("clear_display_calldata_window")
                  .max_height(300.0)
                  .show(ui, |ui| {
                     clear_display_ui(ctx, chain, display, theme, icons.clone(), ui);
                  });

               ui.add_space(12.0);
               ui.label(RichText::new("Raw").size(theme.typography.large));
            }

            let edit_width = ui.available_width() * 0.9;

            let text_edit = TextEdit::multiline(&mut calldata)
               .font(FontId::monospace(theme.typography.normal))
               .desired_width(edit_width)
               .margin(Margin::same(10));

            ScrollArea::vertical()
               .id_salt("raw_calldata")
               .max_height(edit_height)
               .show(ui, |ui| {
                  ui.set_min_width(edit_width);
                  ui.add(text_edit);
               });
         });
      });
}

/// A display name for an address, requesting it if Zeus has not cached one yet.
///
/// The fallback is the truncated address: a diff row can be the first place the user meets a contract,
/// and an address beats a blank cell. The request is what fills the name in on a later frame — the
/// spender of an approval and the collection behind an NFT are the same question.
fn address_label(ctx: &mut ZeusContext, chain: ChainId, address: Address) -> String {
   match ctx.get_address_name(chain.id(), address) {
      Some(name) => name.to_string(),
      None => {
         if !ctx.address_name_requested(chain.id(), address) {
            request_address_name(chain.id(), address);
         }
         truncate_address(address.to_string())
      }
   }
}

fn request_address_name(chain: u64, address: Address) {
   RT.spawn(async move {
      let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
      if ctx.lookup_address_name(chain, address).await {
         SHARED_GUI.write(|gui| {
            gui.request_repaint();
         });
      }
   });
}

#[cfg(test)]
mod tests {
   use super::*;

   /// A row's slots can never add up to more than the row, whatever the strings measure, and the
   /// operator is never widened past its own text.
   ///
   /// This is the contract the shipped rows broke: a `Label` in a horizontal layout **extends** rather
   /// than wraps (egui's wrap mode there is `Extend`) and a hyperlink cannot wrap at all, so a
   /// 63-character collection name widened a 651px row by 197px and pushed the amount column out of the
   /// modal. The order below is the order the row lays its children out, gaps included.
   #[test]
   fn a_row_budget_cannot_overflow() {
      let gap = 4.0;

      for row in [320.0, 651.0, 720.0] {
         for amount in [69.0, 78.0, 120.0] {
            for operator in [0.0, 167.0, 554.0, 2000.0] {
               let budget = row_budget(row, gap, amount, operator);

               // label · arrow · operator · amount, with a gap at each boundary.
               let used = budget.label + gap + ROW_ARROW_SIZE + gap + budget.link + gap + amount;

               assert!(
                  used <= row + 0.01,
                  "a {row}px row with a {amount}px amount and a {operator}px operator needs {used}px"
               );
               assert!(
                  budget.link <= operator,
                  "the operator is never widened"
               );
               assert!(budget.label >= 0.0 && budget.link >= 0.0);
            }
         }
      }
   }

   /// The cap only bites when it has to: a short operator keeps its whole width, and the asset keeps
   /// the larger share of the row either way.
   #[test]
   fn a_row_budget_only_caps_what_is_long() {
      let budget = row_budget(651.0, 4.0, 69.0, 167.0);

      assert_eq!(budget.link, 167.0);
      assert!(budget.label > budget.link);

      // An operator that cannot fit its share is cut to it, never given more.
      let long = row_budget(651.0, 4.0, 69.0, 900.0);
      assert_eq!(
         long.link,
         (651.0 - 69.0 - 4.0) * OPERATOR_WIDTH_SHARE
      );
   }
}
