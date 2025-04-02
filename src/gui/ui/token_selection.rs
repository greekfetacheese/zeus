use eframe::egui::{
   Align, Align2, Color32, FontId, Order, Frame, Layout, Margin, ScrollArea, TextEdit, Ui, Window, emath::Vec2b, vec2,
};

use std::{str::FromStr, sync::Arc};

use crate::assets::icons::Icons;
use crate::core::ZeusCtx;
use crate::core::utils::{RT, eth};
use crate::gui::ui::{button, dapps::uniswap::swap::InOrOut, img_button, rich_text};
use crate::gui::{SHARED_GUI, utils};
use egui_theme::Theme;
use zeus_eth::{alloy_primitives::Address, currency::Currency, utils::NumericValue};

/// A simple window that allows the user to select a token
/// based on the a list of [Currency] we pass to it
///
/// We can also use the search bar to search for a specific token either by its name or symbol.
///
/// If a valid address is passed to the search bar, we can fetch the token from the blockchain if it exists
pub struct TokenSelectionWindow {
   pub open: bool,
   pub size: (f32, f32),
   pub search_query: String,
   pub selected_currency: Option<Currency>,
   /// Did we fetched this token from the blockchain?
   pub token_fetched: bool,
   /// Currency direction, this only applies if we try to select a token from a SwapUi
   pub currency_direction: InOrOut,
}

impl TokenSelectionWindow {
   pub fn new() -> Self {
      Self {
         open: false,
         size: (550.0, 300.0),
         search_query: String::new(),
         selected_currency: None,
         token_fetched: false,
         currency_direction: InOrOut::In,
      }
   }

   pub fn set_currency_direction(&mut self, currency_direction: InOrOut) {
      self.currency_direction = currency_direction;
   }

   pub fn get_currency_direction(&self) -> &InOrOut {
      &self.currency_direction
   }

   /// Get the selected currency if any
   pub fn get_currency(&self) -> Option<&Currency> {
      self.selected_currency.as_ref()
   }

   pub fn reset(&mut self) {
      self.selected_currency = None;
      self.token_fetched = false;
      self.search_query.clear();
   }

   /// Show This [TokenSelectionWindow]
   pub fn show(
      &mut self,
      ctx: ZeusCtx,
      theme: &Theme,
      icons: Arc<Icons>,
      chain_id: u64,
      owner: Address,
      currencies: &Vec<Currency>,
      ui: &mut Ui,
   ) {
      let mut open = self.open;
      let mut close_window = false;
      Window::new(rich_text("Select Token").size(18.0))
         .open(&mut open)
         .order(Order::Foreground)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .resizable(false)
         .collapsible(false)
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_min_size(vec2(self.size.0, self.size.1));
            let ui_width = ui.available_width();

            ui.vertical_centered(|ui| {
               ui.add_space(20.0);
               ui.add(
                  TextEdit::singleline(&mut self.search_query)
                     .hint_text(rich_text("Search tokens or enter an address"))
                     .desired_width(ui_width * 0.7)
                     .margin(Margin::same(10))
                     .font(FontId::proportional(theme.text_sizes.normal)),
               );
               ui.add_space(20.0);
            });

            ui.horizontal(|ui| {
               ui.set_width(ui_width * 0.9);

               ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                  ui.label(rich_text("Asset").size(theme.text_sizes.large));
               });

               ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                  ui.label(rich_text("Balance").size(theme.text_sizes.large));
               });
            });

            ui.add_space(20.0);

            ScrollArea::vertical()
               .auto_shrink(Vec2b::new(false, false))
               .show(ui, |ui| {
                  ui.spacing_mut().item_spacing.y = 10.0;

                  let mut currencies_with_balances: Vec<(&Currency, NumericValue)> = currencies
                     .iter()
                     .map(|currency| {
                        let balance = ctx.get_currency_balance(chain_id, owner, currency);
                        (currency, balance)
                     })
                     .collect();

                  // sort currenies by the highest balance
                  currencies_with_balances.sort_by(|a, b| {
                     b.1.f64() // b's balance
                        .partial_cmp(&a.1.f64()) // a's balance
                        .unwrap_or(std::cmp::Ordering::Equal)
                  });

                  for (currency, balance) in currencies_with_balances {
                     let valid_search = self.valid_search(currency, &self.search_query);

                     if valid_search {
                        let text = format!("{} ({})", currency.name(), currency.symbol());
                        let icon = icons.currency_icon(currency);
                        let button = img_button(icon, rich_text(text).size(theme.text_sizes.normal));

                        ui.horizontal(|ui| {
                           ui.set_width(ui_width * 0.9);
                           egui_theme::utils::bg_color_on_idle(ui, Color32::TRANSPARENT);

                           if ui.add(button).clicked() {
                              self.selected_currency = Some(currency.clone());
                              self.token_fetched = false;
                              close_window = true;
                           }

                           ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                              ui.label(rich_text(balance.formatted()).size(theme.text_sizes.normal));
                           });
                        });

                        ui.add_space(5.0);
                     }
                  }

                  ui.vertical_centered(|ui| {
                     ui.spacing_mut().button_padding = vec2(10.0, 8.0);
                     self.get_token_on_valid_address(ctx, currencies, theme, chain_id, owner, &mut close_window, ui);
                  });
               });
         });
      if close_window {
         open = false;
      }
      self.open = open;
   }

   fn get_token_on_valid_address(
      &mut self,
      ctx: ZeusCtx,
      currencies: &Vec<Currency>,
      theme: &Theme,
      chain: u64,
      owner: Address,
      close_window: &mut bool,
      ui: &mut Ui,
   ) {
      if let Ok(address) = Address::from_str(&self.search_query) {
         // check if currency already exists
         if currencies.iter().any(|c| c.erc20().map_or(false, |t| t.address == address)) {
            return;
         }
         let button = button(rich_text("Add Token").size(theme.text_sizes.normal));
         if ui.add(button).clicked() {
            self.token_fetched = true;
            RT.spawn(async move {
               utils::open_loading("Retrieving token...".to_string());
               let token = match eth::get_erc20_token(ctx, chain, owner, address).await {
                  Ok(token) => {
                     utils::close_loading();
                     token
                  }
                  Err(e) => {
                     SHARED_GUI.write(|gui| {
                     gui.open_msg_window("Failed to fetch token", e.to_string());
                     gui.loading_window.open = false;
                     });
                     return;
                  }
               };
               let currency = Currency::from_erc20(token);
               SHARED_GUI.write(|gui| {
               gui.token_selection.selected_currency = Some(currency);
               });
            });

            // close the token selection window
            *close_window = true;
         }
      }
   }

   fn valid_search(&self, currency: &Currency, query: &str) -> bool {
      let query = query.to_lowercase();

      if query.is_empty() {
         return true;
      }

      if currency.name().to_lowercase().contains(&query) {
         return true;
      }

      if currency.symbol().to_lowercase().contains(&query) {
         return true;
      }

      if let Ok(address) = Address::from_str(&query) {
         if currency.is_erc20() {
            if let Some(token) = currency.erc20() {
               return token.address == address;
            }
         }
      }
      false
   }
}
