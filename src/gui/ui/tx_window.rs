use egui::{
   Align, Align2, Frame, Layout, Margin, Order, RichText, ScrollArea, Ui, Vec2, Window, vec2,
};
use zeus_theme::{OverlayManager, Theme};
use zeus_widgets::{Button, Label, SecureTextEdit};

use super::{address, chain, contract_interact, eth_received, tx_cost, tx_hash, value};
use crate::assets::icons::Icons;
use crate::core::{TransactionRich, ZeusCtx, transaction::*, tx_analysis::TransactionAnalysis};
use crate::utils::estimate_tx_cost;
use zeus_eth::{
   alloy_primitives::{Address, U256},
   currency::{Currency, ERC20Token, NativeCurrency},
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

   pub fn reset(&mut self, ctx: ZeusCtx) {
      self.close(ctx);
      *self = Self::new(self.overlay.clone());
   }

   pub fn close(&mut self, ctx: ZeusCtx) {
      self.overlay.window_closed();
      ctx.set_tx_confirm_window_open(false);
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
   ) {
      if !self.open {
         self.overlay.window_opened();
      }
      ctx.set_tx_confirm_window_open(true);

      let native = NativeCurrency::from(chain.id());
      let main_event = tx.infer_main_event(ctx.clone(), chain.id());
      let gas_limit = tx.gas_used * 15 / 10;

      self.dapp = dapp;
      self.priority_fee = priority_fee;
      self.mev_protect = mev_protect;
      self.gas_used = tx.gas_used;
      self.gas_limit = gas_limit;
      self.adjusted_gas_limit = gas_limit.to_string();
      self.chain = chain;
      self.native_currency = native;
      self.tx = Some(tx);
      self.tx_main_event = Some(main_event);
      self.open = true;
      self.confirmed_or_rejected = None;
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

   /// Calculate the cost of the transaction
   fn calculate_tx_cost(&mut self, ctx: ZeusCtx, gas_used: u64) {
      let chain = self.chain;
      let fee = NumericValue::parse_to_gwei(&self.priority_fee);
      let fee = if fee.is_zero() {
         NumericValue::parse_to_gwei("1")
      } else {
         fee
      };

      let (cost_in_wei, cost_in_usd) =
         estimate_tx_cost(ctx.clone(), chain.id(), gas_used, fee.wei());
      self.tx_cost = cost_in_wei;
      self.tx_cost_usd = cost_in_usd;
   }

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      if !self.open {
         return;
      }

      Window::new("Transaction Confirmation Window")
         .title_bar(false)
         .resizable(false)
         .order(Order::Foreground)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .collapsible(false)
         .frame(Frame::window(ui.style()))
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

                  self.calculate_tx_cost(ctx.clone(), self.gas_used);

                  let analysis = self.tx.as_ref().unwrap();
                  let main_event = self.tx_main_event.as_ref().unwrap();

                  if !self.dapp.is_empty() {
                     ui.label(RichText::new(&self.dapp).size(theme.text_sizes.large));
                  }

                  let frame = theme.frame1;
                  let frame_size = vec2(ui.available_width() * 0.95, 45.0);

                  self.decoded_events.show(
                     ctx.clone(),
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
                              ctx.clone(),
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

                     let clicked = show_decoded_events_button(theme, ui);
                     if clicked {
                        self.decoded_events.open();
                     }
                  }

                  // Tx details
                  ui.allocate_ui(frame_size, |ui| {
                     frame.show(ui, |ui| {
                        chain(self.chain, theme, icons.clone(), ui);
                        address(
                           ctx.clone(),
                           self.chain,
                           "Sender",
                           analysis.sender,
                           theme,
                           ui,
                        );

                        // Contract interaction
                        if analysis.contract_interact {
                           contract_interact(
                              ctx.clone(),
                              self.chain,
                              analysis.interact_to,
                              theme,
                              ui,
                           );
                        }

                        // Value to be sent
                        value(
                           ctx.clone(),
                           self.chain,
                           analysis.value_sent(),
                           theme,
                           ui,
                        );

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
                              analysis.eth_received_usd(ctx.clone()),
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
                     let clicked = show_decoded_events_button(theme, ui);
                     if clicked {
                        self.decoded_events.open();
                     }
                  }

                  ui.add_space(10.0);

                  let sufficient_balance = self.sufficient_balance(
                     ctx.clone(),
                     analysis.value_sent().wei(),
                     analysis.sender,
                  );

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

                              ui.add(
                                 SecureTextEdit::singleline(&mut self.priority_fee)
                                    .visuals(text_edit_visuals)
                                    .margin(Margin::same(10))
                                    .desired_width(fee_width)
                                    .font(egui::FontId::proportional(
                                       theme.text_sizes.normal,
                                    )),
                              );
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
                           self.close(ctx.clone());
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

   fn sufficient_balance(&self, ctx: ZeusCtx, eth_spent: U256, sender: Address) -> bool {
      let balance = ctx.get_eth_balance(self.chain.id(), sender);
      let total_cost = eth_spent + self.tx_cost.wei();
      balance.wei() >= total_cost
   }
}

/// A window to show details for a transaction that has been sent to the network
pub struct TxWindow {
   open: bool,
   overlay: OverlayManager,
   decoded_events: DecodedEvents,
   tx: Option<TransactionRich>,
   size: (f32, f32),
}

impl TxWindow {
   pub fn new(overlay: OverlayManager) -> Self {
      Self {
         open: false,
         overlay: overlay.clone(),
         decoded_events: DecodedEvents::new(overlay),
         tx: None,
         size: (550.0, 400.0),
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn close(&mut self) {
      self.overlay.window_closed();
      self.open = false;
      self.tx = None;
   }

   /// Show this [TxWindow]
   pub fn open(&mut self, tx: Option<TransactionRich>) {
      if !self.open {
         self.overlay.window_opened();
      }
      self.tx = tx;
      self.open = true;
   }

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      if !self.open {
         return;
      }

      let title = RichText::new("Transaction Details").size(theme.text_sizes.heading);
      Window::new(title)
         .resizable(false)
         .collapsible(false)
         .order(Order::Foreground)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);

            Frame::new().inner_margin(Margin::same(5)).show(ui, |ui| {
               ui.vertical_centered(|ui| {
                  ui.spacing_mut().item_spacing = vec2(0.0, 15.0);
                  ui.spacing_mut().button_padding = vec2(10.0, 8.0);

                  let button_visuals = theme.button_visuals();

                  ui.add_space(20.0);

                  if self.tx.is_none() {
                     ui.label(RichText::new("Transaction not found").size(theme.text_sizes.large));
                     let size = vec2(ui.available_width() * 0.8, 45.0);

                     let text = RichText::new("Close").size(theme.text_sizes.normal);
                     let close_button = Button::new(text).min_size(size).visuals(button_visuals);

                     if ui.add(close_button).clicked() {
                        self.close();
                     }
                     return;
                  }

                  let tx = self.tx.as_ref().unwrap();
                  let main_event = &tx.main_event;
                  let chain_id: ChainId = tx.chain.into();

                  let frame = theme.frame1;
                  let frame_size = vec2(ui.available_width() * 0.95, 45.0);

                  self.decoded_events.show(
                     ctx.clone(),
                     chain_id,
                     theme,
                     icons.clone(),
                     &tx.analysis,
                     frame_size,
                     frame,
                     self.size,
                     ui,
                  );

                  let frame_size = vec2(ui.available_width() * 0.9, 45.0);

                  if !main_event.is_other() && tx.success {
                     ui.label(
                        RichText::new(main_event.name()).size(theme.text_sizes.very_large).strong(),
                     );
                     ui.allocate_ui(frame_size, |ui| {
                        frame.show(ui, |ui| {
                           show_event(
                              ctx.clone(),
                              chain_id,
                              theme,
                              icons.clone(),
                              main_event,
                              ui,
                           );
                        });
                     });
                  }

                  // Tx Action is unknown
                  if main_event.is_other() && tx.success {
                     let clicked = show_decoded_events_button(theme, ui);
                     if clicked {
                        self.decoded_events.open();
                     }
                  }

                  if !tx.success {
                     let text = "Transaction failed";
                     ui.label(
                        RichText::new(text).size(theme.text_sizes.large).color(theme.colors.error),
                     );
                  }

                  ui.allocate_ui(frame_size, |ui| {
                     frame.show(ui, |ui| {
                        chain(chain_id, theme, icons.clone(), ui);

                        if tx.contract_interact {
                           contract_interact(ctx.clone(), chain_id, tx.interact_to(), theme, ui);
                        }

                        value(
                           ctx.clone(),
                           chain_id,
                           tx.value_sent.clone(),
                           theme,
                           ui,
                        );

                        tx_cost(chain_id, &tx.tx_cost, &tx.tx_cost_usd, theme, ui);

                        tx_hash(tx.chain.into(), &tx.hash, theme, ui);
                     });
                  });

                  // Show ETH received
                  if !tx.eth_received.is_zero()
                     && !tx.analysis.is_unwrap_weth()
                     && !tx.analysis.is_swap()
                  {
                     let text = "Received";
                     ui.allocate_ui(frame_size, |ui| {
                        frame.show(ui, |ui| {
                           eth_received(
                              tx.chain,
                              tx.eth_received.clone(),
                              tx.eth_received_usd.clone(),
                              theme,
                              icons.clone(),
                              text,
                              ui,
                           );
                        });
                     });
                  }

                  ui.add_space(30.0);

                  let size = vec2(ui.available_width() * 0.8, 45.0);
                  let text = RichText::new("Close").size(theme.text_sizes.normal);
                  let close_button = Button::new(text).min_size(size).visuals(button_visuals);

                  if ui.add(close_button).clicked() {
                     self.close();
                  }
               });
            });
         });
   }
}

fn show_decoded_events_button(theme: &Theme, ui: &mut Ui) -> bool {
   let text = RichText::new("Show all decoded events").size(theme.text_sizes.large);
   let button = Button::new(text).visuals(theme.button_visuals());
   ui.add(button).clicked()
}

pub struct DecodedEvents {
   open: bool,
   overlay: OverlayManager,
}

impl DecodedEvents {
   pub fn new(overlay: OverlayManager) -> Self {
      Self {
         open: false,
         overlay,
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self) {
      self.overlay.window_opened();
      self.open = true;
   }

   pub fn close(&mut self) {
      self.overlay.window_closed();
      self.open = false;
   }

   fn show(
      &mut self,
      ctx: ZeusCtx,
      chain: ChainId,
      theme: &Theme,
      icons: Arc<Icons>,
      analysis: &TransactionAnalysis,
      frame_size: Vec2,
      frame: Frame,
      window_size: (f32, f32),
      ui: &mut Ui,
   ) {
      if !self.open {
         return;
      }

      let title = RichText::new("Decoded Events").size(theme.text_sizes.heading);
      let mut open = self.open;

      Window::new(title)
         .open(&mut open)
         .resizable(false)
         .collapsible(false)
         .order(Order::Tooltip)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, -100.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.vertical_centered(|ui| {
               let width = window_size.0 + 50.0;
               ui.set_width(width);
               ui.set_height(window_size.1);
               ui.spacing_mut().item_spacing = vec2(0.0, 15.0);
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);

               let all_events = analysis.total_events();
               let known_events = analysis.known_events;

               let text = format!(
                  "Decoded {} out of {} total events",
                  known_events, all_events
               );
               ui.label(RichText::new(text).size(theme.text_sizes.very_large));

               ScrollArea::vertical().max_height(window_size.1).show(ui, |ui| {
                  ui.set_width(width);

                  for event in &analysis.decoded_events {
                     ui.allocate_ui(frame_size, |ui| {
                        frame.show(ui, |ui| {
                           ui.label(RichText::new(event.name()).size(theme.text_sizes.heading));

                           show_event(
                              ctx.clone(),
                              chain,
                              theme,
                              icons.clone(),
                              event,
                              ui,
                           );
                        });
                     });
                  }
               });
            });
         });

      if !open {
         self.close();
      }
   }
}

