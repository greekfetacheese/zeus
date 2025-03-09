use eframe::egui::{
   Align, Align2, Button, Color32, FontId, Frame, Grid, Layout, Margin, Response, RichText, ScrollArea, Spinner,
   TextEdit, Ui, Vec2, Window, vec2,
};

use std::str::FromStr;
use std::sync::Arc;

use crate::core::{
   Contact, Wallet, ZeusCtx,
   utils::{
      RT,
      eth::{self, get_currency_balance, send_crypto},
   },
};

use crate::assets::icons::Icons;
use crate::gui::{
   SHARED_GUI,
   ui::{
      TokenSelectionWindow, img_button,
      misc::{ChainSelect, WalletSelect},
      rich_text,
   },
   utils::open_loading,
};
use egui_theme::{Theme, utils::*};

use zeus_eth::{
   alloy_primitives::{
      Address, U256,
      utils::{format_units, parse_units},
   },
   currency::{Currency, ERC20Token, NativeCurrency},
   utils::NumericValue,
};

use super::button;

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
   pub contact_search_open: bool,
   pub search_query: String,
   /// Address in string format
   pub recipient: String,
   pub recipient_name: Option<String>,
   pub size: (f32, f32),
   pub tx_success_window: TxSuccessWindow,
   /// Flag to not spam the rpc when fetching pool data
   pub pool_data_syncing: bool,
}

impl SendCryptoUi {
   pub fn new() -> Self {
      Self {
         open: false,
         chain_select: ChainSelect::new("chain_select_2"),
         wallet_select: WalletSelect::new("wallet_select_2"),
         priority_fee: "1".to_string(),
         currency: Currency::from_native(NativeCurrency::from_chain_id(1).unwrap()),
         amount: String::new(),
         contact_search_open: false,
         search_query: String::new(),
         recipient: String::new(),
         recipient_name: None,
         size: (500.0, 750.0),
         tx_success_window: TxSuccessWindow::new(),
         pool_data_syncing: false,
      }
   }

