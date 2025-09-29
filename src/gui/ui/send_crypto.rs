use eframe::egui::{Align2, Button, FontId, Frame, Margin, RichText, TextEdit, Ui, Window, vec2};

use std::str::FromStr;
use std::sync::Arc;

use crate::core::{
   TransactionAnalysis, TransferParams, ZeusCtx,
   utils::{RT, estimate_tx_cost, eth},
};

use crate::assets::icons::Icons;
use crate::gui::{
   SHARED_GUI,
   ui::{
      ContactsUi, RecipientSelectionWindow, TokenSelectionWindow,
      dapps::amount_field_with_currency_selector,
   },
};
use egui_theme::Theme;

use zeus_eth::{
   alloy_primitives::{Address, Bytes, U256},
   alloy_provider::Provider,
   alloy_rpc_types::BlockId,
   amm::uniswap::DexKind,
   currency::{Currency, NativeCurrency},
   revm_utils::{ForkFactory, new_evm, simulate},
   types::ChainId,
   utils::NumericValue,
};

pub struct SendCryptoUi {
   open: bool,
   pub currency: Currency,
   pub amount: String,
   pub recipient: String,
   pub recipient_name: Option<String>,
   pub search_query: String,
   pub size: (f32, f32),
   pub pool_data_syncing: bool,
   pub syncing_balance: bool,
}

