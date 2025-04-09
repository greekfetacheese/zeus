use eframe::egui::{
   Align, Align2, Button, Color32, FontId, Frame, Grid, Layout, Margin, Order, Response, RichText, Spinner, TextEdit,
   Ui, Window, vec2,
};

use std::str::FromStr;
use std::sync::Arc;

use crate::core::{
   ZeusCtx,
   utils::{
      RT, estimate_gas_cost,
      eth::{self, get_currency_balance, send_crypto},
      tx::{TxMethod, TxParams},
   },
};

use crate::assets::icons::Icons;
use crate::gui::{
   SHARED_GUI,
   ui::{
      ContactsUi, GREEN_CHECK, RecipientSelectionWindow, TokenSelectionWindow,
      misc::{ChainSelect, WalletSelect},
   },
};
use egui_theme::{Theme, utils::*};
use egui_widgets::Label;

use zeus_eth::{
   alloy_primitives::{Address, Bytes, U256, utils::parse_units},
   currency::{Currency, NativeCurrency},
   utils::NumericValue,
};

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
   /// Flag to not spam the rpc when fetching pool data
   pub pool_data_syncing: bool,
   pub syncing_balance: bool,
   /// Review Transaction window
   pub review_tx_window: bool,
   pub progress_window: ProgressWindow,
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
         pool_data_syncing: false,
         syncing_balance: false,
         review_tx_window: false,
         progress_window: ProgressWindow::new("send_crypto_progress_window".to_string()),
      }
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

      recipient_selection.show(ctx.clone(), theme, icons.clone(), contacts_ui, ui);
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
      self.progress_window.show(theme, icons.clone(), ui);

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
               ui.label(RichText::new("Send Crypto").size(theme.text_sizes.heading));
               ui.add_space(20.0);
            });

            #[cfg(feature = "dev")]
            {
               ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                  if ui
                     .add(Button::new(
                        RichText::new("Test Progress Window").size(theme.text_sizes.small),
                     ))
                     .clicked()
                  {
                     test_progress_window();
                  }
               });
            }

            // Chain Selection
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
               ui.label(RichText::new("Chain").size(theme.text_sizes.large));
            });
            ui.add_space(5.0);

            ui.scope(|ui| {
               widget_visuals(ui, theme.get_widget_visuals(bg_color));
               let changed = self.chain_select.show(0, theme, icons.clone(), ui);
               if changed {
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
               ui.label(RichText::new("From").size(theme.text_sizes.large));
            });
            ui.add_space(5.0);

            ctx.read(|ctx| {
               let wallets = &ctx.account.wallets;
               ui.scope(|ui| {
                  widget_visuals(ui, theme.get_widget_visuals(bg_color));
                  ui.spacing_mut().button_padding = vec2(10.0, 12.0);
                  self.wallet_select.show(theme, wallets, icons.clone(), ui);
               });
            });
            ui.add_space(space);

            // Recipient Input
            Grid::new("recipient_name")
               .spacing(vec2(3.0, 0.0))
               .show(ui, |ui| {
                  ui.label(RichText::new("To").size(theme.text_sizes.large));
                  if !recipient.is_empty() {
                     if let Some(name) = &recipient_name {
                        ui.label(RichText::new(name.clone()).size(theme.text_sizes.normal));
                     } else {
                        ui.label(
                           RichText::new("Unknown Address")
                              .size(theme.text_sizes.normal)
                              .color(Color32::RED),
                        );
                     }

                     if !self.valid_recipient(recipient.clone()) {
                        ui.label(
                           RichText::new("Invalid Address")
                              .size(theme.text_sizes.normal)
                              .color(Color32::RED),
                        );
                     }

                     if self.recipient_is_sender(recipient.clone()) {
                        ui.label(
                           RichText::new("Cannot send to yourself")
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
               ui.label(RichText::new("Asset").size(theme.text_sizes.large));
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
               .spacing(vec2(10.0, 0.0))
               .show(ui, |ui| {
                  ui.label(RichText::new("Amount").size(theme.text_sizes.large));
                  widget_visuals(ui, theme.get_button_visuals(bg_color));
                  ui.spacing_mut().button_padding = vec2(5.0, 5.0);
                  let max_button = Button::new(RichText::new("Max").size(theme.text_sizes.small));
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

            // Amount check
            if !self.amount.is_empty() && !self.valid_amount() {
               ui.label(
                  RichText::new("Invalid Amount")
                     .size(theme.text_sizes.small)
                     .color(Color32::RED),
               );
            }

            // Balance check
            if !self.sufficient_balance(ctx.clone()) {
               ui.label(
                  RichText::new("Insufficient balance")
                     .size(theme.text_sizes.small)
                     .color(Color32::RED),
               );
            }

            ui.add_space(space);

            // Priority Fee
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
               ui.label(
                  RichText::new("Priority Fee")
                     .color(theme.colors.text_secondary)
                     .size(theme.text_sizes.large),
               );
               ui.add_space(2.0);
               ui.label(RichText::new("Gwei").size(theme.text_sizes.small));
            });
            ui.add_space(5.0);

            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
               ui.set_width(ui_width * 0.2);
               ui.add(
                  TextEdit::singleline(&mut self.priority_fee)
                     .min_size(vec2(ui_width * 0.2, 25.0))
                     .margin(Margin::same(10))
                     .background_color(theme.colors.text_edit_bg2)
                     .font(egui::FontId::proportional(theme.text_sizes.normal)),
               );
               ui.add_space(5.0);
               ui.spacing_mut().button_padding = vec2(5.0, 5.0);
               widget_visuals(ui, theme.get_button_visuals(bg_color));
               if ui
                  .add(Button::new(
                     RichText::new("Reset").size(theme.text_sizes.small),
                  ))
                  .clicked()
               {
                  self.reset_priority_fee(ctx.clone());
               }
            });
            if !self.priority_fee.is_empty() {
               if self.fee_is_zero() {
                  ui.label(
                     RichText::new("Fee cannot be zero")
                        .size(theme.text_sizes.normal)
                        .color(Color32::RED),
                  );
               }
            }
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
               Button::new(RichText::new("Send").size(theme.text_sizes.normal)).min_size(vec2(ui_width * 0.9, 40.0));

            if !self.valid_inputs(ctx.clone(), recipient) {
               ui.disable();
            }

            ui.vertical_centered(|ui| {
               if ui.add(send).clicked() {
                  self.review_tx_window = true;
               }
            });
            ui.add_space(space);
         });
   }

   fn token_button(&mut self, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) -> Response {
      let icon = icons.currency_icon(&self.currency);
      let button = Button::image_and_text(
         icon,
         RichText::new(self.currency.symbol()).size(theme.text_sizes.normal),
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

         RT.spawn_blocking(move || {
            ctx.update_portfolio_value(chain, owner);
            ctx.save_portfolio_db();
         });
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
                     RT.spawn_blocking(move || {
                        ctx.update_portfolio_value(chain_id, owner);
                        let _ = ctx.save_pool_manager();
                        ctx.save_portfolio_db();
                     });
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
      let gas_used = if self.currency.is_native() {
         chain.transfer_gas()
      } else {
         chain.erc20_transfer_gas()
      };

      estimate_gas_cost(ctx, chain.id(), gas_used, fee)
   }

   fn valid_recipient(&self, recipient: String) -> bool {
      let recipient = Address::from_str(&recipient).unwrap_or(Address::ZERO);
      recipient != Address::ZERO
   }

   fn recipient_is_sender(&self, recipient: String) -> bool {
      let recipient = Address::from_str(&recipient).unwrap_or(Address::ZERO);
      recipient == self.wallet_select.wallet.key.borrow().address()
   }

   fn valid_amount(&self) -> bool {
      let amount = self.amount.parse().unwrap_or(0.0);
      amount > 0.0
   }

   fn fee_is_zero(&self) -> bool {
      let fee = self.priority_fee.parse().unwrap_or(0.0);
      let chain = self.chain_select.chain;
      if chain.uses_priority_fee() {
         fee == 0.0
      } else {
         false
      }
   }

   /// Reset priority fee to the suggested fee
   fn reset_priority_fee(&mut self, ctx: ZeusCtx) {
      let chain = self.chain_select.chain.id();
      let fee = ctx.get_priority_fee(chain).unwrap_or_default();
      self.priority_fee = fee.formatted().clone();
   }

   fn valid_inputs(&self, ctx: ZeusCtx, recipient: String) -> bool {
      self.valid_recipient(recipient.clone())
         && self.valid_amount()
         && self.sufficient_balance(ctx)
         && !self.recipient_is_sender(recipient)
   }

   fn sufficient_balance(&self, ctx: ZeusCtx) -> bool {
      let sender = self.wallet_select.wallet.key.borrow().address();
      let balance = ctx
         .get_eth_balance(self.chain_select.chain.id(), sender)
         .unwrap_or_default();
      let amount = NumericValue::parse_to_wei(&self.amount, self.currency.decimals());
      let amount = amount.wei().unwrap();
      balance.wei().unwrap() >= amount
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
      let currency = self.currency.clone();

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
               ui.label(RichText::new("You are about to send").size(theme.text_sizes.heading));

               // TODO: center this
               ui.add_sized(vec2(width * 0.33, 20.0), |ui: &mut Ui| {
                  let res = Grid::new("amount_send")
                     .spacing(vec2(5.0, 0.0))
                     .show(ui, |ui| {
                        ui.label(
                           RichText::new(format!("{} {}", amount, currency.symbol())).size(theme.text_sizes.large),
                        );
                        ui.add(icons.currency_icon(&currency));
                        ui.label(RichText::new(format!("${}", value.formatted())).size(theme.text_sizes.normal));
                        ui.end_row();
                     });
                  res.response
               });

               ui.label(RichText::new("To").size(theme.text_sizes.large));

               if let Some(name) = recipient_name {
                  ui.label(RichText::new(name).size(theme.text_sizes.large));
               } else {
                  ui.label(RichText::new(recipient.clone()).size(theme.text_sizes.large));
               }

               let confirm = Button::new(RichText::new("Confirm").size(theme.text_sizes.normal));
               if ui.add(confirm).clicked() {
                  should_send_tx = true;
                  self.review_tx_window = false;
               }

               let cancel = Button::new(RichText::new("Cancel").size(theme.text_sizes.normal));
               if ui.add(cancel).clicked() {
                  self.review_tx_window = false;
               }
            });
         });

      if should_send_tx {
         let from = self.wallet_select.wallet.clone();
         let to = Address::from_str(&recipient).unwrap_or(Address::ZERO);
         let amount = NumericValue::parse_to_wei(&self.amount, self.currency.decimals());
         let currency = self.currency.clone();
         let chain = self.chain_select.chain;
         let fee = self.priority_fee.clone();

         let fee = if fee.is_empty() {
            parse_units("1", "gwei").unwrap().get_absolute()
         } else {
            parse_units(&fee, "gwei")
               .unwrap_or(parse_units("1", "gwei").unwrap())
               .get_absolute()
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

         RT.spawn(async move {
            let _ = send_crypto(
               ctx.clone(),
               currency.clone(),
               recipient_address,
               params.clone(),
            )
            .await;
         });
      }
   }
}

