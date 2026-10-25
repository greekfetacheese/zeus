//! The transaction windows' modal renderers, analysis buttons and clear-signing
//! display.

use egui::{
   Align, FontId, Id, Layout, Margin, Order, Rect, RichText, ScrollArea, TextEdit, Ui, emath, pos2,
   scroll_area::ScrollBarVisibility, vec2,
};
use egui_elements::{
   Button, Label, Modal, SecureTextEdit, Theme,
   widgets::{Badge as CornerBadge, BadgeCorner},
};
use elegance::{Indicator, IndicatorState, Switch};

use super::address;
use super::rows::{show_approval_diff_rows, show_balance_diff_rows};
use crate::assets::icons::Icons;
use crate::core::clear_signing::{ClearDisplay, FormattedValue};
use crate::core::tx::{FeeTiers, PriorityTier};
use crate::core::{ApprovalDiff, BalanceDiff, TransactionAnalysis, ZeusContext};
use crate::gui::SHARED_GUI;
use crate::utils::truncate_hash;
use zeus_eth::alloy_primitives::{Address, TxHash};
use zeus_eth::types::ChainId;
use zeus_eth::utils::NumericValue;

use std::sync::Arc;

/// What the options modal may change about the fee.
pub struct FeeSelection<'a> {
   /// The recommendation, or `None` for a chain that has none (every chain but mainnet).
   pub tiers: Option<FeeTiers>,
   pub tier: &'a mut PriorityTier,
   /// Whether the free-text value overrides the tier.
   pub custom: &'a mut bool,
   pub custom_value: &'a mut String,
   /// Each rung's USD cost, in `PriorityTier::ALL` order.
   pub costs_usd: &'a [NumericValue],
}

/// What the modal changed, so the caller can recompute the shown cost and persist the tier.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub struct FeeChange {
   pub fee: bool,
   pub tier: bool,
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

/// Which of the [`show_analysis_buttons`] buttons were clicked
#[derive(Clone, Copy, Debug, Default)]
pub struct AnalysisButtons {
   pub events: bool,
   pub calldata: bool,
   pub balance_and_approvals: bool,
   pub tx_options: bool,
}

