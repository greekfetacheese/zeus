use egui::{
   Align, Align2, Button, Frame, Layout, Margin, Order, RichText, TextEdit, Ui, Window, vec2,
};
use egui_theme::Theme;
use egui_widgets::Label;

use crate::assets::icons::Icons;
use crate::core::{
   TransactionRich, ZeusCtx,
   transaction::*,
   tx_analysis::TransactionAnalysis,
   utils::{estimate_tx_cost, truncate_address, truncate_hash},
};
use zeus_eth::{
   alloy_primitives::{Address, TxHash, U256},
   currency::{Currency, ERC20Token, NativeCurrency},
   types::ChainId,
   utils::NumericValue,
};

use std::sync::Arc;

pub struct TxConfirmationWindow {
   open: bool,
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
         size: (500.0, 550.0),
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn close(&mut self) {
      self.open = false;
   }

   pub fn reset(&mut self) {
      *self = Self::new();
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
         estimate_tx_cost(ctx.clone(), chain.id(), gas_used, fee.wei2());
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
                        RichText::new("Transaction Analysis not found")
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

                  // Show ETH sent
                  if !analysis.value_sent().is_zero()
                     && !analysis.is_native_transfer()
                     && !analysis.is_wrap_eth()
                     && !analysis.is_swap()
                  {
                     ui.add(Label::new(
                        RichText::new("You will spend").size(theme.text_sizes.large),
                        None,
                     ));

                     eth_spent(
                        self.chain.id(),
                        analysis.value_sent(),
                        analysis.value_sent_usd(ctx.clone()),
                        theme,
                        icons.clone(),
                        ui,
                     );
                  }

                  // Show ETH received
                  if !analysis.eth_received().is_zero()
                     && !analysis.is_unwrap_weth()
                     && !analysis.is_swap()
                  {
                     ui.add(Label::new(
                        RichText::new("You will receive").size(theme.text_sizes.large),
                        None,
                     ));

                     eth_received(
                        self.chain.id(),
                        analysis.eth_received(),
                        analysis.eth_received_usd(ctx.clone()),
                        theme,
                        icons.clone(),
                        ui,
                     );
                  }

                  // Action Name
                  ui.label(
                     RichText::new(action.name())
                        .size(theme.text_sizes.heading)
                        .strong(),
                  );

                  if action.is_transfer() {
                     let params = action.transfer_params();
                     transfer_event_ui(
                        ctx.clone(),
                        self.chain,
                        theme,
                        icons.clone(),
                        params,
                        ui,
                     );
                  }

                  if action.is_erc20_transfer() {
                     let params = action.erc20_transfer_params();
                     erc20_transfer_event_ui(
                        ctx.clone(),
                        self.chain,
                        theme,
                        icons.clone(),
                        params,
                        ui,
                     );
                  }

                  if action.is_token_approval() {
                     let params = action.token_approval_params();
                     token_approval_event_ui(self.chain, theme, icons.clone(), params, ui);
                  }

                  if action.is_wrap_eth() {
                     let params = action.wrap_eth_params();
                     wrap_eth_event_ui(self.chain, theme, icons.clone(), params, ui);
                  }

                  if action.is_unwrap_weth() {
                     let params = action.unwrap_weth_params();
                     unwrap_weth_event_ui(self.chain, theme, icons.clone(), params, ui);
                  }

                  if action.is_uniswap_position_op() {
                     let params = action.uniswap_position_params();
                     uniswap_position_op_event_ui(theme, icons.clone(), params, ui);
                  }

                  if action.is_bridge() {
                     let params = action.bridge_params();
                     bridge_event_ui(theme, icons.clone(), &params, ui);
                  }

                  if action.is_swap() {
                     let params = action.swap_params();
                     swap_event_ui(theme, icons.clone(), &params, ui);
                  }

                  ui.add_space(20.0);

                  // Show the Chain we interacted with
                  chain_ui(self.chain, theme, icons.clone(), ui);

                  // Contract interaction
                  if analysis.contract_interact {
                     contract_interact(self.chain, analysis.interact_to, theme, ui);
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
                  transaction_cost(
                     self.chain,
                     &self.tx_cost,
                     &self.tx_cost_usd,
                     theme,
                     ui,
                  );

                  ui.add_space(20.0);

                  let sufficient_balance = self.sufficient_balance(
                     ctx.clone(),
                     analysis.value_sent().wei2(),
                     analysis.sender,
                  );

                  // Ajdust Priority Fee
                  ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                     let text = format!("Priority Fee (Gwei)");
                     ui.label(RichText::new(text).size(theme.text_sizes.normal));
                  });

                  ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                     if self.chain.is_bsc() {
                        ui.disable();
                     }
                     ui.set_width(ui.available_width() * 0.2);
                     ui.add(
                        TextEdit::singleline(&mut self.priority_fee)
                           .margin(Margin::same(10))
                           .background_color(theme.colors.text_edit_bg)
                           .font(egui::FontId::proportional(
                              theme.text_sizes.normal,
                           )),
                     );
                  });

                  // Adjust Gas Limit
                  ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                     let text = format!("Gas Limit");
                     ui.label(RichText::new(text).size(theme.text_sizes.normal));
                  });

                  ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                     ui.set_width(ui.available_width() * 0.2);

                     ui.add(
                        TextEdit::singleline(&mut self.adjusted_gas_limit)
                           .margin(Margin::same(10))
                           .background_color(theme.colors.text_edit_bg)
                           .font(egui::FontId::proportional(
                              theme.text_sizes.normal,
                           )),
                     );
                  });

                  let base_case = self.chain.is_ethereum() && !action.is_other() && action.is_mev_vulnerable();
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

                        let button =
                           Button::new(RichText::new("Confirm").size(theme.text_sizes.normal))
                              .min_size(button_size);

                        if ui.add_enabled(sufficient_balance, button).clicked() {
                           self.confirmed_or_rejected = Some(true);
                           self.close();
                        }

                        let button =
                           Button::new(RichText::new("Reject").size(theme.text_sizes.normal))
                              .min_size(button_size);

                        if ui.add(button).clicked() {
                           self.confirmed_or_rejected = Some(false);
                           self.close();
                        }
                     });
                  });
               });
            });
         });
   }

   fn sufficient_balance(&self, ctx: ZeusCtx, eth_spent: U256, sender: Address) -> bool {
      let balance = ctx.get_eth_balance(self.chain.id(), sender);
      let total_cost = eth_spent + self.tx_cost.wei2();
      balance.wei2() >= total_cost
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
         size: (500.0, 550.0),
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
                  let chain: ChainId = tx.chain.into();

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

                  // Show ETH received
                  if !tx.eth_received.is_zero()
                     && !tx.analysis.is_unwrap_weth()
                     && !tx.analysis.is_swap()
                  {
                     ui.add(Label::new(
                        RichText::new("You received").size(theme.text_sizes.large),
                        None,
                     ));

                     eth_received(
                        tx.chain,
                        tx.eth_received.clone(),
                        tx.eth_received_usd.clone(),
                        theme,
                        icons.clone(),
                        ui,
                     );
                  }

                  // Action Name
                  ui.label(
                     RichText::new(action.name())
                        .size(theme.text_sizes.large)
                        .strong(),
                  );

                  if action.is_transfer() {
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

                  if action.is_erc20_transfer() {
                     let params = action.erc20_transfer_params();
                     erc20_transfer_event_ui(
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
                     token_approval_event_ui(chain, theme, icons.clone(), params, ui);
                  }

                  if action.is_wrap_eth() {
                     let params = action.wrap_eth_params();
                     wrap_eth_event_ui(chain, theme, icons.clone(), params, ui);
                  }

                  if action.is_unwrap_weth() {
                     let params = action.unwrap_weth_params();
                     unwrap_weth_event_ui(chain, theme, icons.clone(), params, ui);
                  }

                  if action.is_uniswap_position_op() {
                     let params = action.uniswap_position_params();
                     uniswap_position_op_event_ui(theme, icons.clone(), params, ui);
                  }

                  if action.is_bridge() {
                     let params = action.bridge_params();
                     bridge_event_ui(theme, icons.clone(), &params, ui);
                  }

                  if action.is_swap() {
                     let params = action.swap_params();
                     swap_event_ui(theme, icons.clone(), &params, ui);
                  }

                  // Show the Chain we interacted with
                  chain_ui(chain, theme, icons.clone(), ui);

                  // Contract interaction
                  if tx.contract_interact {
                     contract_interact(chain, tx.interact_to(), theme, ui);
                  }

                  value(
                     ctx.clone(),
                     chain,
                     tx.value_sent.clone(),
                     theme,
                     ui,
                  );

                  transaction_cost(chain, &tx.tx_cost, &tx.tx_cost_usd, theme, ui);

                  tx_hash(tx.chain.into(), &tx.hash, theme, ui);

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

pub fn transaction_cost(
   chain: ChainId,
   eth_cost: &NumericValue,
   eth_cost_usd: &NumericValue,
   theme: &Theme,
   ui: &mut Ui,
) {
   let native_currency = NativeCurrency::from(chain.id());
   let size = vec2(ui.available_width() * 0.9, 30.0);
   ui.allocate_ui(size, |ui| {
      ui.horizontal(|ui| {
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            let cost = format!(
               "Cost {} {}",
               eth_cost.formatted(),
               native_currency.symbol
            );
            ui.label(RichText::new(cost).size(theme.text_sizes.normal));
         });

         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            ui.label(
               RichText::new(format!("~ ${}", eth_cost_usd.formatted()))
                  .size(theme.text_sizes.normal),
            );
         });
      });
   });
}

