use crate::assets::icons::Icons;
use crate::core::{Contact, WalletInfo, ZeusCtx};
use crate::gui::ui::ContactsUi;
use eframe::egui::{
   Align2, FontId, Frame, Margin, Order, RichText, ScrollArea, Sense, Ui, Window, vec2,
};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use zeus_eth::{alloy_primitives::Address, types::SUPPORTED_CHAINS, utils::NumericValue};
use zeus_theme::{OverlayManager, Theme, utils::frame_it};
use zeus_widgets::{Button, Label, SecureTextEdit};

pub struct RecipientSelectionWindow {
   open: bool,
   overlay: OverlayManager,
   contacts_tab_open: bool,
   wallets_tab_open: bool,
   pub recipient: String,
   recipient_name: Option<String>,
   search_query: String,
   wallets: Vec<WalletInfo>,
   /// Wallet value by address
   wallet_value: HashMap<Address, NumericValue>,
   /// Chains that the wallet has balance on
   wallet_chains: HashMap<Address, Vec<u64>>,
   size: (f32, f32),
}

impl RecipientSelectionWindow {
   pub fn new(overlay: OverlayManager) -> Self {
      Self {
         open: false,
         overlay,
         contacts_tab_open: true,
         wallets_tab_open: false,
         recipient: String::new(),
         recipient_name: None,
         search_query: String::new(),
         wallets: Vec::new(),
         wallet_value: HashMap::new(),
         wallet_chains: HashMap::new(),
         size: (500.0, 550.0),
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self, ctx: ZeusCtx) {
      if !self.open {
         self.overlay.window_opened();
      }
      self.open = true;

      let mut wallets = ctx.get_all_wallets_info();
      let mut portfolios = Vec::new();
      for chain in SUPPORTED_CHAINS {
         for wallet in &wallets {
            portfolios.push(ctx.get_portfolio(chain, wallet.address));
         }
      }

      wallets.sort_by(|a, b| {
         let wallet_a = a.address;
         let wallet_b = b.address;

         let value_a = ctx.get_portfolio_value_all_chains(wallet_a);
         let value_b = ctx.get_portfolio_value_all_chains(wallet_b);

         // Sort in descending order (highest value first)
         value_b
            .f64()
            .partial_cmp(&value_a.f64())
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.name().cmp(&b.name()))
      });

      let mut wallet_value = HashMap::new();
      let mut wallet_chains = HashMap::new();

      for wallet in &wallets {
         let value = ctx.get_portfolio_value_all_chains(wallet.address);
         wallet_value.insert(wallet.address, value);

         let chains = ctx.get_chains_that_have_balance(wallet.address);
         wallet_chains.insert(wallet.address, chains);
      }

      self.wallets = wallets;
      self.wallet_value = wallet_value;
      self.wallet_chains = wallet_chains;
   }

   pub fn close(&mut self) {
      self.overlay.window_closed();
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
      _icons: Arc<Icons>,
      contacts_ui: &mut ContactsUi,
      ui: &mut Ui,
   ) {
      let mut open = self.open;
      if !open {
         return;
      }

      let mut close_window = false;

      contacts_ui.add_contact.show(ctx.clone(), theme, false, ui);
      let contact_added = contacts_ui.add_contact.contact_added();

      if contact_added {
         let contact = contacts_ui.add_contact.get_contact().clone();
         self.recipient = contact.address;
         self.recipient_name = Some(contact.name);
         contacts_ui.add_contact.reset();
         self.close();
      }

      let title = RichText::new("Recipient").size(theme.text_sizes.heading);
      let _window_res = Window::new(title)
         .open(&mut open)
         .order(Order::Foreground)
         .resizable(false)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(Frame::window(ui.style()).inner_margin(Margin::same(10)))
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);
            ui.spacing_mut().button_padding = vec2(10.0, 8.0);
            let size = vec2(ui.available_width() * 0.4, 45.0);
            let button_visuals = theme.button_visuals();
            let text_edit_visuals = theme.text_edit_visuals();

