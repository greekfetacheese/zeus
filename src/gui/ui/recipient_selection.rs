use crate::assets::icons::Icons;
use crate::core::{user::Contact, WalletInfo, ZeusCtx};
use crate::gui::ui::ContactsUi;
use eframe::egui::{
   Align, Align2, Button, FontId, Frame, Layout, Margin, Order, RichText, ScrollArea, TextEdit, Ui,
   Window, vec2,
};
use egui_theme::Theme;
use std::str::FromStr;
use std::sync::Arc;
use zeus_eth::{alloy_primitives::Address, types::SUPPORTED_CHAINS, utils::NumericValue};

pub struct RecipientSelectionWindow {
   open: bool,
   pub recipient: String,
   recipient_name: Option<String>,
   search_query: String,
   size: (f32, f32),
}

impl RecipientSelectionWindow {
   pub fn new() -> Self {
      Self {
         open: false,
         recipient: String::new(),
         recipient_name: None,
         search_query: String::new(),
         size: (500.0, 550.0),
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
   }

   pub fn reset(&mut self) {
      self.recipient = String::new();
      self.recipient_name = None;
      self.search_query.clear();
   }

   pub fn get_recipient(&self) -> String {
      self.recipient.clone()
   }

   pub fn get_recipient_name(&self) -> Option<String> {
      self.recipient_name.clone()
   }

   pub fn show(
      &mut self,
      ctx: ZeusCtx,
      theme: &Theme,
      icons: Arc<Icons>,
      contacts_ui: &mut ContactsUi,
      ui: &mut Ui,
   ) {
      let mut open = self.open;
      let mut close_window = false;

      contacts_ui.add_contact.show(ctx.clone(), theme, false, ui);
      let contact_added = contacts_ui.add_contact.contact_added();
      if contact_added {
         let contact = contacts_ui.add_contact.get_contact().clone();
         self.recipient = contact.address;
         self.recipient_name = Some(contact.name);
         contacts_ui.add_contact.reset();
         open = false;
      }

      let title = RichText::new("Recipient").size(theme.text_sizes.large);
      Window::new(title)
         .open(&mut open)
         .order(Order::Foreground)
         .resizable(false)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(Frame::window(ui.style()).inner_margin(Margin::same(10)))
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);
            ui.spacing_mut().item_spacing = vec2(10.0, 20.0);
            ui.spacing_mut().button_padding = vec2(10.0, 8.0);
            let column_width = ui.available_width() * 0.33;

            // Search bar
            ui.vertical_centered(|ui| {
               ui.add_space(20.0);

               let add_contact =
                  Button::new(RichText::new("Add a contact").size(theme.text_sizes.normal));

               if ui.add(add_contact).clicked() {
                  contacts_ui.add_contact.open = true;
               }

               ui.add(
                  TextEdit::singleline(&mut self.search_query)
                     .hint_text("Search contacts or enter an address")
                     .min_size(vec2(ui.available_width() * 0.80, 25.0))
                     .margin(Margin::same(10))
                     .font(FontId::proportional(theme.text_sizes.normal)),
               );

               let wallets = ctx.wallets_info();
               let contacts = ctx.contacts();

               let query = self.search_query.clone();
               let are_valid_contacts = contacts.iter().any(|c| valid_contact_search(c, &query));
               let are_valid_wallets =
                  wallets.len() >= 1 && wallets.iter().any(|w| valid_wallet_search(w, &query));

               ScrollArea::vertical()
                  .id_salt("recipient_select_scroll")
                  .max_height(self.size.1)
                  .max_width(ui.available_width())
                  .show(ui, |ui| {
                     if are_valid_wallets {
                        self.account_wallets(
                           ctx.clone(),
                           theme,
                           icons.clone(),
                           column_width,
                           &mut close_window,
                           ui,
                        );
                     }

                     if are_valid_contacts {
                        self.account_contacts(ctx.clone(), theme, &mut close_window, ui);
                     }
                  });

               if let Ok(address) = Address::from_str(&self.search_query) {
                  ui.label(RichText::new("Unknown Address").size(theme.text_sizes.large));

                  let address_text =
                     RichText::new(address.to_string()).size(theme.text_sizes.normal);
                  let button = Button::new(address_text);

                  if ui.add(button).clicked() {
                     self.recipient = address.to_string();
                     self.recipient_name = None;
                     close_window = true;
                  }
               }
            });
         });

      if close_window {
         self.open = false;
      } else {
         self.open = open;
      }
   }

   fn account_wallets(
      &mut self,
      ctx: ZeusCtx,
      theme: &Theme,
      icons: Arc<Icons>,
      column_width: f32,
      close_window: &mut bool,
      ui: &mut Ui,
   ) {
      let mut wallets = ctx.wallets_info();
      let mut portfolios = Vec::new();
      for chain in SUPPORTED_CHAINS {
         for wallet in &wallets {
            portfolios.push(ctx.get_portfolio(chain, wallet.address));
         }
      }

      wallets.sort_by(|a, b| {
         let addr_a = a.address;
         let addr_b = b.address;

         // Find the portfolio for each wallet
         let portfolio_a = portfolios.iter().find(|p| p.owner == addr_a);
         let portfolio_b = portfolios.iter().find(|p| p.owner == addr_b);

         // Extract the portfolio value (or use a default if not found)
         let value_a = portfolio_a
            .map(|p| p.value.clone())
            .unwrap_or(NumericValue::default());
         let value_b = portfolio_b
            .map(|p| p.value.clone())
            .unwrap_or(NumericValue::default());

         // Sort in descending order (highest value first)
         // If values are equal, sort by name as a secondary criterion
         value_b
            .f64()
            .partial_cmp(&value_a.f64())
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.name.cmp(&b.name))
      });

      ui.vertical_centered(|ui| {
         ui.label(RichText::new("Your Wallets").size(theme.text_sizes.large));
      });

      ui.spacing_mut().item_spacing = vec2(10.0, 10.0);
      ui.spacing_mut().button_padding = vec2(10.0, 8.0);

      for wallet in &wallets {
         let valid_search = valid_wallet_search(wallet, &self.search_query);
         let value = ctx.get_portfolio_value_all_chains(wallet.address);
         let chains = ctx.get_chains_that_have_balance(wallet.address);

         if valid_search {
            // Main Row
            ui.horizontal(|ui| {
               ui.set_max_width(ui.available_width() * 0.95);

               // Wallet
               ui.with_layout(
                  Layout::left_to_right(Align::Min).with_main_wrap(true),
                  |ui| {
                     ui.set_width(column_width);
                     let name = RichText::new(wallet.name.clone()).size(theme.text_sizes.normal);
                     ui.scope(|ui| {
                        ui.set_width(column_width * 0.8);
                        if ui.add(Button::new(name)).clicked() {
                           self.recipient = wallet.address.to_string();
                           self.recipient_name = Some(wallet.name.clone());
                           *close_window = true;
                        }
                     });
                  },
               );

               // Address
               ui.horizontal(|ui| {
                  ui.set_width(column_width);
                  ui.label(RichText::new(wallet.address_truncated()).size(theme.text_sizes.small));
               });

               // Value
               ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
               ui.horizontal(|ui| {
                  ui.vertical(|ui| {
                     ui.set_width(column_width);
                     ui.horizontal(|ui| {
                        for chain in chains {
                           let icon = icons.chain_icon_x16(chain);
                           ui.add(icon);
                        }
                     });
                     ui.label(
                        RichText::new(format!("${}", value.formatted()))
                           .size(theme.text_sizes.normal),
                     );
                  });
               });
            });
         }
      }
   }

   fn account_contacts(
      &mut self,
      ctx: ZeusCtx,
      theme: &Theme,
      close_window: &mut bool,
      ui: &mut Ui,
   ) {
      let contacts = ctx.contacts();

      ui.label(RichText::new("Your Contacts").size(theme.text_sizes.large));

      ui.spacing_mut().item_spacing = vec2(20.0, 25.0);
      ui.spacing_mut().button_padding = vec2(10.0, 8.0);

      for contact in &contacts {
         let valid_search = valid_contact_search(contact, &self.search_query);

         if valid_search {
            ui.horizontal(|ui| {
               // Contact Name
               ui.scope(|ui| {
                  ui.set_width(ui.available_width() * 0.3);
                  let name = RichText::new(contact.name.clone()).size(theme.text_sizes.normal);
                  let button = Button::new(name).truncate();
                  if ui.add(button).clicked() {
                     self.recipient = contact.address.to_string();
                     self.recipient_name = Some(contact.name.clone());
                     *close_window = true;
                  }
               });

               // Address
               let chain = ctx.chain();
               let explorer = chain.block_explorer();
               let link = format!("{}/address/{}", explorer, &contact.address);
               ui.hyperlink_to(
                  RichText::new(&contact.address_short(10, 10))
                     .size(theme.text_sizes.normal)
                     .color(theme.colors.hyperlink_color),
                  link,
               );
            });
         }
      }
   }
}

fn valid_contact_search(contact: &Contact, query: &str) -> bool {
   let query = query.to_lowercase();

   if query.is_empty() {
      return true;
   }

   contact.name.to_lowercase().contains(&query) || contact.address.to_lowercase().contains(&query)
}

fn valid_wallet_search(wallet: &WalletInfo, query: &str) -> bool {
   let query = query.to_lowercase();

   if query.is_empty() {
      return true;
   }

   wallet.name.to_lowercase().contains(&query)
      || wallet.address_string().to_lowercase().contains(&query)
}