impl SendCryptoUi {
   pub fn new() -> Self {
      Self {
         open: false,
         currency: Currency::from(NativeCurrency::from_chain_id(1).unwrap()),
         amount: String::new(),
         recipient: String::new(),
         recipient_name: None,
         search_query: String::new(),
         size: (500.0, 500.0),
         pool_data_syncing: false,
         syncing_balance: false,
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self) {
      self.open = true;
   }

   pub fn close(&mut self) {
      self.open = false;
      self.clear_recipient();
      self.clear_amount();
      self.clear_search_query();
   }

   pub fn set_currency(&mut self, currency: Currency) {
      self.currency = currency;
   }

   pub fn clear_recipient(&mut self) {
      self.recipient_name = None;
      self.recipient = String::new();
   }

   pub fn clear_amount(&mut self) {
      self.amount = String::new();
   }

   pub fn clear_search_query(&mut self) {
      self.search_query = String::new();
   }

   pub fn show(
      &mut self,
      ctx: ZeusCtx,
      icons: Arc<Icons>,
      theme: &Theme,
      token_selection: &mut TokenSelectionWindow,
      recipient_selection: &mut RecipientSelectionWindow,
      contacts_ui: &mut ContactsUi,
      ui: &mut Ui,
   ) {
      if !self.open {
         return;
      }

      let frame = theme.frame1;
      Window::new("send_crypto_ui")
         .title_bar(false)
         .resizable(false)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 30.0))
         .frame(frame)
         .show(ui.ctx(), |ui| {
            ui.vertical_centered(|ui| {
               Frame::new().inner_margin(Margin::same(10)).show(ui, |ui| {
                  ui.set_width(self.size.0);
                  ui.set_max_height(self.size.1);
                  ui.spacing_mut().item_spacing = vec2(0.0, 15.0);
                  ui.spacing_mut().button_padding = vec2(10.0, 8.0);

                  ui.label(RichText::new("Send Crypto").size(theme.text_sizes.heading));

                  let owner = ctx.current_wallet_address();

                  let chain = ctx.chain();
                  let currencies = ctx.get_currencies(chain.id());
                  let inner_frame = theme.frame2;

                  // Currency Selection
                  let label = String::from("Send");
                  let balance_fn = || ctx.get_currency_balance(chain.id(), owner, &self.currency);
                  let cost = self.cost(ctx.clone());
                  let balance = balance_fn();

                  let max_amount = || {
                     let currency = self.currency.clone();
                     if currency.is_erc20() {
                        return balance;
                     } else {
                        if balance.wei() < cost.wei() {
                           return NumericValue::default();
                        }
                        let max = balance.wei() - cost.wei();
                        NumericValue::format_wei(max, currency.decimals())
                     }
                  };

                  let amount = self.amount.clone();
                  let data_syncing = self.pool_data_syncing;
                  let currency = self.currency.clone();
                  let value = || value(ctx.clone(), currency, owner, amount, data_syncing);

                  inner_frame.show(ui, |ui| {
                     amount_field_with_currency_selector(
                        ctx.clone(),
                        theme,
                        icons.clone(),
                        Some(label),
                        &self.currency,
                        &mut self.amount,
                        Some(token_selection),
                        None,
                        balance_fn,
                        max_amount,
                        value,
                        data_syncing,
                        ui,
                     );
                  });

                  token_selection.show(
                     ctx.clone(),
                     theme,
                     icons.clone(),
                     chain.id(),
                     owner,
                     &currencies,
                     ui,
                  );

                  if let Some(currency) = token_selection.get_currency() {
                     self.currency = currency.clone();
                     token_selection.reset();
                     self.sync_balance(owner, ctx.clone());
                  }

                  ui.add_space(10.0);

                  recipient_selection.show(ctx.clone(), theme, icons.clone(), contacts_ui, ui);
                  let recipient = recipient_selection.get_recipient();
                  let recipient_name = recipient_selection.get_recipient_name();

                  // Recipient Selection
                  inner_frame.show(ui, |ui| {
                     ui.set_width(ui.available_width());
                     ui.horizontal(|ui| {
                        ui.label(RichText::new("Recipient  ").size(theme.text_sizes.large));
                        if !recipient.is_empty() {
                           if let Some(name) = &recipient_name {
                              ui.label(
                                 RichText::new(name.clone()).size(theme.text_sizes.normal).strong(),
                              );
                           } else {
                              ui.label(
                                 RichText::new("Unknown Address")
                                    .size(theme.text_sizes.normal)
                                    .color(theme.colors.error_color),
                              );
                           }
                        }
                     });

                     ui.horizontal(|ui| {
                        let hint = RichText::new("Search contacts or enter an address")
                           .size(theme.text_sizes.normal)
                           .color(theme.colors.text_secondary);
                        let res = ui.add(
                           TextEdit::singleline(&mut recipient_selection.recipient)
                              .hint_text(hint)
                              .min_size(vec2(ui.available_width() * 0.9, 25.0))
                              .margin(Margin::same(10))
                              .background_color(theme.colors.text_edit_bg)
                              .font(FontId::proportional(theme.text_sizes.large)),
                        );
                        if res.clicked() {
                           recipient_selection.open();
                        }
                     });
                  });

                  ui.add_space(10.0);

                  self.send_button(ctx.clone(), theme, owner, recipient, ui);
               });
            });
         });
   }

   fn send_button(
      &mut self,
      ctx: ZeusCtx,
      theme: &Theme,
      owner: Address,
      recipient: String,
      ui: &mut Ui,
   ) {
      let recipient_is_sender = self.recipient_is_sender(owner, &recipient);
      let valid_recipient = self.valid_recipient(&recipient);
      let valid_amount = self.valid_amount();
      let has_balance = self.sufficient_balance(ctx.clone(), owner);
      let has_entered_amount = !self.amount.is_empty();
      let has_entered_recipient = !recipient.is_empty();
      let valid_inputs =
         valid_recipient && !recipient_is_sender && has_balance && has_entered_amount;

      let mut button_text = "Send".to_string();

      if has_entered_amount && !valid_amount {
         button_text = "Invalid Amount".to_string();
      }

      if has_entered_recipient && !valid_recipient {
         button_text = "Invalid Recipient".to_string();
      }

      if has_entered_recipient && recipient_is_sender {
         button_text = "Cannot send to yourself".to_string();
      }

      if !has_balance {
         button_text = format!("Insufficient {} Balance", self.currency.symbol());
      }

      let text = RichText::new(button_text).size(theme.text_sizes.large);
      let send = Button::new(text).min_size(vec2(ui.available_width() * 0.8, 45.0));

      if ui.add_enabled(valid_inputs, send).clicked() {
         match self.send_transaction(ctx, recipient) {
            Ok(_) => {}
            Err(e) => {
               RT.spawn_blocking(move || {
                  SHARED_GUI.write(|gui| {
                     gui.open_msg_window("Error while sending transaction", e.to_string());
                  });
               });
            }
         }
      }
   }

   fn sync_balance(&mut self, owner: Address, ctx: ZeusCtx) {
      self.syncing_balance = true;
      let currency = self.currency.clone();
      let chain = currency.chain_id();
      RT.spawn(async move {
         let balance_manager = ctx.balance_manager();
         if currency.is_native() {
            match balance_manager.update_eth_balance(ctx.clone(), chain, vec![owner]).await {
               Ok(_) => {}
               Err(e) => {
                  tracing::error!("Failed to update ETH balance: {}", e);
               }
            }
         } else {
            let token = currency.to_erc20().into_owned();
            match balance_manager
               .update_tokens_balance(ctx.clone(), chain, owner, vec![token])
               .await
            {
               Ok(_) => {}
               Err(e) => {
                  tracing::error!("Failed to update token balance: {}", e);
               }
            }
         }
         SHARED_GUI.write(|gui| {
            gui.send_crypto.syncing_balance = false;
         });
      });
   }

   fn cost(&self, ctx: ZeusCtx) -> NumericValue {
      let gas_used = if self.currency.is_native() {
         ctx.chain().transfer_gas()
      } else {
         ctx.chain().erc20_transfer_gas()
      };

      let fee = ctx.get_priority_fee(ctx.chain().id()).unwrap_or_default();
      let (cost_in_wei, _) = estimate_tx_cost(ctx.clone(), ctx.chain().id(), gas_used, fee.wei());
      cost_in_wei
   }

   fn valid_recipient(&self, recipient: &str) -> bool {
      let recipient = Address::from_str(recipient).unwrap_or(Address::ZERO);
      recipient != Address::ZERO
   }

   fn recipient_is_sender(&self, owner: Address, recipient: &str) -> bool {
      let recipient = Address::from_str(recipient).unwrap_or(Address::ZERO);
      recipient == owner
   }

   fn valid_amount(&self) -> bool {
      let amount = self.amount.parse().unwrap_or(0.0);
      amount > 0.0
   }

   fn sufficient_balance(&self, ctx: ZeusCtx, sender: Address) -> bool {
      let balance = ctx.get_currency_balance(ctx.chain().id(), sender, &self.currency);
      let amount = NumericValue::parse_to_wei(&self.amount, self.currency.decimals());
      balance.wei() >= amount.wei()
   }

   fn send_transaction(&mut self, ctx: ZeusCtx, recipient: String) -> Result<(), anyhow::Error> {
      let chain = ctx.chain();
      let from = ctx.current_wallet_address();
      let currency = self.currency.clone();
      let recipient_address = Address::from_str(&recipient)?;
      let amount = NumericValue::parse_to_wei(&self.amount, self.currency.decimals());

      RT.spawn(async move {
         SHARED_GUI.write(|gui| {
            gui.loading_window.open("Wait while magic happens");
            gui.request_repaint();
         });

         if currency.is_native() {
            match send_eth(
               ctx.clone(),
               chain,
               from,
               recipient_address,
               amount,
               currency,
            )
            .await
            {
               Ok(_) => {
                  SHARED_GUI.write(|gui| {
                     gui.send_crypto.clear_amount();
                  });
               }
               Err(e) => {
                  tracing::error!("Error sending transaction: {:?}", e);
                  SHARED_GUI.write(|gui| {
                     gui.notification.reset();
                     gui.loading_window.reset();
                     gui.msg_window.open("Transaction Error", e.to_string());
                  });
               }
            }
         } else {
            match send_token(
               ctx.clone(),
               chain,
               from,
               recipient_address,
               currency,
               amount,
            )
            .await
            {
               Ok(_) => {
                  SHARED_GUI.write(|gui| {
                     gui.send_crypto.clear_amount();
                  });
               }
               Err(e) => {
                  tracing::error!("Error sending transaction: {:?}", e);
                  SHARED_GUI.write(|gui| {
                     gui.notification.reset();
                     gui.loading_window.reset();
                     gui.msg_window.open("Transaction Error", e.to_string());
                  });
               }
            }
         }
      });
      Ok(())
   }
}

