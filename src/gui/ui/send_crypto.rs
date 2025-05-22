use eframe::egui::{
   Align, Align2, Button, Color32, FontId, Frame, Grid, Layout, Margin, Response, RichText,
   Spinner, TextEdit, Ui, Window, vec2,
};

use std::str::FromStr;
use std::sync::Arc;

use crate::core::{
   ZeusCtx,
   utils::{
      RT,
      estimate_tx_cost,
      eth::{self, get_currency_balance},
   },
};

use crate::assets::icons::Icons;
use crate::gui::{
   SHARED_GUI,
   ui::{ContactsUi, RecipientSelectionWindow, TokenSelectionWindow},
};
use egui_theme::{Theme, utils::*};

use zeus_eth::{
   alloy_primitives::{Address, Bytes, U256},
   amm::DexKind,
   currency::{Currency, NativeCurrency},
   utils::NumericValue,
};

pub struct SendCryptoUi {
   pub open: bool,
   pub currency: Currency,
   pub amount: String,
   pub recipient: String,
   pub recipient_name: Option<String>,
   pub search_query: String,
   pub size: (f32, f32),
   /// Flag to not spam the rpc when fetching pool data
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
         size: (500.0, 750.0),
         pool_data_syncing: false,
         syncing_balance: false,
      }
   }

   pub fn set_currency(&mut self, currency: Currency) {
      self.currency = currency;
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

      let owner = ctx.current_wallet().address;

      recipient_selection.show(ctx.clone(), theme, icons.clone(), contacts_ui, ui);
      let recipient = recipient_selection.get_recipient();
      let recipient_name = recipient_selection.get_recipient_name();

      let frame = theme.frame1;
      let bg_color = frame.fill;
      Window::new("send_crypto_ui")
         .title_bar(false)
         .resizable(false)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(frame)
         .show(ui.ctx(), |ui| {
            Frame::new().inner_margin(Margin::same(10)).show(ui, |ui| {
               let ui_width = self.size.0;
               let space = 15.0;
               ui.set_max_width(ui_width);
               ui.set_max_height(self.size.1);
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);

               // Title
               ui.vertical_centered(|ui| {
                  ui.label(RichText::new("Send Crypto").size(theme.text_sizes.heading));
                  ui.add_space(20.0);
               });

               ui.add_space(space);

               let chain = ctx.chain();
               let currencies = ctx.get_currencies(chain.id());

               // Recipient Input
               Grid::new("recipient_name")
                  .spacing(vec2(3.0, 0.0))
                  .show(ui, |ui| {
                     ui.label(RichText::new("Recipient  ").size(theme.text_sizes.large));
                     if !recipient.is_empty() {
                        if let Some(name) = &recipient_name {
                           ui.label(
                              RichText::new(name.clone())
                                 .size(theme.text_sizes.normal)
                                 .strong(),
                           );
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
               ui.add_space(5.0);

               if !recipient.is_empty() {
                  if !self.valid_recipient(&recipient) {
                     ui.label(
                        RichText::new("Invalid Address")
                           .size(theme.text_sizes.normal)
                           .color(Color32::RED),
                     );
                  }

                  if self.recipient_is_sender(owner, &recipient) {
                     ui.label(
                        RichText::new("Cannot send to yourself")
                           .size(theme.text_sizes.normal)
                           .color(Color32::RED),
                     );
                  }
               }

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

                     let balance = ctx.get_currency_balance(chain.id(), owner, &self.currency);
                     ui.label(
                        RichText::new(format!("Balance: {}", balance.format_abbreviated()))
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
                     let max_button =
                        Button::new(RichText::new("Max").size(theme.text_sizes.small));
                     if ui.add(max_button).clicked() {
                        self.amount = self.max_amount(owner, ctx.clone()).flatten().clone();
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
               if !self.sufficient_balance(ctx.clone(), owner) {
                  ui.label(
                     RichText::new("Insufficient balance")
                        .size(theme.text_sizes.small)
                        .color(Color32::RED),
                  );
               }

               ui.add_space(space);

               // Value
               let value = self.value(owner, ctx.clone());
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

               // Send Button
               widget_visuals(ui, theme.get_button_visuals(bg_color));
               let send = Button::new(RichText::new("Send").size(theme.text_sizes.normal))
                  .min_size(vec2(ui_width * 0.9, 40.0));

               if !self.valid_inputs(ctx.clone(), owner, &recipient) {
                  ui.disable();
               }

               ui.vertical_centered(|ui| {
                  if ui.add(send).clicked() {
                     self.send_transaction(ctx, recipient);
                  }
               });
               // ui.add_space(space);
            });
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

   fn sync_balance(&mut self, owner: Address, ctx: ZeusCtx) {
      self.syncing_balance = true;
      let currency = self.currency.clone();
      RT.spawn(async move {
         match get_currency_balance(ctx.clone(), owner, currency.clone()).await {
            Ok(_) => {
               SHARED_GUI.write(|gui| {
                  gui.send_crypto.syncing_balance = false;
               });
            }
            Err(e) => {
               tracing::error!("Error getting balance: {:?}", e);
               SHARED_GUI.write(|gui| {
                  gui.send_crypto.syncing_balance = false;
               });
               return;
            }
         };
      });
   }

   fn value(&mut self, owner: Address, ctx: ZeusCtx) -> NumericValue {
      let price = ctx.get_currency_price_opt(&self.currency);
      let amount = self.amount.parse().unwrap_or(0.0);

      if amount == 0.0 {
         return NumericValue::default();
      }

      if !price.is_none() {
         return NumericValue::value(amount, price.unwrap().f64());
      } else {
         // no pool data available to calculate the price

         // don't spam the rpc in the next frames
         if self.pool_data_syncing {
            return NumericValue::default();
         }

         let currency = self.currency.clone();
         let pools = ctx.write(|ctx| ctx.pool_manager.get_pools_that_have_currency(&currency));
         let chain_id = ctx.chain().id();

         if pools.is_empty() {
            let token = currency.to_erc20().into_owned();
            self.pool_data_syncing = true;
            let manager = ctx.pool_manager();
            let dexes = DexKind::main_dexes(chain_id);

            RT.spawn(async move {
            let client = ctx.get_client(chain_id).await.unwrap();
               match manager
                  .sync_pools_for_tokens(client.clone(), chain_id, vec![token], dexes, false)
                  .await
               {
                  Ok(_) => {
                     SHARED_GUI.write(|gui| {
                        gui.send_crypto.pool_data_syncing = false;
                     });

                     let _ = manager.update(client, chain_id).await;

                     RT.spawn_blocking(move || {
                        ctx.calculate_portfolio_value(chain_id, owner);
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
   fn max_amount(&self, owner: Address, ctx: ZeusCtx) -> NumericValue {
      let chain = ctx.chain();
      let currency = self.currency.clone();
      let gas_used = if currency.is_native() {
         chain.transfer_gas()
      } else {
         chain.erc20_transfer_gas()
      };

      let fee = ctx.get_priority_fee(chain.id()).unwrap_or_default();
      let (cost_wei, _) = estimate_tx_cost(ctx.clone(), chain.id(), gas_used, fee.wei2());

      let balance = ctx.get_currency_balance(chain.id(), owner, &currency);

      if currency.is_erc20() {
         return balance;
      } else {
         if balance.wei().unwrap() < cost_wei.wei2() {
            return NumericValue::default();
         }
         let max = balance.wei().unwrap() - cost_wei.wei2();
         return NumericValue::format_wei(max, currency.decimals());
      }
   }

   fn valid_recipient(&self, recipient: &String) -> bool {
      let recipient = Address::from_str(&recipient).unwrap_or(Address::ZERO);
      recipient != Address::ZERO
   }

   fn recipient_is_sender(&self, owner: Address, recipient: &String) -> bool {
      let recipient = Address::from_str(&recipient).unwrap_or(Address::ZERO);
      recipient == owner
   }

   fn valid_amount(&self) -> bool {
      let amount = self.amount.parse().unwrap_or(0.0);
      amount > 0.0
   }

   fn valid_inputs(&self, ctx: ZeusCtx, owner: Address, recipient: &String) -> bool {
      self.valid_recipient(recipient)
         && self.valid_amount()
         && self.sufficient_balance(ctx.clone(), owner)
         && !self.recipient_is_sender(owner, recipient)
   }

   fn sufficient_balance(&self, ctx: ZeusCtx, sender: Address) -> bool {
      let balance = ctx.get_currency_balance(ctx.chain().id(), sender, &self.currency);
      let amount = NumericValue::parse_to_wei(&self.amount, self.currency.decimals());
      balance.wei2() >= amount.wei2()
   }

   fn send_transaction(&mut self, ctx: ZeusCtx, recipient: String) {
      let chain = ctx.chain();
      let from = ctx.current_wallet().address;
      let currency = self.currency.clone();
      let recipient_address = Address::from_str(&recipient).unwrap_or(Address::ZERO);

      let amount = NumericValue::parse_to_wei(&self.amount, self.currency.decimals());
      let (call_data, interact_to) = if currency.is_native() {
         (Bytes::default(), recipient_address)
      } else {
         let c = currency.clone();
         let token = c.erc20().unwrap();
         let data = token.encode_transfer(recipient_address, amount.wei2());
         (data, token.address)
      };

      let value = if currency.is_native() {
         amount.wei2()
      } else {
         U256::ZERO
      };

      RT.spawn(async move {
         SHARED_GUI.write(|gui| {
            gui.loading_window.open("Wait while magic happens");
            gui.request_repaint();
         });

         match eth::send_crypto(
            ctx.clone(),
            chain,
            from,
            interact_to,
            call_data,
            value,
         )
         .await
         {
            Ok(_) => {}
            Err(e) => {
               tracing::error!("Error sending transaction: {:?}", e);
               SHARED_GUI.write(|gui| {
                  gui.progress_window.reset();
                  gui.loading_window.reset();
                  gui.msg_window.open("Transaction Error", &e.to_string());
               });
            }
         }
      });
   }
}