pub fn tx_hash(chain: ChainId, tx_hash: &TxHash, theme: &Theme, ui: &mut Ui) {
   let size = vec2(ui.available_width() * 0.9, 30.0);
   ui.allocate_ui(size, |ui| {
      ui.horizontal(|ui| {
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            let text = "Transaction hash";
            ui.label(RichText::new(text).size(theme.text_sizes.normal));
         });

         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            let hash_str = truncate_hash(tx_hash.to_string());
            let explorer = chain.block_explorer();
            let link = format!("{}/tx/{}", explorer, tx_hash.to_string());
            ui.hyperlink_to(
               RichText::new(hash_str)
                  .size(theme.text_sizes.normal)
                  .color(theme.colors.hyperlink_color),
               link,
            );
         });
      });
   });
}

pub fn value(ctx: ZeusCtx, chain: ChainId, value: NumericValue, theme: &Theme, ui: &mut Ui) {
   let eth = Currency::from(NativeCurrency::from(chain.id()));
   let size = vec2(ui.available_width() * 0.9, 30.0);
   ui.allocate_ui(size, |ui| {
      ui.horizontal(|ui| {
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            let value = value.format_abbreviated();
            let text = format!("Value {} {}", value, eth.symbol());
            ui.label(RichText::new(text).size(theme.text_sizes.normal));
         });

         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            let value_usd = ctx.get_currency_value_for_amount(value.f64(), &eth);
            ui.label(
               RichText::new(format!("~ ${}", value_usd.format_abbreviated()))
                  .size(theme.text_sizes.normal),
            );
         });
      });
   });
}