fn value(
   ctx: ZeusCtx,
   currency: Currency,
   owner: Address,
   amount: String,
   pool_data_syncing: bool,
) -> NumericValue {
   let price = ctx.get_currency_price(&currency);
   let amount = amount.parse().unwrap_or(0.0);

   if price.f64() != 0.0 {
      return NumericValue::value(amount, price.f64());
   } else {
      // no pool data available to calculate the price

      if pool_data_syncing {
         return NumericValue::default();
      }

      let pools = ctx.write(|ctx| ctx.pool_manager.get_pools_that_have_currency(&currency));
      let chain_id = ctx.chain().id();

      if pools.is_empty() {
         let token = currency.to_erc20().into_owned();
         let manager = ctx.pool_manager();
         let dexes = DexKind::main_dexes(chain_id);

         RT.spawn(async move {
            SHARED_GUI.write(|gui| {
               gui.send_crypto.pool_data_syncing = true;
            });

            match manager.sync_pools_for_tokens(ctx.clone(), vec![token], dexes).await {
               Ok(_) => {}
               Err(e) => {
                  tracing::error!("Error getting pools: {:?}", e);
               }
            };

            let pools = manager.get_pools_that_have_currency(&currency);
            match manager.update_state_for_pools(ctx.clone(), chain_id, pools).await {
               Ok(_) => {}
               Err(e) => {
                  tracing::error!("Error updating pool state: {:?}", e);
               }
            }

            SHARED_GUI.write(|gui| {
               gui.send_crypto.pool_data_syncing = false;
            });

            RT.spawn_blocking(move || {
               ctx.calculate_portfolio_value(chain_id, owner);
               ctx.save_pool_manager();
               ctx.save_portfolio_db();
            });
         });
      } else {
         RT.spawn(async move {
            SHARED_GUI.write(|gui| {
               gui.send_crypto.pool_data_syncing = true;
            });

            let manager = ctx.pool_manager();
            let pools = manager.get_pools_that_have_currency(&currency);
            match manager.update_state_for_pools(ctx.clone(), chain_id, pools).await {
               Ok(_) => {}
               Err(e) => {
                  tracing::error!("Error updating pool state: {:?}", e);
               }
            }
            SHARED_GUI.write(|gui| {
               gui.send_crypto.pool_data_syncing = false;
            });
         });
      }

      NumericValue::default()
   }
}