pub fn eoa_delegate_event_ui(
   ctx: ZeusCtx,
   chain: ChainId,
   theme: &Theme,
   params: &EOADelegateParams,
   ui: &mut Ui,
) {
   address(
      ctx.clone(),
      chain,
      "Wallet",
      params.eoa,
      theme,
      ui,
   );

   address(
      ctx.clone(),
      chain,
      "Delegate to",
      params.address,
      theme,
      ui,
   );

   // Nonce
   ui.horizontal(|ui| {
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         ui.label(RichText::new("Nonce").size(theme.text_sizes.large));
      });

      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         ui.label(RichText::new(format!("{}", params.nonce)).size(theme.text_sizes.normal));
      });
   });
}

pub fn permit_event_ui(
   ctx: ZeusCtx,
   chain: ChainId,
   theme: &Theme,
   icons: Arc<Icons>,
   params: &PermitParams,
   ui: &mut Ui,
) {
   let is_unlimited = params.amount.wei() == U256::MAX;
   let amount = if is_unlimited {
      "Unlimited".to_string()
   } else {
      params.amount.abbreviated()
   };

   let show_usd_value = !is_unlimited && params.amount_usd.is_some();
   let expiration = params.expiration.to_relative();

   let tint = theme.image_tint_recommended;
   let icon = icons.currency_icon(&params.token, tint);
   let text = if show_usd_value {
      let amount_usd = params.amount_usd.as_ref().unwrap();
      RichText::new(format!(
         "{} {} ~ ${}",
         amount,
         params.token.symbol(),
         amount_usd.abbreviated()
      ))
      .size(theme.text_sizes.large)
   } else {
      RichText::new(format!("{} {}", amount, params.token.symbol())).size(theme.text_sizes.large)
   };

   let label = Label::new(text, Some(icon)).image_on_left().interactive(false);
   ui.add(label);

   // Owner
   address(
      ctx.clone(),
      chain,
      "Owner",
      params.owner,
      theme,
      ui,
   );

   // Spender
   address(
      ctx.clone(),
      chain,
      "Spender",
      params.spender,
      theme,
      ui,
   );

   // Expiration
   ui.horizontal(|ui| {
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         ui.label(RichText::new("Expiration").size(theme.text_sizes.large));
      });

      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         ui.label(RichText::new(expiration).size(theme.text_sizes.large));
      });
   });
}