pub struct ProgressWindow {
   pub id: String,
   pub open: bool,
   pub sending: bool,
   pub sending_done: bool,
   pub currency: Currency,
   pub amount: NumericValue,
   pub amount_usd: NumericValue,
   pub size: (f32, f32),
}

impl ProgressWindow {
   pub fn new(id: String) -> Self {
      Self {
         id,
         open: false,
         sending: true,
         sending_done: false,
         currency: Currency::from(NativeCurrency::from_chain_id(1).unwrap()),
         amount: NumericValue::default(),
         amount_usd: NumericValue::default(),
         size: (350.0, 150.0),
      }
   }

   /// Open the progress window
   pub fn open(&mut self) {
      self.open = true;
   }

   pub fn set_currency(&mut self, currency: Currency) {
      self.currency = currency;
   }

   pub fn set_amount(&mut self, amount: NumericValue) {
      self.amount = amount;
   }

   pub fn set_amount_usd(&mut self, amount: NumericValue) {
      self.amount_usd = amount;
   }

   pub fn sending(&mut self) {
      self.sending = true;
      self.sending_done = false;
   }

   pub fn done_sending(&mut self) {
      self.sending = false;
      self.sending_done = true;
   }

   pub fn reset_and_close(&mut self) {
      self.open = false;
      self.sending();
   }

