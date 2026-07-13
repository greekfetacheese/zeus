//! This is the UI that shows the portfolio of the current wallet
//!
//! Showed when Home is selected

use crate::assets::icons::Icons;
use crate::core::ZeusCtx;
use crate::gui::{SHARED_GUI, ui::TokenSelectionWindow};
use crate::utils::RT;
use eframe::egui::{
   Align, CornerRadius, CursorIcon, Frame, Grid, Layout, Margin, RichText, ScrollArea, Spinner, Ui,
   Vec2, vec2,
};
use std::collections::HashMap;
use std::sync::Arc;
use zeus_railgun::caip::AssetId;

use zeus_eth::{
   alloy_primitives::{Address, U256},
   currency::{Currency, ERC20Token},
   utils::NumericValue,
};
use zeus_theme::{ButtonVisuals, Theme};
use zeus_widgets::{Button, Label};

type Balance = NumericValue;
type Value = NumericValue;
type Price = NumericValue;

type TokenList = Vec<(ERC20Token, Balance, Value, Price)>;
type TokenMap = HashMap<u64, HashMap<Address, TokenList>>;

fn process_public_tokens(ctx: ZeusCtx, chain_id: u64, owner: Address) -> TokenList {
   let portfolio = ctx.get_portfolio(chain_id, owner);
   let tokens = portfolio.tokens;

   let mut token_list: TokenList = tokens
      .iter()
      .map(|token| {
         let price = ctx.get_token_price(token);
         let balance = ctx.get_token_balance(chain_id, owner, token.address);
         let value = ctx.get_token_value_for_owner(chain_id, owner, token);
         (token.clone(), balance, value, price)
      })
      .collect();

   token_list
      .sort_by(|a, b| b.2.f64().partial_cmp(&a.2.f64()).unwrap_or(std::cmp::Ordering::Equal));

   token_list
}

async fn process_private_tokens(
   ctx: ZeusCtx,
   chain_id: u64,
   owner: Address,
) -> Result<TokenList, anyhow::Error> {
   let mut token_list: TokenList = Vec::new();

   if !ctx.railgun_is_supported(chain_id.into()) {
      return Ok(token_list);
   }

   let mut provider = ctx.get_railgun_provider(chain_id).await?;
   let wallet_info = ctx.get_wallet_info_by_address(owner, true);

   if wallet_info.is_none() {
      return Ok(token_list);
   }

   let wallet_info = wallet_info.unwrap();

   if let Some(address) = wallet_info.railgun_address {
      let private_balances = provider.balance(address).await;

      for entry in private_balances {
         let token_address = match entry.asset {
            AssetId::Erc20(address) => address,
            _ => continue,
         };

         let erc20 = ctx.get_token(chain_id, token_address).await?;
         let balance = NumericValue::format_wei(U256::from(entry.amount), erc20.decimals);
         let price = ctx.get_token_price(&erc20);
         let value = NumericValue::value(balance.f64(), price.f64());
         token_list.push((erc20.clone(), balance, value, price));
      }

      token_list
         .sort_by(|a, b| b.2.f64().partial_cmp(&a.2.f64()).unwrap_or(std::cmp::Ordering::Equal));
   }

   Ok(token_list)
}

pub struct PortfolioUi {
   open: bool,
   loading: bool,
   pub show_spinner: bool,

   /// Cached and sorted list of public tokens by value
   pub public_tokens: TokenMap,

   /// Cached and sorted list of private tokens by value
   pub private_tokens: TokenMap,
}

