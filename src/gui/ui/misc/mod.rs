use eframe::egui::{
   Align, Align2, Button, Color32, Frame, Grid, Id, Layout, Order, RichText, ScrollArea, Spinner, Ui, Vec2, Window,
   vec2,
};
use egui::Sense;
use std::sync::Arc;
use zeus_eth::utils::NumericValue;

use crate::assets::icons::Icons;
use crate::core::utils::update::update_portfolio_state;
use crate::core::{
   WalletInfo, ZeusCtx,
   utils::{RT, eth},
};
use crate::gui::SHARED_GUI;
use crate::gui::ui::TokenSelectionWindow;

use egui_theme::{Theme, utils::*};
use egui_widgets::{ComboBox, Label};
use zeus_eth::{currency::Currency, types::ChainId, alloy_primitives::Address};

pub mod tx_history;

/// A ComboBox to select a chain
pub struct ChainSelect {
   pub id: &'static str,
   pub grid_id: &'static str,
   pub chain: ChainId,
   pub size: Vec2,
   pub show_icon: bool,
}

impl ChainSelect {
   pub fn new(id: &'static str, default_chain: u64) -> Self {
      Self {
         id,
         grid_id: "chain_select_grid",
         chain: ChainId::new(default_chain).unwrap(),
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
   pub fn show(&mut self, ignore_chain: u64, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) -> bool {
      let selected_chain = self.chain;
      let mut clicked = false;
      let supported_chains = ChainId::supported_chains();
      let icon = icons.chain_icon(&selected_chain.id());
      let selected_chain = Label::new(
         RichText::new(selected_chain.name()).size(theme.text_sizes.normal),
         Some(icon),
      )
      .text_first(false)
      .sense(Sense::click());

      // Add the ComboBox with the specified size
      ComboBox::new(self.id, selected_chain)
         .width(self.size.x)
         .show_ui(ui, |ui| {
            for chain in supported_chains {
               if chain.id() == ignore_chain {
                  continue;
               }

               let text = RichText::new(chain.name()).size(theme.text_sizes.normal);
               let icon = icons.chain_icon(&chain.id());
               let chain_label = Label::new(text.clone(), Some(icon))
                  .text_first(false)
                  .sense(Sense::click());

               if ui.add(chain_label).clicked() {
                  self.chain = chain.clone();
                  clicked = true;
               }
            }
         });
      clicked
   }
}

/// A ComboBox to select a wallet
pub struct WalletSelect {
   pub id: &'static str,
   /// Selected Wallet
   pub wallet: WalletInfo,
   pub size: Vec2,
   pub button_padding: Vec2,
}

impl WalletSelect {
   pub fn new(id: &'static str) -> Self {
      Self {
         id,
         wallet: WalletInfo::default(),
         size: (200.0, 25.0).into(),
         button_padding: vec2(10.0, 4.0),
      }
   }

   pub fn size(mut self, size: impl Into<Vec2>) -> Self {
      self.size = size.into();
      self
   }

   pub fn button_padding(mut self, button_padding: impl Into<Vec2>) -> Self {
      self.button_padding = button_padding.into();
      self
   }

   /// Show the ComboBox
   ///
   /// Returns true if the wallet was changed
   pub fn show(&mut self, theme: &Theme, wallets: &Vec<WalletInfo>, _icons: Arc<Icons>, ui: &mut Ui) -> bool {
      let mut clicked = false;
      let text = RichText::new(&self.wallet.name).size(theme.text_sizes.normal);

      ComboBox::new(self.id, Label::new(text, None).sense(Sense::click()))
         .width(self.size.x)
         .show_ui(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 5.0;
            ui.spacing_mut().button_padding = self.button_padding;

            for wallet in wallets {
               let text = RichText::new(wallet.name.clone()).size(theme.text_sizes.normal);
               let wallet_label = Label::new(text, None).sense(Sense::click());

               if ui.add(wallet_label).clicked() {
                  self.wallet = wallet.clone();
                  clicked = true;
               }
            }
         });

      clicked
   }
}

/// Testing Window
pub struct TestingWindow {
   pub open: bool,
   pub size: (f32, f32),
   pub chain: ChainId,
   pub id: Id,
}

impl TestingWindow {
   pub fn new() -> Self {
      Self {
         open: false,
         size: (500.0, 400.0),
         chain: ChainId::new(1).unwrap(),
         id: Id::new("test_window"),
      }
   }

