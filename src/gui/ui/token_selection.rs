use eframe::egui::{
   Align, Align2, Color32, Frame, Layout, ScrollArea, TextEdit, Ui, Window, emath::Vec2b, vec2,
};

use std::{str::FromStr, sync::Arc};

use crate::assets::icons::Icons;
use crate::core::ZeusCtx;
use crate::core::utils::{RT, eth};
use crate::gui::ui::{button, dapps::uniswap::swap::InOrOut, img_button, rich_text};
use crate::gui::{SHARED_GUI, utils};
use zeus_eth::{alloy_primitives::Address, currency::Currency};

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

   /// Currency direction, this only applies if we try to select a token from a SwapUi
   pub currency_direction: InOrOut,
}

impl TokenSelectionWindow {
   pub fn new() -> Self {
      Self {
         open: false,
         size: (400.0, 200.0),
         search_query: String::new(),
         selected_currency: None,
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
      self.search_query.clear();
   }

   /// Show This [TokenSelectionWindow]
   pub fn show(
      &mut self,
      ctx: ZeusCtx,
      chain_id: u64,
      owner: Address,
      icons: Arc<Icons>,
      currencies: &Vec<Currency>,
      ui: &mut Ui,
   ) {
      let mut open = self.open;
      let mut close_window = false;
      Window::new(rich_text("Select Token").size(18.0))
         .open(&mut open)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .resizable(false)
         .collapsible(false)
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_min_size(vec2(self.size.0, self.size.1));

            ui.vertical_centered(|ui| {
               ui.add(
                  TextEdit::singleline(&mut self.search_query)
                     .hint_text(rich_text("Search tokens or enter an address"))
                     .min_size((200.0, 30.0).into()),
               );
               ui.add_space(15.0);
            });

            ScrollArea::vertical()
               .auto_shrink(Vec2b::new(false, false))
               .show(ui, |ui| {
                  ui.spacing_mut().item_spacing.y = 10.0;

                  for currency in currencies {
                     let ctx = ctx.clone();
                     let valid_search = self.valid_search(currency, &self.search_query);

                     if valid_search {
                        let name = rich_text(currency.name().clone());
                        let symbol = format!("({})", currency.symbol().clone());

                        let icon = icons.currency_icon(currency);
                        let button = img_button(icon, name);

                        ui.horizontal(|ui| {
                           egui_theme::utils::bg_color_on_idle(ui, Color32::TRANSPARENT);
                           if ui.add(button).clicked() {
                              self.selected_currency = Some(currency.clone());
                              close_window = true;
                           }

                           ui.label(rich_text(symbol));
                           ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                              let balance = ctx.get_currency_balance(chain_id, owner, currency);
                              ui.label(rich_text(balance.formatted()).size(15.0));
                           });
                        });

                        ui.add_space(5.0);
                     }
                  }

                  ui.vertical_centered(|ui| {
                     self.get_token_on_valid_address(ctx, chain_id, owner, &mut close_window, ui);
                     });
               });
         });
      if close_window {
         open = false;
      }
      self.open = open;
   }

   fn get_token_on_valid_address(
      &self,
      ctx: ZeusCtx,
      chain: u64,
      owner: Address,
      close_window: &mut bool,
      ui: &mut Ui,
   ) {
      if let Ok(address) = Address::from_str(&self.search_query) {
         let button = button(rich_text("Add Token"));
         if ui.add(button).clicked() {
            RT.spawn(async move {
               utils::open_loading("Retrieving token...".to_string());
               let token = match eth::get_erc20_token(ctx, chain, owner, address).await {
                  Ok(token) => {
                     utils::close_loading();
                     token
                  }
                  Err(e) => {
                     let mut gui = SHARED_GUI.write().unwrap();
                     gui.open_msg_window("Failed to fetch token", e.to_string());
                     gui.loading_window.open = false;
                     return;
                  }
               };
               let currency = Currency::from_erc20(token);
               let mut gui = SHARED_GUI.write().unwrap();
               gui.token_selection.selected_currency = Some(currency);
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
