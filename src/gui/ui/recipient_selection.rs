use eframe::egui::{
   Align, Align2, Color32, Button, FontId, Frame, Grid, Layout, Margin, Order, RichText, ScrollArea, TextEdit, Ui, Window, vec2,
};

use std::str::FromStr;
use crate::core::{Contact, Wallet, ZeusCtx};
use crate::gui::ui::WalletSelect;
use egui_theme::{Theme, utils::widget_visuals};
use zeus_eth::alloy_primitives::Address;

pub struct RecipientSelectionWindow {
   pub open: bool,
   /// Address in string format
   pub recipient: String,
   pub recipient_name: Option<String>,
   pub search_query: String,
   pub size: (f32, f32),
}

impl RecipientSelectionWindow {
   pub fn new() -> Self {
      Self {
         open: false,
         recipient: String::new(),
         recipient_name: None,
         search_query: String::new(),
         size: (450.0, 350.0),
      }
   }

   pub fn reset(&mut self) {
      self.recipient.clear();
      self.recipient_name = None;
      self.search_query.clear();
   }

   pub fn get_recipient(&self) -> String {
      self.recipient.clone()
   }

   pub fn get_recipient_name(&self) -> Option<String> {
      self.recipient_name.clone()
   }

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, wallet_select: &WalletSelect, ui: &mut Ui) {
      let mut open = self.open;
      let mut close_window = false;
      let frame = Frame::window(ui.style());
      let bg_color = frame.fill;

      Window::new("Recipient")
         .open(&mut open)
         .order(Order::Foreground)
         .resizable(false)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(frame)
         .show(ui.ctx(), |ui| {
            ui.set_width(450.0);
            ui.set_height(350.0);
            ui.spacing_mut().button_padding = vec2(10.0, 8.0);
            let ui_width = ui.available_width();
            let column_width = ui.available_width() / 3.0;

            // Search bar
            ui.vertical_centered(|ui| {
               widget_visuals(ui, theme.get_text_edit_visuals(bg_color));
               ui.add_space(20.0);
               ui.add(
                  TextEdit::singleline(&mut self.search_query)
                     .hint_text("Search contacts or enter an address")
                     .min_size(vec2(ui_width * 0.80, 25.0))
                     .margin(Margin::same(10))
                     .font(FontId::proportional(theme.text_sizes.normal)),
               );
               ui.add_space(20.0);
            });

            let wallets = ctx.account().wallets;
            let contacts = ctx.contacts();

            // TODO: Optimize this
            let query = self.search_query.clone();
            let are_valid_contacts = contacts.iter().any(|c| valid_contact_search(c, &query));
            let are_valid_wallets = wallets.len() >= 2 && wallets.iter().any(|w| valid_wallet_search(w, &query));

            ScrollArea::vertical()
               .id_salt("recipient_select_scroll")
               .max_height(350.0)
               .max_width(ui_width)
               .show(ui, |ui| {
                  if are_valid_wallets {
                     self.account_wallets(
                        ctx.clone(),
                        theme,
                        wallet_select,
                        bg_color,
                        column_width,
                        &mut close_window,
                        ui,
                     );
                  }
                  ui.add_space(20.0);
                  if are_valid_contacts {
                     self.account_contacts(
                        ctx.clone(),
                        theme,
                        bg_color,
                        column_width,
                        &mut close_window,
                        ui,
                     );
                  }
               });

            if let Ok(address) = Address::from_str(&self.search_query) {
               ui.vertical_centered(|ui| {
                  ui.label(RichText::new("Unknown Address").size(theme.text_sizes.large));
                  ui.add_space(20.0);

                  let address_text = RichText::new(address.to_string()).size(theme.text_sizes.normal);
                  let button = Button::new(address_text);

                  if ui.add(button).clicked() {
                     self.recipient = address.to_string();
                     self.recipient_name = None;
                     close_window = true;
                  }
               });
            }
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
      _wallet_select: &WalletSelect,
      bg_color: Color32,
      column_width: f32,
      close_window: &mut bool,
      ui: &mut Ui,
   ) {
      let wallets = ctx.account().wallets;

      ui.vertical_centered(|ui| {
         ui.label(RichText::new("Your Wallets").size(theme.text_sizes.large));
         ui.add_space(20.0);
      });

      ui.spacing_mut().item_spacing = vec2(10.0, 25.0);
      ui.spacing_mut().button_padding = vec2(10.0, 8.0);
      widget_visuals(ui, theme.get_button_visuals(bg_color));

      Grid::new("recipient_select_wallet_grid")
         .spacing(vec2(5.0, 10.0))
         .show(ui, |ui| {
            ui.label(RichText::new("Wallet Name").size(theme.text_sizes.normal));
            ui.label(RichText::new("Address").size(theme.text_sizes.normal));
            ui.label(RichText::new("Value").size(theme.text_sizes.normal));
            ui.end_row();

            for wallet in &wallets {
               let address = wallet.address();
               let valid_search = valid_wallet_search(wallet, &self.search_query);
               let value = ctx.get_portfolio_value_all_chains(address);

               if valid_search {
                  /* 
                  // exclude the current wallet
                  if address == wallet_select.wallet.address() {
                     continue;
                  }
                  */

                  // Wallet Name
                  ui.with_layout(
                     Layout::left_to_right(Align::Min).with_main_wrap(true),
                     |ui| {
                        ui.set_width(column_width);
                        let name = RichText::new(wallet.name.clone()).size(theme.text_sizes.normal);
                        if ui.add(Button::new(name)).clicked() {
                           self.recipient = address.to_string();
                           self.recipient_name = Some(wallet.name.clone());
                           *close_window = true;
                        }
                     },
                  );

                  // Address
                  ui.with_layout(
                     Layout::left_to_right(Align::Min).with_main_wrap(true),
                     |ui| {
                        ui.set_width(column_width);
                        ui.label(RichText::new(wallet.address_truncated()).size(theme.text_sizes.normal));
                     },
                  );

                  // Value
                  ui.with_layout(
                     Layout::left_to_right(Align::Min).with_main_wrap(true),
                     |ui| {
                        ui.set_width(column_width);
                        ui.label(RichText::new(format!("${}", value.formatted())).size(theme.text_sizes.normal));
                     },
                  );
                  ui.end_row();
               }
            }
         });
   }

   fn account_contacts(
      &mut self,
      ctx: ZeusCtx,
      theme: &Theme,
      bg_color: Color32,
      column_width: f32,
      close_window: &mut bool,
      ui: &mut Ui,
   ) {
      let contacts = ctx.contacts();

      ui.vertical_centered(|ui| {
         ui.label(RichText::new("Contacts").size(theme.text_sizes.large));
      });

      ui.spacing_mut().item_spacing = vec2(10.0, 25.0);
      ui.spacing_mut().button_padding = vec2(10.0, 8.0);
      widget_visuals(ui, theme.get_button_visuals(bg_color));

      Grid::new("recipient_select_contact_grid")
         .spacing(vec2(5.0, 10.0))
         .show(ui, |ui| {
            ui.label(RichText::new("Contact Name").size(theme.text_sizes.normal));
            ui.label(RichText::new("Address").size(theme.text_sizes.normal));
            ui.end_row();

            for contact in &contacts {
               let valid_search = valid_contact_search(contact, &self.search_query);

               if valid_search {
                  // Contact Name
                  ui.with_layout(
                     Layout::left_to_right(Align::Min).with_main_wrap(true),
                     |ui| {
                        ui.set_width(column_width);
                        let name = RichText::new(contact.name.clone()).size(theme.text_sizes.normal);
                        if ui.add(Button::new(name)).clicked() {
                           self.recipient = contact.address.clone();
                           self.recipient_name = Some(contact.name.clone());
                           *close_window = true;
                        }
                     },
                  );

                  // Address
                  ui.with_layout(
                     Layout::left_to_right(Align::Min).with_main_wrap(true),
                     |ui| {
                        ui.set_width(column_width);
                        ui.label(RichText::new(contact.address_short()).size(theme.text_sizes.normal));
                     },
                  );
                  ui.end_row();
               }
            }
         });
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

   wallet.name.to_lowercase().contains(&query) || wallet.address_string().to_lowercase().contains(&query)
}