pub fn contract_interact(chain: ChainId, interact_to: Address, theme: &Theme, ui: &mut Ui) {
   let size = vec2(ui.available_width() * 0.9, 30.0);
   ui.allocate_ui(size, |ui| {
      ui.horizontal(|ui| {
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            let text = RichText::new("Contract interaction").size(theme.text_sizes.normal);
            ui.label(text);
         });

         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            let interact_addr = interact_to.to_string();
            let interact_short = truncate_address(interact_addr.clone());
            let explorer = chain.block_explorer();
            let link = format!("{}/address/{}", explorer, interact_addr);

            ui.hyperlink_to(
               RichText::new(interact_short)
                  .size(theme.text_sizes.normal)
                  .color(theme.colors.hyperlink_color),
               link,
            );
         });
      });
   });
}

pub fn chain_ui(chain: ChainId, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
   let size = vec2(ui.available_width() * 0.9, 30.0);
   ui.allocate_ui(size, |ui| {
      ui.horizontal(|ui| {
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.label(RichText::new("Chain").size(theme.text_sizes.normal));
         });

         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            let icon = icons.chain_icon(chain.id());
            let label = Label::new(
               RichText::new(chain.name()).size(theme.text_sizes.normal),
               Some(icon),
            )
            .image_on_left();
            ui.add(label);
         });
      });
   });
}

