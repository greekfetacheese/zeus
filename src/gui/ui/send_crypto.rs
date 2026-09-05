//! UI that allows the user to send ETH or ERC20 tokens (public) or private
//! Railgun (zk → zk) transfers when privacy mode is enabled.

use eframe::egui::{CursorIcon, FontId, Frame, Margin, OpenUrl, RichText, Ui, vec2};

use std::{
   collections::HashMap,
   str::FromStr,
   sync::Arc,
   time::{Duration, Instant},
};

use crate::core::{
   DecodedEvent, TransactionAnalysis, TransferParams, ZeusContext, ZeusCtx, send_transaction,
};
use crate::utils::{RT, estimate_tx_cost};

use crate::assets::icons::Icons;
use crate::gui::{
   SHARED_GUI,
   ui::{
      ContactsUi, RecipientSelectionWindow, TokenSelectionWindow,
      common::{AmountField, AmountFieldParams, show_with_fade},
      dapps::railgun::private_transfer,
   },
};
use crate::utils::simulate::fetch_accounts_info;
use egui_elements::{Button, SecureTextEdit, Theme};
use egui_lucide::Lucide;

use zeus_eth::{
   alloy_primitives::{Address, Bytes, U256},
   alloy_provider::Provider,
   alloy_rpc_types::BlockId,
   currency::{Currency, ERC20Token, NativeCurrency},
   revm_utils::{ForkFactory, Host, new_evm, simulate},
   types::ChainId,
   utils::NumericValue,
};

use anyhow::anyhow;

const POOL_UPDATE_TIMEOUT: u64 = 60;

pub struct SendCryptoUi {
   open: bool,
   pub currency: Currency,
   pub amount_field: AmountField,
   pub recipient: String,
   pub recipient_name: Option<String>,
   pub search_query: String,
   /// Optional memo for private (zk → zk) transfers.
   pub memo: String,
   pub size: (f32, f32),
   pub price_syncing: bool,
   pub syncing_balance: bool,
   pub sending_tx: bool,
   last_price_update: HashMap<Address, Instant>,
}

