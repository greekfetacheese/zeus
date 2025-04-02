use eframe::egui::{
   Align, Align2, Button, Color32, FontId, Frame, Grid, Layout, Margin, Order, Response, RichText, Spinner, TextEdit,
   Ui, Vec2, Window, vec2,
};

use std::str::FromStr;
use std::sync::Arc;

use crate::core::{
   ZeusCtx,
   utils::{
      RT,
      eth::{self, get_currency_balance, send_crypto},
      tx::{TxMethod, TxParams},
   },
};

use crate::assets::icons::Icons;
use crate::gui::{
   SHARED_GUI,
   ui::{
      RecipientSelectionWindow, TokenSelectionWindow, button, img_button,
      misc::{ChainSelect, WalletSelect},
      rich_text,
   },
   utils::open_loading,
};
use egui_theme::{Theme, utils::*};

use anyhow::anyhow;
use zeus_eth::{
   alloy_primitives::{
      Address, Bytes, U256,
      utils::{format_units, parse_units},
   },
   currency::{Currency, ERC20Token, NativeCurrency},
   utils::NumericValue,
};

// This is a temporary solution to just show that the transaction was sent
pub struct TxSuccessWindow {
   pub open: bool,
   pub explorer: String,
   pub size: Vec2,
}

impl TxSuccessWindow {
   pub fn new() -> Self {
      Self {
         open: false,
         explorer: String::new(),
         size: vec2(350.0, 150.0),
      }
   }

   pub fn open(&mut self, explorer_link: impl Into<String>) {
      self.open = true;
      self.explorer = explorer_link.into();
   }

   pub fn show(&mut self, theme: &Theme, ui: &mut Ui) {
      if !self.open {
         return;
      }

      let msg = rich_text("Transaction Sent!").size(theme.text_sizes.very_large);
      let msg2 = rich_text("View on:").size(theme.text_sizes.very_large);
      let ok = button(rich_text("Ok").size(theme.text_sizes.normal));

      Window::new("Tx Success")
         .title_bar(false)
         .resizable(false)
         .movable(true)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .collapsible(false)
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.vertical_centered(|ui| {
               ui.set_min_size(self.size);
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);
               ui.add_space(20.0);

               ui.label(msg);
               ui.add_space(5.0);
               ui.label(msg2);
               ui.add_space(5.0);
               ui.hyperlink(&self.explorer);
               ui.add_space(20.0);

               if ui.add(ok).clicked() {
                  self.open = false;
               }
            });
         });
   }
}

pub struct SendCryptoUi {
   pub open: bool,
   pub chain_select: ChainSelect,
   pub wallet_select: WalletSelect,
   pub priority_fee: String,
   pub currency: Currency,
   pub amount: String,
   pub recipient: String,
   pub recipient_name: Option<String>,
   pub search_query: String,
   pub size: (f32, f32),
   pub tx_success_window: TxSuccessWindow,
   /// Flag to not spam the rpc when fetching pool data
   pub pool_data_syncing: bool,
   pub syncing_balance: bool,
   /// Review Transaction window
   pub review_tx_window: bool,
}

impl SendCryptoUi {
   pub fn new() -> Self {
      Self {
         open: false,
         chain_select: ChainSelect::new("chain_select_2", 1),
         wallet_select: WalletSelect::new("wallet_select_2"),
         priority_fee: "1".to_string(),
         currency: Currency::from_native(NativeCurrency::from_chain_id(1).unwrap()),
         amount: String::new(),
         recipient: String::new(),
         recipient_name: None,
         search_query: String::new(),
         size: (500.0, 750.0),
         tx_success_window: TxSuccessWindow::new(),
         pool_data_syncing: false,
         syncing_balance: false,
         review_tx_window: false,
      }
   }

