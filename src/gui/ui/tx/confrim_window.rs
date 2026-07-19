use egui::{Align, Align2, Frame, Layout, Margin, Order, RichText, Ui, Window, vec2};
use zeus_theme::{OverlayManager, Theme};
use zeus_widgets::{Button, Label, SecureTextEdit};

use super::{address, chain, contract_interact, eth_received, events::*, tx_cost, value};
use crate::assets::icons::Icons;
use crate::core::{DecodedEvent, TransactionAnalysis, ZeusContext, ZeusCtx};
use crate::gui::SHARED_GUI;
use crate::utils::{RT, estimate_tx_cost};
use zeus_eth::{
   alloy_primitives::{Address, U256},
   currency::NativeCurrency,
   types::ChainId,
   utils::NumericValue,
};

use std::sync::Arc;

pub struct TxConfirmationWindow {
   open: bool,
   overlay: OverlayManager,
   decoded_events: DecodedEvents,
   /// True to confirm, false to reject
   confirmed_or_rejected: Option<bool>,
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
   size: (f32, f32),
}

impl TxConfirmationWindow {
   pub fn new(overlay: OverlayManager) -> Self {
      Self {
         open: false,
         overlay: overlay.clone(),
         decoded_events: DecodedEvents::new(overlay),
         confirmed_or_rejected: None,
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
         size: (550.0, 400.0),
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn reset(&mut self, ctx: &mut ZeusContext) {
      self.close(ctx);
      *self = Self::new(self.overlay.clone());
   }

   pub fn close(&mut self, ctx: &mut ZeusContext) {
      self.overlay.window_closed();
      ctx.tx_confirm_window_open = false;
      self.open = false;
   }

   /// Open this [TxConfirmationWindow]
   pub fn open(
      &mut self,
      ctx: ZeusCtx,
      dapp: String,
      chain: ChainId,
      tx: TransactionAnalysis,
      priority_fee: String,
      mev_protect: bool,
      sponsored: bool,
   ) {
      if !self.open {
         self.overlay.window_opened();
      }

      self.sponsored = sponsored;
      
      if sponsored {
         self.tx_cost = NumericValue::default();
         self.tx_cost_usd = NumericValue::default();
      }

      RT.spawn_blocking(move || {
         ctx.set_tx_confirm_window_open(true);

         let native = NativeCurrency::from(chain.id());
         let main_event = tx.infer_main_event(ctx.clone(), chain.id());
         let gas_used = tx.gas_used;
         let gas_limit = gas_used * 15 / 10;

         SHARED_GUI.write(|gui| {
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
            gui.tx_confirmation_window.open = true;
            gui.tx_confirmation_window.confirmed_or_rejected = None;

            ctx.write(|ctx| {
               gui.tx_confirmation_window.calculate_tx_cost(ctx, gas_used);
            });
         });
      });
   }

   pub fn get_confirmed_or_rejected(&self) -> Option<bool> {
      self.confirmed_or_rejected
   }

   pub fn get_priority_fee(&self) -> NumericValue {
      NumericValue::parse_to_gwei(&self.priority_fee)
   }

   pub fn get_gas_limit(&self) -> u64 {
      self.gas_limit
   }

   // TODO: Adjust the UI for txs that are sponsored by another account
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

      let window_frame = theme.frame1;

      Window::new("Transaction Confirmation Window")
         .title_bar(false)
         .resizable(false)
         .order(Order::Foreground)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .collapsible(false)
         .frame(window_frame)
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);

            let button_visuals = theme.button_visuals();
            let text_edit_visuals = theme.text_edit_visuals();

            Frame::new().show(ui, |ui| {
               ui.vertical_centered(|ui| {
                  ui.spacing_mut().item_spacing = vec2(0.0, 15.0);
                  ui.spacing_mut().button_padding = vec2(10.0, 8.0);

                  if self.tx.is_none() {
                     ui.label(
                        RichText::new("Transaction Analysis not found, this is a bug")
                           .size(theme.text_sizes.large),
                     );
                     return;
                  }

                  let analysis = self.tx.as_ref().unwrap();
                  let main_event = self.tx_main_event.as_ref().unwrap();

                  if !self.dapp.is_empty() {
                     ui.label(RichText::new(&self.dapp).size(theme.text_sizes.large));
                  }

                  let frame = theme.frame2;
                  let frame_size = vec2(ui.available_width() * 0.95, 45.0);

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

                  // Action Name
                  ui.label(RichText::new(main_event.name()).size(theme.text_sizes.heading));

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

                  // Tx Action is unknown
                  if main_event.is_other() {
                     let text = "Review the decoded events and proceed with caution";
                     ui.label(
                        RichText::new(text)
                           .size(theme.text_sizes.large)
                           .color(theme.colors.warning),
                     );

                     let text =
                        RichText::new("Show all decoded events").size(theme.text_sizes.large);
                     let button = Button::new(text).visuals(theme.button_visuals());
                     let clicked = ui.add(button).clicked();
                     if clicked {
                        self.decoded_events.open();
                     }
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
                           contract_interact(ctx, self.chain, analysis.interact_to, theme, ui);
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

                  // Give the option to see all the decoded events
                  if !main_event.is_other() {
                     let text =
                        RichText::new("Show all decoded events").size(theme.text_sizes.large);
                     let button = Button::new(text).visuals(theme.button_visuals());
                     let clicked = ui.add(button).clicked();
                     if clicked {
                        self.decoded_events.open();
                     }
                  }

                  ui.add_space(10.0);

                  let sufficient_balance =
                     self.sufficient_balance(ctx, analysis.value_sent().wei(), analysis.sender);

                  let mut recalculate_tx_cost = false;

                  let size = vec2(ui.available_width() * 0.7, 45.0);
                  ui.allocate_ui(size, |ui| {
                     frame.show(ui, |ui| {
                        ui.set_width(size.x);
                        ui.spacing_mut().item_spacing = vec2(15.0, 10.0);

                        ui.horizontal(|ui| {
                           let availabled_width = ui.available_width();
                           let fee_width = ui.available_width() * 0.3;
                           let gas_width = ui.available_width() * 0.5;

                           // Ajdust Priority Fee
                           ui.vertical(|ui| {
                              let text = "Priority Fee (Gwei)";
                              ui.label(RichText::new(text).size(theme.text_sizes.normal));

                              if self.chain.is_bsc() {
                                 ui.disable();
                              }

                              let res = ui.add(
                                 SecureTextEdit::singleline(&mut self.priority_fee)
                                    .visuals(text_edit_visuals)
                                    .margin(Margin::same(10))
                                    .desired_width(fee_width)
                                    .font(egui::FontId::proportional(
                                       theme.text_sizes.normal,
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
                                 ui.label(RichText::new(text).size(theme.text_sizes.normal));

                                 ui.add(
                                    SecureTextEdit::singleline(&mut self.adjusted_gas_limit)
                                       .visuals(text_edit_visuals)
                                       .margin(Margin::same(10))
                                       .desired_width(gas_width)
                                       .font(egui::FontId::proportional(
                                          theme.text_sizes.normal,
                                       )),
                                 );
                              });
                           });
                        });
                     });
                  });

                  ui.add_space(10.0);

                  let base_case = self.chain.is_ethereum()
                     && !main_event.is_other()
                     && main_event.is_mev_vulnerable();
                  let show_mev_protect = base_case || main_event.is_other();
                  let tint = theme.image_tint_recommended;

                  if recalculate_tx_cost {
                     self.calculate_tx_cost(ctx, self.gas_used);
                  }

                  if show_mev_protect {
                     let icon = if self.mev_protect {
                        icons.green_circle(tint)
                     } else {
                        icons.red_circle(tint)
                     };

                     let text = if self.mev_protect {
                        "MEV Protect is enabled"
                     } else {
                        "MEV Protect is disabled"
                     };

                     let text = RichText::new(text).size(theme.text_sizes.normal);
                     ui.add(Label::new(text, Some(icon)).interactive(false));
                  }

                  if !sufficient_balance {
                     ui.label(
                        RichText::new("Insufficient balance to send transaction")
                           .size(theme.text_sizes.large)
                           .color(theme.colors.error),
                     );
                  }

                  // Buttons
                  let size = vec2(ui.available_width() * 0.9, 45.0);
                  ui.allocate_ui(size, |ui| {
                     ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 20.0;

                        let button_size = vec2(ui.available_width() * 0.5, 45.0);

                        let text = RichText::new("Confirm").size(theme.text_sizes.large);
                        let confirm =
                           Button::new(text).min_size(button_size).visuals(button_visuals);

                        if ui.add_enabled(sufficient_balance, confirm).clicked() {
                           self.confirmed_or_rejected = Some(true);
                           self.close(ctx);
                        }

                        let text = RichText::new("Reject").size(theme.text_sizes.large);
                        let reject =
                           Button::new(text).min_size(button_size).visuals(button_visuals);

                        if ui.add(reject).clicked() {
                           self.confirmed_or_rejected = Some(false);
                           self.close(ctx);
                        }
                     });
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
