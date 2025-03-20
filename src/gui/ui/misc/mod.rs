use eframe::egui::{
   Align, Align2, Color32, ComboBox, Frame, Grid, Layout, Order, RichText, ScrollArea, Spinner, Ui, Vec2, Window, vec2,
};
use std::sync::Arc;
use zeus_eth::utils::NumericValue;

use crate::assets::icons::Icons;
use crate::core::{
   Portfolio, Wallet, ZeusCtx,
   utils::{RT, eth},
};
use crate::gui::SHARED_GUI;
use crate::gui::ui::{TokenSelectionWindow, button, rich_text};

use egui_theme::{Theme, utils::*};
use zeus_eth::{currency::Currency, types::ChainId};

/// A ComboBox to select a chain
pub struct ChainSelect {
   pub id: &'static str,
   pub chain: ChainId,
   pub size: Vec2,
   pub show_icon: bool,
}

impl ChainSelect {
   pub fn new(id: &'static str) -> Self {
      Self {
         id,
         chain: ChainId::new(1).unwrap(),
         size: (200.0, 25.0).into(),
         show_icon: true,
      }
   }

   pub fn size(mut self, size: impl Into<Vec2>) -> Self {
      self.size = size.into();
      self
   }

   pub fn show_icon(mut self, show: bool) -> Self {
      self.show_icon = show;
      self
   }

   /// Show the ComboBox
   ///
   /// Returns true if the chain was changed
   pub fn show(&mut self, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) -> bool {
      let selected_chain = self.chain.id();
      let mut clicked = false;

      // hack to keep the icon centered relative to the combobox
      // But if show_icon is true we cannot apply ui.vertical_centered on the combobox
      if self.show_icon {
         Grid::new("chain_select")
            .spacing(vec2(0.0, 0.0))
            .show(ui, |ui| {
               let icon = icons.chain_icon(&selected_chain);
               ui.add(icon);
               clicked = self.combo_box(theme, ui);
            });
      } else {
         clicked = self.combo_box(theme, ui);
      }

      clicked
   }

   fn combo_box(&mut self, theme: &Theme, ui: &mut Ui) -> bool {
      let mut selected_chain = self.chain.clone();
      let supported_chains = ChainId::supported_chains();
      let mut clicked = false;

      // Add the ComboBox with the specified size
      ui.add_sized(self.size, |ui: &mut Ui| {
         ComboBox::from_id_salt(self.id)
            .width(self.size.x)
            .selected_text(rich_text(selected_chain.name()).size(theme.text_sizes.normal))
            .show_ui(ui, |ui| {
               for chain in supported_chains {
                  let value = ui.selectable_value(
                     &mut selected_chain,
                     chain.clone(),
                     rich_text(chain.name()).size(theme.text_sizes.normal),
                  );
                  if value.clicked() {
                     self.chain = selected_chain.clone();
                     clicked = true;
                  }
               }
            })
            .response
      });
      clicked
   }
}

/// A ComboBox to select a wallet
pub struct WalletSelect {
   pub id: &'static str,
   /// Selected Wallet
   pub wallet: Wallet,
   pub size: Vec2,
   pub show_icon: bool,
}

impl WalletSelect {
   pub fn new(id: &'static str) -> Self {
      Self {
         id,
         wallet: Wallet::new_rng(String::new()),
         size: (200.0, 25.0).into(),
         show_icon: true,
      }
   }

   pub fn size(mut self, size: impl Into<Vec2>) -> Self {
      self.size = size.into();
      self
   }

   pub fn show_icon(mut self, show_icon: bool) -> Self {
      self.show_icon = show_icon;
      self
   }

   /// Show the ComboBox
   ///
   /// Returns true if the wallet was changed
   pub fn show(&mut self, theme: &Theme, wallets: &Vec<Wallet>, icons: Arc<Icons>, ui: &mut Ui) -> bool {
      let mut clicked = false;

      if self.show_icon {
         Grid::new("wallet_select")
            .spacing(vec2(0.0, 0.0))
            .show(ui, |ui| {
               ui.add(icons.wallet());
               clicked = self.combo_box(theme, wallets, ui);
            });
      } else {
         clicked = self.combo_box(theme, wallets, ui);
      }

      clicked
   }

