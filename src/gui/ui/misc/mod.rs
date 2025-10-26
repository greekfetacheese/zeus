use eframe::egui::{
   Align, Align2, Button, Color32, Frame, Grid, Layout, Order, RichText, ScrollArea, Sense,
   Spinner, Ui, Vec2, Window, vec2,
};
use std::sync::Arc;
use zeus_eth::amm::uniswap::DexKind;

use crate::assets::icons::Icons;
use crate::core::{Wallet, ZeusCtx, utils::RT};
use crate::gui::SHARED_GUI;
use crate::gui::ui::TokenSelectionWindow;

use egui_widgets::{ComboBox, Label};
use zeus_eth::{
   alloy_primitives::Address,
   currency::{Currency, ERC20Token},
   types::ChainId,
};
use zeus_theme::Theme;

pub mod dev;
pub mod sync;
pub mod tx_history;

/// A ComboBox to select a chain
pub struct ChainSelect {
   pub id: &'static str,
   pub chain: ChainId,
   pub size: Vec2,
   pub show_icon: bool,
   pub expansion: Option<f32>,
}

impl ChainSelect {
   pub fn new(id: &'static str, default_chain: u64) -> Self {
      Self {
         id,
         chain: ChainId::new(default_chain).unwrap(),
         size: (200.0, 25.0).into(),
         show_icon: true,
         expansion: Some(4.0),
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
   pub fn show(
      &mut self,
      ignore_chain: u64,
      theme: &Theme,
      icons: Arc<Icons>,
      ui: &mut Ui,
   ) -> bool {
      let current_chain = self.chain;
      let mut clicked = false;
      let supported_chains = ChainId::supported_chains();
      let expansion = self.expansion;

      let text_size = theme.text_sizes.normal;
      let tint = theme.image_tint_recommended;
      let icon = icons.chain_icon(current_chain.id(), tint);

      let current_chain_label = Label::new(
         RichText::new(current_chain.name()).size(text_size),
         Some(icon),
      )
      .image_on_left()
      .sense(Sense::click());

      ComboBox::new(self.id, current_chain_label)
         .width(self.size.x)
         .show_ui(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 10.0;

            for chain in supported_chains {
               if chain.id() == ignore_chain {
                  continue;
               }

               let text = RichText::new(chain.name()).size(text_size);
               let icon = icons.chain_icon(chain.id(), tint);

               let is_selected = chain == current_chain;
               let chain_label = Label::new(text.clone(), Some(icon))
                  .image_on_left()
                  .expand(expansion)
                  .fill_width(true)
                  .selected(is_selected)
                  .sense(Sense::click());

               if ui.add(chain_label).clicked() {
                  self.chain = chain;
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
   pub wallet: Wallet,
   pub size: Vec2,
   pub expansion: Option<f32>,
}

impl WalletSelect {
   pub fn new(id: &'static str) -> Self {
      Self {
         id,
         wallet: Wallet::new_rng("I should not be here2".to_string()),
         size: (200.0, 25.0).into(),
         expansion: Some(6.0),
      }
   }

   pub fn size(mut self, size: impl Into<Vec2>) -> Self {
      self.size = size.into();
      self
   }

   pub fn expansion(mut self, expansion: f32) -> Self {
      self.expansion = Some(expansion);
      self
   }

   /// Show the ComboBox
   ///
   /// Returns true if the wallet was changed
   pub fn show(&mut self, theme: &Theme, ctx: ZeusCtx, icons: Arc<Icons>, ui: &mut Ui) -> bool {
      let mut clicked = false;
      let expansion = self.expansion;

      let wallet_icon = icons.wallet_main_x24();
      let text = RichText::new(&self.wallet.name_with_id_short()).size(theme.text_sizes.normal);

      let current_wallet_label = Label::new(text, Some(wallet_icon))
         .image_on_left()
         .expand(expansion)
         .sense(Sense::click());

      ComboBox::new(self.id, current_wallet_label)
         .width(self.size.x)
         .show_ui(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 14.0;

            ctx.read(|ctx| {
               for wallet in ctx.vault_ref().all_wallets() {
                  let is_selected = wallet.address() == self.wallet.address();
                  let text =
                     RichText::new(wallet.name_with_id_short()).size(theme.text_sizes.normal);

                  let wallet_label = Label::new(text, None)
                     .fill_width(true)
                     .expand(expansion)
                     .selected(is_selected)
                     .sense(Sense::click());

                  if ui.add(wallet_label).clicked() {
                     self.wallet = wallet.clone();
                     clicked = true;
                  }
               }
            });
         });

      clicked
   }
}

/// A Window to prompt the user to confirm an action
pub struct ConfirmWindow {
   pub open: bool,
   pub confirm: Option<bool>,
   pub msg: String,
   pub msg2: Option<String>,
   pub size: (f32, f32),
}

impl ConfirmWindow {
   pub fn new() -> Self {
      Self {
         open: false,
         confirm: None,
         msg: String::new(),
         msg2: None,
         size: (200.0, 100.0),
      }
   }

   pub fn open(&mut self, msg: impl Into<String>) {
      self.open = true;
      self.msg = msg.into();
   }

   pub fn set_msg2(&mut self, msg: impl Into<String>) {
      self.msg2 = Some(msg.into());
   }

   pub fn get_confirm(&self) -> Option<bool> {
      self.confirm
   }

   pub fn reset(&mut self) {
      self.open = false;
      self.msg.clear();
      self.msg2.take();
      self.confirm = None;
   }

   pub fn show(&mut self, theme: &Theme, ui: &mut Ui) {
      if !self.open {
         return;
      }

      Window::new("confirm_window")
         .title_bar(false)
         .resizable(false)
         .order(Order::Foreground)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .collapsible(false)
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);
            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = 15.0;
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);

               ui.label(RichText::new(&self.msg).size(theme.text_sizes.normal));

               if let Some(msg) = &self.msg2 {
                  ui.label(RichText::new(msg).size(theme.text_sizes.normal));
               }

               if ui
                  .add(Button::new(
                     RichText::new("Confirm").size(theme.text_sizes.normal),
                  ))
                  .clicked()
               {
                  self.open = false;
                  self.confirm = Some(true);
               }

               if ui
                  .add(Button::new(
                     RichText::new("Reject").size(theme.text_sizes.normal),
                  ))
                  .clicked()
               {
                  self.open = false;
                  self.confirm = Some(false);
               }
            });
         });
   }
}