   pub fn show(
      &mut self,
      ctx: ZeusCtx,
      icons: Arc<Icons>,
      theme: &Theme,
      token_selection: &mut TokenSelectionWindow,
      ui: &mut Ui,
   ) {
      if !self.open {
         return;
      }

      let frame = theme.frame1;
      let bg_color = frame.fill;
      ui.add_space(25.0);
      frame.show(ui, |ui| {
         let ui_width = self.size.0;
         let space = 15.0;
         ui.set_max_width(ui_width);
         ui.spacing_mut().button_padding = vec2(10.0, 8.0);

         // Title
         ui.label(rich_text("Send Crypto").size(theme.text_sizes.heading));

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
            let clicked = self.chain_select.show(theme, icons.clone(), ui);
            if clicked {
               let chain = self.chain_select.chain.id();
               self.currency = Currency::from_native(NativeCurrency::from_chain_id(chain).unwrap());
            }
         });

         ui.add_space(space);

         let chain = self.chain_select.chain.id();
         let owner = self.wallet_select.wallet.key.inner().address();
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
            let wallets = &ctx.profile.wallets;
            ui.scope(|ui| {
               widget_visuals(ui, theme.get_widget_visuals(bg_color));
               self.wallet_select.show(theme, wallets, icons.clone(), ui);
            });
         });
         ui.add_space(space);

         // Recipient Input
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.label(
               rich_text("To")
                  .color(theme.colors.text_secondary)
                  .size(theme.text_sizes.large),
            );
            if let Some(name) = &self.recipient_name {
               ui.label(RichText::new(name.clone()).size(theme.text_sizes.normal));
            }
         });
         ui.add_space(5.0);

         ui.horizontal(|ui| {
            widget_visuals(ui, theme.get_text_edit_visuals(bg_color));
            let res = ui.add(
               TextEdit::multiline(&mut self.recipient)
                  .hint_text("Search contacts or enter an address")
                  .desired_rows(2)
                  .desired_width(ui_width * 0.80)
                  .margin(Margin::same(5))
                  .font(FontId::proportional(theme.text_sizes.normal))
                  .background_color(theme.colors.text_edit_bg_color),
            );
            if res.clicked() {
               self.contact_search_open = true;
            }
         });
         ui.add_space(space);

         self.recipient_selection(ctx.clone(), theme, ui);

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
               token_selection.show(ctx.clone(), chain, owner, icons.clone(), &currencies, ui);
               if let Some(currency) = token_selection.get_currency() {
                  self.currency = currency.clone();
                  token_selection.reset();
               }
               ui.end_row();
            });
         ui.add_space(space);

         // Amount Input
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.label(
               rich_text("Amount")
                  .color(theme.colors.text_secondary)
                  .size(theme.text_sizes.large),
            );
         });
         ui.add_space(5.0);

         ui.horizontal(|ui| {
            widget_visuals(ui, theme.get_text_edit_visuals(bg_color));
            ui.add(
               TextEdit::singleline(&mut self.amount)
                  .hint_text("0")
                  .font(egui::FontId::proportional(theme.text_sizes.large))
                  .desired_width(ui_width * 0.25)
                  .margin(Margin::same(10))
                  .background_color(theme.colors.text_edit_bg_color),
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
            widget_visuals(ui, theme.get_text_edit_visuals(bg_color));
            ui.add(
               TextEdit::singleline(&mut self.priority_fee)
                  .desired_width(60.0)
                  .background_color(theme.colors.text_edit_bg_color)
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
         let cost = self.cost(ctx.clone());
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
         let send = Button::new(rich_text("Send").size(theme.text_sizes.normal)).min_size(vec2(ui_width - 20.0, 40.0)); // Full width minus padding
         if ui.add(send).clicked() {
            self.send(ctx.clone());
         }
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


   fn value(&mut self, ctx: ZeusCtx) -> NumericValue {
      let price = ctx.get_currency_price_opt(&self.currency);
      let amount = self.amount.parse().unwrap_or(0.0);

      if amount == 0.0 {
         return NumericValue::default();
      }

      if !price.is_none() {
         return NumericValue::currency_value(amount, price.unwrap().float());
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

         let v2_pools = ctx.get_v2_pools(token.clone());
         let v3_pools = ctx.get_v3_pools(token.clone());
         let owner = self.wallet_select.wallet.key.inner().address();
         let chain_id = self.chain_select.chain.id();

         if v2_pools.is_empty() || v3_pools.is_empty() {
         self.pool_data_syncing = true;
         RT.spawn(async move {
            match eth::get_pools_for_token(
               ctx.clone(),
               token.clone(),
               v2_pools.is_empty(),
               v3_pools.is_empty(),
            )
            .await
            {
               Ok(_) => {
                  {
                     let mut gui = SHARED_GUI.write().unwrap();
                     gui.send_crypto.pool_data_syncing = false;
                  }
                  let client = ctx.get_client_with_id(chain_id).unwrap();
                  let pool_manager = ctx.pool_manager();
                  pool_manager
                     .update_and_clean(client, chain_id)
                     .await
                     .unwrap();
                  ctx.update_portfolio_value(chain_id, owner);
                  let _ = ctx.save_pool_data();
                  let _ = ctx.save_portfolio_db();
               }
               Err(e) => {
                  tracing::error!("Error getting pools: {:?}", e);
                  let mut gui = SHARED_GUI.write().unwrap();
                  gui.send_crypto.pool_data_syncing = false;
               }
            };
         });
      }

         return NumericValue::default();
      }
   }

   fn cost(&self, ctx: ZeusCtx) -> NumericValue {
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
      let native_token = ERC20Token::native_wrapped_token(chain.id());
      let price = ctx
         .get_token_price(&native_token)
         .unwrap_or_default()
         .float();

      let cost = gas_used * fee;
      let cost = format_units(cost, native_token.decimals).unwrap_or_default();
      let cost: f64 = cost.parse().unwrap_or_default();

      // cost in usd
      NumericValue::currency_value(cost, price)
   }

   fn send(&self, ctx: ZeusCtx) {
      let from = self.wallet_select.wallet.clone();
      let recipient = self.recipient.clone();
      let to = Address::from_str(&recipient).unwrap_or(Address::ZERO);
      let amount = NumericValue::from_str(&self.amount, self.currency.decimals());
      let currency = self.currency.clone();
      let chain = self.chain_select.chain;
      let fee = self.priority_fee.clone();
      let explorer = chain.block_explorer().to_string();
      let ctx_clone = ctx.clone();

      RT.spawn(async move {
         open_loading("Sending Transaction...".into());
         match send_crypto(
            ctx_clone,
            from.clone(),
            to,
            currency.clone(),
            amount.uint().unwrap(),
            fee,
            chain.id(),
         )
         .await
         {
            Ok(tx) => {
               {
                  let mut gui = SHARED_GUI.write().unwrap();
                  gui.loading_window.reset();
                  let link = format!("{}/tx/{}", explorer, tx.transaction_hash);
                  gui.send_crypto.tx_success_window.open(link);
               }

               // if recipient is a wallet owned by the user then update the balance
               // Also update the sender's balance
               let profile = ctx.profile();
               if profile.wallet_address_exists(&recipient) {
                  let recipient_balance = get_currency_balance(ctx.clone(), to, currency.clone())
                     .await
                     .unwrap();

                  ctx.write(|ctx| {
                     ctx.balance_db
                        .insert_currency_balance(to, recipient_balance, &currency);
                  });
                  ctx.update_portfolio_value(chain.id(), to);
               }
               let sender = from.key.inner().address();
               let sender_balance = get_currency_balance(ctx.clone(), sender, currency.clone())
                  .await
                  .unwrap();
               ctx.write(|ctx| {
                  ctx.balance_db
                     .insert_currency_balance(sender, sender_balance, &currency);
               });
               ctx.update_portfolio_value(chain.id(), sender);
            }
            Err(e) => {
               let mut gui = SHARED_GUI.write().unwrap();
               gui.loading_window.reset();
               gui.msg_window.open("Transaction Error", &e.to_string());
            }
         }
      });
   }

   /// Recipient selection window
   fn recipient_selection(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      let contacts = ctx.contacts();
      let wallets = ctx.profile().wallets;

      let mut open = self.contact_search_open;
      let mut close_it = false;
      let frame = theme.frame2;
      let bg_color = frame.fill;
      Window::new("Recipient")
         .open(&mut open)
         .collapsible(false)
         .resizable(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(frame)
         .show(ui.ctx(), |ui| {
            ui.set_width(450.0);
            ui.set_height(350.0);

            ui.vertical_centered(|ui| {
               widget_visuals(ui, theme.get_text_edit_visuals(bg_color));
               ui.add_space(20.0);
               ui.add(
                  TextEdit::multiline(&mut self.search_query)
                     .hint_text("Search contacts or enter an address")
                     .desired_rows(2)
                     .min_size(vec2(200.0, 30.0))
                     .margin(Margin::same(10))
                     .font(FontId::proportional(theme.text_sizes.normal)),
               );
               ui.add_space(20.0);
            });

            ui.vertical_centered(|ui| {
               ScrollArea::vertical().max_height(350.0).show(ui, |ui| {
                  ui.spacing_mut().item_spacing.y = 16.0;
                  ui.spacing_mut().button_padding = vec2(10.0, 8.0);
                  widget_visuals(ui, theme.get_button_visuals(bg_color));

                  // First show the wallets owned by the current account
                  ui.label(rich_text("Your Wallets").size(theme.text_sizes.large));
                  for wallet in &wallets {
                     let valid_search = valid_wallet_search(wallet, &self.search_query);

                     if valid_search {
                        let address = wallet.key.inner().address();
                        // exclude the current wallet
                        if address == self.wallet_select.wallet.key.inner().address() {
                           continue;
                        }

                        ui.add_sized(vec2(200.0, 10.0), |ui: &mut Ui| {
                           let res = ui.horizontal(|ui| {
                              let name = rich_text(wallet.name.clone()).size(theme.text_sizes.normal);
                              let button = button(name);
                              if ui.add(button).clicked() {
                                 self.recipient = address.to_string();
                                 self.recipient_name = Some(wallet.name.clone());
                                 close_it = true;
                              }
                              ui.label(
                                 RichText::new(wallet.address_truncated())
                                    .size(theme.text_sizes.normal)
                                    .strong(),
                              );
                           });
                           res.response
                        });
                     }
                  }

                  // Then show the contacts
                  ui.label(rich_text("Contacts").size(theme.text_sizes.large));
                  for contact in &contacts {
                     let valid_search = valid_contact_search(contact, &self.search_query);

                     if valid_search {
                        ui.add_sized(vec2(200.0, 10.0), |ui: &mut Ui| {
                           let res = ui.horizontal(|ui| {
                              let name = rich_text(contact.name.clone()).size(theme.text_sizes.normal);
                              let button = button(name);
                              if ui.add(button).clicked() {
                                 self.recipient = contact.address.clone();
                                 self.recipient_name = Some(contact.name.clone());
                                 close_it = true;
                              }
                              ui.label(
                                 RichText::new(contact.address_short())
                                    .size(theme.text_sizes.normal)
                                    .strong(),
                              );
                           });
                           res.response
                        });
                     }
                  }

                  // When a valid address is pasted
                  // TODO FIX: If this address exists on wallets or contacts it will show up both times
                  if let Ok(address) = Address::from_str(&self.search_query) {
                     ui.label(rich_text("Unknown Address").size(theme.text_sizes.large));
                     let address_text = rich_text(address.to_string()).size(theme.text_sizes.normal);
                     let button = button(address_text);
                     if ui.add(button).clicked() {
                        self.recipient = address.to_string();
                        self.recipient_name = None;
                        close_it = true;
                     }
                  }
               });
            });
         });

      if close_it {
         open = false;
      }
      self.contact_search_open = open;
   }
}

fn valid_contact_search(contact: &Contact, query: &str) -> bool {
   let query = query.to_lowercase();

   if query.is_empty() {
      return true;
   }

   contact.name.to_lowercase().contains(&query) || contact.address.to_lowercase().contains(&query)
}

fn valid_wallet_search(wallet: &Wallet, query: &str) -> bool {
   let query = query.to_lowercase();

   if query.is_empty() {
      return true;
   }

   wallet.name.to_lowercase().contains(&query) || wallet.address().to_lowercase().contains(&query)
}