            ui.vertical_centered(|ui| {
               ui.add_space(20.0);

               let text = RichText::new("Add a contact").size(theme.text_sizes.normal);
               let add_contact = Button::new(text).visuals(button_visuals);

               if ui.add(add_contact).clicked() {
                  contacts_ui.add_contact.open();
               }

               ui.add_space(15.0);

               // Search bar
               let hint = RichText::new("Search contacts or enter an address")
                  .size(theme.text_sizes.normal)
                  .color(theme.colors.text_muted);

               ui.add(
                  SecureTextEdit::singleline(&mut self.search_query)
                     .visuals(text_edit_visuals)
                     .hint_text(hint)
                     .min_size(vec2(ui.available_width() * 0.80, 25.0))
                     .margin(Margin::same(10))
                     .font(FontId::proportional(theme.text_sizes.normal)),
               );

               ui.add_space(15.0);

               ui.allocate_ui(size, |ui| {
                  ui.horizontal(|ui| {
                     let contacts_text = RichText::new("Contacts").size(theme.text_sizes.large);
                     let wallet_text = RichText::new("Wallets").size(theme.text_sizes.large);

                     let contact_button = Button::selectable(self.contacts_tab_open, contacts_text)
                        .visuals(button_visuals);

                     if ui.add(contact_button).clicked() {
                        self.contacts_tab_open = true;
                        self.wallets_tab_open = false;
                     }

                     ui.add_space(10.0);

                     let wallet_button = Button::selectable(self.wallets_tab_open, wallet_text)
                        .visuals(button_visuals);

                     if ui.add(wallet_button).clicked() {
                        self.wallets_tab_open = true;
                        self.contacts_tab_open = false;
                     }
                  });
               });

               ui.add_space(15.0);

               if self.contacts_tab_open {
                  self.contacts_tab(ctx.clone(), theme, &mut close_window, ui);
               }

               if self.wallets_tab_open {
                  self.wallets_tab(theme, &mut close_window, ui);
               }

               if let Ok(address) = Address::from_str(&self.search_query) {
                  if ctx.wallet_exists(address)
                     || ctx.get_contact_by_address(&address.to_string()).is_some()
                  {
                     return;
                  }

                  ui.label(RichText::new("Unknown Address").size(theme.text_sizes.large));

                  let address_text =
                     RichText::new(address.to_string()).size(theme.text_sizes.normal);
                  let button = Button::new(address_text).visuals(button_visuals);

                  if ui.add(button).clicked() {
                     self.recipient = address.to_string();
                     self.recipient_name = None;
                     close_window = true;
                  }
               }
            });
         });

      /*
      if let Some(inner) = window_res {
         let window_rect = inner.response.rect;

         if contacts_ui.add_contact.is_open() {
            let tint = self.overlay.tint_1();
            self.overlay.paint_overlay_at(ui.ctx(), window_rect, Order::Foreground, tint);
         }
      }
      */

      if close_window || !open {
         self.close();
      }
   }

   fn contacts_tab(&mut self, ctx: ZeusCtx, theme: &Theme, close_window: &mut bool, ui: &mut Ui) {
      let contacts = ctx.contacts();
      let are_valid_contacts = contacts.iter().any(|c| valid_contact_search(c, &self.search_query));

      ScrollArea::vertical()
         .id_salt("contact_tabs_scroll")
         .max_height(self.size.1)
         .max_width(ui.available_width())
         .show(ui, |ui| {
            if are_valid_contacts {
               self.show_contacts(ctx.clone(), theme, close_window, ui);
            }
         });
   }

   fn show_contacts(&mut self, ctx: ZeusCtx, theme: &Theme, close_window: &mut bool, ui: &mut Ui) {
      let contacts = ctx.contacts();

      ui.spacing_mut().item_spacing = vec2(0.0, 15.0);
      ui.spacing_mut().button_padding = vec2(0.0, 5.0);

      let mut frame = theme.frame1;
      let visuals = theme.frame1_visuals;
      let label_visuals = theme.label_visuals();

      for contact in &contacts {
         let valid_search = valid_contact_search(contact, &self.search_query);

         if valid_search {
            let res = frame_it(&mut frame, Some(visuals), ui, |ui| {
               ui.set_width(ui.available_width());
               let name = RichText::new(contact.name.clone())
                  .size(theme.text_sizes.normal)
                  .color(theme.colors.info);
               ui.horizontal(|ui| {
                  ui.label(name);
               });

               ui.add_space(6.0);

               let address_text = RichText::new(&contact.address).size(theme.text_sizes.small);
               let label = Label::new(address_text, None)
                  .visuals(label_visuals)
                  .expand(Some(6.0))
                  .sense(Sense::click());

               ui.horizontal(|ui| {
                  if ui.add(label).clicked() {
                     ui.ctx().copy_text(contact.address.clone());
                  }
               });
            });

            if res.interact(Sense::click()).clicked() {
               self.recipient = contact.address.to_string();
               self.recipient_name = Some(contact.name.clone());
               *close_window = true;
            }
         }
      }
   }

   fn wallets_tab(&mut self, theme: &Theme, close_window: &mut bool, ui: &mut Ui) {
      let wallets = &self.wallets;
      let are_valid_wallets =
         !wallets.is_empty() && wallets.iter().any(|w| valid_wallet_search(w, &self.search_query));

      ScrollArea::vertical()
         .id_salt("wallets_tabs_scroll")
         .max_height(self.size.1)
         .max_width(ui.available_width())
         .show(ui, |ui| {
            if are_valid_wallets {
               self.show_wallets(theme, close_window, ui);
            }
         });
   }

   fn show_wallets(&mut self, theme: &Theme, close_window: &mut bool, ui: &mut Ui) {
      ui.spacing_mut().item_spacing = vec2(0.0, 15.0);
      ui.spacing_mut().button_padding = vec2(0.0, 5.0);

      let mut frame = theme.frame1;
      let visuals = theme.frame1_visuals;
      let label_visuals = theme.label_visuals();

      let wallets = &self.wallets;

      for wallet in wallets {
         let valid_search = valid_wallet_search(wallet, &self.search_query);
         let value = self.wallet_value.get(&wallet.address).cloned().unwrap_or_default();

         if valid_search {
            let res = frame_it(&mut frame, Some(visuals), ui, |ui| {
               ui.set_width(ui.available_width());
               ui.horizontal(|ui| {
                  let name_text = RichText::new(wallet.name())
                     .size(theme.text_sizes.normal)
                     .color(theme.colors.info);
                  ui.label(name_text);

                  ui.add_space(10.0);

                  let value_text = RichText::new(format!("${}", value.abbreviated()))
                     .size(theme.text_sizes.normal);
                  ui.label(value_text);
               });

               ui.add_space(6.0);

               let address_text =
                  RichText::new(&wallet.address.to_string()).size(theme.text_sizes.small);
               let label = Label::new(address_text, None)
                  .visuals(label_visuals)
                  .expand(Some(6.0))
                  .sense(Sense::click());

               ui.horizontal(|ui| {
                  if ui.add(label).clicked() {
                     ui.ctx().copy_text(wallet.address.to_string());
                  }
               });
            });

            if res.interact(Sense::click()).clicked() {
               self.recipient = wallet.address.to_string();
               self.recipient_name = Some(wallet.name());
               *close_window = true;
            }
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

   wallet.name().to_lowercase().contains(&query)
      || wallet.address.to_string().to_lowercase().contains(&query)
}