   pub fn show(
      &mut self,
      ctx: ZeusCtx,
      icons: Arc<Icons>,
      theme: &Theme,
      token_selection: &mut TokenSelectionWindow,
      recipient_selection: &mut RecipientSelectionWindow,
      ui: &mut Ui,
   ) {
      if !self.open {
         return;
      }

      recipient_selection.show(ctx.clone(), theme, &self.wallet_select, ui);
      let recipient = recipient_selection.get_recipient();
      let recipient_name = recipient_selection.get_recipient_name();
      self.review_transaction(
         ctx.clone(),
         theme,
         icons.clone(),
         recipient.clone(),
         recipient_name.clone(),
         ui,
      );

      let frame = theme.frame1;
      let bg_color = frame.fill;
      Window::new("send_crypto_ui")
         .title_bar(false)
         .resizable(false)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(frame)
         .show(ui.ctx(), |ui| {
            let ui_width = self.size.0;
            let space = 15.0;
            ui.set_max_width(ui_width);
            ui.spacing_mut().button_padding = vec2(10.0, 8.0);

            // Title
            ui.vertical_centered(|ui| {
               ui.label(rich_text("Send Crypto").size(theme.text_sizes.heading));
               ui.add_space(20.0);
            });

            // Chain Selection
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
               ui.label(
                  rich_text("Chain")
                     .color(theme.colors.text_secondary)
                     .size(theme.text_sizes.large),
               );
            });
            ui.add_space(5.0);

            ui.scope(|ui| {
               widget_visuals(ui, theme.get_widget_visuals(bg_color));
               let clicked = self.chain_select.show(0, theme, icons.clone(), ui);
               if clicked {
                  let chain = self.chain_select.chain.id();
                  self.priority_fee = ctx
                     .get_priority_fee(chain)
                     .unwrap_or_default()
                     .formatted()
                     .clone();
                  self.currency = Currency::from_native(NativeCurrency::from_chain_id(chain).unwrap());
               }
            });

            ui.add_space(space);

            let chain = self.chain_select.chain.id();
            let owner = self.wallet_select.wallet.key.borrow().address();
            let currencies = ctx.get_currencies(chain);

            // From Wallet
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
               ui.label(
                  rich_text("From")
                     .color(theme.colors.text_secondary)
                     .size(theme.text_sizes.large),
               );
            });
            ui.add_space(5.0);

            ctx.read(|ctx| {
               let wallets = &ctx.account.wallets;
               ui.scope(|ui| {
                  widget_visuals(ui, theme.get_widget_visuals(bg_color));
                  self.wallet_select.show(theme, wallets, icons.clone(), ui);
               });
            });
            ui.add_space(space);

            // Recipient Input
            Grid::new("recipient_name")
               .spacing(vec2(3.0, 0.0))
               .show(ui, |ui| {
                  ui.label(
                     rich_text("To")
                        .color(theme.colors.text_secondary)
                        .size(theme.text_sizes.large),
                  );
                  if !recipient_selection.recipient.is_empty() {
                     if let Some(name) = &recipient_selection.recipient_name {
                        ui.label(RichText::new(name.clone()).size(theme.text_sizes.normal));
                     } else {
                        ui.label(
                           RichText::new("Unknown Address")
                              .size(theme.text_sizes.normal)
                              .color(Color32::RED),
                        );
                     }
                  }
                  ui.end_row();
               });
            ui.add_space(5.0);

            ui.horizontal(|ui| {
               widget_visuals(ui, theme.get_text_edit_visuals(bg_color));
               let res = ui.add(
                  TextEdit::singleline(&mut recipient_selection.recipient)
                     .hint_text("Search contacts or enter an address")
                     .min_size(vec2(ui_width * 0.85, 25.0))
                     .margin(Margin::same(10))
                     .background_color(theme.colors.text_edit_bg2)
                     .font(FontId::proportional(theme.text_sizes.large)),
               );
               if res.clicked() {
                  recipient_selection.open = true;
               }
            });
            ui.add_space(space);

            // Token Selection
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
               ui.label(
                  rich_text("Asset")
                     .color(theme.colors.text_secondary)
                     .size(theme.text_sizes.large),
               );
            });
            ui.add_space(5.0);

            Grid::new("token_selection")
               .spacing(vec2(5.0, 0.0))
               .show(ui, |ui| {
                  bg_color_on_idle(ui, Color32::TRANSPARENT);
                  let res = self.token_button(theme, icons.clone(), ui);
                  if res.clicked() {
                     token_selection.open = true;
                  }

                  let balance = ctx.get_currency_balance(chain, owner, &self.currency);
                  ui.label(
                     RichText::new(format!("Balance: {}", balance.formatted()))
                        .color(theme.colors.text_secondary)
                        .size(theme.text_sizes.normal),
                  );

                  if self.syncing_balance {
                     ui.add(Spinner::new().size(17.0).color(Color32::WHITE));
                  }

                  token_selection.show(
                     ctx.clone(),
                     theme,
                     icons.clone(),
                     chain,
                     owner,
                     &currencies,
                     ui,
                  );

                  if let Some(currency) = token_selection.get_currency() {
                     self.currency = currency.clone();
                     token_selection.reset();
                     self.sync_balance(ctx.clone());
                  }
                  ui.end_row();
               });
            ui.add_space(space);

            // Amount Input
            Grid::new("amount_input")
               .spacing(vec2(5.0, 0.0))
               .show(ui, |ui| {
                  ui.label(
                     rich_text("Amount")
                        .color(theme.colors.text_secondary)
                        .size(theme.text_sizes.large),
                  );
                  widget_visuals(ui, theme.get_button_visuals(bg_color));
                  ui.spacing_mut().button_padding = vec2(0.0, 0.0);
                  let max_button = button(rich_text("Max").size(theme.text_sizes.small));
                  if ui.add(max_button).clicked() {
                     self.amount = self.max_amount(ctx.clone()).flatten().clone();
                  }
                  ui.end_row();
               });

            ui.add_space(5.0);

            ui.horizontal(|ui| {
               ui.set_width(ui_width * 0.5);
               widget_visuals(ui, theme.get_text_edit_visuals(bg_color));
               ui.add(
                  TextEdit::singleline(&mut self.amount)
                     .hint_text("0")
                     .font(egui::FontId::proportional(theme.text_sizes.large))
                     .background_color(theme.colors.text_edit_bg2)
                     .min_size(vec2(ui_width * 0.5, 25.0))
                     .margin(Margin::same(10)),
               );
            });
            ui.add_space(space);

            // Priority Fee
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
               ui.label(
                  rich_text("Priority Fee")
                     .color(theme.colors.text_secondary)
                     .size(theme.text_sizes.large),
               );
            });
            ui.add_space(5.0);

            ui.horizontal(|ui| {
               ui.set_width(ui_width * 0.2);
               widget_visuals(ui, theme.get_text_edit_visuals(bg_color));
               ui.add(
                  TextEdit::singleline(&mut self.priority_fee)
                     .min_size(vec2(ui_width * 0.2, 25.0))
                     .margin(Margin::same(10))
                     .background_color(theme.colors.text_edit_bg2)
                     .font(egui::FontId::proportional(theme.text_sizes.normal)),
               );
               ui.add_space(5.0);
               ui.label(
                  rich_text("Gwei")
                     .color(theme.colors.text_secondary)
                     .size(theme.text_sizes.normal),
               );
            });
            ui.add_space(space);

            // Value
            let value = self.value(ctx.clone());
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
               ui.label(
                  RichText::new(format!("Value≈ ${}", value.formatted()))
                     .color(theme.colors.text_secondary)
                     .size(theme.text_sizes.normal)
                     .strong(),
               );
               if self.pool_data_syncing {
                  ui.add(Spinner::new().size(17.0).color(Color32::WHITE));
               }
            });
            ui.add_space(space);

            // Estimated Cost
            let (_, cost) = self.cost(ctx.clone());
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
               ui.label(
                  RichText::new(format!("Estimated Cost≈ ${}", cost.formatted()))
                     .color(theme.colors.text_secondary)
                     .size(theme.text_sizes.normal)
                     .strong(),
               );
            });
            ui.add_space(space);

            // Send Button
            widget_visuals(ui, theme.get_button_visuals(bg_color));
            let send =
               Button::new(rich_text("Send").size(theme.text_sizes.normal)).min_size(vec2(ui_width * 0.9, 40.0));
            ui.vertical_centered(|ui| {
               if ui.add(send).clicked() {
                  self.validate_and_send(ctx.clone(), recipient);
               }
            });
            ui.add_space(space);
         });
   }

   fn token_button(&mut self, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) -> Response {
      let icon = icons.currency_icon(&self.currency);
      let button = img_button(
         icon,
         rich_text(self.currency.symbol()).size(theme.text_sizes.normal),
      );
      ui.add(button)
   }

   fn sync_balance(&mut self, ctx: ZeusCtx) {
      self.syncing_balance = true;
      let chain = self.chain_select.chain.id();
      let owner = self.wallet_select.wallet.key.borrow().address();
      let currency = self.currency.clone();
      RT.spawn(async move {
         let balance = match get_currency_balance(ctx.clone(), owner, currency.clone()).await {
            Ok(b) => {
               SHARED_GUI.write(|gui| {
                  gui.send_crypto.syncing_balance = false;
               });
               b
            }
            Err(e) => {
               tracing::error!("Error getting balance: {:?}", e);
               SHARED_GUI.write(|gui| {
                  gui.send_crypto.syncing_balance = false;
               });
               return;
            }
         };

         ctx.write(|ctx| {
            ctx.balance_db
               .insert_currency_balance(owner, balance, &currency);
         });

         ctx.update_portfolio_value(chain, owner);
         let _ = ctx.save_portfolio_db();
      });
   }

   fn value(&mut self, ctx: ZeusCtx) -> NumericValue {
      let price = ctx.get_currency_price_opt(&self.currency);
      let amount = self.amount.parse().unwrap_or(0.0);

      if amount == 0.0 {
         return NumericValue::default();
      }

      if !price.is_none() {
         return NumericValue::value(amount, price.unwrap().f64());
      } else {
         // probably no pool data available to calculate the price

         let token = self.currency.erc20().cloned();

         if token.is_none() {
            return NumericValue::default();
         }

         // don't spam the rpc in the next frames
         if self.pool_data_syncing {
            return NumericValue::default();
         }

         let token = token.unwrap();

         let v2_pools = ctx.get_v2_pools(&token);
         let v3_pools = ctx.get_v3_pools(&token);
         let owner = self.wallet_select.wallet.key.borrow().address();
         let chain_id = self.chain_select.chain.id();

         if v2_pools.is_empty() || v3_pools.is_empty() {
            self.pool_data_syncing = true;
            RT.spawn(async move {
               match eth::sync_pools_for_token(
                  ctx.clone(),
                  token.clone(),
                  v2_pools.is_empty(),
                  v3_pools.is_empty(),
               )
               .await
               {
                  Ok(_) => {
                     SHARED_GUI.write(|gui| {
                        gui.send_crypto.pool_data_syncing = false;
                     });
                     let client = ctx.get_client_with_id(chain_id).unwrap();
                     let pool_manager = ctx.pool_manager();
                     pool_manager.update(client, chain_id).await.unwrap();
                     ctx.update_portfolio_value(chain_id, owner);
                     let _ = ctx.save_pool_data();
                     let _ = ctx.save_portfolio_db();
                  }
                  Err(e) => {
                     tracing::error!("Error getting pools: {:?}", e);
                     SHARED_GUI.write(|gui| {
                        gui.send_crypto.pool_data_syncing = false;
                     });
                  }
               };
            });
         }

         return NumericValue::default();
      }
   }

   /// Max amount = Balance - Cost
   fn max_amount(&self, ctx: ZeusCtx) -> NumericValue {
      let chain = self.chain_select.chain;
      let currency = self.currency.clone();
      let owner = self.wallet_select.wallet.key.borrow().address();
      let balance = ctx.get_currency_balance(chain.id(), owner, &currency);
      let (cost_wei, _) = self.cost(ctx.clone());

      if currency.is_erc20() {
         return balance;
      } else {
         if balance.wei().unwrap() < cost_wei {
            return NumericValue::default();
         }
         let max = balance.wei().unwrap() - cost_wei;
         return NumericValue::format_wei(max, currency.decimals());
      }
   }

   fn cost(&self, ctx: ZeusCtx) -> (U256, NumericValue) {
      let fee = if self.priority_fee.is_empty() {
         parse_units("1", "gwei").unwrap().get_absolute()
      } else {
         parse_units(&self.priority_fee, "gwei")
            .unwrap_or(parse_units("1", "gwei").unwrap())
            .get_absolute()
      };

      let chain = self.chain_select.chain;
      let base_fee = ctx.get_base_fee(chain.id()).unwrap_or_default().next;
      let fee = fee + U256::from(base_fee);

      let gas_used = if self.currency.is_native() {
         U256::from(chain.transfer_gas())
      } else {
         U256::from(chain.erc20_transfer_gas())
      };

      // native token price
      let native_token = ERC20Token::wrapped_native_token(chain.id());
      let price = ctx.get_token_price(&native_token).unwrap_or_default().f64();

      let cost_wei = gas_used * fee;
      let cost = format_units(cost_wei, native_token.decimals).unwrap_or_default();
      let cost: f64 = cost.parse().unwrap_or_default();

      // cost in usd
      let cost_usd = NumericValue::value(cost, price);
      (cost_wei, cost_usd)
   }

   fn validate_params(&self, ctx: ZeusCtx, recipient: String) -> Result<TxParams, anyhow::Error> {
      let from = self.wallet_select.wallet.clone();
      let to = Address::from_str(&recipient).unwrap_or(Address::ZERO);
      let amount = NumericValue::parse_to_wei(&self.amount, self.currency.decimals());
      let currency = self.currency.clone();
      let chain = self.chain_select.chain;
      let fee = self.priority_fee.clone();

      let balance = ctx.get_currency_balance(chain.id(), from.key.borrow().address(), &currency);

      if amount.wei().unwrap() > balance.wei().unwrap() {
         return Err(anyhow!("Insufficient balance"));
      }

      if to.is_zero() {
         return Err(anyhow!("Invalid recipient address"));
      }

      if from.key.borrow().address() == to {
         return Err(anyhow!("Cannot send to yourself"));
      }

      if amount.is_zero() {
         return Err(anyhow!("Amount cannot be 0"));
      }

      let fee = if fee.is_empty() {
         parse_units("1", "gwei")?.get_absolute()
      } else {
         parse_units(&fee, "gwei")?.get_absolute()
      };

      let miner_tip = U256::from(fee);
      let call_data = if currency.is_native() {
         Bytes::default()
      } else {
         let token = currency.erc20().unwrap();
         let data = token.encode_transfer(to, amount.wei().unwrap());
         data
      };

      let value = if currency.is_native() {
         amount.wei().unwrap()
      } else {
         U256::ZERO
      };

      let gas_used = if currency.is_native() {
         chain.transfer_gas()
      } else {
         chain.erc20_transfer_gas()
      };

      let base_fee = ctx.get_base_fee(chain.id()).unwrap_or_default().next;

      let tx_method = if currency.is_native() {
         let currency = currency.native().cloned().unwrap();
         TxMethod::Transfer(currency)
      } else {
         let token = currency.erc20().cloned().unwrap();
         TxMethod::ERC20Transfer((token, amount))
      };

      let params = TxParams::new(
         tx_method,
         from.key.clone(),
         to,
         value,
         chain,
         miner_tip,
         base_fee,
         call_data,
         gas_used,
      );

      params.sufficient_balance(balance)?;

      Ok(params)
   }

   fn review_transaction(
      &mut self,
      ctx: ZeusCtx,
      theme: &Theme,
      icons: Arc<Icons>,
      recipient: String,
      recipient_name: Option<String>,
      ui: &mut Ui,
   ) {
      if !self.review_tx_window {
         return;
      }

      let amount = self.amount.clone();
      let value = self.value(ctx.clone());
      let sender = self.wallet_select.wallet.clone();
      let chain = self.chain_select.chain;
      let currency = self.currency.clone();
      let explorer = chain.block_explorer().to_owned();

      let recipient_address = Address::from_str(&recipient).unwrap_or(Address::ZERO);

      let mut should_send_tx = false;

      Window::new("Review Transaction")
         .title_bar(false)
         .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
         .order(Order::Foreground)
         .resizable(false)
         .collapsible(false)
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_width(400.0);
            ui.set_height(300.0);
            ui.spacing_mut().item_spacing.y = 15.0;
            ui.spacing_mut().button_padding = vec2(10.0, 8.0);
            let width = ui.available_width();

            ui.vertical_centered(|ui| {
               ui.add_space(20.0);
               ui.label(rich_text("You are about to send").size(theme.text_sizes.heading));

               // TODO: center this
               ui.add_sized(vec2(width * 0.33, 20.0), |ui: &mut Ui| {
                  let res = Grid::new("amount_send")
                     .spacing(vec2(5.0, 0.0))
                     .show(ui, |ui| {
                        ui.label(rich_text(format!("{} {}", amount, currency.symbol())).size(theme.text_sizes.large));
                        ui.add(icons.currency_icon(&currency));
                        ui.label(rich_text(format!("${}", value.formatted())).size(theme.text_sizes.normal));
                        ui.end_row();
                     });
                  res.response
               });

               ui.label(rich_text("To").size(theme.text_sizes.large));

               if let Some(name) = recipient_name {
                  ui.label(rich_text(name).size(theme.text_sizes.large));
               } else {
                  ui.label(rich_text(recipient.clone()).size(theme.text_sizes.large));
               }

               let confirm = Button::new(rich_text("Confirm").size(theme.text_sizes.normal));
               if ui.add(confirm).clicked() {
                  should_send_tx = true;
                  self.review_tx_window = false;
               }

               let cancel = Button::new(rich_text("Cancel").size(theme.text_sizes.normal));
               if ui.add(cancel).clicked() {
                  self.review_tx_window = false;
               }
            });
         });

      if should_send_tx {
         let params = match self.validate_params(ctx.clone(), recipient) {
            Ok(p) => p,
            Err(e) => {
               std::thread::spawn(move || {
                  SHARED_GUI.write(|gui| {
                     gui.msg_window.open("Transaction Error", &e.to_string());
                  });
               });
               return;
            }
         };

         RT.spawn(async move {
            open_loading("Sending Transaction...".into());

            match send_crypto(ctx.clone(), currency.clone(), params.clone()).await {
               Ok(tx) => {
                  SHARED_GUI.write(|gui| {
                     gui.loading_window.reset();
                     let link = format!("{}/tx/{}", explorer, tx.transaction_hash);
                     gui.send_crypto.tx_success_window.open(link);
                  });

                  // if recipient is a wallet owned by the user then update the balance
                  // Also update the sender's balance
                  let account = ctx.account();
                  if account.wallet_address_exists(recipient_address) {
                     let recipient_balance = get_currency_balance(ctx.clone(), recipient_address, currency.clone())
                        .await
                        .unwrap();

                     ctx.write(|ctx| {
                        ctx.balance_db
                           .insert_currency_balance(recipient_address, recipient_balance, &currency);
                     });
                     ctx.update_portfolio_value(chain.id(), recipient_address);
                  }
                  let sender_addr = sender.key.borrow().address();
                  let sender_balance = get_currency_balance(ctx.clone(), sender_addr, currency.clone())
                     .await
                     .unwrap();
                  ctx.write(|ctx| {
                     ctx.balance_db
                        .insert_currency_balance(sender_addr, sender_balance, &currency);
                  });
                  ctx.update_portfolio_value(chain.id(), sender_addr);
               }
               Err(e) => {
                  SHARED_GUI.write(|gui| {
                     gui.loading_window.reset();
                     gui.msg_window.open("Transaction Error", &e.to_string());
                  });
               }
            }
         });
      }
   }

   /// Validate the params and open the Review Tx window
   fn validate_and_send(&mut self, ctx: ZeusCtx, recipient: String) {
      match self.validate_params(ctx.clone(), recipient) {
         Ok(_) => {
            self.review_tx_window = true;
         }
         Err(e) => {
            std::thread::spawn(move || {
               SHARED_GUI.write(|gui| {
                  gui.msg_window.open("Transaction Error", &e.to_string());
               });
            });
            return;
         }
      };
   }
}