/// Window to indicate a loading state
pub struct LoadingWindow {
   open: bool,
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
      self.size = (200.0, 100.0);
   }

   pub fn new_size(&mut self, size: (f32, f32)) {
      self.size = size;
   }

   pub fn show(&mut self, ui: &mut Ui) {
      if !self.open {
         return;
      }

      Window::new("Loading")
         .title_bar(false)
         .order(Order::Debug)
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
   pub size: (f32, f32),
}

impl MsgWindow {
   pub fn new() -> Self {
      Self {
         open: false,
         title: String::new(),
         message: String::new(),
         size: (300.0, 100.0),
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

      Window::new(title)
         .resizable(false)
         .order(Order::Debug)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .collapsible(false)
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = 20.0;
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);

               ui.label(msg);

               let size = vec2(ui.available_width() * 0.5, 25.0);
               let ok_button =
                  Button::new(RichText::new("OK").size(theme.text_sizes.normal)).min_size(size);
               if ui.add(ok_button).clicked() {
                  self.open = false;
               }
            });
         });
   }
}

pub struct PortfolioUi {
   open: bool,
   pub show_spinner: bool,
}

impl PortfolioUi {
   pub fn new() -> Self {
      Self {
         open: false,
         show_spinner: false,
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
      let owner = ctx.current_wallet_address();
      let portfolio = ctx.get_portfolio(chain_id, owner);
      let tokens = &portfolio.tokens;

      ui.vertical_centered_justified(|ui| {
         ui.set_width(ui.available_width() * 0.8);

         ui.spacing_mut().item_spacing = Vec2::new(16.0, 20.0);

         ui.horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);

               let add_token =
                  Button::new(RichText::new("Add Token").size(theme.text_sizes.normal));
               if ui.add(add_token).clicked() {
                  token_selection.open(ctx.clone(), chain_id, owner);
               }

               let refresh = Button::new(RichText::new("⟲").size(theme.text_sizes.normal));
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
                     let wallet_name = ctx.current_wallet_name();
                     ui.label(RichText::new(wallet_name).size(theme.text_sizes.very_large));
                     ui.add_space(8.0);
                     ui.label(
                        RichText::new(format!(
                           "${}",
                           portfolio.value.abbreviated()
                        ))
                        .heading()
                        .size(theme.text_sizes.heading + 4.0),
                     );
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

                     let native_wrapped = ERC20Token::wrapped_native_token(chain_id);
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

                     // Show the Wrapped native next if there is a balance

                     let wrapped_balance =
                        ctx.get_token_balance(chain_id, owner, native_wrapped.address);

                     if !wrapped_balance.is_zero() {
                        self.token(
                           theme,
                           icons.clone(),
                           &native_wrapped,
                           ui,
                           column_widths[0],
                        );

                        self.price_balance_value_token(
                           ctx.clone(),
                           theme,
                           chain_id,
                           owner,
                           &native_wrapped,
                           ui,
                           column_widths[0],
                        );

                        self.remove_token(
                           ctx.clone(),
                           owner,
                           &native_wrapped,
                           ui,
                           column_widths[4],
                        );

                        ui.end_row();
                     }

                     // Show the rest of the tokens

                     for token in tokens {
                        self.token(theme, icons.clone(), token, ui, column_widths[0]);

                        self.price_balance_value_token(
                           ctx.clone(),
                           theme,
                           chain_id,
                           owner,
                           token,
                           ui,
                           column_widths[0],
                        );

                        self.remove_token(ctx.clone(), owner, token, ui, column_widths[4]);

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
   }

   fn native(
      &self,
      theme: &Theme,
      icons: Arc<Icons>,
      currency: &Currency,
      ui: &mut Ui,
      width: f32,
   ) {
      let tint = theme.image_tint_recommended;
      let icon = icons.currency_icon(currency, tint);

      ui.horizontal(|ui| {
         ui.set_width(width);
         ui.add(icon);
         let text = RichText::new(currency.symbol()).size(theme.text_sizes.normal);
         let label = Label::new(text, None).wrap();
         ui.scope(|ui| {
            ui.set_max_width(100.0);
            ui.add(label).on_hover_text(currency.name());
         });
      });
   }

   fn token(&self, theme: &Theme, icons: Arc<Icons>, token: &ERC20Token, ui: &mut Ui, width: f32) {
      let tint = theme.image_tint_recommended;
      let icon = icons.token_icon_x32(token.address, token.chain_id, tint);

      ui.horizontal(|ui| {
         ui.set_width(width);
         ui.add(icon);
         let text = RichText::new(&token.symbol).size(theme.text_sizes.normal);
         let label = Label::new(text, None).wrap();
         ui.scope(|ui| {
            ui.set_max_width(100.0);
            ui.add(label).on_hover_text(&token.name);
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
         ui.label(
            RichText::new(format!("${}", value.abbreviated())).size(theme.text_sizes.normal),
         );
      });
   }

   fn price_balance_value_token(
      &self,
      ctx: ZeusCtx,
      theme: &Theme,
      chain: u64,
      owner: Address,
      token: &ERC20Token,
      ui: &mut Ui,
      width: f32,
   ) {
      let price = ctx.get_token_price(token);

      ui.horizontal(|ui| {
         ui.set_width(width);
         ui.label(RichText::new(format!("${}", price.formatted())).size(theme.text_sizes.normal));
      });

      let balance = ctx.get_token_balance(chain, owner, token.address);

      ui.horizontal(|ui| {
         ui.set_width(width);
         ui.label(RichText::new(balance.abbreviated()).size(theme.text_sizes.normal));
      });

      let value = ctx.get_token_value_for_owner(chain, owner, token);
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
         let dex = DexKind::main_dexes(chain);

         match pool_manager.sync_pools_for_tokens(ctx.clone(), tokens.clone(), dex).await {
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

         RT.spawn_blocking(move || {
            ctx.save_portfolio_db();
            ctx.save_balance_manager();
            ctx.save_pool_manager();
            ctx.save_price_manager();
         });

         SHARED_GUI.write(|gui| {
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
      let dex_kinds = DexKind::main_dexes(chain_id);
      self.show_spinner = true;
      RT.spawn(async move {
         match manager
            .sync_pools_for_tokens(ctx_clone.clone(), vec![token.clone()], dex_kinds)
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
         });
      });
   }

   fn remove_token(
      &self,
      ctx: ZeusCtx,
      owner: Address,
      token: &ERC20Token,
      ui: &mut Ui,
      width: f32,
   ) {
      ui.horizontal(|ui| {
         ui.set_width(width);
         if ui.button("X").clicked() {
            let chain = ctx.chain().id();

            let mut portfolio = ctx.get_portfolio(chain, owner);
            portfolio.remove_token(token);
            ctx.write(|ctx| ctx.portfolio_db.insert_portfolio(chain, owner, portfolio));

            RT.spawn_blocking(move || {
               ctx.calculate_portfolio_value(chain, owner);
               ctx.save_all();
            });
         }
      });
   }
}