pub fn token_approval_event_ui(
   ctx: ZeusCtx,
   chain: ChainId,
   theme: &Theme,
   icons: Arc<Icons>,
   params: &TokenApproveParams,
   ui: &mut Ui,
) {
   let token_details = params.token.iter().zip(params.amount.iter()).zip(params.amount_usd.iter());

   for ((token, amount), amount_usd) in token_details {
      let is_unlimited = amount.wei() == U256::MAX;
      let amount = if is_unlimited {
         "Unlimited".to_string()
      } else {
         amount.abbreviated()
      };

      let show_usd_value = !is_unlimited && amount_usd.is_some();
      let tint = theme.image_tint_recommended;

      let icon = icons.currency_icon(&Currency::from(token.clone()), tint);
      let text = if show_usd_value {
         let amount_usd = amount_usd.as_ref().unwrap();
         RichText::new(format!(
            "{} {} ~ ${}",
            amount,
            token.symbol,
            amount_usd.abbreviated()
         ))
         .size(theme.text_sizes.large)
      } else {
         RichText::new(format!("{} {}", amount, token.symbol)).size(theme.text_sizes.large)
      };

      let label = Label::new(text, Some(icon)).image_on_left().interactive(false);
      ui.add(label);
   }

   // Owner
   address(
      ctx.clone(),
      chain,
      "Owner",
      params.owner,
      theme,
      ui,
   );

   // Spender
   address(
      ctx.clone(),
      chain,
      "Spender",
      params.spender,
      theme,
      ui,
   );
}

