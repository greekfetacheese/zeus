use eframe::egui::{
   Align, Align2, Button, FontId, Frame, Layout, Margin, Order, RichText, ScrollArea,
   Sense, TextEdit, Ui, Window, emath::Vec2b, vec2,
};

use crate::assets::icons::Icons;
use crate::core::ZeusCtx;
use crate::utils::{RT, truncate_symbol_or_name};
use crate::gui::SHARED_GUI;
use crate::gui::ui::dapps::uniswap::swap::InOrOut;
use std::{str::FromStr, sync::Arc};

use zeus_eth::{
   alloy_primitives::Address,
   currency::{Currency, ERC20Token},
   utils::NumericValue,
};

use zeus_widgets::Label;
use zeus_theme::{Theme, OverlayManager, utils::frame_it};

/// A simple window that allows the user to select a token
///
/// We can also use the search bar to search for a specific token either by its name or symbol.
///
/// If a valid address is passed to the search bar, we can fetch the token from the blockchain if it exists
pub struct TokenSelectionWindow {
   open: bool,
   overlay: OverlayManager,
   pub size: (f32, f32),
   pub search_query: String,
   pub selected_currency: Option<Currency>,
   /// Did we fetched this token from the blockchain?
   pub token_fetched: bool,
   /// Currency direction, this only applies if we try to select a token from a SwapUi
   pub currency_direction: InOrOut,

   /// Cached and sorted list of currencies with their balances.
   ///
   /// (Currency, Balance, Value)
   processed_currencies: Vec<(Currency, NumericValue, NumericValue)>,
}

impl TokenSelectionWindow {
   pub fn new(overlay: OverlayManager) -> Self {
      Self {
         open: false,
         overlay,
         size: (550.0, 500.0),
         search_query: String::new(),
         selected_currency: None,
         token_fetched: false,
         currency_direction: InOrOut::In,
         processed_currencies: Vec::new(),
      }
   }

   pub fn open(&mut self, ctx: ZeusCtx, chain_id: u64, owner: Address) {
      self.overlay.window_opened();
      self.open = true;
      self.process_currencies(ctx, chain_id, owner);
   }

   pub fn reset(&mut self) {
      self.close();
      self.search_query.clear();
      self.selected_currency = None;
      self.token_fetched = false;
      self.currency_direction = InOrOut::In;
      self.processed_currencies = Vec::new();
   }

   pub fn close(&mut self) {
      self.overlay.window_closed();
      self.open = false;
   }

   pub fn process_currencies(&mut self, ctx: ZeusCtx, chain_id: u64, owner: Address) {
      let currencies = ctx.get_currencies(chain_id);

      let mut currency_list: Vec<(Currency, NumericValue, NumericValue)> = currencies
         .iter()
         .map(|currency| {
            let balance = ctx.get_currency_balance(chain_id, owner, currency);
            let value = ctx.get_currency_value_for_amount(balance.f64(), currency);
            (currency.clone(), balance, value)
         })
         .collect();

      currency_list
         .sort_by(|a, b| b.2.f64().partial_cmp(&a.2.f64()).unwrap_or(std::cmp::Ordering::Equal));

      self.processed_currencies = currency_list;
   }

