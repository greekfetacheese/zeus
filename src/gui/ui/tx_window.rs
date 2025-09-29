use egui::{
   Align, Align2, Button, Frame, Layout, Margin, Order, RichText, ScrollArea, TextEdit, Ui, Vec2,
   Window, vec2,
};
use egui_theme::Theme;
use egui_widgets::Label;

use super::{address, chain, contract_interact, eth_received, tx_cost, tx_hash, value};
use crate::assets::icons::Icons;
use crate::core::{
   TransactionRich, ZeusCtx, transaction::*, tx_analysis::TransactionAnalysis,
   utils::estimate_tx_cost,
};
use zeus_eth::{
   alloy_primitives::{Address, U256},
   currency::{Currency, ERC20Token, NativeCurrency},
   types::ChainId,
   utils::NumericValue,
};

use std::sync::Arc;

pub struct TxConfirmationWindow {
   open: bool,
   show_all_decoded_events: bool,
   /// True to confirm, false to reject
   confirmed_or_rejected: Option<bool>,
   dapp: String,
   chain: ChainId,
   native_currency: NativeCurrency,
   /// Tx to be confirmed and sent to the network
   tx: Option<TransactionAnalysis>,
   tx_action: Option<TransactionAction>,
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
   pub fn new() -> Self {
      Self {
         open: false,
         show_all_decoded_events: false,
         confirmed_or_rejected: None,
         dapp: String::new(),
         chain: ChainId::default(),
         native_currency: NativeCurrency::default(),
         tx: None,
         tx_action: None,
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
      ctx.set_tx_confirm_window_open(false);
      *self = Self::new();
   }

   pub fn close(&mut self, ctx: ZeusCtx) {
      ctx.set_tx_confirm_window_open(false);
      self.open = false;
   }

   /// Open this [TxConfirmationWindow]
   ///
   /// - `dapp` dapp name, if not just pass an empty string
   /// - `chain` the chain id to be used
   /// - `tx` is the transaction to be confirmed
   /// - `priority_fee` set a starting value for the priority fee
   /// - `mev_protect` whether we use an MEV protect endpoint or not
   pub fn open(
      &mut self,
      ctx: ZeusCtx,
      dapp: String,
      chain: ChainId,
      tx: TransactionAnalysis,
      priority_fee: String,
      mev_protect: bool,
   ) {
      ctx.set_tx_confirm_window_open(true);

      let native = NativeCurrency::from(chain.id());
      let action = tx.infer_action(ctx.clone(), chain.id());
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
      self.tx_action = Some(action);
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

            Frame::new().inner_margin(Margin::same(5)).show(ui, |ui| {
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
                  let action = self.tx_action.as_ref().unwrap();

                  if !self.dapp.is_empty() {
                     ui.label(RichText::new(&self.dapp).size(theme.text_sizes.large));
                  }

                  let frame = theme.frame2;
                  let frame_size = vec2(ui.available_width() * 0.95, 45.0);
                  let mut open = self.show_all_decoded_events;

                  let should_open = self.show_all_decoded_events(
                     &mut open,
                     ctx.clone(),
                     theme,
                     icons.clone(),
                     analysis,
                     frame_size,
                     frame,
                     ui,
                  );

                  self.show_all_decoded_events = should_open;

                  // Action Name
                  ui.label(RichText::new(action.name()).size(theme.text_sizes.heading));

                  if !action.is_other() {
                     ui.allocate_ui(frame_size, |ui| {
                        frame.show(ui, |ui| {
                           show_action(
                              ctx.clone(),
                              self.chain,
                              theme,
                              icons.clone(),
                              action,
                              ui,
                           );
                        });
                     });
                  }

                  // Tx Action is unknown
                  if action.is_other() {
                     let text = "Review the decoded events and proceed with caution";
                     ui.label(
                        RichText::new(text)
                           .size(theme.text_sizes.large)
                           .color(theme.colors.error_color),
                     );

                     let clicked = self.show_decoded_events_button(theme, ui);
                     if clicked {
                        self.show_all_decoded_events = true;
                     }
                  }

                  ui.add_space(10.0);

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
                  if !action.is_other() {
                     let clicked = self.show_decoded_events_button(theme, ui);
                     if clicked {
                        self.show_all_decoded_events = true;
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
                                 TextEdit::singleline(&mut self.priority_fee)
                                    .margin(Margin::same(10))
                                    .background_color(theme.colors.text_edit_bg)
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
                                    TextEdit::singleline(&mut self.adjusted_gas_limit)
                                       .margin(Margin::same(10))
                                       .background_color(theme.colors.text_edit_bg)
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

                  let base_case =
                     self.chain.is_ethereum() && !action.is_other() && action.is_mev_vulnerable();
                  let show_mev_protect = base_case || action.is_other();

                  if show_mev_protect {
                     let icon = if self.mev_protect {
                        icons.green_circle()
                     } else {
                        icons.red_circle()
                     };

                     let text = if self.mev_protect {
                        "MEV Protect is enabled"
                     } else {
                        "MEV Protect is disabled"
                     };

                     let text = RichText::new(text).size(theme.text_sizes.normal);
                     ui.add(Label::new(text, Some(icon)));
                  }

                  if !sufficient_balance {
                     ui.label(
                        RichText::new("Insufficient balance to send transaction")
                           .size(theme.text_sizes.large)
                           .color(theme.colors.error_color),
                     );
                  }

                  // Buttons
                  let size = vec2(ui.available_width() * 0.9, 45.0);
                  ui.allocate_ui(size, |ui| {
                     ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 20.0;

                        let button_size = vec2(ui.available_width() * 0.5, 45.0);

                        let confirm =
                           Button::new(RichText::new("Confirm").size(theme.text_sizes.normal))
                              .min_size(button_size);

                        if ui.add_enabled(sufficient_balance, confirm).clicked() {
                           self.confirmed_or_rejected = Some(true);
                           self.close(ctx.clone());
                        }

                        let reject =
                           Button::new(RichText::new("Reject").size(theme.text_sizes.normal))
                              .min_size(button_size);

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

   fn show_decoded_events_button(&self, theme: &Theme, ui: &mut Ui) -> bool {
      let text = RichText::new("Show all decoded events").size(theme.text_sizes.normal);
      let button = Button::new(text);

      ui.add(button).clicked()
   }

   fn sufficient_balance(&self, ctx: ZeusCtx, eth_spent: U256, sender: Address) -> bool {
      let balance = ctx.get_eth_balance(self.chain.id(), sender);
      let total_cost = eth_spent + self.tx_cost.wei();
      balance.wei() >= total_cost
   }

   fn show_all_decoded_events(
      &self,
      open: &mut bool,
      ctx: ZeusCtx,
      theme: &Theme,
      icons: Arc<Icons>,
      analysis: &TransactionAnalysis,
      frame_size: Vec2,
      frame: Frame,
      ui: &mut Ui,
   ) -> bool {
      let title = RichText::new("Decoded Events").size(theme.text_sizes.heading);

      Window::new(title)
         .open(open)
         .resizable(false)
         .collapsible(false)
         .order(Order::Tooltip)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, -100.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.vertical_centered(|ui| {
               let width = self.size.0 + 50.0;
               ui.set_width(width);
               ui.set_height(self.size.1);
               ui.spacing_mut().item_spacing = vec2(0.0, 15.0);
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);

               let all_events = analysis.total_events();
               let known_events = analysis.known_events;

               let text = format!(
                  "Decoded {} out of {} total events",
                  known_events, all_events
               );
               ui.label(RichText::new(text).size(theme.text_sizes.very_large));

               ScrollArea::vertical().max_height(self.size.1).show(ui, |ui| {
                  ui.set_width(width);

                  show_decoded_events(
                     ctx.clone(),
                     self.chain,
                     theme,
                     icons.clone(),
                     analysis,
                     frame_size,
                     frame,
                     ui,
                  );
               });
            });
         });

      *open
   }
}

/// A window to show details for a transaction that has been sent to the network
pub struct TxWindow {
   open: bool,
   tx: Option<TransactionRich>,
   size: (f32, f32),
}

impl TxWindow {
   pub fn new() -> Self {
      Self {
         open: false,
         tx: None,
         size: (550.0, 400.0),
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn close(&mut self) {
      self.open = false;
      self.tx = None;
   }

   /// Show this [TxWindow]
   pub fn open(&mut self, tx: Option<TransactionRich>) {
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

                  ui.add_space(20.0);

                  if self.tx.is_none() {
                     ui.label(RichText::new("Transaction not found").size(theme.text_sizes.large));
                     let size = vec2(ui.available_width() * 0.8, 45.0);
                     let close_button =
                        Button::new(RichText::new("Close").size(theme.text_sizes.normal))
                           .min_size(size);

                     if ui.add(close_button).clicked() {
                        self.close();
                     }
                     return;
                  }

                  let tx = self.tx.as_ref().unwrap();
                  let action = &tx.action;
                  let chain_id: ChainId = tx.chain.into();

                  /*
                  // Show ETH sent
                  if !tx.value_sent.is_zero()
                     && !tx.analysis.is_wrap_eth()
                     && !tx.analysis.is_swap()
                  {
                     ui.add(Label::new(
                        RichText::new("You spent").size(theme.text_sizes.large),
                        None,
                     ));

                     eth_spent(
                        tx.chain,
                        tx.value_sent.clone(),
                        tx.value_sent_usd.clone(),
                        theme,
                        icons.clone(),
                        ui,
                     );
                  }
                  */

                  let frame_size = vec2(ui.available_width() * 0.9, 45.0);
                  let frame = theme.frame2;

                  // Action Name
                  ui.label(RichText::new(action.name()).size(theme.text_sizes.very_large).strong());

                  if !action.is_other() {
                     ui.allocate_ui(frame_size, |ui| {
                        frame.show(ui, |ui| {
                           show_action(
                              ctx.clone(),
                              chain_id,
                              theme,
                              icons.clone(),
                              action,
                              ui,
                           );
                        });
                     });
                  }

                  // Tx Action is unknown
                  if action.is_other() {
                     ui.label(RichText::new("Decoded Events").size(theme.text_sizes.large));

                     ScrollArea::vertical().max_height(self.size.1 / 2.0).show(ui, |ui| {
                        ui.set_width(self.size.0);

                        show_decoded_events(
                           ctx.clone(),
                           chain_id,
                           theme,
                           icons.clone(),
                           &tx.analysis,
                           frame_size,
                           frame,
                           ui,
                        );
                     });
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
                  let close_button =
                     Button::new(RichText::new("Close").size(theme.text_sizes.normal))
                        .min_size(size);

                  if ui.add(close_button).clicked() {
                     self.close();
                  }
               });
            });
         });
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
         amount.format_abbreviated()
      };

      let show_usd_value = !is_unlimited && amount_usd.is_some();

      let icon = icons.currency_icon(&Currency::from(token.clone()));
      let text = if show_usd_value {
         let amount_usd = amount_usd.as_ref().unwrap();
         RichText::new(format!(
            "{} {} ~ ${}",
            amount,
            token.symbol,
            amount_usd.format_abbreviated()
         ))
         .size(theme.text_sizes.normal)
      } else {
         RichText::new(format!("{} {}", amount, token.symbol)).size(theme.text_sizes.normal)
      };

      let label = Label::new(text, Some(icon)).image_on_left();
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

   // Currency to Send
   ui.allocate_ui(size, |ui| {
      ui.horizontal(|ui| {
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            let currency = &params.currency;
            let amount = &params.amount;
            let icon = icons.currency_icon(currency);
            let text = RichText::new(format!(
               "{} {} ",
               amount.format_abbreviated(),
               currency.symbol()
            ))
            .size(theme.text_sizes.large);
            let label = Label::new(text, Some(icon)).image_on_left();
            ui.add(label);
         });

         // Value
         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            let amount = params.amount_usd.clone().unwrap_or_default();
            ui.label(
               RichText::new(format!("~ ${}", amount.format_abbreviated()))
                  .size(theme.text_sizes.large),
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
                  real_amount_sent_usd.format_abbreviated()
               ))
               .size(theme.text_sizes.large),
            );

            let currency = &params.currency;
            let text = RichText::new(format!(
               "{} {} ",
               real_amount_sent.format_abbreviated(),
               currency.symbol()
            ))
            .size(theme.text_sizes.large);
            let label = Label::new(text, None);
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
      let icon = icons.currency_icon(&currency_in);
      let text = RichText::new(format!(
         "- {} {} ",
         amount.format_abbreviated(),
         currency_in.symbol()
      ))
      .size(theme.text_sizes.normal)
      .color(theme.colors.error_color);
      let label = Label::new(text, Some(icon)).image_on_left();
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         ui.add(label);
      });