impl SendCryptoUi {
   pub fn new() -> Self {
      Self {
         open: false,
         currency: Currency::from(NativeCurrency::from_chain_id(1).unwrap()),
         amount_field: AmountField::new(),
         recipient: String::new(),
         recipient_name: None,
         search_query: String::new(),
         memo: String::new(),
         size: (500.0, 690.0),
         price_syncing: false,
         syncing_balance: false,
         sending_tx: false,
         last_price_update: HashMap::new(),
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
      self.amount_field.reset();
      self.clear_search_query();
      self.memo.clear();
   }

   pub fn set_currency(&mut self, currency: Currency) {
      self.currency = currency;
   }

   /// Default token for the active mode / chain (native for public, WETH for private).
   pub fn default_currency(&mut self, privacy_mode: bool, chain_id: u64) {
      self.currency = if privacy_mode {
         Currency::from(ERC20Token::wrapped_native_token(chain_id))
      } else {
         Currency::from(NativeCurrency::from(chain_id))
      };
   }

   pub fn clear_recipient(&mut self) {
      self.recipient_name = None;
      self.recipient = String::new();
   }

   pub fn clear_search_query(&mut self) {
      self.search_query = String::new();
   }

   fn show_railgun_not_supported(&self, theme: &Theme, ui: &mut Ui) {
      ui.vertical_centered(|ui| {
         ui.set_width(self.size.0);
         ui.set_max_height(self.size.1);
         ui.spacing_mut().item_spacing = vec2(0.0, 10.0);

         let text = RichText::new("Railgun is not supported for the selected chain")
            .size(theme.typography.very_large);
         ui.label(text);
      });
   }

   fn show_railgun_not_enabled(&self, theme: &Theme, ui: &mut Ui) {
      ui.vertical_centered(|ui| {
         ui.set_width(self.size.0);
         ui.set_max_height(self.size.1);
         ui.spacing_mut().item_spacing = vec2(0.0, 10.0);

         let text = RichText::new("Railgun is disabled").size(theme.typography.very_large);
         ui.label(text);
         ui.label(
            RichText::new("Enable it in Settings/Railgun to send private transfers.")
               .size(theme.typography.large),
         );
      });
   }

   pub fn show(
      &mut self,
      ctx: &mut ZeusContext,
      icons: Arc<Icons>,
      theme: &Theme,
      token_selection: &mut TokenSelectionWindow,
      recipient_selection: &mut RecipientSelectionWindow,
      contacts_ui: &mut ContactsUi,
      ui: &mut Ui,
   ) {
      show_with_fade(ui, "send_crypto_ui_fade", self.open, |ui| {
         let privacy_mode = ctx.privacy_mode;
         // Private transfer: both tokens and recipients are private (notes + 0zk).
         let token_privacy_mode = privacy_mode;
         let recipient_privacy_mode = privacy_mode;

         let frame = theme.frame1;

         ui.vertical_centered(|ui| {
            frame.show(ui, |ui| {
               Frame::new().inner_margin(Margin::same(10)).show(ui, |ui| {
                  ui.set_width(self.size.0);
                  ui.set_max_height(self.size.1);
                  ui.spacing_mut().item_spacing = vec2(0.0, 10.0);
                  ui.spacing_mut().button_padding = vec2(10.0, 8.0);

                  if privacy_mode && !ctx.railgun_is_supported(ctx.chain) {
                     self.show_railgun_not_supported(theme, ui);
                     return;
                  }

                  if privacy_mode && !ctx.is_railgun_enabled(ctx.chain.id()) {
                     self.show_railgun_not_enabled(theme, ui);
                     return;
                  }

                  let text_edit_visuals = theme.text_edit_visuals();

                  let title = if privacy_mode {
                     "Private Transfer"
                  } else {
                     "Send Crypto"
                  };
                  ui.label(RichText::new(title).size(theme.typography.heading));

                  let owner = ctx.current_wallet_info().address;
                  let owner_zk = ctx.current_wallet_info().zk_address();
                  let chain = ctx.chain;
                  let inner_frame = theme.frame2;

                  // Currency Selection
                  let balance = self.balance_for_mode(ctx, owner, privacy_mode);
                  let cost = self.cost(ctx, privacy_mode);
                  let max_amount = if privacy_mode || self.currency.is_erc20() {
                     balance.clone()
                  } else if balance.wei() < cost.wei() {
                     NumericValue::default()
                  } else {
                     NumericValue::format_wei(
                        balance.wei() - cost.wei(),
                        self.currency.decimals(),
                     )
                  };

                  let amount = self.amount_field.amount.clone();
                  let currency = self.currency.clone();
                  let data_syncing = self.price_syncing || self.syncing_balance;
                  let should_calculate_price = self.should_calculate_price(&currency);
                  let value = value(ctx, currency, amount, should_calculate_price);

                  inner_frame.show(ui, |ui| {
                     ui.set_width(ui.available_width());
                     self.amount_field.show(
                        AmountFieldParams::new(
                           theme,
                           icons.clone(),
                           &self.currency,
                           owner,
                           chain.id(),
                        )
                        .privacy_mode(token_privacy_mode)
                        .balance(balance)
                        .max_amount(max_amount)
                        .value(value)
                        .label("Amount")
                        .token_selection(token_selection, None)
                        .loading(data_syncing)
                        .show_slider(true),
                        ui,
                     );
                  });

                  if let Some(currency) = token_selection.get_selected_currency() {
                     self.currency = currency.clone();
                     token_selection.reset();
                     self.sync_balance(owner, privacy_mode);
                  }

                  recipient_selection.show(
                     ctx,
                     theme,
                     icons.clone(),
                     recipient_privacy_mode,
                     contacts_ui,
                     ui,
                  );
                  let recipient = recipient_selection.get_recipient();

                  // Recipient Selection
                  inner_frame.show(ui, |ui| {
                     ui.set_width(ui.available_width());
                     ui.horizontal(|ui| {
                        ui.label(RichText::new("Recipient").size(theme.typography.large));
                        ui.add_space(10.0);

                        if !recipient.is_empty(recipient_privacy_mode) {
                           if let Some(name) = &recipient.name {
                              ui.label(
                                 RichText::new(name)
                                    .size(theme.typography.large)
                                    .color(theme.colors.info),
                              );
                           } else {
                              ui.label(
                                 RichText::new("Unknown Address")
                                    .size(theme.typography.large)
                                    .color(theme.colors.error),
                              );
                           }

                           ui.add_space(5.0);

                           if !recipient_privacy_mode && !recipient.evm_address.is_empty() {
                              let block_explorer = chain.block_explorer();
                              let link = format!(
                                 "{}/address/{}",
                                 block_explorer, recipient.evm_address
                              );
                              let icon =
                                 Lucide::ExternalLink.size(18.0).color(theme.colors.text).image();

                              let res = ui.add(icon).on_hover_cursor(CursorIcon::PointingHand);

                              if res.clicked() {
                                 let url = OpenUrl::new_tab(link);
                                 ui.ctx().open_url(url);
                              }
                           }
                        }
                     });

                     ui.horizontal(|ui| {
                        let hint = if recipient_privacy_mode {
                           RichText::new("Search contacts or enter a 0zk address")
                              .size(theme.typography.normal)
                              .color(theme.colors.text_muted)
                        } else {
                           RichText::new("Search contacts or enter an address")
                              .size(theme.typography.normal)
                              .color(theme.colors.text_muted)
                        };

                        let address_edit = if recipient_privacy_mode {
                           &mut recipient_selection.recipient.zk_address
                        } else {
                           &mut recipient_selection.recipient.evm_address
                        };

                        let res = ui.add(
                           SecureTextEdit::singleline(address_edit)
                              .visuals(text_edit_visuals)
                              .hint_text(hint)
                              .min_size(vec2(ui.available_width(), 25.0))
                              .margin(Margin::same(10))
                              .font(FontId::proportional(theme.typography.normal)),
                        );
                        if res.clicked() {
                           recipient_selection.open();
                        }
                     });
                  });

                  if privacy_mode {
                     inner_frame.show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                           ui.label(RichText::new("Memo").size(theme.typography.large));
                           ui.add_space(8.0);
                           ui.label(
                              RichText::new("(optional)")
                                 .size(theme.typography.small)
                                 .color(theme.colors.text_muted),
                           );
                        });
                        ui.add(
                           SecureTextEdit::singleline(&mut self.memo)
                              .visuals(text_edit_visuals)
                              .hint_text(
                                 RichText::new("Shown in private history")
                                    .size(theme.typography.normal)
                                    .color(theme.colors.text_muted),
                              )
                              .min_size(vec2(ui.available_width(), 25.0))
                              .margin(Margin::same(10))
                              .font(FontId::proportional(theme.typography.normal)),
                        );
                     });
                  }

                  let recipient_str = if recipient_privacy_mode {
                     recipient.zk_address
                  } else {
                     recipient.evm_address
                  };

                  self.send_button(
                     ctx,
                     theme,
                     owner,
                     owner_zk,
                     recipient_str,
                     privacy_mode,
                     ui,
                  );
               });
            });
         });
      });
   }

   fn should_calculate_price(&self, currency: &Currency) -> bool {
      let now = Instant::now();
      let last_updated = self.last_price_update.get(&currency.address()).cloned();
      if last_updated.is_none() {
         return true;
      }

      let last_updated = last_updated.unwrap();
      let timeout = Duration::from_secs(POOL_UPDATE_TIMEOUT);
      let time_passed = now.duration_since(last_updated);
      time_passed > timeout
   }

   fn send_button(
      &mut self,
      ctx: &mut ZeusContext,
      theme: &Theme,
      owner: Address,
      owner_zk: String,
      recipient: String,
      privacy_mode: bool,
      ui: &mut Ui,
   ) {
      let button_visuals = theme.button_visuals();
      let sending_tx = self.sending_tx;
      let recipient_is_sender =
         self.recipient_is_sender(owner, &owner_zk, &recipient, privacy_mode);
      let valid_recipient = self.valid_recipient(&recipient, privacy_mode);
      let valid_amount = self.valid_amount();
      let has_balance = self.sufficient_balance(ctx, owner, privacy_mode);
      let has_entered_amount = !self.amount_field.amount.is_empty();
      let has_entered_recipient = !recipient.trim().is_empty();
      let valid_token = if privacy_mode {
         self.currency.is_erc20()
      } else {
         true
      };

      let valid_inputs = valid_recipient
         && !recipient_is_sender
         && has_balance
         && has_entered_amount
         && valid_amount
         && valid_token
         && has_entered_recipient
         && !sending_tx;

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

      if privacy_mode && !valid_token {
         button_text = "Invalid Token".to_string();
      }

      let text = RichText::new(button_text).size(theme.typography.large);
      let send = Button::new(text)
         .min_size(vec2(ui.available_width() * 0.8, 45.0))
         .visuals(button_visuals);

      if ui.add_enabled(valid_inputs, send).clicked() {
         self.sending_tx = true;

         if privacy_mode {
            self.send_private_transfer(ctx, recipient);
         } else {
            match self.send_public_transaction(ctx, recipient) {
               Ok(_) => {}
               Err(e) => {
                  self.sending_tx = false;

                  RT.spawn_blocking(move || {
                     SHARED_GUI.write(|gui| {
                        let msg = format!("Error while sending transaction: {}", e);
                        gui.open_msg_window(msg);
                     });
                  });
               }
            }
         }
      }
   }

   fn sync_balance(&mut self, owner: Address, privacy_mode: bool) {
      self.syncing_balance = true;
      let currency = self.currency.clone();
      let chain = currency.chain_id();

      RT.spawn(async move {
         let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());

         if privacy_mode {
            ctx.update_private_data(chain, owner).await;
         } else {
            let balance_manager = ctx.balance_manager();
            if currency.is_native() {
               match balance_manager
                  .update_eth_balance(ctx.clone(), chain, vec![owner], false)
                  .await
               {
                  Ok(_) => {}
                  Err(e) => {
                     tracing::error!("Failed to update ETH balance: {}", e);
                  }
               }
            } else {
               let token = currency.to_erc20().into_owned();
               match balance_manager
                  .update_tokens_balance(ctx.clone(), chain, owner, vec![token], false)
                  .await
               {
                  Ok(_) => {}
                  Err(e) => {
                     tracing::error!("Failed to update token balance: {}", e);
                  }
               }
            }
         }
         SHARED_GUI.write(|gui| {
            gui.send_crypto.syncing_balance = false;
         });
      });
   }

   fn cost(&self, ctx: &mut ZeusContext, privacy_mode: bool) -> NumericValue {
      let gas_used = if privacy_mode {
         500_000
      } else if self.currency.is_native() {
         ctx.chain.transfer_gas()
      } else {
         ctx.chain.erc20_transfer_gas()
      };

      let fee = ctx.priority_fee.get(ctx.chain.id()).cloned().unwrap_or_default();
      let (cost_in_wei, _) = estimate_tx_cost(ctx, ctx.chain.id(), gas_used, fee.wei());
      cost_in_wei
   }

   fn valid_recipient(&self, recipient: &str, privacy_mode: bool) -> bool {
      if privacy_mode {
         // Full 0zk parse is expensive (~15–20ms)
         // Actual check happens on execution path
         let r = recipient.trim();
         r.starts_with("0zk") && r.len() > 20
      } else {
         let recipient = Address::from_str(recipient).unwrap_or(Address::ZERO);
         recipient != Address::ZERO
      }
   }

   fn recipient_is_sender(
      &self,
      owner: Address,
      owner_zk: &str,
      recipient: &str,
      privacy_mode: bool,
   ) -> bool {
      if privacy_mode {
         !owner_zk.is_empty() && owner_zk == recipient.trim()
      } else {
         let recipient = Address::from_str(recipient).unwrap_or(Address::ZERO);
         recipient == owner
      }
   }

   fn valid_amount(&self) -> bool {
      let amount = self.amount_field.amount.parse().unwrap_or(0.0);
      amount > 0.0
   }

   fn balance_for_mode(
      &self,
      ctx: &mut ZeusContext,
      owner: Address,
      privacy_mode: bool,
   ) -> NumericValue {
      if !privacy_mode {
         return ctx.get_currency_balance(ctx.chain.id(), owner, &self.currency);
      }

      let portfolio = ctx.read_wallet_state(|ws| ws.portfolio_db.get(ctx.chain.id(), owner));
      if let Some(token) = self.currency.erc20_opt() {
         for (t, balance, _value, _price) in portfolio.private_tokens() {
            if t.address == token.address {
               return balance.clone();
            }
         }
      }
      NumericValue::default()
   }

   fn sufficient_balance(
      &self,
      ctx: &mut ZeusContext,
      sender: Address,
      privacy_mode: bool,
   ) -> bool {
      let balance = self.balance_for_mode(ctx, sender, privacy_mode);
      let amount = NumericValue::parse_to_wei(
         &self.amount_field.amount,
         self.currency.decimals(),
      );
      balance.wei() >= amount.wei()
   }

   fn send_private_transfer(&mut self, ctx: &mut ZeusContext, recipient: String) {
      let chain = ctx.chain;
      let from = ctx.current_wallet_info().address;
      let currency = self.currency.clone();
      let amount = NumericValue::parse_to_wei(
         &self.amount_field.amount,
         self.currency.decimals(),
      );
      let memo = self.memo.clone();

      ctx.railgun_status.set_op_in_progress(chain.id(), true);

      RT.spawn_blocking(move || {
         let ctx = SHARED_GUI.write(|gui| {
            gui.loading_window.open("Wait while magic happens");
            gui.request_repaint();
            gui.ctx.clone()
         });

         let result = RT.block_on(private_transfer(
            ctx.clone(),
            chain,
            currency,
            amount,
            from,
            recipient,
            memo,
         ));

         match result {
            Ok(_) => {
               SHARED_GUI.write(|gui| {
                  gui.send_crypto.sending_tx = false;
                  gui.send_crypto.amount_field.reset();
                  gui.send_crypto.memo.clear();
                  gui.loading_window.reset();
                  gui.request_repaint();
               });
            }
            Err(e) => {
               tracing::error!("Error sending private transfer: {:?}", e);
               SHARED_GUI.write(|gui| {
                  gui.send_crypto.sending_tx = false;
                  gui.notification.reset();
                  gui.loading_window.reset();
                  let msg = format!("Private Transfer Error: {}", e);
                  gui.msg_window.open(msg);
                  gui.request_repaint();
               });
            }
         }

         ctx.write(|ctx| {
            ctx.railgun_status.set_op_in_progress(chain.id(), false);
         });
      });
   }

   fn send_public_transaction(
      &mut self,
      ctx: &mut ZeusContext,
      recipient: String,
   ) -> Result<(), anyhow::Error> {
      let chain = ctx.chain;
      let from = ctx.current_wallet_info().address;
      let currency = self.currency.clone();
      let recipient_address = Address::from_str(&recipient)?;
      let amount = NumericValue::parse_to_wei(
         &self.amount_field.amount,
         self.currency.decimals(),
      );

      RT.spawn(async move {
         let ctx = SHARED_GUI.write(|gui| {
            gui.loading_window.open("Wait while magic happens");
            gui.request_repaint();
            gui.ctx.clone()
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
                     gui.send_crypto.sending_tx = false;
                     gui.send_crypto.amount_field.reset();
                  });
               }
               Err(e) => {
                  tracing::error!("Error sending transaction: {:?}", e);
                  SHARED_GUI.write(|gui| {
                     gui.send_crypto.sending_tx = false;
                     gui.notification.reset();
                     gui.loading_window.reset();
                     let msg = format!("Transaction Error: {}", e);
                     gui.msg_window.open(msg);
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
                     gui.send_crypto.sending_tx = false;
                     gui.send_crypto.amount_field.reset();
                  });
               }
               Err(e) => {
                  tracing::error!("Error sending transaction: {:?}", e);
                  SHARED_GUI.write(|gui| {
                     gui.send_crypto.sending_tx = false;
                     gui.notification.reset();
                     gui.loading_window.reset();
                     let msg = format!("Transaction Error: {}", e);
                     gui.msg_window.open(msg);
                  });
               }
            }
         }
      });
      Ok(())
   }
}