   fn combo_box(&mut self, theme: &Theme, wallets: &Vec<Wallet>, ui: &mut Ui) -> bool {
      let mut clicked = false;

      // Add the ComboBox with the specified size
      ui.add_sized(self.size, |ui: &mut Ui| {
         ComboBox::from_id_salt(self.id)
            .width(self.size.x)
            .selected_text(rich_text(self.wallet.name.clone()).size(theme.text_sizes.normal))
            .show_ui(ui, |ui| {
               ui.spacing_mut().item_spacing.y = 10.0;

               for wallet in wallets {
                  let value = ui.selectable_value(
                     &mut self.wallet,
                     wallet.clone(),
                     rich_text(wallet.name.clone()).size(theme.text_sizes.normal),
                  );

                  if value.clicked() {
                     clicked = true;
                     self.wallet = wallet.clone();
                  }
               }
            })
            .response
      });
      clicked
   }
}

/// Window to indicate a loading state
pub struct LoadingWindow {
   pub open: bool,
   pub msg: String,
   pub size: (f32, f32),
   pub anchor: (Align2, Vec2),
}

impl LoadingWindow {
   pub fn new() -> Self {
      Self {
         open: false,
         msg: String::new(),
         size: (200.0, 100.0),
         anchor: (Align2::CENTER_CENTER, vec2(0.0, 0.0)),
      }
   }

   pub fn open(&mut self, msg: impl Into<String>) {
      self.open = true;
      self.msg = msg.into();
   }

   pub fn reset(&mut self) {
      self.open = false;
      self.msg = String::new();
   }

   pub fn show(&mut self, ui: &mut Ui) {
      if !self.open {
         return;
      }

      Window::new("Loading")
         .title_bar(false)
         .order(Order::Foreground)
         .resizable(false)
         .anchor(self.anchor.0, self.anchor.1)
         .collapsible(false)
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);
            ui.vertical_centered(|ui| {
               ui.add(Spinner::new().size(50.0).color(Color32::WHITE));
               ui.label(rich_text(&self.msg).size(17.0));
            });
         });
   }
}

/// Simple window diplaying a message, for example an error
#[derive(Default)]
pub struct MsgWindow {
   pub open: bool,
   pub title: String,
   pub message: String,
}

impl MsgWindow {
   pub fn new() -> Self {
      Self {
         open: false,
         title: String::new(),
         message: String::new(),
      }
   }

   /// Open the window with this title and message
   pub fn open(&mut self, title: impl Into<String>, msg: impl Into<String>) {
      self.open = true;
      self.title = title.into();
      self.message = msg.into();
   }

   pub fn reset(&mut self) {
      self.open = false;
      self.title.clear();
      self.message.clear();
   }

   pub fn show(&mut self, ui: &mut Ui) {
      if !self.open {
         return;
      }

      let title = rich_text(self.title.clone()).size(20.0);
      let msg = rich_text(&self.message).size(16.0);
      let ok = button(rich_text("Ok"));

      Window::new(title)
         .resizable(false)
         .order(Order::Foreground)
         .movable(true)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .collapsible(false)
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.vertical_centered(|ui| {
               ui.set_min_size(vec2(300.0, 100.0));
               ui.scope(|ui| {
                  ui.spacing_mut().item_spacing.y = 20.0;

                  ui.label(msg);

                  if ui.add(ok).clicked() {
                     self.open = false;
                  }
               });
            });
         });
   }
}

pub struct PortfolioUi {
   pub open: bool,
   pub show_spinner: bool,
}

impl PortfolioUi {
   pub fn new() -> Self {
      Self {
         open: true,
         show_spinner: false,
      }
   }