pub fn eth_spent(
   chain: u64,
   eth_spent: NumericValue,
   eth_spent_usd: NumericValue,
   theme: &Theme,
   icons: Arc<Icons>,
   ui: &mut Ui,
) {
   let native = NativeCurrency::from(chain);
   let icon = icons.native_currency_icon_x24(chain);
   let text = format!(
      "{} {} ≈ {}",
      eth_spent.format_abbreviated(),
      native.symbol,
      eth_spent_usd.format_abbreviated()
   );
   let text = RichText::new(text).size(theme.text_sizes.normal);
   ui.add(Label::new(text, Some(icon)).image_on_left());
}

pub fn eth_received(
   chain: u64,
   eth_received: NumericValue,
   eth_received_usd: NumericValue,
   theme: &Theme,
   icons: Arc<Icons>,
   ui: &mut Ui,
) {
   let native = NativeCurrency::from(chain);
   let icon = icons.native_currency_icon_x24(chain);
   let text = format!(
      "You will receive {} {} ≈ {}",
      eth_received.format_abbreviated(),
      native.symbol,
      eth_received_usd.format_abbreviated()
   );
   let text = RichText::new(text).size(theme.text_sizes.normal);
   ui.add(Label::new(text, Some(icon)).image_on_left());
}

pub fn token_approval_event_ui(
   chain: ChainId,
   theme: &Theme,
   icons: Arc<Icons>,
   params: &TokenApproveParams,
   ui: &mut Ui,
) {
   let spender_addr = params.spender.to_string();
   let spender_short = truncate_address(spender_addr.clone());
   let explorer = chain.block_explorer();
   let link = format!("{}/address/{}", explorer, spender_addr);

   let token_details = params
      .token
      .iter()
      .zip(params.amount.iter())
      .zip(params.amount_usd.iter());

   for ((token, amount), amount_usd) in token_details {
      let is_unlimited = amount.wei2() == U256::MAX;
      let amount = if is_unlimited {
         "Unlimited".to_string()
      } else {
         amount.format_abbreviated()
      };

      let show_usd_value = if !is_unlimited && amount_usd.is_some() {
         true
      } else {
         false
      };

      let icon = icons.currency_icon_x24(&Currency::from(token.clone()));
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

   // Spender
   let size = vec2(ui.available_width() * 0.9, 30.0);
   ui.allocate_ui(size, |ui| {
      ui.horizontal(|ui| {
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            let text = RichText::new("Spender").size(theme.text_sizes.normal);
            ui.label(text);
         });

         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            ui.hyperlink_to(
               RichText::new(spender_short)
                  .size(theme.text_sizes.normal)
                  .color(theme.colors.hyperlink_color),
               link,
            );
         });
      });
   });
}

fn transfer_event_ui(
   ctx: ZeusCtx,
   chain: ChainId,
   theme: &Theme,
   icons: Arc<Icons>,
   params: &TransferParams,
   ui: &mut Ui,
) {
   let size = vec2(ui.available_width() * 0.9, 30.0);

   // Currency to Send
   ui.allocate_ui(size, |ui| {
      ui.horizontal(|ui| {
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            let currency = &params.currency;
            let amount = &params.amount;
            let icon = icons.currency_icon_x24(&currency);
            let text = RichText::new(&format!(
               "{} {} ",
               amount.format_abbreviated(),
               currency.symbol()
            ))
            .size(theme.text_sizes.normal);
            let label = Label::new(text, Some(icon)).image_on_left();
            ui.add(label);
         });

         // Value
         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            let amount = params.amount_usd.clone().unwrap_or_default();
            ui.label(
               RichText::new(&format!("~ ${}", amount.format_abbreviated()))
                  .size(theme.text_sizes.normal),
            );
         });
      });
   });

   // Recipient
   ui.allocate_ui(size, |ui| {
      ui.horizontal(|ui| {
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.label(RichText::new("Recipient").size(theme.text_sizes.large));
         });

         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            let recipient_address = params.recipient;
            let recipient_short = truncate_address(recipient_address.to_string());
            let explorer = chain.block_explorer();
            let link = format!(
               "{}/address/{}",
               explorer,
               recipient_address.to_string()
            );
            let contact = ctx.get_contact_by_address(&recipient_address.to_string());
            let wallet = ctx.get_wallet_info(recipient_address);
            let recipient = if contact.is_some() {
               contact.unwrap().name
            } else if wallet.is_some() {
               wallet.unwrap().name
            } else {
               recipient_short
            };
            ui.hyperlink_to(
               RichText::new(recipient)
                  .size(theme.text_sizes.normal)
                  .color(theme.colors.hyperlink_color),
               link,
            );
         });
      });
   });
}

