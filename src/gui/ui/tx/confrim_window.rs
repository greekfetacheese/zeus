use egui::{Align, Id, Layout, Margin, Order, RichText, ScrollArea, Ui, vec2};
use egui_elements::{Button, Label, Modal, SecureTextEdit, Theme};

use super::{
   address, chain, clear_display_ui, events::*, show_analysis_buttons, show_approval_diff_rows,
   show_balance_diff_rows, show_calldata_modal, show_tx_diffs_modal, tx_cost, value,
};
use crate::assets::icons::Icons;
use crate::core::clear_signing::{self, ClearDisplay};
use crate::core::{DecodedEvent, TransactionAnalysis, ZeusContext, ZeusCtx};
use crate::gui::SHARED_GUI;
use crate::gui::ui::common::delayed_action_label;
use crate::utils::{RT, estimate_tx_cost};
use elegance::{Indicator, IndicatorState};
use zeus_eth::{
   alloy_primitives::{Address, U256},
   currency::NativeCurrency,
   types::ChainId,
   utils::NumericValue,
};

use std::sync::Arc;
use std::time::{Duration, Instant};

pub struct TxConfirmationWindow {
   open: bool,
   /// True if the tx is coming from Zeus
   source_is_zeus: bool,
   decoded_events: DecodedEvents,
   /// True to confirm, false to reject
   confirmed_or_rejected: Option<bool>,
   /// Bumped on every `open`. Late clear-signing tasks must not apply to a newer prompt.
   open_generation: u64,
   dapp: String,
   chain: ChainId,
   native_currency: NativeCurrency,
   /// Tx to be confirmed and sent to the network
   tx: Option<TransactionAnalysis>,
   tx_main_event: Option<DecodedEvent>,
   /// Adjust priority fee
   priority_fee: String,
   mev_protect: bool,
   /// True if the tx is sponsored by another account
   sponsored: bool,
   gas_used: u64,
   /// Adjust gas limit
   gas_limit: u64,
   adjusted_gas_limit: String,
   tx_cost: NumericValue,
   tx_cost_usd: NumericValue,
   show_calldata: bool,
   show_diffs: bool,
   clear_display: Option<ClearDisplay>,
   /// When the prompt became visible. `None` skips the Sign/Confirm delay (in-app Zeus txs).
   opened_at: Option<Instant>,
   size: (f32, f32),
}