async fn send_eth(
   ctx: ZeusCtx,
   chain: ChainId,
   from: Address,
   recipient: Address,
   amount: NumericValue,
   currency: Currency,
) -> Result<(), anyhow::Error> {
   let mev_protect = false;
   let dapp = "".to_string();
   let interact_to = recipient;
   let value = amount.wei();
   let call_data = Bytes::default();
   let auth_list = Vec::new();

   let (_, _) = eth::send_transaction(
      ctx.clone(),
      dapp,
      None,
      None,
      chain,
      mev_protect,
      from,
      interact_to,
      call_data,
      value,
      auth_list,
   )
   .await?;

   match update_balances(ctx.clone(), chain.id(), currency, from, recipient).await {
      Ok(_) => {}
      Err(e) => {
         tracing::error!("Error updating balances: {:?}", e);
      }
   }

   Ok(())
}

async fn send_token(
   ctx: ZeusCtx,
   chain: ChainId,
   from: Address,
   recipient: Address,
   currency: Currency,
   amount: NumericValue,
) -> Result<(), anyhow::Error> {
   let token = currency.to_erc20().into_owned();

   let mev_protect = false;
   let dapp = "".to_string();
   let interact_to = token.address;
   let value = U256::ZERO;
   let call_data = token.encode_transfer(recipient, amount.wei());

   let client = ctx.get_client(chain.id()).await?;
   let eth_balance_before_fut = client.get_balance(from).into_future();
   let block = client.get_block(BlockId::latest()).await?;

   let factory = ForkFactory::new_sandbox_factory(client.clone(), chain.id(), None, None);
   let fork_db = factory.new_sandbox_fork();

   let mut transfer_params = TransferParams {
      currency: token.clone().into(),
      sender: from,
      recipient,
      ..Default::default()
   };

   let recipient_balance_before = token.balance_of(client.clone(), recipient, None).await?;

   let real_amount_sent;
   let gas_used;

   {
      let mut evm = new_evm(chain, block.as_ref(), fork_db);

      let res = simulate::transfer_token(
         &mut evm,
         token.address,
         from,
         recipient,
         amount.wei(),
         true,
      )?;

      let balance_after = simulate::erc20_balance(&mut evm, token.address, recipient)?;

      let real_amount = if balance_after > recipient_balance_before {
         balance_after - recipient_balance_before
      } else {
         U256::ZERO
      };

      real_amount_sent = real_amount;
      gas_used = res.gas_used();
   }

   let amount_usd = ctx.get_token_value_for_amount(amount.f64(), &token);
   let real_amount_sent = NumericValue::format_wei(real_amount_sent, token.decimals);
   let real_amount_send_usd = ctx.get_token_value_for_amount(real_amount_sent.f64(), &token);

   transfer_params.amount = amount;
   transfer_params.amount_usd = Some(amount_usd);
   transfer_params.real_amount_sent = Some(real_amount_sent);
   transfer_params.real_amount_sent_usd = Some(real_amount_send_usd);

   let eth_balance_before = eth_balance_before_fut.await?;

   let tx_analysis = TransactionAnalysis {
      chain: chain.id(),
      sender: from,
      interact_to,
      contract_interact: true,
      value,
      call_data: call_data.clone(),
      gas_used,
      eth_balance_before: eth_balance_before,
      eth_balance_after: eth_balance_before,
      decoded_selector: "ERC20 Transfer".to_string(),
      transfers: vec![transfer_params],
      logs_len: 1,
      known_events: 1,
      ..Default::default()
   };

   let auth_list = Vec::new();

   let (_, _) = eth::send_transaction(
      ctx.clone(),
      dapp,
      None,
      Some(tx_analysis),
      chain,
      mev_protect,
      from,
      interact_to,
      call_data,
      value,
      auth_list,
   )
   .await?;

   match update_balances(ctx.clone(), chain.id(), currency, from, recipient).await {
      Ok(_) => {}
      Err(e) => {
         tracing::error!("Error updating balances: {:?}", e);
      }
   }

   Ok(())
}

async fn update_balances(
   ctx: ZeusCtx,
   chain: u64,
   currency: Currency,
   sender: Address,
   recipient: Address,
) -> Result<(), anyhow::Error> {
   let exists = ctx.wallet_exists(recipient);
   let manager = ctx.balance_manager();

   manager.update_eth_balance(ctx.clone(), chain, vec![sender]).await?;

   if currency.is_erc20() {
      let token = currency.to_erc20().into_owned();
      manager.update_tokens_balance(ctx.clone(), chain, sender, vec![token]).await?;
   }

   if exists {
      if currency.is_native() {
         manager.update_eth_balance(ctx.clone(), chain, vec![recipient]).await?;
      } else {
         let token = currency.to_erc20().into_owned();
         manager
            .update_tokens_balance(ctx.clone(), chain, recipient, vec![token])
            .await?;
      }
      ctx.calculate_portfolio_value(chain, recipient);
   }

   ctx.calculate_portfolio_value(chain, sender);
   ctx.save_balance_manager();
   ctx.save_portfolio_db();

   Ok(())
}