fn transfer_event_ui(
   ctx: ZeusCtx,
   chain: ChainId,
   theme: &Theme,
   icons: Arc<Icons>,
   params: &TransferParams,
   ui: &mut Ui,
) {
   let size = vec2(ui.available_width(), 30.0);
   let tint = theme.image_tint_recommended;

   // Currency to Send
   ui.allocate_ui(size, |ui| {
      ui.horizontal(|ui| {
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            let currency = &params.currency;
            let amount = &params.amount;
            let icon = icons.currency_icon(currency, tint);
            let text = RichText::new(format!(
               "{} {} ",
               amount.abbreviated(),
               currency.symbol()
            ))
            .size(theme.text_sizes.large);
            let label = Label::new(text, Some(icon)).image_on_left().interactive(false);
            ui.add(label);
         });

         // Value
         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            let amount = params.amount_usd.clone().unwrap_or_default();
            ui.label(
               RichText::new(format!("~ ${}", amount.abbreviated())).size(theme.text_sizes.large),
            );
         });
      });
   });

   // Sender
   address(
      ctx.clone(),
      chain,
      "Sender",
      params.sender,
      theme,
      ui,
   );

   // Recipient
   ui.allocate_ui(size, |ui| {
      address(
         ctx.clone(),
         chain,
         "Recipient",
         params.recipient,
         theme,
         ui,
      );
   });

   // Actual amount sent
   if params.real_amount_sent.is_some() {
      let real_amount_sent = params.real_amount_sent.as_ref().unwrap();
      let real_amount_sent_usd = params.real_amount_sent_usd.clone().unwrap_or_default();

      ui.horizontal(|ui| {
         let text = "Actual amount sent";
         ui.label(RichText::new(text).size(theme.text_sizes.large));

         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            ui.label(
               RichText::new(format!(
                  "~ ${}",
                  real_amount_sent_usd.abbreviated()
               ))
               .size(theme.text_sizes.large),
            );

            let currency = &params.currency;
            let text = RichText::new(format!(
               "{} {} ",
               real_amount_sent.abbreviated(),
               currency.symbol()
            ))
            .size(theme.text_sizes.large);
            let label = Label::new(text, None).interactive(false);
            ui.add(label);
         });
      });
   }
}