impl PortfolioUi {
   pub fn new() -> Self {
      Self {
         open: false,
         loading: false,
         show_spinner: false,
         public_tokens: HashMap::new(),
         private_tokens: HashMap::new(),
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self, ctx: ZeusCtx) {
      self.open = true;

      let chain_id = ctx.chain().id();
      let owner = ctx.current_wallet_info(false).address;
      self.process_tokens(ctx, chain_id, owner);
   }

   pub fn close(&mut self) {
      self.open = false;
   }

   pub fn process_tokens(&mut self, ctx: ZeusCtx, chain_id: u64, owner: Address) {
      self.loading = true;

      RT.spawn(async move {
         let tokens = process_public_tokens(ctx.clone(), chain_id, owner);
         let private = false;

         SHARED_GUI.write(|gui| {
            gui.portofolio.add_token_list(private, chain_id, owner, tokens);
         });

         let tokens = match process_private_tokens(ctx.clone(), chain_id, owner).await {
            Ok(tokens) => tokens,
            Err(e) => {
               tracing::error!("Failed to process private tokens: {:?}", e);
               Vec::new()
            }
         };

         let private = true;

         SHARED_GUI.write(|gui| {
            gui.portofolio.add_token_list(private, chain_id, owner, tokens);
            gui.portofolio.loading = false;
         });
      });
   }

   pub fn add_token_list(&mut self, private: bool, chain_id: u64, owner: Address, list: TokenList) {
      if !private {
         if let Some(tokens) = self.public_tokens.get_mut(&chain_id) {
            tokens.insert(owner, list);
         } else {
            let mut tokens = HashMap::new();
            tokens.insert(owner, list);
            self.public_tokens.insert(chain_id, tokens);
         }
      } else {
         if let Some(tokens) = self.private_tokens.get_mut(&chain_id) {
            tokens.insert(owner, list);
         } else {
            let mut tokens = HashMap::new();
            tokens.insert(owner, list);
            self.private_tokens.insert(chain_id, tokens);
         }
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
      let wallet_info = ctx.current_wallet_info(false);
      let owner = wallet_info.address;
      let portfolio = ctx.get_portfolio(chain_id, owner);

      Frame::new().outer_margin(Margin::same(5)).show(ui, |ui| {
         ui.vertical_centered_justified(|ui| {
            ui.set_width(ui.available_width() * 0.7);

            ui.spacing_mut().item_spacing = Vec2::new(16.0, 15.0);

            let frame = theme.frame1;

            frame.show(ui, |ui| {
               ui.horizontal(|ui| {
                  // Wallet Name - Total Value (centered)
                  ui.vertical_centered(|ui| {
                     ui.label(RichText::new(wallet_info.name()).size(theme.text_sizes.very_large));
                     ui.label(
                        RichText::new(format!("${}", portfolio.value.abbreviated()))
                           .heading()
                           .size(theme.text_sizes.heading + 4.0),
                     );
                  });

                  // Refresh - Add Token (right)
                  ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                     ui.spacing_mut().button_padding = vec2(10.0, 8.0);

                     let button_visuals = theme.button_visuals();
                     let text = RichText::new("Add Token").size(theme.text_sizes.normal);
                     let add_token = Button::new(text).visuals(button_visuals);

                     if ui.add(add_token).clicked() {
                        token_selection.open(ctx.clone(), chain_id, owner);
                     }

                     let tint = theme.image_tint_recommended;
                     let icon = match theme.dark_mode {
                        true => icons.refresh_white_x22(tint),
                        false => icons.refresh_dark_x22(tint),
                     };

                     if !self.show_spinner {
                        let mut visuals = ButtonVisuals::default();
                        visuals.bg_hover = button_visuals.bg_hover;
                        visuals.corner_radius = CornerRadius::same(25);
                        let button = Button::image(icon).small().visuals(visuals);
                        let res = ui.add(button).on_hover_cursor(CursorIcon::PointingHand);

                        if res.clicked() {
                           self.refresh(owner, ctx.clone());
                        }
                     } else {
                        ui.add(Spinner::new().size(17.0).color(theme.colors.text));
                     }
                  });
               });
            });

            // Token List
            ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
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
                     .striped(true)
                     .show(ui, |ui| {
                        // Header
                        ui.label(RichText::new("Asset").size(theme.text_sizes.large));

                        ui.label(RichText::new("Price").size(theme.text_sizes.large));

                        ui.label(RichText::new("Balance").size(theme.text_sizes.large));

                        ui.label(RichText::new("Value").size(theme.text_sizes.large));

                        ui.end_row();

                        let native_currency = Currency::native(chain_id);

                        // Show the native currency first
                        self.native(
                           theme,
                           icons.clone(),
                           &native_currency,
                           ui,
                           column_widths[0],
                        );

                        self.price_balance_value_native(
                           ctx.clone(),
                           theme,
                           chain_id,
                           owner,
                           &native_currency,
                           ui,
                           column_widths[0],
                        );

                        ui.end_row();

                        let is_privacy_mode = ctx.read(|ctx| ctx.privacy_mode);
                        let empty_map = HashMap::new();
                        let empty_list = Vec::new();
                        let token_map = if is_privacy_mode {
                           self.private_tokens.get(&chain_id).unwrap_or(&empty_map)
                        } else {
                           self.public_tokens.get(&chain_id).unwrap_or(&empty_map)
                        };
                        let token_list = token_map.get(&owner).unwrap_or(&empty_list);

                        // Show the rest of the tokens
                        for (token, balance, value, price) in token_list {
                           self.token(theme, icons.clone(), token, ui, column_widths[0]);

                           self.price_balance_value_token(
                              theme,
                              balance,
                              value,
                              price,
                              ui,
                              column_widths[0],
                           );

                           self.remove_token(
                              ctx.clone(),
                              theme,
                              owner,
                              token,
                              ui,
                              column_widths[4],
                           );

                           ui.end_row();
                        }
                     });
               });

               // Token selection
               token_selection.show(
                  ctx.clone(),
                  theme,
                  icons.clone(),
                  chain_id,
                  owner,
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
      });
   }

   fn native(
      &self,
      theme: &Theme,
      icons: Arc<Icons>,
      currency: &Currency,
      ui: &mut Ui,
      width: f32,
   ) {
      let visuals = theme.label_visuals();
      let tint = theme.image_tint_recommended;
      let icon = icons.currency_icon_x32(currency, tint);

      ui.horizontal(|ui| {
         ui.set_width(width);
         ui.add(icon);
         let text = RichText::new(currency.symbol()).size(theme.text_sizes.normal);
         let label = Label::new(text, None).wrap().visuals(visuals).interactive(false);
         ui.scope(|ui| {
            ui.set_max_width(100.0);
            ui.add(label).on_hover_text(currency.name());
         });
      });
   }

   fn token(&self, theme: &Theme, icons: Arc<Icons>, token: &ERC20Token, ui: &mut Ui, width: f32) {
      let visuals = theme.label_visuals();
      let tint = theme.image_tint_recommended;
      let icon = icons.token_icon_x32(token.address, token.chain_id, tint);

      ui.horizontal(|ui| {
         ui.set_width(width);
         ui.add(icon);
         let text = RichText::new(&*token.symbol).size(theme.text_sizes.normal);
         let label = Label::new(text, None).wrap().visuals(visuals).interactive(false);
         ui.scope(|ui| {
            ui.set_max_width(100.0);
            ui.add(label).on_hover_text(&*token.name);
         });
      });
   }

   fn price_balance_value_native(
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
         ui.label(RichText::new(balance.abbreviated()).size(theme.text_sizes.normal));
      });