fn value(
   ctx: &mut ZeusContext,
   currency: Currency,
   amount: String,
   should_fetch_price: bool,
) -> NumericValue {
   let price = ctx.get_currency_price(&currency);
   let amount = amount.parse().unwrap_or(0.0);
   let value = NumericValue::value(amount, price.f64());

   if should_fetch_price {
      let price_manager = ctx.price_manager.clone();
      let pool_manager = ctx.pool_manager.clone();
      let chain = currency.chain_id();

      RT.spawn(async move {
         let ctx = SHARED_GUI.write(|gui| {
            gui.send_crypto.price_syncing = true;
            gui.send_crypto.last_price_update.insert(currency.address(), Instant::now());
            gui.ctx.clone()
         });

         match price_manager
            .calculate_prices(
               ctx,
               chain,
               pool_manager,
               vec![currency.to_erc20().into_owned()],
            )
            .await
         {
            Ok(_) => {
               SHARED_GUI.write(|gui| {
                  gui.send_crypto.price_syncing = false;
               });
            }
            Err(e) => {
               SHARED_GUI.write(|gui| {
                  gui.send_crypto.price_syncing = false;
               });
               tracing::error!("Error calculating price: {:?}", e);
            }
         }
      });
   }

   value
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

   let (_, _) = send_transaction(
      ctx.clone(),
      dapp,
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

   let client = ctx.get_zeus_client();

   let block_fut = client.request(chain.id(), |client| async move {
      client.get_block(BlockId::latest()).await.map_err(|e| anyhow!("{:?}", e))
   });

   let eth_balance_before_fut = client.request(chain.id(), |client| async move {
      client
         .get_balance(from)
         .block_id(BlockId::latest())
         .await
         .map_err(|e| anyhow!("{:?}", e))
   });

   let recipient_token_balance_before_fut = client.request(chain.id(), |client| {
      let token_clone = token.clone();
      async move { token_clone.balance_of(client.clone(), recipient, None).await }
   });

   let (block, eth_balance_before, recipient_token_balance_before) = tokio::try_join!(
      block_fut,
      eth_balance_before_fut,
      recipient_token_balance_before_fut
   )?;

   let block = if let Some(block) = block {
      block
   } else {
      return Err(anyhow!(
         "No block found, this is usally a provider issue"
      ));
   };

   let block_id = BlockId::number(block.number());

   let mut accounts = Vec::new();
   accounts.push(from);
   accounts.push(recipient);
   accounts.push(token.address);
   accounts.push(block.header.beneficiary);

   let accounts_info = fetch_accounts_info(ctx.clone(), chain.id(), block_id, accounts).await;

   let fork_client = ctx.get_client(chain.id()).await?;
   let mut factory =
      ForkFactory::new_sandbox_factory(fork_client, chain.id(), None, Some(block_id));

   for info in accounts_info {
      factory.insert_account_info(info.address, info.info);
   }

   let fork_db = factory.new_sandbox_fork();

   let mut transfer_params = TransferParams {
      currency: token.clone().into(),
      sender: from,
      recipient,
      ..Default::default()
   };

   let real_amount_sent;
   let eth_balance_after;
   let logs;
   let gas_used;

   {
      let mut evm = new_evm(chain, Some(&block), fork_db);

      let res = simulate::transfer_token(
         &mut evm,
         token.address,
         from,
         recipient,
         amount.wei(),
         true,
      )?;

      let recipient_token_balance_after =
         simulate::erc20_balance(&mut evm, token.address, recipient)?;

      let real_amount = if recipient_token_balance_after > recipient_token_balance_before {
         recipient_token_balance_after - recipient_token_balance_before
      } else {
         return Err(anyhow!(
            "Simulation Error: Recipient did not receive any tokens after transfer, you are probably try to interact with a malicious token"
         ));
      };

      let state = evm.balance(from);
      eth_balance_after = if let Some(state) = state {
         state.data
      } else {
         U256::ZERO
      };

      real_amount_sent = real_amount;
      gas_used = res.tx_gas_used();
      logs = res.logs().to_vec();
   }

   let amount_usd = ctx.get_token_value_for_amount(amount.f64(), &token);
   let real_amount_sent = NumericValue::format_wei(real_amount_sent, token.decimals);
   let real_amount_send_usd = ctx.get_token_value_for_amount(real_amount_sent.f64(), &token);

   transfer_params.amount = amount;
   transfer_params.amount_usd = Some(amount_usd);
   transfer_params.real_amount_sent = Some(real_amount_sent);
   transfer_params.real_amount_sent_usd = Some(real_amount_send_usd);

   let contract_interact = Some(true);
   let auth_list = Vec::new();

   let mut tx_analysis = TransactionAnalysis::new(
      ctx.clone(),
      chain.id(),
      from,
      interact_to,
      contract_interact,
      call_data.clone(),
      value,
      logs,
      gas_used,
      eth_balance_before,
      eth_balance_after,
      auth_list.clone(),
   )
   .await?;

   tx_analysis.set_main_event(DecodedEvent::Transfer(transfer_params));

   let (_, _) = send_transaction(
      ctx.clone(),
      dapp,
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

   manager.update_eth_balance(ctx.clone(), chain, vec![sender], true).await?;

   if currency.is_erc20() {
      let token = currency.to_erc20().into_owned();
      manager
         .update_tokens_balance(ctx.clone(), chain, sender, vec![token], true)
         .await?;
   }

   if exists {
      if currency.is_native() {
         manager.update_eth_balance(ctx.clone(), chain, vec![recipient], true).await?;
      } else {
         let token = currency.to_erc20().into_owned();
         manager
            .update_tokens_balance(ctx.clone(), chain, recipient, vec![token], true)
            .await?;
      }
      ctx.update_public_data(chain, recipient);
   }

   ctx.update_public_data(chain, sender);
   Ok(())
}