      // Value
      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         let value = params.amount_usd.clone().unwrap_or_default();
         ui.label(
            RichText::new(format!("~ ${}", value.format_abbreviated()))
               .size(theme.text_sizes.normal),
         );
      });
   });

   // Received Currency
   ui.horizontal(|ui| {
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         let amount = &params.received;
         let icon = icons.currency_icon(&currency_out);
         let text = RichText::new(format!(
            "+ {} {}",
            amount.format_abbreviated(),
            currency_out.symbol()
         ))
         .size(theme.text_sizes.normal)
         .color(theme.colors.success_color);
         let label = Label::new(text, Some(icon)).image_on_left();
         ui.add(label);
      });

      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         let value = params.received_usd.clone().unwrap_or_default();
         let text = RichText::new(format!("~ ${}", value.format_abbreviated()))
            .size(theme.text_sizes.normal);
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
         let icon = icons.chain_icon(chain.id());
         let label = Label::new(
            RichText::new(chain.name()).size(theme.text_sizes.normal),
            Some(icon),
         )
         .image_on_left();
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
         let icon = icons.chain_icon(chain.id());
         let label = Label::new(
            RichText::new(chain.name()).size(theme.text_sizes.normal),
            Some(icon),
         )
         .image_on_left();
         ui.add(label);
      });
   });
}