      let value = ctx.get_currency_value_for_owner(chain, owner, currency);
      ui.horizontal(|ui| {
         ui.set_width(width);
         ui.label(RichText::new(format!("${}", value.abbreviated())).size(theme.text_sizes.normal));
      });
   }

   fn price_balance_value_token(
      &self,
      theme: &Theme,
      balance: &NumericValue,
      value: &NumericValue,
      price: &NumericValue,
      ui: &mut Ui,
      width: f32,
   ) {
      ui.horizontal(|ui| {
         ui.set_width(width);
         ui.label(RichText::new(format!("${}", price.formatted())).size(theme.text_sizes.normal));
      });

      ui.horizontal(|ui| {
         ui.set_width(width);
         ui.label(RichText::new(balance.abbreviated()).size(theme.text_sizes.normal));
      });

      ui.horizontal(|ui| {
         ui.set_width(width);
         ui.label(RichText::new(format!("${}", value.formatted())).size(theme.text_sizes.normal));
      });
   }

   fn refresh(&mut self, owner: Address, ctx: ZeusCtx) {
      self.show_spinner = true;
      RT.spawn(async move {
         let chain = ctx.chain().id();
         let portfolio = ctx.get_portfolio(chain, owner);
         let tokens = portfolio.tokens.clone();

         // Update the eth and token balances
         let balance_manager = ctx.balance_manager();

         match balance_manager.update_eth_balance(ctx.clone(), chain, vec![owner], false).await {
            Ok(_) => {}
            Err(e) => tracing::error!("Error updating eth balance: {:?}", e),
         }

         match balance_manager
            .update_tokens_balance(ctx.clone(), chain, owner, tokens.clone(), false)
            .await
         {
            Ok(_) => {}
            Err(e) => tracing::error!("Error updating tokens balance: {:?}", e),
         }

         // Update the pool state that includes these tokens
         let pool_manager = ctx.pool_manager();

         match pool_manager.sync_pools_for_tokens(ctx.clone(), chain, tokens.clone()).await {
            Ok(_) => {}
            Err(e) => tracing::error!("Error syncing pools: {:?}", e),
         }

         let mut pools = Vec::new();

         for token in tokens {
            if token.is_base() {
               continue;
            }

            let c = token.into();
            pools.extend(pool_manager.get_pools_that_have_currency(&c));
         }

         match pool_manager.update_state_for_pools(ctx.clone(), chain, pools).await {
            Ok(_) => {}
            Err(e) => tracing::error!("Error updating pool state: {:?}", e),
         }

         ctx.calculate_portfolio_value(chain, owner);

         let ctx_clone = ctx.clone();
         RT.spawn_blocking(move || {
            ctx_clone.save_portfolio_db();
            ctx_clone.save_balance_manager();
            ctx_clone.save_pool_manager();
            ctx_clone.save_price_manager();
         });

         SHARED_GUI.write(|gui| {
            gui.portofolio.process_tokens(ctx.clone(), chain, owner);
            gui.portofolio.show_spinner = false;
         });
      });
   }

   // Add a currency to the portfolio and update the portfolio value
   fn add_currency(
      &mut self,
      ctx: ZeusCtx,
      owner: Address,
      token_fetched: bool,
      currency: Currency,
   ) {
      if currency.is_native() {
         return;
      }

      let chain_id = ctx.chain().id();

      let mut portfolio = ctx.get_portfolio(chain_id, owner);
      portfolio.add_token(currency.to_erc20().into_owned());
      ctx.write(|ctx| {
         ctx.portfolio_db.insert_portfolio(chain_id, owner, portfolio);
      });

      let ctx_clone = ctx.clone();
      RT.spawn_blocking(move || {
         ctx_clone.save_portfolio_db();

         SHARED_GUI.write(|gui| {
            gui.portofolio.process_tokens(ctx_clone, chain_id, owner);
         });
      });

      let token = currency.to_erc20().into_owned();

      // if token was fetched from the blockchain, we don't need to sync the pools or the balance
      if token_fetched {
         tracing::info!(
            "Token {} was fetched from the blockchain, no need to sync pools or balance",
            token.symbol
         );
         return;
      }

      let manager = ctx.pool_manager();
      let ctx_clone = ctx.clone();
      self.show_spinner = true;
      RT.spawn(async move {
         match manager
            .sync_pools_for_tokens(ctx_clone.clone(), chain_id, vec![token.clone()])
            .await
         {
            Ok(_) => {
               tracing::info!("Synced Pools for {}", token.symbol);
            }
            Err(e) => tracing::error!(
               "Error syncing pools for {}: {:?}",
               token.symbol,
               e
            ),
         }

         // Avoid potentialy syncing hundreds of pools
         if !currency.is_base() {
            match manager.update_for_currencies(ctx_clone.clone(), chain_id, vec![currency]).await {
               Ok(_) => {
                  tracing::info!("Updated pool state for {}", token.symbol);
               }
               Err(e) => {
                  tracing::error!(
                     "Error updating pool state for {}: {:?}",
                     token.symbol,
                     e
                  );
               }
            }
         }

         let balance_manager = ctx_clone.balance_manager();
         match balance_manager
            .update_tokens_balance(
               ctx_clone.clone(),
               chain_id,
               owner,
               vec![token],
               false,
            )
            .await
         {
            Ok(_) => {}
            Err(e) => tracing::error!("Error updating tokens balance: {:?}", e),
         }

         SHARED_GUI.write(|gui| {
            gui.portofolio.show_spinner = false;
         });

         RT.spawn_blocking(move || {
            ctx_clone.calculate_portfolio_value(chain_id, owner);
            ctx_clone.save_all();

            SHARED_GUI.write(|gui| {
               gui.portofolio.process_tokens(ctx_clone, chain_id, owner);
            });
         });
      });
   }

   fn remove_token(
      &self,
      ctx: ZeusCtx,
      theme: &Theme,
      owner: Address,
      token: &ERC20Token,
      ui: &mut Ui,
      width: f32,
   ) {
      let visuals = theme.button_visuals();
      ui.horizontal(|ui| {
         ui.set_width(width);
         let button = Button::new(RichText::new("X").size(theme.text_sizes.small))
            .visuals(visuals)
            .small();

         if ui.add(button).clicked() {
            let chain = ctx.chain().id();

            let mut portfolio = ctx.get_portfolio(chain, owner);
            portfolio.remove_token(token);
            ctx.write(|ctx| ctx.portfolio_db.insert_portfolio(chain, owner, portfolio));

            RT.spawn_blocking(move || {
               SHARED_GUI.write(|gui| {
                  gui.portofolio.process_tokens(ctx.clone(), chain, owner);
               });

               ctx.calculate_portfolio_value(chain, owner);
               ctx.save_all();
            });
         }
      });
   }
}