fn bridge_event_ui(
   ctx: ZeusCtx,
   chain: ChainId,
   theme: &Theme,
   icons: Arc<Icons>,
   params: &BridgeParams,
   ui: &mut Ui,
) {
   let origin_chain = params.origin_chain;
   let destination_chain = params.destination_chain;
   let tint = theme.image_tint_recommended;

   // EOA's always receive native ETH
   let currency_in = if params.input_currency.is_native_wrapped() {
      NativeCurrency::from(origin_chain).into()
   } else {
      params.input_currency.clone()
   };

   let currency_out = if params.output_currency.is_native_wrapped() {
      NativeCurrency::from(destination_chain).into()
   } else {
      params.output_currency.clone()
   };

   // Input currency column
   ui.horizontal(|ui| {
      let amount = &params.amount;
      let icon = icons.currency_icon(&currency_in, tint);
      let text = RichText::new(format!(
         "- {} {} ",
         amount.abbreviated(),
         currency_in.symbol()
      ))
      .size(theme.text_sizes.large)
      .color(theme.colors.error);
      let label = Label::new(text, Some(icon)).image_on_left().interactive(false);
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         ui.add(label);
      });

      // Value
      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         let value = params.amount_usd.clone().unwrap_or_default();
         ui.label(
            RichText::new(format!("~ ${}", value.abbreviated())).size(theme.text_sizes.large),
         );
      });
   });

   // Received Currency
   ui.horizontal(|ui| {
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         let amount = &params.received;
         let icon = icons.currency_icon(&currency_out, tint);
         let text = RichText::new(format!(
            "+ {} {}",
            amount.abbreviated(),
            currency_out.symbol()
         ))
         .size(theme.text_sizes.large)
         .color(theme.colors.success);
         let label = Label::new(text, Some(icon)).image_on_left().interactive(false);
         ui.add(label);
      });

      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         let value = params.received_usd.clone().unwrap_or_default();
         let text =
            RichText::new(format!("~ ${}", value.abbreviated())).size(theme.text_sizes.large);
         ui.label(text);
      });
   });

   // Depositor
   address(
      ctx.clone(),
      chain,
      "Depositor",
      params.depositor,
      theme,
      ui,
   );

   // Recipient
   address(
      ctx.clone(),
      chain,
      "Recipient",
      params.recipient,
      theme,
      ui,
   );

   // Origin Chain Column
   ui.horizontal(|ui| {
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         ui.label(RichText::new("Origin Chain").size(theme.text_sizes.large));
      });

      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         let chain: ChainId = params.origin_chain.into();
         let icon = icons.chain_icon(chain.id(), tint);
         let text = RichText::new(chain.name()).size(theme.text_sizes.large);
         let label = Label::new(text, Some(icon)).image_on_left().interactive(false);
         ui.add(label);
      });
   });

   // Destination Chain Column
   ui.horizontal(|ui| {
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         ui.label(RichText::new("Destination Chain").size(theme.text_sizes.large));
      });

      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         let chain: ChainId = params.destination_chain.into();
         let icon = icons.chain_icon(chain.id(), tint);
         let text = RichText::new(chain.name()).size(theme.text_sizes.large);
         let label = Label::new(text, Some(icon)).image_on_left().interactive(false);
         ui.add(label);
      });
   });
}

