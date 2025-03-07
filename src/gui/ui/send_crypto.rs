use eframe::egui::{
   Align, Align2, Button, Color32, FontId, Grid, Layout, Margin, Response, RichText, ScrollArea, SelectableLabel,
   TextEdit, Ui, Window, vec2,
};

use std::str::FromStr;
use std::sync::Arc;

use crate::core::{
   Contact, Wallet, ZeusCtx,
   utils::{RT, eth::send_crypto},
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
use egui_theme::{
   Theme,
   utils::*,
};

use zeus_eth::{
   alloy_primitives::{
      Address, U256,
      utils::{format_units, parse_units},
   },
   currency::{Currency, ERC20Token, NativeCurrency},
   utils::NumericValue,
};

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
            .spacing(vec2(0.0, 0.0))
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
                  .font(egui::FontId::proportional(theme.text_sizes.normal))
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
         let price = ctx.get_currency_price(&self.currency);
         let amount: f64 = self.amount.parse().unwrap_or(0.0);
         let value = if price.float() == 0.0 || amount == 0.0 {
            0.0
         } else {
            price.float() * amount
         };

         // Value
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.label(
               RichText::new(format!("Value≈ ${:.2}", value))
                  .color(theme.colors.text_secondary)
                  .size(theme.text_sizes.normal)
                  .strong(),
            );
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

   // TODO: Actually calulcate the cost based on the base fee + priority fee and the actual gas used
   fn cost(&self, ctx: ZeusCtx) -> NumericValue {
      let fee = if self.priority_fee.is_empty() {
         parse_units("1", "gwei").unwrap().get_absolute()
      } else {
         parse_units(&self.priority_fee, "gwei")
            .unwrap_or(parse_units("1", "gwei").unwrap())
            .get_absolute()
      };

      let chain = self.chain_select.chain;

      if self.currency.is_native() {
         let token = ERC20Token::native_wrapped_token(chain.id());
         let gas = U256::from(chain.transfer_gas());
         // cost in wei
         let cost = gas * fee;
         let cost = format_units(cost, token.decimals).unwrap_or_default();

         // cost in usd
         let cost: f64 = cost.parse().unwrap_or_default();
         let price = ctx.get_token_price(&token).float();
         // cost * price
         NumericValue::currency_value(cost, price)
      } else {
         let token = self.currency.erc20().unwrap();
         let gas = U256::from(chain.erc20_transfer_gas());
         // cost in wei
         let cost = gas * fee;
         let cost = format_units(cost, token.decimals).unwrap_or_default();

         // cost in usd
         let cost: f64 = cost.parse().unwrap_or_default();
         let price = ctx.get_token_price(&token).float();
         // cost * price
         NumericValue::currency_value(cost, price)
      }
   }

   fn token_button(&mut self, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) -> Response {
      let icon = icons.currency_icon(&self.currency);
      let button = img_button(
         icon,
         rich_text(self.currency.symbol()).size(theme.text_sizes.normal),
      );
      ui.add(button)
   }

   fn send(&self, ctx: ZeusCtx) {
      let from = self.wallet_select.wallet.clone();
      let to = Address::from_str(&self.recipient).unwrap_or(Address::ZERO);
      let amount = U256::from_str(&self.amount).unwrap_or_default();
      let currency = self.currency.clone();
      let chain = self.chain_select.chain.id();
      let fee = self.priority_fee.clone();

      RT.spawn(async move {
         open_loading(true, "Sending Transaction...".into());
         match send_crypto(ctx, from, to, currency, amount, fee, chain).await {
            Ok(_) => {
               let mut gui = SHARED_GUI.write().unwrap();
               gui.loading_window.reset();
               gui.msg_window.open("Transaction Sent", "");
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
                     .margin(Margin::same(3))
                     .font(FontId::proportional(theme.text_sizes.normal)),
               );
               ui.add_space(20.0);
            });

            ui.vertical_centered(|ui| {
               ScrollArea::vertical().max_height(350.0).show(ui, |ui| {
                  ui.spacing_mut().item_spacing.y = 16.0;

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
                              if ui.add(SelectableLabel::new(false, name)).clicked() {
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
                              if ui.add(SelectableLabel::new(false, name)).clicked() {
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
                     if ui.add(SelectableLabel::new(false, address_text)).clicked() {
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