fn swap_event_ui(theme: &Theme, icons: Arc<Icons>, params: &SwapParams, ui: &mut Ui) {
   // Input currency column
   ui.horizontal(|ui| {
      let currency = &params.input_currency;
      let amount = &params.amount_in;
      let icon = icons.currency_icon(currency);
      let text = RichText::new(format!(
         "- {} {} ",
         amount.format_abbreviated(),
         currency.symbol()
      ))
      .size(theme.text_sizes.large)
      .color(theme.colors.error_color);
      let label = Label::new(text, Some(icon)).image_on_left();
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         ui.add(label);
      });

      // Value
      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         let value = params.amount_in_usd.clone().unwrap_or_default();
         ui.label(
            RichText::new(format!("~ ${}", value.format_abbreviated()))
               .size(theme.text_sizes.large),
         );
      });
   });

   // Received Currency
   ui.horizontal(|ui| {
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         let currency = &params.output_currency;
         let amount = &params.received;
         let icon = icons.currency_icon(currency);
         let text = RichText::new(format!(
            "+ {} {}",
            amount.format_abbreviated(),
            currency.symbol()
         ))
         .size(theme.text_sizes.large)
         .color(theme.colors.success_color);
         let label = Label::new(text, Some(icon)).image_on_left();
         ui.add(label);
      });

      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         let value = params.received_usd.clone().unwrap_or_default();
         let text = RichText::new(format!("~ ${}", value.format_abbreviated()))
            .size(theme.text_sizes.large);
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
         let amount_symbol = format!(
            "{} {}",
            amount.format_abbreviated(),
            currency.symbol()
         );
         let amount_usd = format!("~ ${}", amount_usd.format_abbreviated());
         let text =
            RichText::new(format!("{} {}", amount_symbol, amount_usd)).size(theme.text_sizes.large);
         let label = Label::new(text, None);
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
   let weth = Currency::from(ERC20Token::wrapped_native_token(chain.id()));
   let weth_icon = icons.currency_icon(&weth);

   // Amount received + USD Value
   ui.horizontal(|ui| {
      let text = RichText::new(format!(
         "+ {} {}",
         params.weth_received.format_abbreviated(),
         weth.symbol()
      ))
      .size(theme.text_sizes.normal)
      .color(theme.colors.success_color);
      let label = Label::new(text, Some(weth_icon)).image_on_left();
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         ui.add(label);
      });

      // USD Value
      let weth_received_usd = params.weth_received_usd.clone().unwrap_or_default();
      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         let text = RichText::new(format!(
            "~ ${}",
            weth_received_usd.format_abbreviated()
         ))
         .size(theme.text_sizes.normal);
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
   let eth = NativeCurrency::from(chain.id());
   let eth_icon = icons.native_currency_icon(chain.id());

   // Amount received + USD Value
   ui.horizontal(|ui| {
      let text = RichText::new(format!(
         "+ {} {}",
         params.weth_unwrapped.format_abbreviated(),
         eth.symbol
      ))
      .size(theme.text_sizes.normal)
      .color(theme.colors.success_color);
      let label = Label::new(text, Some(eth_icon)).image_on_left();
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         ui.add(label);
      });

      // USD Value
      let weth_unwrapped_usd = params.weth_unwrapped_usd.clone().unwrap_or_default();
      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         let text = RichText::new(format!(
            "~ ${}",
            weth_unwrapped_usd.format_abbreviated()
         ))
         .size(theme.text_sizes.normal);
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

   // Currency A and Amount & value
   ui.horizontal(|ui| {
      let icon = icons.currency_icon(currency0);

      let text = format!(
         "{} {}",
         amount0.format_abbreviated(),
         currency0.symbol()
      );
      let text = RichText::new(text).size(theme.text_sizes.normal);

      let label = Label::new(text, Some(icon)).image_on_left();
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         ui.add(label);
      });

      // Value
      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         let amount = amount0_usd.format_abbreviated();
         ui.label(RichText::new(format!("~ ${}", amount)).size(theme.text_sizes.normal));
      });
   });

   // Currency B and Amount & value
   ui.horizontal(|ui| {
      let icon = icons.currency_icon(currency1);
      let text = format!(
         "{} {}",
         amount1.format_abbreviated(),
         currency1.symbol()
      );

      let text = RichText::new(text).size(theme.text_sizes.normal);
      let label = Label::new(text, Some(icon)).image_on_left();
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         ui.add(label);
      });

      // Value
      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         let amount = amount1_usd.format_abbreviated();
         ui.label(RichText::new(format!("~ ${}", amount)).size(theme.text_sizes.normal));
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
         let icon = icons.currency_icon(currency0);
         let text = format!(
            "{} {}",
            min_amount0.format_abbreviated(),
            currency0.symbol()
         );
         let text = RichText::new(text).size(theme.text_sizes.normal);

         let label = Label::new(text, Some(icon)).image_on_left();
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.add(label);
         });

         // Value
         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            let amount = min_amount0_usd.format_abbreviated();
            ui.label(RichText::new(format!("~ ${}", amount)).size(theme.text_sizes.normal));
         });
      });
   }

   // Minimum Amount B and Amount & value
   if min_amount1.is_some() {
      let min_amount1 = min_amount1.unwrap();
      ui.horizontal(|ui| {
         let icon = icons.currency_icon(currency1);
         let text = format!(
            "{} {}",
            min_amount1.format_abbreviated(),
            currency1.symbol()
         );

         let text = RichText::new(text).size(theme.text_sizes.normal);
         let label = Label::new(text, Some(icon)).image_on_left();
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.add(label);
         });

         // Value
         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            let amount = min_amount1_usd.format_abbreviated();
            ui.label(RichText::new(format!("~ ${}", amount)).size(theme.text_sizes.normal));
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

fn show_decoded_events(
   ctx: ZeusCtx,
   chain: ChainId,
   theme: &Theme,
   icons: Arc<Icons>,
   analysis: &TransactionAnalysis,
   frame_size: Vec2,
   frame: Frame,
   ui: &mut Ui,
) {
   let erc20_transfers = &analysis.erc20_transfers();

   for erc20_transfer in erc20_transfers {
      ui.allocate_ui(frame_size, |ui| {
         frame.show(ui, |ui| {
            ui.label(RichText::new("ERC20 Transfer").size(theme.text_sizes.large));

            transfer_event_ui(
               ctx.clone(),
               chain,
               theme,
               icons.clone(),
               erc20_transfer,
               ui,
            );
         });
      });
   }

   for token_approval in &analysis.token_approvals {
      ui.allocate_ui(frame_size, |ui| {
         frame.show(ui, |ui| {
            ui.label(RichText::new("Token Approval").size(theme.text_sizes.large));

            token_approval_event_ui(
               ctx.clone(),
               chain,
               theme,
               icons.clone(),
               token_approval,
               ui,
            );
         });
      });
   }

   for wrap_eth in &analysis.eth_wraps {
      ui.allocate_ui(frame_size, |ui| {
         frame.show(ui, |ui| {
            ui.label(RichText::new("Wrap ETH").size(theme.text_sizes.large));

            wrap_eth_event_ui(
               ctx.clone(),
               chain,
               theme,
               icons.clone(),
               wrap_eth,
               ui,
            );
         });
      });
   }

   for unwrap_weth in &analysis.weth_unwraps {
      ui.allocate_ui(frame_size, |ui| {
         frame.show(ui, |ui| {
            ui.label(RichText::new("Unwrap WETH").size(theme.text_sizes.large));

            unwrap_weth_event_ui(
               ctx.clone(),
               chain,
               theme,
               icons.clone(),
               unwrap_weth,
               ui,
            );
         });
      });
   }

   for swap in &analysis.swaps {
      ui.allocate_ui(frame_size, |ui| {
         frame.show(ui, |ui| {
            ui.label(RichText::new("Swap").size(theme.text_sizes.large));
            swap_event_ui(theme, icons.clone(), swap, ui);
         });
      });
   }

   for bridge in &analysis.bridge {
      ui.allocate_ui(frame_size, |ui| {
         frame.show(ui, |ui| {
            ui.label(RichText::new("Bridge").size(theme.text_sizes.large));

            bridge_event_ui(
               ctx.clone(),
               chain,
               theme,
               icons.clone(),
               bridge,
               ui,
            );
         });
      });
   }

   for uniswap_position_op in &analysis.positions_ops {
      ui.allocate_ui(frame_size, |ui| {
         frame.show(ui, |ui| {
            ui.label(RichText::new("Uniswap Position Operation").size(theme.text_sizes.large));

            uniswap_position_op_event_ui(
               ctx.clone(),
               chain,
               theme,
               icons.clone(),
               uniswap_position_op,
               ui,
            );
         });
      });
   }

   for eoa_delegate in &analysis.eoa_delegates {
      ui.allocate_ui(frame_size, |ui| {
         frame.show(ui, |ui| {
            ui.label(RichText::new("Wallet Delegation").size(theme.text_sizes.large));
            eoa_delegate_event_ui(ctx.clone(), chain, theme, eoa_delegate, ui);
         });
      });
   }
}

fn show_action(
   ctx: ZeusCtx,
   chain: ChainId,
   theme: &Theme,
   icons: Arc<Icons>,
   action: &TransactionAction,
   ui: &mut Ui,
) {
   if action.is_native_transfer() || action.is_erc20_transfer() {
      let params = action.transfer_params();
      transfer_event_ui(
         ctx.clone(),
         chain,
         theme,
         icons.clone(),
         params,
         ui,
      );
   }

   if action.is_token_approval() {
      let params = action.token_approval_params();
      token_approval_event_ui(
         ctx.clone(),
         chain,
         theme,
         icons.clone(),
         params,
         ui,
      );
   }

   if action.is_wrap_eth() {
      let params = action.wrap_eth_params();
      wrap_eth_event_ui(
         ctx.clone(),
         chain,
         theme,
         icons.clone(),
         params,
         ui,
      );
   }

   if action.is_unwrap_weth() {
      let params = action.unwrap_weth_params();
      unwrap_weth_event_ui(
         ctx.clone(),
         chain,
         theme,
         icons.clone(),
         params,
         ui,
      );
   }

   if action.is_uniswap_position_op() {
      let params = action.uniswap_position_params();
      uniswap_position_op_event_ui(
         ctx.clone(),
         chain,
         theme,
         icons.clone(),
         params,
         ui,
      );
   }

   if action.is_bridge() {
      let params = action.bridge_params();
      bridge_event_ui(
         ctx.clone(),
         chain,
         theme,
         icons.clone(),
         params,
         ui,
      );
   }

   if action.is_swap() {
      let params = action.swap_params();
      swap_event_ui(theme, icons.clone(), params, ui);
   }

   if action.is_eoa_delegate() {
      let params = action.eoa_delegate_params();
      eoa_delegate_event_ui(ctx.clone(), chain, theme, params, ui);
   }
}