fn swap_event_ui(theme: &Theme, icons: Arc<Icons>, params: &SwapParams, ui: &mut Ui) {
   let tint = theme.image_tint_recommended;

   // Input currency column
   ui.horizontal(|ui| {
      let currency = &params.input_currency;
      let amount = &params.amount_in;
      let icon = icons.currency_icon(currency, tint);
      let text = RichText::new(format!(
         "- {} {} ",
         amount.abbreviated(),
         currency.symbol()
      ))
      .size(theme.text_sizes.large)
      .color(theme.colors.error);
      let label = Label::new(text, Some(icon)).image_on_left().interactive(false);
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         ui.add(label);
      });

      // Value
      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         let value = params.amount_in_usd.clone().unwrap_or_default();
         ui.label(
            RichText::new(format!("~ ${}", value.abbreviated())).size(theme.text_sizes.large),
         );
      });
   });

   // Received Currency
   ui.horizontal(|ui| {
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         let currency = &params.output_currency;
         let amount = &params.received;
         let icon = icons.currency_icon(currency, tint);
         let text = RichText::new(format!(
            "+ {} {}",
            amount.abbreviated(),
            currency.symbol()
         ))
         .size(theme.text_sizes.large)
         .color(theme.colors.success);
         let label = Label::new(text, Some(icon)).image_on_left().interactive(false);
         ui.add(label);
      });

      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         let value = params.received_usd.clone().unwrap_or_default();
         let text =
            RichText::new(format!("~ ${}", value.abbreviated())).size(theme.text_sizes.large);
         ui.label(text);
      });
   });

   // Minimum Received
   let amount = params.min_received.clone();
   let amount_usd = params.min_received_usd.clone();
   if amount.is_some() && amount_usd.is_some() {
      ui.horizontal(|ui| {
         ui.label(RichText::new("Minimum Received").size(theme.text_sizes.large));
         ui.add_space(15.0);

         let amount = amount.unwrap();
         let amount_usd = amount_usd.unwrap();
         let currency = &params.output_currency;
         let amount_symbol = format!("{} {}", amount.abbreviated(), currency.symbol());
         let amount_usd = format!("~ ${}", amount_usd.abbreviated());
         let text =
            RichText::new(format!("{} {}", amount_symbol, amount_usd)).size(theme.text_sizes.large);
         let label = Label::new(text, None).interactive(false);
         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            ui.add(label);
         });
      });
   }
}