impl TxConfirmationWindow {
   pub fn new() -> Self {
      Self {
         open: false,
         source_is_zeus: false,
         decoded_events: DecodedEvents::new(),
         confirmed_or_rejected: None,
         open_generation: 0,
         dapp: String::new(),
         chain: ChainId::default(),
         native_currency: NativeCurrency::default(),
         tx: None,
         tx_main_event: None,
         priority_fee: String::new(),
         mev_protect: false,
         sponsored: false,
         gas_used: 0,
         gas_limit: 0,
         adjusted_gas_limit: String::new(),
         tx_cost: NumericValue::default(),
         tx_cost_usd: NumericValue::default(),
         show_calldata: false,
         show_diffs: false,
         clear_display: None,
         opened_at: None,
         size: (550.0, 400.0),
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn reset(&mut self) {
      self.close();
      *self = Self::new();
   }

   pub fn close(&mut self) {
      self.open = false;
   }

   /// Open this [TxConfirmationWindow]
   pub fn open(
      &mut self,
      ctx: ZeusCtx,
      source_is_zeus: bool,
      dapp: String,
      chain: ChainId,
      tx: TransactionAnalysis,
      priority_fee: String,
      mev_protect: bool,
      sponsored: bool,
   ) {
      self.open_inner(
         ctx,
         source_is_zeus,
         dapp,
         chain,
         tx,
         priority_fee,
         mev_protect,
         sponsored,
         None,
      );
   }

   /// Open with a pre-built ERC-7730 display (dev dummy; skips registry lookup).
   pub fn open_with_clear_display(
      &mut self,
      ctx: ZeusCtx,
      dapp: String,
      chain: ChainId,
      tx: TransactionAnalysis,
      priority_fee: String,
      mev_protect: bool,
      sponsored: bool,
      display: ClearDisplay,
   ) {
      self.open_inner(
         ctx,
         false,
         dapp,
         chain,
         tx,
         priority_fee,
         mev_protect,
         sponsored,
         Some(display),
      );
   }

   fn open_inner(
      &mut self,
      ctx: ZeusCtx,
      source_is_zeus: bool,
      dapp: String,
      chain: ChainId,
      tx: TransactionAnalysis,
      priority_fee: String,
      mev_protect: bool,
      sponsored: bool,
      prebuilt_display: Option<ClearDisplay>,
   ) {
      self.sponsored = sponsored;
      self.source_is_zeus = source_is_zeus;
      // `send_transaction` polls this immediately. Confirm/Reject only `close()`
      // the window, so the previous answer stays in this field and would be
      // treated as the new prompt (auto-confirm + spinner while this task
      // is still fetching a clear-signing descriptor).
      self.confirmed_or_rejected = None;
      self.open_generation = self.open_generation.wrapping_add(1);
      let generation = self.open_generation;

      if sponsored {
         self.tx_cost = NumericValue::default();
         self.tx_cost_usd = NumericValue::default();
      }

      RT.spawn(async move {
         let native = NativeCurrency::from(chain.id());
         let main_event = tx.infer_main_event(ctx.clone(), chain.id());
         let gas_used = tx.gas_used;
         let gas_limit = gas_used * 15 / 10;

         let clear_display = if prebuilt_display.is_some() {
            prebuilt_display
         } else if tx.contract_interact && tx.call_data.len() >= 4 {
            clear_signing::try_clear_sign_calldata(
               ctx.clone(),
               chain.id(),
               tx.sender,
               tx.interact_to,
               tx.value,
               &tx.call_data,
            )
            .await
         } else {
            None
         };

         SHARED_GUI.write(|gui| {
            if gui.tx_confirmation_window.open_generation != generation {
               return;
            }

            gui.tx_confirmation_window.dapp = dapp;
            gui.tx_confirmation_window.priority_fee = priority_fee;
            gui.tx_confirmation_window.mev_protect = mev_protect;
            gui.tx_confirmation_window.gas_used = gas_used;
            gui.tx_confirmation_window.gas_limit = gas_limit;
            gui.tx_confirmation_window.adjusted_gas_limit = gas_limit.to_string();
            gui.tx_confirmation_window.chain = chain;
            gui.tx_confirmation_window.native_currency = native;
            gui.tx_confirmation_window.tx = Some(tx);
            gui.tx_confirmation_window.tx_main_event = Some(main_event);
            gui.tx_confirmation_window.clear_display = clear_display;
            gui.tx_confirmation_window.open = true;
            gui.tx_confirmation_window.confirmed_or_rejected = None;
            gui.tx_confirmation_window.opened_at = if source_is_zeus {
               None
            } else {
               Some(Instant::now())
            };

            ctx.write(|ctx| {
               gui.tx_confirmation_window.calculate_tx_cost(ctx, gas_used);
            });
            gui.request_repaint();
         });
      });
   }

   pub fn get_confirmed_or_rejected(&self) -> Option<bool> {
      self.confirmed_or_rejected
   }

   /// Close without confirming and record a rejection, so the awaiting
   /// `confirm_tx` errors out instead of hanging. Used when auto-lock fires.
   pub fn reject(&mut self) {
      self.confirmed_or_rejected = Some(false);
      self.close();
   }

   pub fn get_priority_fee(&self) -> NumericValue {
      NumericValue::parse_to_gwei(&self.priority_fee)
   }

   pub fn get_gas_limit(&self) -> u64 {
      self.gas_limit
   }

   pub fn get_clear_display(&self) -> Option<ClearDisplay> {
      self.clear_display.clone()
   }

   /// Calculate the cost of the transaction
   fn calculate_tx_cost(&mut self, ctx: &mut ZeusContext, gas_used: u64) {
      if self.sponsored {
         return;
      }

      let chain = self.chain;
      let fee = NumericValue::parse_to_gwei(&self.priority_fee);
      let fee = if fee.is_zero() && chain.supports_type_2_tx() {
         NumericValue::parse_to_gwei("1")
      } else {
         fee
      };

      let (cost_in_wei, cost_in_usd) = estimate_tx_cost(ctx, chain.id(), gas_used, fee.wei());
      self.tx_cost = cost_in_wei;
      self.tx_cost_usd = cost_in_usd;
   }

   pub fn show(&mut self, ctx: &mut ZeusContext, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      if !self.open {
         return;
      }

      let mut open = self.open;
      let id = Id::new("tx_confirmation_window");
      let frame = theme.window_frame.fill(theme.frame1.fill);

      Modal::new(id, &mut open)
         .backdrop_order(Order::Middle)
         .content_order(Order::Foreground)
         .closable(false)
         .frame(frame)
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);

            let button_visuals = theme.button_visuals();
            let text_edit_visuals = theme.text_edit_visuals();

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing = vec2(0.0, theme.spacing.md);
               ui.spacing_mut().button_padding = theme.button_padding;

               if self.tx.is_none() {
                  ui.label(
                     RichText::new("Transaction Analysis not found, this is a bug")
                        .size(theme.typography.large),
                  );
                  return;
               }

               let analysis = self.tx.as_ref().unwrap();
               let main_event = self.tx_main_event.as_ref().unwrap();

               if !self.dapp.is_empty() {
                  ui.label(RichText::new(&self.dapp).size(theme.typography.large));
               }

               let frame = theme.frame2;
               let avail_width_margin = 0.98;
               let frame_size = vec2(ui.available_width() * avail_width_margin, 45.0);

               // Decoded events window
               self.decoded_events.show(
                  ctx,
                  self.chain,
                  theme,
                  icons.clone(),
                  analysis,
                  frame_size,
                  frame,
                  self.size,
                  ui,
               );

               let calldata = analysis.call_data.to_string();
               let clear_display = self.clear_display.as_ref();

               // Calldata window
               show_calldata_modal(
                  &mut self.show_calldata,
                  theme,
                  ctx,
                  self.chain,
                  icons.clone(),
                  clear_display,
                  calldata,
                  ui,
               );

               show_tx_diffs_modal(
                  &mut self.show_diffs,
                  theme,
                  ctx,
                  self.chain,
                  icons.clone(),
                  &analysis.balance_diff,
                  &analysis.approval_diff,
                  ui,
               );

               // Action Name
               let action_name = if main_event.is_other() {
                  self
                     .clear_display
                     .as_ref()
                     .map(|d| d.heading.clone())
                     .unwrap_or_else(|| main_event.name())
               } else {
                  main_event.name()
               };

               let mut title_text = RichText::new(action_name).size(theme.typography.heading);

               if !self.source_is_zeus {
                  if main_event.is_permit() {
                     if main_event.permit_params().is_unlimited() {
                        title_text = title_text.color(theme.colors.error);
                     }
                  }

                  if main_event.is_token_approval() {
                     if main_event.token_approval_params().is_unlimited() {
                        title_text = title_text.color(theme.colors.error);
                     }
                  }
               }

               ui.label(title_text);

               let completly_unknown = main_event.is_other() && self.clear_display.is_none();
               let has_more_than_one_diff =
                  analysis.balance_diff.len() > 1 || analysis.approval_diff.len() > 1;
               let show_extra_warning =
                  main_event.is_other_or_eoa_delegate() && has_more_than_one_diff;

               let msg = "Review the balance and approval changes before proceeding";

               if show_extra_warning || completly_unknown {
                  ui.label(
                     RichText::new(msg).size(theme.typography.large).color(theme.colors.warning),
                  );
               }

               // Main event details
               if !main_event.is_other() {
                  ui.allocate_ui(frame_size, |ui| {
                     frame.show(ui, |ui| {
                        show_event(
                           ctx,
                           self.chain,
                           theme,
                           icons.clone(),
                           main_event,
                           ui,
                        );
                     });
                  });
               }

               // Clear display UI
               if main_event.is_other() {
                  let frame_size = vec2(ui.available_width() * avail_width_margin, 300.0);

                  if let Some(display) = self.clear_display.clone() {
                     ui.allocate_ui(frame_size, |ui| {
                        frame.show(ui, |ui| {
                           ScrollArea::vertical()
                              .id_salt("clear_diplay_ui")
                              .max_height(300.0)
                              .show(ui, |ui| {
                                 clear_display_ui(
                                    ctx,
                                    self.chain,
                                    &display,
                                    theme,
                                    icons.clone(),
                                    ui,
                                 );
                              });
                        });
                     });
                  }
               }

               // For unkown txs (Unless its an EOA Delegate) we show the diffs right away only if the len is 1
               // so we dont fuck up the UI with too many widgets

               // ? Ideally need to be showwn right away in a scroll area
               // ? but right now this is ugly
               // ? Also egui doesn't respect the ScrollBarVisibility::AlwaysVisible
               // ? which makes things even worse

               let should_show_balance_diff =
                  main_event.is_other_or_eoa_delegate() && analysis.balance_diff.len() == 1;

               let should_show_approval_diff =
                  main_event.is_other_or_eoa_delegate() && analysis.approval_diff.len() == 1;

               if should_show_balance_diff {
                  ui.allocate_ui(frame_size, |ui| {
                     ui.label(RichText::new("Balance Changes").size(theme.typography.large));
                     show_balance_diff_rows(
                        ctx,
                        self.chain,
                        theme,
                        icons.clone(),
                        &analysis.balance_diff,
                        ui,
                     );
                  });
               }

               if should_show_approval_diff {
                  ui.allocate_ui(frame_size, |ui| {
                     ui.label(RichText::new("Approval Changes").size(theme.typography.large));
                     show_approval_diff_rows(
                        ctx,
                        self.chain,
                        theme,
                        icons.clone(),
                        &analysis.approval_diff,
                        ui,
                     );
                  });
               }

               // Tx details
               ui.allocate_ui(frame_size, |ui| {
                  frame.show(ui, |ui| {
                     chain(self.chain, theme, icons.clone(), ui);
                     address(
                        ctx,
                        self.chain,
                        "Sender",
                        analysis.sender,
                        theme,
                        ui,
                     );

                     // Contract interaction
                     if analysis.contract_interact {
                        let label = "Contract interaction";
                        address(
                           ctx,
                           self.chain,
                           label,
                           analysis.interact_to,
                           theme,
                           ui,
                        );
                     }

                     // Value to be sent
                     value(ctx, self.chain, analysis.value_sent(), theme, ui);

                     // Transaction cost
                     tx_cost(
                        self.chain,
                        &self.tx_cost,
                        &self.tx_cost_usd,
                        theme,
                        ui,
                     );
                  });
               });

               /*
               // Show ETH received
               if !analysis.eth_received().is_zero()
                  && !analysis.is_unwrap_weth()
                  && !analysis.is_swap()
               {
                  let text = "You will receive";
                  ui.allocate_ui(frame_size, |ui| {
                     frame.show(ui, |ui| {
                        eth_received(
                           self.chain.id(),
                           analysis.eth_received(),
                           analysis.eth_received_usd(ctx),
                           theme,
                           icons.clone(),
                           text,
                           ui,
                        );
                     });
                  });
               }
               */

               // Decoded Events / Calldata / Balance & Approvals
               let buttons_size = ui.available_width() * avail_width_margin;
               let buttons = show_analysis_buttons(analysis, theme, buttons_size, ui);

               if buttons.events {
                  self.decoded_events.open();
               }

               if buttons.calldata {
                  self.show_calldata = true;
               }

               if buttons.balance_and_approvals {
                  self.show_diffs = true;
               }

               let sufficient_balance =
                  self.sufficient_balance(ctx, analysis.value_sent().wei(), analysis.sender);

               let mut recalculate_tx_cost = false;

               let size = vec2(ui.available_width() * 0.7, 45.0);

               // Priority Fee / Gas Limit
               ui.allocate_ui(size, |ui| {
                  frame.show(ui, |ui| {
                     ui.set_width(size.x);
                     ui.spacing_mut().item_spacing = vec2(theme.spacing.md, theme.spacing.sm);

                     ui.horizontal(|ui| {
                        let availabled_width = ui.available_width();
                        let fee_width = ui.available_width() * 0.3;
                        let gas_width = ui.available_width() * 0.5;

                        // Ajdust Priority Fee
                        ui.vertical(|ui| {
                           let text = "Priority Fee (Gwei)";
                           ui.label(RichText::new(text).size(theme.typography.normal));

                           if self.chain.is_bsc() {
                              ui.disable();
                           }

                           let res = ui.add(
                              SecureTextEdit::singleline(&mut self.priority_fee)
                                 .visuals(text_edit_visuals)
                                 .margin(Margin::same(10))
                                 .desired_width(fee_width)
                                 .font(egui::FontId::proportional(
                                    theme.typography.normal,
                                 )),
                           );

                           if res.changed() {
                              recalculate_tx_cost = true;
                           }
                        });

                        // Take the available space because otherwise the gas limit
                        // will not be pushed to the far right
                        ui.add_space(availabled_width - (fee_width + gas_width));

                        // Adjust Gas Limit
                        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                           ui.vertical(|ui| {
                              let text = "Gas Limit";
                              ui.label(RichText::new(text).size(theme.typography.normal));

                              ui.add(
                                 SecureTextEdit::singleline(&mut self.adjusted_gas_limit)
                                    .visuals(text_edit_visuals)
                                    .margin(Margin::same(10))
                                    .desired_width(gas_width)
                                    .font(egui::FontId::proportional(
                                       theme.typography.normal,
                                    )),
                              );
                           });
                        });
                     });
                  });
               });