   pub fn show(&mut self, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      if !self.open {
         return;
      }

      Window::new(&self.id)
         .title_bar(false)
         .movable(false)
         .resizable(false)
         .collapsible(false)
         .order(Order::Foreground)
         .frame(Frame::window(ui.style()))
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);
            ui.vertical_centered(|ui| {
               ui.add_space(20.0);
               ui.spacing_mut().item_spacing.y = 20.0;
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);
               let ui_width = ui.available_width();

               ui.add_sized(vec2(ui_width * 0.6, self.size.1), |ui: &mut Ui| {
                  let res = Grid::new("across_progress_window")
                     .spacing(vec2(5.0, 10.0))
                     .show(ui, |ui| {
                        ui.label(RichText::new("Sending Transaction").size(theme.text_sizes.normal));
                        if self.sending {
                           ui.add(Spinner::new().size(17.0).color(Color32::WHITE));
                        }
                        if self.sending_done {
                           ui.label(RichText::new(GREEN_CHECK).size(theme.text_sizes.normal));
                        }
                        ui.end_row();

                        let text = format!(
                           "You have sent {} {} (${})",
                           self.amount.formatted(),
                           self.currency.symbol(),
                           self.amount_usd.formatted(),
                        );
                        let text = RichText::new(text).size(theme.text_sizes.normal);

                        if self.sending_done {
                           let icon = icons.currency_icon(&self.currency);
                           ui.add(Label::new(text, Some(icon)).selectable(false));
                           ui.end_row();
                        }
                     });
                  res.response
               });

               if ui
                  .add(Button::new(
                     RichText::new("Ok").size(theme.text_sizes.normal),
                  ))
                  .clicked()
               {
                  self.reset_and_close();
               }
            });
         });
   }
}

#[cfg(feature = "dev")]
fn test_progress_window() {
   RT.spawn(async move {
      SHARED_GUI.write(|gui| {
         gui.send_crypto.progress_window.open();
      });

      tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

      SHARED_GUI.write(|gui| {
         gui.send_crypto.progress_window.done_sending();
      });
   });
}