fn wrap_eth_event_ui(
   ctx: ZeusCtx,
   chain: ChainId,
   theme: &Theme,
   icons: Arc<Icons>,
   params: &WrapETHParams,
   ui: &mut Ui,
) {
   let tint = theme.image_tint_recommended;
   let weth = Currency::from(ERC20Token::wrapped_native_token(chain.id()));
   let weth_icon = icons.currency_icon(&weth, tint);

   // Amount received + USD Value
   ui.horizontal(|ui| {
      let text = RichText::new(format!(
         "+ {} {}",
         params.weth_received.abbreviated(),
         weth.symbol()
      ))
      .size(theme.text_sizes.large)
      .color(theme.colors.success);
      let label = Label::new(text, Some(weth_icon)).image_on_left().interactive(false);
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         ui.add(label);
      });

      // USD Value
      let weth_received_usd = params.weth_received_usd.clone().unwrap_or_default();
      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         let text = RichText::new(format!("~ ${}", weth_received_usd.abbreviated()))
            .size(theme.text_sizes.large);
         ui.label(text);
      });
   });

   // Recipient
   address(
      ctx.clone(),
      chain,
      "Recipient",
      params.recipient,
      theme,
      ui,
   );
}

fn unwrap_weth_event_ui(
   ctx: ZeusCtx,
   chain: ChainId,
   theme: &Theme,
   icons: Arc<Icons>,
   params: &UnwrapWETHParams,
   ui: &mut Ui,
) {
   let tint = theme.image_tint_recommended;
   let eth = NativeCurrency::from(chain.id());
   let eth_icon = icons.native_currency_icon(chain.id(), tint);

   // Amount received + USD Value
   ui.horizontal(|ui| {
      let text = RichText::new(format!(
         "+ {} {}",
         params.weth_unwrapped.abbreviated(),
         eth.symbol
      ))
      .size(theme.text_sizes.large)
      .color(theme.colors.success);
      let label = Label::new(text, Some(eth_icon)).image_on_left().interactive(false);
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         ui.add(label);
      });

      // USD Value
      let weth_unwrapped_usd = params.weth_unwrapped_usd.clone().unwrap_or_default();
      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         let text = RichText::new(format!("~ ${}", weth_unwrapped_usd.abbreviated()))
            .size(theme.text_sizes.large);
         ui.label(text);
      });
   });

   // Source
   address(
      ctx.clone(),
      chain,
      "Source",
      params.src,
      theme,
      ui,
   );
}

