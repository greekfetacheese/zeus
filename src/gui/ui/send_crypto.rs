use eframe::egui::{Align2, Color32, Frame, Id, Response, RichText, ScrollArea, TextEdit, Ui, Window, vec2};
use egui::{Button, FontId, Margin};

use std::str::FromStr;
use std::sync::Arc;

use crate::core::{
   ZeusCtx,
   Contact,
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
   utils::{border_on_hover, border_on_idle},
};

use zeus_eth::{
   alloy_primitives::{
      Address, U256,
      utils::{format_units, parse_units},
   },
   currency::{Currency, ERC20Token, NativeCurrency},
   types::ChainId,
   utils::NumericValue
};

pub struct SendCryptoUi {
   pub open: bool,
   pub chain: ChainId,
   pub chain_select: ChainSelect,
   pub wallet_select: WalletSelect,
   pub priority_fee: String,
   pub currency: Currency,
   pub amount: String,
   pub contact_search_open: bool,
   pub search_query: String,
   pub selected_contact: Option<Contact>,
   pub recipient: String,
}

impl SendCryptoUi {
   pub fn new(theme: &Theme) -> Self {
      let chain_select = ChainSelect::new("chain_select_2", None, Some(theme.colors.bg_color)).show_icon(false);
      Self {
         open: false,
         chain: ChainId::new(1).unwrap(),
         chain_select,
         wallet_select: WalletSelect::new("wallet_select_2", None, Some(theme.colors.bg_color)),
         priority_fee: "1".to_string(),
         currency: Currency::from_native(NativeCurrency::from_chain_id(1).unwrap()),
         amount: String::new(),
         contact_search_open: false,
         search_query: String::new(),
         recipient: String::new(),
         selected_contact: None,
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

      Window::new("Send Crypto")
         .id(Id::new("send_crypto_ui"))
         .collapsible(false)
         .resizable(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_width(400.0);
            ui.spacing_mut().item_spacing.y = 16.0;
            // increase the size of comboboxes and buttons
            ui.spacing_mut().button_padding = vec2(5.0, 5.0);

            ui.separator();

            // Chain Selection
            ui.label(
               rich_text("CHAIN")
                  .color(theme.colors.text_secondary)
                  .size(14.0),
            );

            let clicked = self.chain_select.show(ui, icons.clone());
            if clicked {
               let chain = self.chain_select.chain.id();
               self.currency = Currency::from_native(NativeCurrency::from_chain_id(chain).unwrap());
            }

            let chain = self.chain_select.chain.id();
            let owner = self.wallet_select.wallet.key.inner().address();
            let currencies = ctx.get_currencies(chain);

            // From Wallet
            ui.label(
               rich_text("FROM")
                  .color(theme.colors.text_secondary)
                  .size(14.0),
            );

            ctx.read(|ctx| {
               let wallets = &ctx.profile.wallets;
               self.wallet_select.show(wallets, ui);
            });

            // Recipient Input
            ui.label(
               rich_text("TO")
                  .color(theme.colors.text_secondary)
                  .size(14.0),
            );
            if let Some(contact) = &self.selected_contact {
               ui.label(RichText::new(contact.name.clone()).size(13.0));
            }

            let res = ui.add(
               TextEdit::singleline(&mut self.recipient)
                  .hint_text("Search contacts or enter address")
                  .min_size(vec2(300.0, 30.0))
                  .margin(Margin::same(3))
                  .font(FontId::proportional(13.0)),
            );

            if res.clicked() {
               self.contact_search_open = true;
            }

            // Integrated contact search dropdown
            if self.contact_search_open {
               let contacts = ctx.contacts();
               Frame::menu(ui.style()).show(ui, |ui| {
                  ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                     ui.set_width(300.0);
                     ui.scope(|ui| {
                        border_on_idle(ui, 1.0, theme.colors.border_color_idle);
                        border_on_hover(ui, 1.0, theme.colors.border_color_hover);
                        ui.add(
                           TextEdit::singleline(&mut self.search_query)
                              .hint_text("Search contacts")
                              .min_size(vec2(200.0, 30.0))
                              .margin(Margin::same(3))
                              .font(FontId::proportional(13.0)),
                        );
                     });

                     ui.separator();

                     for contact in contacts.iter().filter(|c| {
                        c.name
                           .to_lowercase()
                           .contains(&self.search_query.to_lowercase())
                     }) {
                        ui.horizontal(|ui| {
                           if ui.selectable_label(false, &contact.name).clicked() {
                              self.recipient = contact.address.clone();
                              self.selected_contact = Some(contact.clone());
                              self.contact_search_open = false;
                           }
                           ui.label(
                              RichText::new(contact.address_short())
                                 .color(theme.colors.text_secondary)
                                 .size(12.0),
                           );
                        });
                     }
                  });
               });
            }

            // Token Selection
            ui.vertical(|ui| {
               ui.label(
                  rich_text("ASSET")
                     .color(theme.colors.text_secondary)
                     .size(14.0),
               );
               ui.horizontal(|ui| {
                  egui_theme::utils::bg_color_on_idle(ui, Color32::TRANSPARENT);

                  // Token button with icon and balance
                  let response = self.token_button(icons.clone(), ui);
                  if response.clicked() {
                     token_selection.open = true;
                  }

                  token_selection.show(ctx.clone(), chain, owner, icons.clone(), &currencies, ui);

                  if let Some(currency) = token_selection.get_currency() {
                     self.currency = currency.clone();
                     token_selection.reset();
                  }

                  // Balance display
                  let balance = ctx.get_currency_balance(chain, owner, &self.currency);
                  ui.label(
                     RichText::new(format!("Balance: {}", balance.formatted()))
                        .color(theme.colors.text_secondary)
                        .size(13.0),
                  );
               });
            });

            // Amount Input
            ui.vertical(|ui| {
               ui.label(
                  rich_text("AMOUNT")
                     .color(theme.colors.text_secondary)
                     .size(14.0),
               );
               ui.add(
                  TextEdit::singleline(&mut self.amount)
                     .hint_text(rich_text("0.00").color(theme.colors.text_secondary))
                     .font(egui::FontId::proportional(20.0))
                     .desired_width(200.0),
               );

               // Priority Fee
               ui.horizontal(|ui| {
                  ui.label(
                     rich_text("Priority Fee")
                        .color(theme.colors.text_secondary)
                        .size(13.0),
                  );
                  ui.add(TextEdit::singleline(&mut self.priority_fee).desired_width(30.0));
                  ui.label(
                     rich_text("Gwei")
                        .color(theme.colors.text_secondary)
                        .size(13.0),
                  );
               });

               // Calculate the value
               let price = ctx.get_currency_price(&self.currency);
               let amount: f64 = self.amount.parse().unwrap_or(0.0);
               let value = if price.float() == 0.0 || amount == 0.0 {
                  0.0
               } else {
                  price.float() * amount
               };

               ui.label(
                  RichText::new(format!("Value≈ ${}", value))
                     .color(theme.colors.text_secondary)
                     .size(14.0)
                     .strong(),
               );
            });

            let cost = self.cost(ctx.clone());
            ui.label(
               RichText::new(format!("Estimated Cost≈ ${}", cost.formatted()))
                  .color(theme.colors.text_secondary)
                  .size(14.0)
                  .strong(),
            );

            // Send Button
            ui.horizontal(|ui| {
               let send = Button::new(rich_text("Send")).min_size(vec2(60.0, 30.0));
               if ui.add(send).clicked() {
                  self.send(ctx.clone());
               }
            });
         });
   }

   // TODO: Actually calulcate the cost based on the base fee + priority fee and the actual gas used
   fn cost(&self, ctx: ZeusCtx) -> NumericValue {
      let fee = if self.priority_fee.is_empty() {
         parse_units("1", "gwei").unwrap().get_absolute()
      } else {
         parse_units(&self.priority_fee, "gwei")
            .unwrap()
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

   fn token_button(&mut self, icons: Arc<Icons>, ui: &mut Ui) -> Response {
      let icon;
      let chain = self.chain_select.chain.id();
      if self.currency.is_native() {
         icon = icons.currency_icon(chain);
      } else {
         let token = self.currency.erc20().unwrap();
         icon = icons.token_icon(token.address, chain);
      }

      let button = img_button(icon, rich_text(self.currency.symbol()).size(13.0));
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
}
