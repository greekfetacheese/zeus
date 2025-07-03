use eframe::egui::{
   Align, Align2, Button, Color32, FontId, Frame, Layout, Margin, Order, RichText, ScrollArea,
   TextEdit, Ui, Window, emath::Vec2b, vec2,
};

use std::{str::FromStr, sync::Arc};

use crate::assets::icons::Icons;
use crate::core::ZeusCtx;
use crate::core::utils::{RT, eth, truncate_symbol_or_name};
use crate::gui::SHARED_GUI;
use crate::gui::ui::dapps::uniswap::swap::InOrOut;
use egui_theme::{Theme, utils};
use zeus_eth::{
   alloy_primitives::Address,
   currency::{Currency, ERC20Token},
   utils::NumericValue,
};

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
         size: (550.0, 500.0),
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
      Window::new(RichText::new("Select Token").size(theme.text_sizes.heading))
         .open(&mut open)
         .order(Order::Foreground)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .resizable(false)
         .collapsible(false)
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);
            let ui_width = ui.available_width();

            ui.vertical_centered(|ui| {
               ui.add_space(20.0);
               ui.add(
                  TextEdit::singleline(&mut self.search_query)
                     .hint_text(RichText::new("Search tokens or enter an address"))
                     .desired_width(ui_width * 0.7)
                     .background_color(theme.colors.text_edit_bg)
                     .margin(Margin::same(10))
                     .font(FontId::proportional(theme.text_sizes.normal)),
               );
               ui.add_space(20.0);
            });

            ui.horizontal(|ui| {
               ui.set_width(ui_width * 0.9);

               ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                  ui.label(RichText::new("Asset").size(theme.text_sizes.large));
               });

               ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                  ui.label(RichText::new("Balance").size(theme.text_sizes.large));
               });
            });

            ui.add_space(20.0);

            ScrollArea::vertical()
               .auto_shrink(Vec2b::new(false, false))
               .show(ui, |ui| {
                  ui.spacing_mut().item_spacing.y = 10.0;
                  utils::no_border(ui);
                  utils::bg_color_on_idle(ui, Color32::TRANSPARENT);

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
                        let name = truncate_symbol_or_name(currency.name(), 20);
                        let symbol = truncate_symbol_or_name(currency.symbol(), 10);
                        let text = format!("{} ({})", name, symbol);
                        let icon = icons.currency_icon(currency);
                        let button = Button::image_and_text(
                           icon,
                           RichText::new(text).size(theme.text_sizes.normal),
                        );

                        ui.horizontal(|ui| {
                           ui.set_width(ui_width * 0.9);

                           if ui.add(button).clicked() {
                              self.selected_currency = Some(currency.clone());
                              self.token_fetched = false;
                              close_window = true;
                           }

                           ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                              ui.label(
                                 RichText::new(balance.format_abbreviated())
                                    .size(theme.text_sizes.normal),
                              );
                           });
                        });

                        ui.add_space(5.0);
                     }
                  }

                  ui.vertical_centered(|ui| {
                     ui.spacing_mut().button_padding = vec2(10.0, 8.0);
                     self.get_token_on_valid_address(
                        ctx,
                        currencies,
                        theme,
                        chain_id,
                        owner,
                        &mut close_window,
                        ui,
                     );
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
         if currencies
            .iter()
            .any(|c| c.erc20().map_or(false, |t| t.address == address))
         {
            return;
         }
         let button = Button::new(RichText::new("Add Token").size(theme.text_sizes.normal));
         if ui.add(button).clicked() {
            self.token_fetched = true;
            RT.spawn(async move {
               SHARED_GUI.write(|gui| {
                  gui.loading_window.open("Retrieving token...");
               });

               let token = match get_erc20_token(ctx, chain, owner, address).await {
                  Ok(token) => {
                     SHARED_GUI.write(|gui| {
                        gui.loading_window.open = false;
                     });
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
               let currency = Currency::from(token);
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

async fn get_erc20_token(
   ctx: ZeusCtx,
   chain: u64,
   owner: Address,
   token_address: Address,
) -> Result<ERC20Token, anyhow::Error> {
   let client = ctx.get_client(chain).await?;
   let token = ERC20Token::new(client.clone(), token_address, chain).await?;
   let manager = ctx.balance_manager();
   manager
      .update_tokens_balance(ctx.clone(), chain, owner, vec![token.clone()])
      .await?;

   let currency = Currency::from(token.clone());

   // Update the db
   ctx.write(|ctx| {
      ctx.currency_db.insert_currency(chain, currency.clone());
   });

   // If there is a balance add the token to the portfolio
   let balance = manager.get_token_balance(chain, owner, token.address);
   if !balance.is_zero() {
      let mut portfolio = ctx.get_portfolio(chain, owner);
      portfolio.add_token(currency.clone());
      ctx.write(|ctx| ctx.portfolio_db.insert_portfolio(chain, owner, portfolio));
   }

   // Sync the pools for the token
   let ctx_clone = ctx.clone();
   let token_clone = token.clone();
   RT.spawn(async move {
      ctx_clone.write(|ctx| {
         ctx.data_syncing = true;
      });

      match eth::sync_pools_for_tokens(
         ctx_clone.clone(),
         chain,
         vec![token_clone.clone()],
         false,
      )
      .await
      {
         Ok(_) => {
            tracing::info!("Synced Pools for {}", token_clone.symbol);
         }
         Err(e) => tracing::error!(
            "Error syncing pools for {}: {:?}",
            token_clone.symbol,
            e
         ),
      }

      let pool_manager = ctx_clone.pool_manager();
      match pool_manager
         .update_for_currencies(ctx_clone.clone(), chain, vec![currency])
         .await
      {
         Ok(_) => {
            tracing::info!("Updated pool state for {}", token_clone.symbol);
         }
         Err(e) => {
            tracing::error!(
               "Error updating pool state for {}: {:?}",
               token_clone.symbol,
               e
            );
         }
      }

      RT.spawn_blocking(move || {
         ctx_clone.calculate_portfolio_value(chain, owner);
         ctx_clone.write(|ctx| ctx.data_syncing = false);
         ctx_clone.save_all();
      });
   });

   Ok(token)
}