/// The Events / Calldata / State [/ Tx Options] buttons.
///
/// A [`CornerBadge`] is painted on top of the button and reserves no layout
/// space, so the row relies on `button_padding` for the badge to clear the
/// label. Note that `Button::min_size` is only a floor: a button grows past its
/// share of the row as soon as its label plus that padding is wider than the
/// share, which stretches the whole column. "Tx Options" at
/// `theme.typography.large` takes ~110 points that way, so [`width`] must be
/// wide enough for `n * 110 + gap` — the modals hosting this row are sized for
/// it.
///
/// `include_tx_options` is the confirmation window's own button: it opens the
/// modal holding the fee and gas fields. A transaction that is already sent
/// (`TxWindow`) has nothing left to edit, so it passes `false`.
pub fn show_analysis_buttons(
   analysis: &TransactionAnalysis,
   theme: &Theme,
   width: f32,
   include_tx_options: bool,
   ui: &mut Ui,
) -> AnalysisButtons {
   let mut clicked = AnalysisButtons::default();

   let has_diffs = !analysis.balance_diff.is_empty() || !analysis.approval_diff.is_empty();

   let n = match (has_diffs, include_tx_options) {
      (true, true) => 4.0,
      (true, false) | (false, true) => 3.0,
      (false, false) => 2.0,
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

         if include_tx_options {
            let text = RichText::new("Tx Options").size(theme.typography.large);
            let button = Button::new(text).visuals(button_visuals).min_size(size);

            clicked.tx_options = ui.add(button).clicked();
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

/// The Priority Fee and Gas Limit fields, plus the MEV status, in a modal of their
/// own.
///
/// They used to hold a frame of their own in the confirmation window, where two
/// rarely-touched fields competed for height with what the transaction actually does.
/// Returns what was edited, so the caller can recompute the shown cost and persist a
/// tier change.
pub fn show_tx_options_modal(
   open: &mut bool,
   theme: &Theme,
   chain: ChainId,
   fee: &mut FeeSelection<'_>,
   gas_limit: &mut String,
   gas_used: u64,
   mev_protect: bool,
   show_mev_protect: bool,
   ui: &mut Ui,
) -> FeeChange {
   let mut fee_change = FeeChange::default();

   let heading = RichText::new("Transaction Options").size(theme.typography.heading);
   let modal_frame = theme.window_frame.fill(theme.frame1.fill);
   let modal_width = 460.0;
   let text_edit_visuals = theme.text_edit_visuals();

   Modal::new("tx_options", open)
      .backdrop_order(Order::Foreground)
      .content_order(Order::Tooltip)
      .heading(heading)
      .center_header(true)
      .frame(modal_frame)
      .max_width(modal_width)
      .show(ui.ctx(), |ui| {
         ui.set_width(ui.available_width());
         ui.spacing_mut().item_spacing.y = theme.spacing.md;

         // The preset rows carry a label and a value on one line, so they need the whole modal
         // width; a chain without a recommendation keeps the original two-column layout.
         let field_width = (ui.available_width() - theme.spacing.md) * 0.5;

         if fee.tiers.is_some() {
            ui.vertical(|ui| {
               ui.label(RichText::new("Priority Fee (Gwei)").size(theme.typography.large));

               // BSC has no priority fee to set
               if chain.is_bsc() {
                  ui.disable();
               }

               if let Some(tiers) = fee.tiers {
                  for (index, tier) in PriorityTier::ALL.into_iter().enumerate() {
                     let selected = *fee.tier == tier && !*fee.custom;
                     let gwei = NumericValue::format_to_gwei(tiers.tip(tier));
                     let usd = fee.costs_usd.get(index).cloned().unwrap_or_default();
                     let right = if usd.is_zero() {
                        format!("{} gwei", gwei.formatted)
                     } else {
                        format!(
                           "{} gwei  ·  ${}",
                           gwei.formatted,
                           usd.abbreviated()
                        )
                     };
                     let row = Button::selectable(
                        selected,
                        RichText::new(tier.label()).size(theme.typography.large),
                     )
                     .right_text(right)
                     .min_size(vec2(ui.available_width(), 32.0));

                     if ui.add(row).clicked() {
                        *fee.tier = tier;
                        *fee.custom = false;
                        fee_change = FeeChange {
                           fee: true,
                           tier: true,
                        };
                     }
                  }

                  if ui.add(Switch::new(fee.custom, "Custom fee")).changed() {
                     fee_change = FeeChange {
                        fee: true,
                        tier: false,
                     };
                  }

                  // The field slides open instead of jumping: the consumed space animates, a body
                  // held at its natural height is clipped to that space, and the text edit is
                  // never squeezed.
                  let custom_id = Id::unique("tx_options_custom_fee");
                  let unclipped = ui.clip_rect();
                  let top = ui.cursor().top();
                  // The field's natural height, measured in a scratch ui that never touches the
                  // modal's cursor: the text edit's own layout must not be squeezed while it
                  // slides.
                  let natural = {
                     let mut scratch = ui.new_child(
                        egui::UiBuilder::new().id_salt(custom_id.with("measure")).max_rect(
                           Rect::from_min_size(ui.cursor().min, ui.max_rect().size()),
                        ),
                     );
                     scratch.set_invisible();
                     scratch
                        .add(
                           SecureTextEdit::singleline(fee.custom_value)
                              .id_source(custom_id.with("measure_field"))
                              .visuals(text_edit_visuals)
                              .margin(Margin::same(10))
                              .desired_width(field_width)
                              .font(FontId::proportional(theme.typography.large)),
                        )
                        .rect
                        .height()
                  };

                  // Ticked every frame, closed included: an animator that never sees `false`
                  // snaps the next time it opens.
                  let reveal = ui.ctx().animate_bool_with_time_and_easing(
                     custom_id.with("reveal"),
                     *fee.custom,
                     ui.style().animation_time,
                     emath::easing::cubic_out,
                  );
                  let height = ui.ctx().animate_value_with_time(
                     custom_id.with("height"),
                     if *fee.custom { natural } else { 0.0 },
                     ui.style().animation_time,
                  );

                  // Hidden, the field still takes an interaction pass on the frame that turns the
                  // switch off; the fade is already zero there, so it stays unseen.
                  if reveal > 0.0 || *fee.custom {
                     // Hold the body at its natural height and clip the painted output to the
                     // animated space: egui never clips a child on its own. The child's own rect
                     // is discarded and only the animated height is consumed, so the Gas Limit
                     // block below rides the same slide instead of jumping at its end.
                     let area = Rect::from_min_size(
                        pos2(ui.cursor().min.x, top),
                        vec2(ui.available_width(), height),
                     );

                     let mut reveal_ui = ui.new_child(egui::UiBuilder::new().max_rect(area));
                     reveal_ui.set_clip_rect(Rect::from_min_max(
                        unclipped.min,
                        pos2(unclipped.max.x, top + height),
                     ));
                     reveal_ui.multiply_opacity(reveal);
                     reveal_ui.set_min_height(natural);

                     let res = reveal_ui.add(
                        SecureTextEdit::singleline(fee.custom_value)
                           .id_source(custom_id.with("field"))
                           .visuals(text_edit_visuals)
                           .margin(Margin::same(10))
                           .desired_width(field_width)
                           .font(FontId::proportional(theme.typography.large)),
                     );

                     if res.changed() {
                        fee_change.fee = true;
                     }

                     ui.advance_cursor_after_rect(area);
                  }
               }
            });

            ui.vertical(|ui| {
               ui.label(RichText::new("Gas Limit").size(theme.typography.large));

               ui.add(
                  SecureTextEdit::singleline(gas_limit)
                     .visuals(text_edit_visuals)
                     .margin(Margin::same(10))
                     .desired_width(field_width)
                     .font(FontId::proportional(theme.typography.large)),
               );
            });
         } else {
            // Unchanged: today's two-column layout for chains with no recommendation.
            ui.horizontal(|ui| {
               ui.spacing_mut().item_spacing.x = theme.spacing.md;

               ui.vertical(|ui| {
                  ui.label(RichText::new("Priority Fee (Gwei)").size(theme.typography.large));

                  if chain.is_bsc() {
                     ui.disable();
                  }

                  let res = ui.add(
                     SecureTextEdit::singleline(fee.custom_value)
                        .visuals(text_edit_visuals)
                        .margin(Margin::same(10))
                        .desired_width(field_width)
                        .font(FontId::proportional(theme.typography.large)),
                  );

                  if res.changed() {
                     fee_change.fee = true;
                  }
               });

               ui.vertical(|ui| {
                  ui.label(RichText::new("Gas Limit").size(theme.typography.large));

                  ui.add(
                     SecureTextEdit::singleline(gas_limit)
                        .visuals(text_edit_visuals)
                        .margin(Margin::same(10))
                        .desired_width(field_width)
                        .font(FontId::proportional(theme.typography.large)),
                  );
               });
            });
         }

         let hint = format!(
            "Simulated gas: {gas_used} — raise the limit if the transaction runs out of gas"
         );
         ui.label(RichText::new(hint).size(theme.typography.small).color(theme.colors.text_muted));

         if show_mev_protect {
            let (icon, text) = if mev_protect {
               (
                  Indicator::new(IndicatorState::On).size(12.0),
                  "MEV Protect is enabled",
               )
            } else {
               (
                  Indicator::new(IndicatorState::Off).size(12.0),
                  "MEV Protect is disabled",
               )
            };

            let text = RichText::new(text).size(theme.typography.large);

            // One row: as direct children of the modal body's vertical layout each
            // of these would land on a line of its own, the indicator included.
            ui.allocate_ui(vec2(ui.available_width(), 25.0), |ui| {
               ui.horizontal_centered(|ui| {
                  ui.spacing_mut().item_spacing.x = theme.spacing.sm;
                  ui.add(Label::new(text, None).interactive(false));
                  ui.add(icon);
               });
            });
         }
      });

   fee_change
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

pub(super) fn request_address_name(chain: u64, address: Address) {
   crate::runtime::rt().spawn(async move {
      let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
      if ctx.lookup_address_name(chain, address).await {
         SHARED_GUI.write(|gui| {
            gui.request_repaint();
         });
      }
   });
}