fn uniswap_position_op_event_ui(
   ctx: ZeusCtx,
   chain: ChainId,
   theme: &Theme,
   icons: Arc<Icons>,
   params: &UniswapPositionParams,
   ui: &mut Ui,
) {
   let currency0 = &params.currency0;
   let currency1 = &params.currency1;
   let amount0 = &params.amount0;
   let amount1 = &params.amount1;
   let amount0_usd = params.amount0_usd.clone().unwrap_or_default();
   let amount1_usd = params.amount1_usd.clone().unwrap_or_default();
   let min_amount0 = params.min_amount0.clone();
   let min_amount1 = params.min_amount1.clone();
   let min_amount0_usd = params.min_amount0_usd.clone().unwrap_or_default();
   let min_amount1_usd = params.min_amount1_usd.clone().unwrap_or_default();

   let tint = theme.image_tint_recommended;

   // Currency A and Amount & value
   ui.horizontal(|ui| {
      let icon = icons.currency_icon(currency0, tint);

      let text = format!("{} {}", amount0.abbreviated(), currency0.symbol());
      let text = RichText::new(text).size(theme.text_sizes.large);

      let label = Label::new(text, Some(icon)).image_on_left().interactive(false);
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         ui.add(label);
      });

      // Value
      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         let amount = amount0_usd.abbreviated();
         ui.label(RichText::new(format!("~ ${}", amount)).size(theme.text_sizes.large));
      });
   });

   // Currency B and Amount & value
   ui.horizontal(|ui| {
      let icon = icons.currency_icon(currency1, tint);
      let text = format!("{} {}", amount1.abbreviated(), currency1.symbol());

      let text = RichText::new(text).size(theme.text_sizes.large);
      let label = Label::new(text, Some(icon)).image_on_left().interactive(false);
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         ui.add(label);
      });

      // Value
      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         let amount = amount1_usd.abbreviated();
         ui.label(RichText::new(format!("~ ${}", amount)).size(theme.text_sizes.large));
      });
   });

   let text = if params.op_is_add_liquidity() {
      "Minimum Liquidity to be added"
   } else {
      "Minimum Liquidity to be removed"
   };

   if min_amount0.is_some() && min_amount1.is_some() {
      ui.label(RichText::new(text).size(theme.text_sizes.large));
   }

   // Minimum Amount A and Amount & value
   if min_amount0.is_some() {
      let min_amount0 = min_amount0.unwrap();
      ui.horizontal(|ui| {
         let icon = icons.currency_icon(currency0, tint);
         let text = format!(
            "{} {}",
            min_amount0.abbreviated(),
            currency0.symbol()
         );
         let text = RichText::new(text).size(theme.text_sizes.large);

         let label = Label::new(text, Some(icon)).image_on_left();
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.add(label);
         });

         // Value
         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            let amount = min_amount0_usd.abbreviated();
            ui.label(RichText::new(format!("~ ${}", amount)).size(theme.text_sizes.large));
         });
      });
   }

   // Minimum Amount B and Amount & value
   if min_amount1.is_some() {
      let min_amount1 = min_amount1.unwrap();
      ui.horizontal(|ui| {
         let icon = icons.currency_icon(currency1, tint);
         let text = format!(
            "{} {}",
            min_amount1.abbreviated(),
            currency1.symbol()
         );

         let text = RichText::new(text).size(theme.text_sizes.large);
         let label = Label::new(text, Some(icon)).image_on_left().interactive(false);
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.add(label);
         });

         // Value
         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            let amount = min_amount1_usd.abbreviated();
            ui.label(RichText::new(format!("~ ${}", amount)).size(theme.text_sizes.large));
         });
      });
   }

   // Recipient
   if params.recipient.is_some() {
      address(
         ctx.clone(),
         chain,
         "Recipient",
         params.recipient.unwrap(),
         theme,
         ui,
      );
   }
}

fn show_event(
   ctx: ZeusCtx,
   chain: ChainId,
   theme: &Theme,
   icons: Arc<Icons>,
   event: &DecodedEvent,
   ui: &mut Ui,
) {
   if event.is_native_transfer() || event.is_erc20_transfer() {
      let params = event.transfer_params();
      transfer_event_ui(
         ctx.clone(),
         chain,
         theme,
         icons.clone(),
         params,
         ui,
      );
   }

   if event.is_token_approval() {
      let params = event.token_approval_params();
      token_approval_event_ui(
         ctx.clone(),
         chain,
         theme,
         icons.clone(),
         params,
         ui,
      );
   }

   if event.is_permit() {
      let params = event.permit_params();
      permit_event_ui(
         ctx.clone(),
         chain,
         theme,
         icons.clone(),
         params,
         ui,
      );
   }

   if event.is_wrap_eth() {
      let params = event.wrap_eth_params();
      wrap_eth_event_ui(
         ctx.clone(),
         chain,
         theme,
         icons.clone(),
         params,
         ui,
      );
   }

   if event.is_unwrap_weth() {
      let params = event.unwrap_weth_params();
      unwrap_weth_event_ui(
         ctx.clone(),
         chain,
         theme,
         icons.clone(),
         params,
         ui,
      );
   }

   if event.is_uniswap_position_op() {
      let params = event.uniswap_position_params();
      uniswap_position_op_event_ui(
         ctx.clone(),
         chain,
         theme,
         icons.clone(),
         params,
         ui,
      );
   }

   if event.is_bridge() {
      let params = event.bridge_params();
      bridge_event_ui(
         ctx.clone(),
         chain,
         theme,
         icons.clone(),
         params,
         ui,
      );
   }

   if event.is_swap() {
      let params = event.swap_params();
      swap_event_ui(theme, icons.clone(), params, ui);
   }

   if event.is_eoa_delegate() {
      let params = event.eoa_delegate_params();
      eoa_delegate_event_ui(ctx.clone(), chain, theme, params, ui);
   }
}