fn erc20_transfer_event_ui(
   ctx: ZeusCtx,
   chain: ChainId,
   theme: &Theme,
   icons: Arc<Icons>,
   params: &ERC20TransferParams,
   ui: &mut Ui,
) {
   let size = vec2(ui.available_width() * 0.9, 30.0);
   // token to Send
   ui.allocate_ui(size, |ui| {
      ui.horizontal(|ui| {
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            let token = &params.token;
            let amount = &params.amount;
            let icon = icons.token_icon_x24(token.address, token.chain_id);
            let text = RichText::new(&format!(
               "{} {} ",
               amount.format_abbreviated(),
               token.symbol
            ))
            .size(theme.text_sizes.normal);
            let label = Label::new(text, Some(icon)).image_on_left();
            ui.add(label);
         });

         // Value
         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            let amount = params.amount_usd.clone().unwrap_or_default();
            ui.label(
               RichText::new(&format!("~ ${}", amount.format_abbreviated()))
                  .size(theme.text_sizes.normal),
            );
         });
      });
   });

   // Recipient
   ui.allocate_ui(size, |ui| {
      ui.horizontal(|ui| {
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.label(RichText::new("Recipient").size(theme.text_sizes.large));
         });

         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            let recipient_address = params.recipient;
            let recipient_short = truncate_address(recipient_address.to_string());
            let explorer = chain.block_explorer();
            let link = format!(
               "{}/address/{}",
               explorer,
               recipient_address.to_string()
            );
            let contact = ctx.get_contact_by_address(&recipient_address.to_string());
            let wallet = ctx.get_wallet_info(recipient_address);
            let recipient = if contact.is_some() {
               contact.unwrap().name
            } else if wallet.is_some() {
               wallet.unwrap().name
            } else {
               recipient_short
            };
            ui.hyperlink_to(
               RichText::new(recipient)
                  .size(theme.text_sizes.normal)
                  .color(theme.colors.hyperlink_color),
               link,
            );
         });
      });
   });
}