   pub fn clear_processed_currencies(&mut self) {
      self.processed_currencies.clear();
      self.processed_currencies.shrink_to_fit();
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

   /// Show This [TokenSelectionWindow]
   pub fn show(
      &mut self,
      ctx: ZeusCtx,
      theme: &Theme,
      icons: Arc<Icons>,
      chain_id: u64,
      owner: Address,
      ui: &mut Ui,
   ) {
      let mut open = self.open;

      if !open {
         return;
      }
      
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
                     .margin(Margin::same(10))
                     .font(FontId::proportional(theme.text_sizes.normal)),
               );
               ui.add_space(20.0);
            });

            ui.vertical_centered(|ui| {
               self.get_token_on_valid_address(ctx, theme, chain_id, owner, &mut close_window, ui);
            });

            let filtered_list: Vec<_> = self
               .processed_currencies
               .iter()
               .filter(|(currency, _, _)| self.valid_search(currency, &self.search_query))
               .collect();

            let num_rows = filtered_list.len();
            let row_height = 80.0;
            let tint = theme.image_tint_recommended;
            let mut frame = theme.frame2.outer_margin(Margin::same(5));
            let frame_visuals = theme.frame2_visuals;

            ScrollArea::vertical().auto_shrink(Vec2b::new(false, false)).show_rows(
               ui,
               row_height,
               num_rows,
               |ui, row_range| {
                  for row_index in row_range {
                     if let Some((currency, balance, value)) = filtered_list.get(row_index) {
                        let name = truncate_symbol_or_name(currency.name(), 25);
                        let symbol = truncate_symbol_or_name(currency.symbol(), 10);
                        let text = format!("{}\n{}", name, symbol);
                        let icon = icons.currency_icon(currency, tint);
                        let rich_text = RichText::new(text).size(theme.text_sizes.normal);
                        let label = Label::new(rich_text, Some(icon))
                           .interactive(false)
                           .wrap()
                           .image_on_left();

                        let res = frame_it(&mut frame, Some(frame_visuals), ui, |ui| {
                           ui.horizontal(|ui| {
                              ui.set_width(ui.available_width());

                              ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                                 ui.set_width(ui.available_width() * 0.3);
                                 ui.set_height(50.0);
                                 ui.add(label);
                              });

                              ui.add_space(ui.available_width() * 0.7);

                              if !balance.is_zero() {
                                 ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                                    let value_text = format!("${:.12}", value.abbreviated());

                                    ui.vertical(|ui| {
                                       ui.label(
                                          RichText::new(value_text).size(theme.text_sizes.normal),
                                       );

                                       ui.label(
                                          RichText::new(format!("{:.12}", balance.abbreviated()))
                                             .size(theme.text_sizes.normal),
                                       );
                                    });
                                 });
                              }
                           });
                        });

                        if res.interact(Sense::click()).clicked() {
                           self.selected_currency = Some((*currency).clone());
                           self.token_fetched = false;
                           close_window = true;
                        }
                     }
                  }
               },
            );
         });

      if close_window || !open {
         self.close();
         self.clear_processed_currencies();
      }
   }

   fn get_token_on_valid_address(
      &mut self,
      ctx: ZeusCtx,
      theme: &Theme,
      chain: u64,
      owner: Address,
      close_window: &mut bool,
      ui: &mut Ui,
   ) {
      if let Ok(address) = Address::from_str(&self.search_query) {
         let token = ctx.read(|ctx| ctx.currency_db.get_erc20_token(chain, address));
         if token.is_some() {
            return;
         }

         ui.add_space(20.0);
         let size = vec2(ui.available_width() * 0.7, 40.0);

         let button =
            Button::new(RichText::new("Add Token").size(theme.text_sizes.large)).min_size(size);
         if ui.add(button).clicked() {
            self.token_fetched = true;
            RT.spawn(async move {
               SHARED_GUI.write(|gui| {
                  gui.loading_window.open("Retrieving token...");
               });

               let token = match get_erc20_token(ctx, chain, owner, address).await {
                  Ok(token) => {
                     SHARED_GUI.write(|gui| {
                        gui.loading_window.reset();
                     });
                     token
                  }
                  Err(e) => {
                     SHARED_GUI.write(|gui| {
                        gui.open_msg_window("Failed to fetch token", e.to_string());
                        gui.loading_window.reset();
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
            if let Some(token) = currency.erc20_opt() {
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
   let z_client = ctx.get_zeus_client();

   let token = z_client
      .request(chain, |client| async move {
         ERC20Token::new(client, token_address, chain).await
      })
      .await?;

   let manager = ctx.balance_manager();
   manager
      .update_tokens_balance(
         ctx.clone(),
         chain,
         owner,
         vec![token.clone()],
         false,
      )
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
      portfolio.add_token(token.clone());
      ctx.write(|ctx| ctx.portfolio_db.insert_portfolio(chain, owner, portfolio));
   }

   // Sync the pools for the token
   let ctx_clone = ctx.clone();
   let token_clone = token.clone();
   RT.spawn(async move {
      ctx_clone.write(|ctx| {
         ctx.data_syncing = true;
      });

      let pool_manager = ctx_clone.pool_manager();

      match pool_manager
         .sync_pools_for_tokens(ctx_clone.clone(), chain, vec![token_clone.clone()])
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
         ctx_clone.save_currency_db();
         ctx_clone.save_portfolio_db();
         ctx_clone.save_pool_manager();
         ctx_clone.save_price_manager();
      });
   });

   Ok(token)
}