               let base_case = !main_event.is_other() && main_event.is_mev_vulnerable();
               let show_mev_protect = base_case || main_event.is_other_or_eoa_delegate();

               if recalculate_tx_cost {
                  self.calculate_tx_cost(ctx, self.gas_used);
               }

               if show_mev_protect {
                  let icon = if self.mev_protect {
                     Indicator::new(IndicatorState::On).size(12.0)
                  } else {
                     Indicator::new(IndicatorState::Off).size(12.0)
                  };

                  let text = if self.mev_protect {
                     "MEV Protect is enabled"
                  } else {
                     "MEV Protect is disabled"
                  };

                  let text = RichText::new(text).size(theme.typography.large);
                  let size = vec2(ui.available_width() * 0.3, 15.0);

                  ui.allocate_ui_with_layout(size, Layout::left_to_right(Align::Center), |ui| {
                     ui.add(Label::new(text, None).interactive(false));
                     ui.add_space(10.0);
                     ui.add(icon);
                  });
               }

               if !sufficient_balance {
                  ui.label(
                     RichText::new("Insufficient balance to send transaction")
                        .size(theme.typography.large)
                        .color(theme.colors.error),
                  );
               }

               // Buttons
               let size = vec2(ui.available_width() * avail_width_margin, 45.0);
               ui.allocate_ui(size, |ui| {
                  ui.horizontal(|ui| {
                     ui.spacing_mut().item_spacing.x = theme.spacing.xl;

                     // leave room for the gap between the two buttons,
                     // otherwise the row overflows to the right
                     let button_size = vec2(
                        (ui.available_width() - theme.spacing.xl) * 0.5,
                        45.0,
                     );

                     let (confirm_ready, confirm_label) =
                        delayed_action_label(self.opened_at, "Confirm");
                     if !confirm_ready {
                        ui.ctx().request_repaint_after(Duration::from_millis(100));
                     }
                     let text = RichText::new(confirm_label).size(theme.typography.large);
                     let confirm = Button::new(text).min_size(button_size).visuals(button_visuals);

                     if ui.add_enabled(sufficient_balance && confirm_ready, confirm).clicked() {
                        self.confirmed_or_rejected = Some(true);
                        self.close();
                     }

                     let text = RichText::new("Reject").size(theme.typography.large);
                     let reject = Button::new(text).min_size(button_size).visuals(button_visuals);

                     if ui.add(reject).clicked() {
                        self.confirmed_or_rejected = Some(false);
                        self.close();
                     }
                  });
               });
            });
         });
   }

   fn sufficient_balance(&self, ctx: &mut ZeusContext, eth_spent: U256, sender: Address) -> bool {
      let balance = ctx.get_eth_balance(self.chain.id(), sender);
      let total_cost = eth_spent + self.tx_cost.wei();
      balance.wei() >= total_cost
   }
}

#[cfg(test)]
mod tests {
   use super::*;

   /// Auto-lock cancels a pending confirmation: the window closes and the
   /// awaiting `confirm_tx` is answered "rejected" so it errors out.
   #[test]
   fn reject_answers_rejected_and_closes() {
      let mut window = TxConfirmationWindow::new();
      window.open = true;

      window.reject();

      assert!(!window.is_open());
      assert_eq!(window.get_confirmed_or_rejected(), Some(false));
   }
}