fn bridge_event_ui(theme: &Theme, icons: Arc<Icons>, params: &BridgeParams, ui: &mut Ui) {
   let size = vec2(ui.available_width() * 0.9, 30.0);

   // Input currency column
   ui.allocate_ui(size, |ui| {
      ui.horizontal(|ui| {
         let currency = &params.input_currency;
         let amount = &params.amount;
         let icon = icons.currency_icon_x24(&currency);
         let text = RichText::new(&format!(
            "- {} {} ",
            amount.format_abbreviated(),
            currency.symbol()
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
               RichText::new(&format!("~ ${}", value.format_abbreviated()))
                  .size(theme.text_sizes.normal),
            );
         });
      });
   });

   // Received Currency
   ui.allocate_ui(size, |ui| {
      ui.horizontal(|ui| {
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            let currency = &params.output_currency;
            let amount = &params.received;
            let icon = icons.currency_icon_x24(&currency);
            let text = RichText::new(format!(
               "+ {} {}",
               amount.format_abbreviated(),
               currency.symbol()
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
   });

   // Origin Chain Column
   ui.allocate_ui(size, |ui| {
      ui.horizontal(|ui| {
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.label(RichText::new("Origin Chain").size(theme.text_sizes.normal));
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
   });

   // Destination Chain Column
   ui.allocate_ui(size, |ui| {
      ui.horizontal(|ui| {
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.label(RichText::new("Destination Chain").size(theme.text_sizes.normal));
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
   });
}

fn swap_event_ui(theme: &Theme, icons: Arc<Icons>, params: &SwapParams, ui: &mut Ui) {
   let size = vec2(ui.available_width() * 0.9, 30.0);

   // Input currency column
   ui.allocate_ui(size, |ui| {
      ui.horizontal(|ui| {
         let currency = &params.input_currency;
         let amount = &params.amount_in;
         let icon = icons.currency_icon_x24(&currency);
         let text = RichText::new(&format!(
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
               RichText::new(&format!("~ ${}", value.format_abbreviated()))
                  .size(theme.text_sizes.large),
            );
         });
      });
   });

   // Received Currency
   ui.allocate_ui(size, |ui| {
      ui.horizontal(|ui| {
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            let currency = &params.output_currency;
            let amount = &params.received;
            let icon = icons.currency_icon_x24(&currency);
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
   });

   // Minimum Received
   let amount = params.min_received.clone();
   let amount_usd = params.min_received_usd.clone();
   if amount.is_some() && amount_usd.is_some() {
      ui.allocate_ui(size, |ui| {
         ui.horizontal(|ui| {
            ui.label(RichText::new("Minimum Received").size(theme.text_sizes.large));
            ui.add_space(15.0);

            let amount = amount.unwrap();
            let amount_usd = amount_usd.unwrap();
            let currency = &params.output_currency;
            let icon = icons.currency_icon_x24(&currency);
            let amount_symbol = format!(
               "{} {}",
               amount.format_abbreviated(),
               currency.symbol()
            );
            let amount_usd = format!("~ ${}", amount_usd.format_abbreviated());
            let text = RichText::new(format!("{} {}", amount_symbol, amount_usd))
               .size(theme.text_sizes.large);
            let label = Label::new(text, Some(icon)).image_on_left();
            ui.add(label);
         });
      });
   }
}

fn wrap_eth_event_ui(
   chain: ChainId,
   theme: &Theme,
   icons: Arc<Icons>,
   params: &WrapETHParams,
   ui: &mut Ui,
) {
   let weth = Currency::from(ERC20Token::wrapped_native_token(chain.id()));
   let weth_icon = icons.currency_icon(&weth);

   let size = vec2(ui.available_width() * 0.9, 30.0);
   ui.allocate_ui(size, |ui| {
      ui.horizontal(|ui| {
         let text = RichText::new(&format!(
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
            let text = RichText::new(&format!(
               "~ ${}",
               weth_received_usd.format_abbreviated()
            ))
            .size(theme.text_sizes.normal);
            ui.label(text);
         });
      });
   });
}

fn unwrap_weth_event_ui(
   chain: ChainId,
   theme: &Theme,
   icons: Arc<Icons>,
   params: &UnwrapWETHParams,
   ui: &mut Ui,
) {
   let eth = NativeCurrency::from(chain.id());
   let eth_icon = icons.native_currency_icon(chain.id());

   let size = vec2(ui.available_width() * 0.9, 30.0);
   ui.allocate_ui(size, |ui| {
      ui.horizontal(|ui| {
         let text = RichText::new(&format!(
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
            let text = RichText::new(&format!(
               "~ ${}",
               weth_unwrapped_usd.format_abbreviated()
            ))
            .size(theme.text_sizes.normal);
            ui.label(text);
         });
      });
   });
}

fn uniswap_position_op_event_ui(
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

   let size = vec2(ui.available_width() * 0.9, 30.0);

   // Currency A and Amount & value
   ui.allocate_ui(size, |ui| {
      ui.horizontal(|ui| {
         let icon = icons.currency_icon(&currency0);

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
            ui.label(RichText::new(&format!("~ ${}", amount)).size(theme.text_sizes.normal));
         });
      });
   });

   // Currency B and Amount & value
   ui.allocate_ui(size, |ui| {
      ui.horizontal(|ui| {
         let icon = icons.currency_icon(&currency1);
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
            ui.label(RichText::new(&format!("~ ${}", amount)).size(theme.text_sizes.normal));
         });
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
      ui.allocate_ui(size, |ui| {
         ui.horizontal(|ui| {
            let icon = icons.currency_icon(&currency0);
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
               ui.label(RichText::new(&format!("~ ${}", amount)).size(theme.text_sizes.normal));
            });
         });
      });
   }

   // Minimum Amount B and Amount & value
   if min_amount1.is_some() {
      let min_amount1 = min_amount1.unwrap();
      ui.allocate_ui(size, |ui| {
         ui.horizontal(|ui| {
            let icon = icons.currency_icon(&currency1);
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
               ui.label(RichText::new(&format!("~ ${}", amount)).size(theme.text_sizes.normal));
            });
         });
      });
   }
}