   pub fn show(
      &mut self,
      ctx: ZeusCtx,
      theme: &Theme,
      icons: Arc<Icons>,
      token_selection: &mut TokenSelectionWindow,
      ui: &mut Ui,
   ) {
      if !self.open {
         return;
      }

      let chain_id = ctx.chain().id();
      let owner = ctx.wallet().key.borrow().address();
      let portfolio = ctx.get_portfolio(chain_id, owner);
      let currencies = portfolio.currencies();

      ui.vertical_centered_justified(|ui| {
         ui.set_width(ui.available_width() * 0.8);

         ui.spacing_mut().item_spacing = Vec2::new(16.0, 20.0);

         ui.horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);
               // bg_color_on_idle(ui, ui.style().visuals.extreme_bg_color);
               let visuals = theme.get_button_visuals(theme.colors.bg_color);
               widget_visuals(ui, visuals);

               let add_token = button(rich_text("Add Token").size(theme.text_sizes.normal));
               if ui.add(add_token).clicked() {
                  token_selection.open = true;
               }

               let refresh = button(rich_text("Refresh").size(theme.text_sizes.normal));
               if ui.add(refresh).clicked() {
                  self.update_prices(ctx.clone());
               }

               if self.show_spinner {
                  ui.add(Spinner::new().size(17.0).color(Color32::WHITE));
               }
            });
         });

         // Total Value
         ui.vertical(|ui| {
            Frame::group(ui.style())
               .inner_margin(16.0)
               .fill(ui.style().visuals.extreme_bg_color)
               .show(ui, |ui| {
                  ui.vertical_centered(|ui| {
                     let wallet_name = ctx.profile().current_wallet.name.clone();
                     ui.label(RichText::new(wallet_name).size(theme.text_sizes.very_large));
                     ui.add_space(8.0);
                     ui.label(
                        RichText::new(format!("${}", portfolio.value.formatted()))
                           .heading()
                           .size(theme.text_sizes.heading + 4.0),
                     );
                  });
               });
         });

         // Token List
         ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
               ui.set_width(ui.available_width());

               let column_widths = [
                  ui.available_width() * 0.2, // Asset
                  ui.available_width() * 0.2, // Price
                  ui.available_width() * 0.2, // Balance
                  ui.available_width() * 0.2, // Value
                  ui.available_width() * 0.1, // Remove button
               ];

               // Center the grid within the available space
               ui.horizontal(|ui| {
                  ui.add_space((ui.available_width() - column_widths.iter().sum::<f32>()) / 2.0);

                  Grid::new("currency_grid")
                     .num_columns(5)
                     .spacing([20.0, 30.0])
                     .show(ui, |ui| {
                        // Header
                        ui.label(RichText::new("Asset").size(theme.text_sizes.large));

                        ui.label(RichText::new("Price").size(theme.text_sizes.large));

                        ui.label(RichText::new("Balance").size(theme.text_sizes.large));

                        ui.label(RichText::new("Value").size(theme.text_sizes.large));

                        ui.end_row();

                        // Token Rows
                        let native_wrapped = currencies.iter().find(|c| c.is_native_wrapped());
                        let native_currency = currencies.iter().find(|c| c.is_native());
                        let tokens: Vec<_> = currencies.iter().filter(|c| c.is_erc20()).collect();

                        if let Some(native) = native_currency {
                           self.token(theme, icons.clone(), native, ui, column_widths[0]);
                           self.price_balance_value(ctx.clone(), theme, chain_id, native, ui, column_widths[0]);
                           self.remove_currency(ctx.clone(), native, ui, column_widths[4]);
                           ui.end_row();
                        }

                        if let Some(wrapped) = native_wrapped {
                           self.token(theme, icons.clone(), wrapped, ui, column_widths[0]);
                           self.price_balance_value(ctx.clone(), theme, chain_id, wrapped, ui, column_widths[0]);
                           self.remove_currency(ctx.clone(), wrapped, ui, column_widths[4]);
                           ui.end_row();
                        }

                        for token in tokens {
                           if token.is_native_wrapped() {
                              continue;
                           }
                           self.token(theme, icons.clone(), token, ui, column_widths[0]);
                           self.price_balance_value(ctx.clone(), theme, chain_id, token, ui, column_widths[0]);
                           self.remove_currency(ctx.clone(), token, ui, column_widths[4]);
                           ui.end_row();
                        }
                     });
               });

               // Token selection
               let all_currencies = ctx.get_currencies(chain_id);
               token_selection.show(
                  ctx.clone(),
                  theme,
                  icons.clone(),
                  chain_id,
                  owner,
                  &all_currencies,
                  ui,
               );
               let currency = token_selection.get_currency().cloned();

               if let Some(currency) = currency {
                  token_selection.reset();
                  self.add_currency(ctx.clone(), currency);
               }
            });
      });
   }

   fn token(&self, theme: &Theme, icons: Arc<Icons>, currency: &Currency, ui: &mut Ui, width: f32) {
      let icon = icons.currency_icon(currency);
      ui.horizontal(|ui| {
         ui.set_width(width);
         ui.add(icon);
         ui.label(
            RichText::new(currency.symbol())
               .size(theme.text_sizes.normal),
         )
         .on_hover_text(currency.name());
      });
   }

   fn price_balance_value(
      &self,
      ctx: ZeusCtx,
      theme: &Theme,
      chain: u64,
      currency: &Currency,
      ui: &mut Ui,
      width: f32,
   ) {
      let price = ctx.get_currency_price(currency);

      ui.horizontal(|ui| {
         ui.set_width(width);
         ui.label(rich_text(format!("${}", price.formatted())).size(theme.text_sizes.normal));
      });

      let owner = ctx.wallet().key.borrow().address();
      let balance = ctx.get_currency_balance(chain, owner, currency);

      ui.horizontal(|ui| {
         ui.set_width(width);
         ui.label(rich_text(balance.formatted()).size(theme.text_sizes.normal));
      });

      let value = ctx.get_currency_value(chain, owner, currency);
      ui.horizontal(|ui| {
         ui.set_width(width);
         ui.label(rich_text(format!("${}", value.formatted())).size(theme.text_sizes.normal));
      });
   }

   fn update_prices(&mut self, ctx: ZeusCtx) {
      self.show_spinner = true;
      RT.spawn(async move {
         let pool_manager = ctx.pool_manager();
         let chain = ctx.chain().id();
         let owner = ctx.profile().wallet_address();
         let client = ctx.get_client_with_id(chain).unwrap();

         match pool_manager.update_pool_state(client, chain).await {
            Ok(_) => tracing::info!("Updated prices for chain: {}", chain),
            Err(e) => tracing::error!("Error updating prices: {:?}", e),
         }

         ctx.update_portfolio_value(chain, owner);
         let _ = ctx.save_pool_data();
         let _ = ctx.save_portfolio_db();

         let mut gui = SHARED_GUI.write().unwrap();
         gui.portofolio.show_spinner = false;
      });
   }

   // Add a currency to the portfolio and update the portfolio value
   fn add_currency(&mut self, ctx: ZeusCtx, currency: Currency) {
      let chain_id = ctx.chain().id();
      let owner = ctx.wallet().key.borrow().address();

      // Add the token to the portfolio
      ctx.write(|ctx| {
         let portfolio = ctx.portfolio_db.get_portfolio_mut(chain_id, owner);
         if portfolio.is_none() {
            let mut portfolio = Portfolio::empty(chain_id, owner);
            portfolio.add_currency(currency.clone());
            ctx.portfolio_db
               .insert_portfolio(chain_id, owner, portfolio);
         } else {
            let portfolio = portfolio.unwrap();
            portfolio.add_currency(currency.clone());
         }
      });
      let _ = ctx.save_portfolio_db();

      if currency.is_native() {
         return;
      }

      let token = currency.erc20().cloned().unwrap();

      // If no pool data is available, fetch it
      let v3_pools = ctx.get_v3_pools(token.clone());

      let token2 = token.clone();
      let ctx2 = ctx.clone();
      self.show_spinner = true;
      RT.spawn(async move {
         match eth::sync_pools_for_token(ctx2.clone(), token2.clone(), true, v3_pools.is_empty()).await {
            Ok(_) => {
               tracing::info!("Synced Pools for {}", token2.symbol);
            }
            Err(e) => tracing::error!("Error syncing pools for {}: {:?}", token2.symbol, e),
         }

         let pool_manager = ctx2.pool_manager();
         let client = ctx2.get_client_with_id(chain_id).unwrap();
         match pool_manager.update_and_clean(client, chain_id).await {
            Ok(_) => {
               tracing::info!("Updated pool state for {}", token2.symbol);
               let mut gui = SHARED_GUI.write().unwrap();
               gui.portofolio.show_spinner = false;
            }
            Err(e) => {
               tracing::error!("Error updating pool state for {}: {:?}", token2.symbol, e);
               let mut gui = SHARED_GUI.write().unwrap();
               gui.portofolio.show_spinner = false;
            },
         }

         let balance = match eth::get_token_balance(ctx2.clone(), owner, token.clone()).await {
            Ok(b) => b,
            Err(e) => {
               tracing::error!("Error getting token balance: {:?}", e);
               NumericValue::default()
            }
         };
         ctx2.write(|ctx| {
            ctx.balance_db
               .insert_token_balance(chain_id, owner, balance.wei().unwrap(), &token);
         });
         ctx2.update_portfolio_value(chain_id, owner);
         let _ = ctx2.save_pool_data();
         let _ = ctx2.save_balance_db();
         let _ = ctx2.save_portfolio_db();
      });
   }

   fn remove_currency(&self, ctx: ZeusCtx, currency: &Currency, ui: &mut Ui, width: f32) {
      ui.horizontal(|ui| {
         ui.set_width(width);
         if ui.button("X").clicked() {
            let owner = ctx.wallet().key.borrow().address();
            let chain = ctx.chain().id();
            ctx.write(|ctx| {
               let portfolio = ctx.portfolio_db.get_portfolio_mut(chain, owner);
               if let Some(portfolio) = portfolio {
                  portfolio.remove_currency(currency);
               }
            });
            let _ = ctx.save_portfolio_db();
         }
      });
   }
}