   pub fn open(&mut self) {
      self.open = true;
   }

   pub fn reset(&mut self) {
      self.open = false;
   }

   pub fn show(&mut self, theme: &Theme, _icons: Arc<Icons>, ui: &mut Ui) {
      if !self.open {
         return;
      }

      Window::new(RichText::new("Testing Window").size(theme.text_sizes.normal))
         .title_bar(true)
         .resizable(false)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing = vec2(0.0, 10.0);
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);
               ui.set_width(self.size.0);
               ui.set_height(self.size.1);

               if ui.button("Close").clicked() {
                  self.open = false;
               }
            });
         });
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
               ui.label(RichText::new(&self.msg).size(17.0));
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

   pub fn show(&mut self, theme: &Theme, ui: &mut Ui) {
      if !self.open {
         return;
      }

      let title = RichText::new(self.title.clone()).size(theme.text_sizes.large);
      let msg = RichText::new(&self.message).size(theme.text_sizes.normal);
      let ok = Button::new(RichText::new("Ok").size(theme.text_sizes.normal));

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
                  ui.spacing_mut().button_padding = vec2(10.0, 8.0);

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
      let current_wallet = ctx.current_wallet();
      let owner = current_wallet.address;
      let portfolio = ctx.get_portfolio(chain_id, owner);
      let currencies = portfolio.currencies();

      ui.vertical_centered_justified(|ui| {
         ui.set_width(ui.available_width() * 0.8);

         ui.spacing_mut().item_spacing = Vec2::new(16.0, 20.0);

         ui.horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);
               let visuals = theme.get_button_visuals(theme.colors.bg_color);
               widget_visuals(ui, visuals);

               let add_token = Button::new(RichText::new("Add Token").size(theme.text_sizes.normal));
               if ui.add(add_token).clicked() {
                  token_selection.open = true;
               }

               let refresh = Button::new(RichText::new("Refresh").size(theme.text_sizes.normal));
               if ui.add(refresh).clicked() {
                  self.refresh(owner, ctx.clone());
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
                     let wallet_name = current_wallet.name.clone();
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
                           self.price_balance_value(ctx.clone(), theme, chain_id, owner, native, ui, column_widths[0]);
                           self.remove_currency(ctx.clone(), owner, native, ui, column_widths[4]);
                           ui.end_row();
                        }

                        if let Some(wrapped) = native_wrapped {
                           self.token(theme, icons.clone(), wrapped, ui, column_widths[0]);
                           self.price_balance_value(ctx.clone(), theme, chain_id, owner, wrapped, ui, column_widths[0]);
                           self.remove_currency(ctx.clone(), owner, wrapped, ui, column_widths[4]);
                           ui.end_row();
                        }

                        for token in tokens {
                           if token.is_native_wrapped() {
                              continue;
                           }
                           self.token(theme, icons.clone(), token, ui, column_widths[0]);
                           self.price_balance_value(ctx.clone(), theme, chain_id, owner, token, ui, column_widths[0]);
                           self.remove_currency(ctx.clone(), owner, token, ui, column_widths[4]);
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
                  let token_fetched = token_selection.token_fetched;
                  token_selection.reset();
                  self.add_currency(ctx.clone(), owner, token_fetched, currency);
               }
            });
      });
   }

   fn token(&self, theme: &Theme, icons: Arc<Icons>, currency: &Currency, ui: &mut Ui, width: f32) {
      let icon = icons.currency_icon(currency);
      ui.horizontal(|ui| {
         ui.set_width(width);
         ui.add(icon);
         ui.label(RichText::new(currency.symbol()).size(theme.text_sizes.normal))
            .on_hover_text(currency.name());
      });
   }

   fn price_balance_value(
      &self,
      ctx: ZeusCtx,
      theme: &Theme,
      chain: u64,
      owner: Address,
      currency: &Currency,
      ui: &mut Ui,
      width: f32,
   ) {
      let price = ctx.get_currency_price(currency);

      ui.horizontal(|ui| {
         ui.set_width(width);
         ui.label(RichText::new(format!("${}", price.formatted())).size(theme.text_sizes.normal));
      });

      let balance = ctx.get_currency_balance(chain, owner, currency);

      ui.horizontal(|ui| {
         ui.set_width(width);
         ui.label(RichText::new(balance.formatted()).size(theme.text_sizes.normal));
      });

      let value = ctx.get_currency_value(chain, owner, currency);
      ui.horizontal(|ui| {
         ui.set_width(width);
         ui.label(RichText::new(format!("${}", value.formatted())).size(theme.text_sizes.normal));
      });
   }

   fn refresh(&mut self, owner: Address, ctx: ZeusCtx) {
      self.show_spinner = true;
      RT.spawn(async move {
         let chain = ctx.chain().id();

         match update_portfolio_state(ctx, chain, owner).await {
            Ok(_) => {
               tracing::info!("Updated portfolio state");
            }
            Err(e) => {
               tracing::error!("Error updating portfolio state: {:?}", e);
            }
         }

         SHARED_GUI.write(|gui| {
            gui.portofolio.show_spinner = false;
         });
      });
   }

   // Add a currency to the portfolio and update the portfolio value
   fn add_currency(&mut self, ctx: ZeusCtx, owner: Address, token_fetched: bool, currency: Currency) {
      let chain_id = ctx.chain().id();

      ctx.write(|ctx| {
         ctx.portfolio_db
            .add_currency(chain_id, owner, currency.clone());
      });

      let ctx_clone = ctx.clone();
      RT.spawn_blocking(move || {
         let _ = ctx_clone.save_portfolio_db();
      });

      if currency.is_native() {
         return;
      }

      let token = currency.erc20().cloned().unwrap();

      // if token was fetched from the blockchain, we don't need to sync the pools or the balance
      if token_fetched {
         tracing::info!(
            "Token {} was fetched from the blockchain, no need to sync pools or balance",
            token.symbol
         );
         return;
      }

      let v3_pools = ctx.get_v3_pools(&token);
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
         match pool_manager.update(client, chain_id).await {
            Ok(_) => {
               tracing::info!("Updated pool state for {}", token2.symbol);
               SHARED_GUI.write(|gui| {
                  gui.portofolio.show_spinner = false;
               });
            }
            Err(e) => {
               tracing::error!("Error updating pool state for {}: {:?}", token2.symbol, e);
               SHARED_GUI.write(|gui| {
                  gui.portofolio.show_spinner = false;
               });
            }
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
         RT.spawn_blocking(move || {
            ctx2.update_portfolio_value(chain_id, owner);
            ctx2.save_all();
         });
      });
   }

   fn remove_currency(&self, ctx: ZeusCtx, owner: Address, currency: &Currency, ui: &mut Ui, width: f32) {
      ui.horizontal(|ui| {
         ui.set_width(width);
         if ui.button("X").clicked() {
            let chain = ctx.chain().id();
            ctx.write(|ctx| {
               ctx.portfolio_db.remove_currency(chain, owner, currency);
            });
            RT.spawn_blocking(move || {
            ctx.update_portfolio_value(chain, owner);
            ctx.save_all();
            });
         }
      });
   }
}
