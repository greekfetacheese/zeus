use super::UniswapSettingsUi;
use crate::core::ZeusCtx;
use crate::gui::ui::dapps::uniswap::ProtocolVersion;
use crate::gui::ui::*;
use crate::{assets::icons::Icons, gui::SHARED_GUI};
use egui::{
   Align, Button, Color32, ComboBox, FontId, Frame, Grid, Id, Layout, Margin, RichText, ScrollArea,
   Spinner, TextEdit, Ui, Window, vec2,
};
use egui_theme::Theme;
use std::sync::Arc;
use std::{collections::HashSet, time::Instant};
use zeus_eth::amm::uniswap::{quoter::*, router::SwapType};
use zeus_eth::utils::NumericValue;

use crate::core::utils::{RT, eth};
use zeus_eth::{
   alloy_primitives::Address,
   amm::{AnyUniswapPool, UniswapPool},
   currency::{Currency, erc20::ERC20Token, native::NativeCurrency},
};

/// Time in seconds to wait before updating the pool state again
const POOL_STATE_EXPIRY: u64 = 90;

#[derive(Debug, Copy, Clone, PartialEq)]
enum Action {
   Swap,
   WrapETH,
   UnwrapWETH,
}

impl Action {
   pub fn is_wrap(&self) -> bool {
      matches!(self, Self::WrapETH)
   }

   pub fn is_unwrap(&self) -> bool {
      matches!(self, Self::UnwrapWETH)
   }

   pub fn is_swap(&self) -> bool {
      matches!(self, Self::Swap)
   }
}

/// Currency direction
///
/// This is mostly used for the [swap_section()] and in the [TokenSelectionWindow]
/// to identify if the user is selecting the currency to be sold or bought
#[derive(Copy, Clone, PartialEq)]
pub enum InOrOut {
   In,
   Out,
}

impl InOrOut {
   pub fn to_string(&self) -> String {
      (match self {
         Self::In => "Sell",
         Self::Out => "Buy",
      })
      .to_string()
   }
}

pub struct SimulateWindow {
   size: (f32, f32),
   /// Selected pool at its initial state
   pool_initial: Option<AnyUniswapPool>,
   /// Selected pool at its mutated state
   pool_after: Option<AnyUniswapPool>,
}

impl SimulateWindow {
   pub fn new() -> Self {
      Self {
         size: (300.0, 500.0),
         pool_initial: None,
         pool_after: None,
      }
   }

   pub fn set_initial_pool(&mut self, pool: Option<AnyUniswapPool>) {
      self.pool_initial = pool;
      self.pool_after = None;
   }

   pub fn set_pool_after(&mut self, pool: Option<AnyUniswapPool>) {
      self.pool_after = pool;
   }

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, settings: &UniswapSettingsUi, ui: &mut Ui) {
      Window::new("Simulate")
         .id(Id::new("swap_ui_simulate_window"))
         .resizable(true)
         .collapsible(true)
         .movable(true)
         .default_pos((1000.0, 70.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.vertical(|ui| {
               ui.set_width(self.size.0);
               ui.set_height(self.size.1);
               ui.spacing_mut().item_spacing = vec2(0.0, 15.0);

               if self.pool_initial.is_none() {
                  let text = RichText::new("No Pool Selected").size(theme.text_sizes.normal);
                  ui.label(text);
                  return;
               }

               ui.vertical_centered(|ui| {
                  ui.spacing_mut().button_padding = vec2(10.0, 8.0);
                  let text = RichText::new("Reset Pool State").size(theme.text_sizes.normal);
                  let button = Button::new(text);

                  if ui.add(button).clicked() {
                     self.set_pool_after(None);
                     let pool = self.pool_initial.clone();
                     let settings_clone = settings.clone();
                     let ctx_clone = ctx.clone();
                     RT.spawn_blocking(move || {
                        SHARED_GUI.write(|gui| {
                           gui.uniswap.swap_ui.pool = pool;
                           gui.uniswap.swap_ui.get_quote(ctx_clone, &settings_clone);
                        });
                     });
                  }

                  ui.label(RichText::new("Pool Info").size(theme.text_sizes.normal));
               });

               let pool = self.pool_initial.as_ref().unwrap();

               ScrollArea::vertical().show(ui, |ui| {
                  // Pair
                  let token0 = pool.currency0();
                  let token1 = pool.currency1();
                  let pair = format!("{} - {}", token0.symbol(), token1.symbol());
                  let text = RichText::new(pair).size(theme.text_sizes.normal);
                  ui.label(text);

                  // Price
                  let base_price = ctx.get_currency_price(pool.base_currency());
                  let quote_price = pool.quote_price(base_price.f64()).unwrap_or_default();
                  let quote_price = NumericValue::currency_price(quote_price);

                  // Quote USD Price
                  let price = format!(
                     "{} ${}",
                     pool.quote_currency().symbol(),
                     quote_price.formatted(),
                  );
                  let text = RichText::new(price).size(theme.text_sizes.normal);
                  ui.label(text);

                  // Base USD Price
                  let price = format!(
                     "{} ${}",
                     pool.base_currency().symbol(),
                     base_price.formatted(),
                  );
                  let text = RichText::new(price).size(theme.text_sizes.normal);
                  ui.label(text);

                  // Pool balances
                  let (token0_balance, token1_balance) = pool.pool_balances();

                  ui.label(RichText::new("Pool Balances").size(theme.text_sizes.normal));
                  let token0_balance = format!(
                     "{} {}",
                     token0.symbol(),
                     token0_balance.format_abbreviated(),
                  );
                  let text = RichText::new(token0_balance).size(theme.text_sizes.normal);
                  ui.label(text);

                  let token1_balance = format!(
                     "{} {}",
                     token1.symbol(),
                     token1_balance.format_abbreviated(),
                  );
                  let text = RichText::new(token1_balance).size(theme.text_sizes.normal);
                  ui.label(text);

                  if self.pool_after.is_none() {
                     return;
                  }

                  // Pool State after the swaps
                  let pool_after = self.pool_after.as_ref().unwrap();

                  ui.vertical_centered(|ui| {
                     ui.label(
                        RichText::new("Pool State after swaps").size(theme.text_sizes.normal),
                     );
                  });

                  let quote_price = pool_after.quote_price(base_price.f64()).unwrap_or_default();
                  let quote_price = NumericValue::currency_price(quote_price);

                  // Quote USD Price
                  let price = format!(
                     "{} ${}",
                     pool.quote_currency().symbol(),
                     quote_price.formatted(),
                  );
                  let text = RichText::new(price).size(theme.text_sizes.normal);
                  ui.label(text);

                  // TODO: Actually calculate the token balances for V3
                  // Pool balances
                  let (token0_balance, token1_balance) = pool_after.pool_balances();

                  ui.label(RichText::new("Pool Balances").size(theme.text_sizes.normal));
                  let token0_balance = format!(
                     "{} {}",
                     token0.symbol(),
                     token0_balance.format_abbreviated(),
                  );
                  let text = RichText::new(token0_balance).size(theme.text_sizes.normal);
                  ui.label(text);

                  let token1_balance = format!(
                     "{} {}",
                     token1.symbol(),
                     token1_balance.format_abbreviated(),
                  );
                  let text = RichText::new(token1_balance).size(theme.text_sizes.normal);
                  ui.label(text);
               });
            });
         });
   }
}

/// A Swap UI for a DEX like Uniswap
pub struct SwapUi {
   pub open: bool,
   pub size: (f32, f32),
   pub currency_in: Currency,
   pub currency_out: Currency,
   pub amount_in: String,
   pub amount_out: String,
   /// Last time pool state was updated
   pub last_pool_state_updated: Option<Instant>,
   /// Last time quote was updated
   pub last_quote_updated: Option<Instant>,
   pub pool_data_syncing: bool,
   pub syncing_pools: bool,
   pub getting_quote: bool,
   pub quote: Quote,
   pub protocol_version: ProtocolVersion,

   /// Pool to simulate if simulate mode is on
   pub pool: Option<AnyUniswapPool>,
   pub simulate_window: SimulateWindow,
}

impl SwapUi {
   pub fn new() -> Self {
      let currency = NativeCurrency::from(1);
      let currency_in = Currency::from(currency);
      let currency_out = Currency::from(ERC20Token::wrapped_native_token(1));
      Self {
         open: true,
         size: (450.0, 700.0),
         currency_in,
         currency_out,
         amount_in: "".to_string(),
         amount_out: "".to_string(),
         last_pool_state_updated: None,
         last_quote_updated: None,
         pool_data_syncing: false,
         syncing_pools: false,
         getting_quote: false,
         quote: Quote::default(),
         protocol_version: ProtocolVersion::V3,
         pool: None,
         simulate_window: SimulateWindow::new(),
      }
   }

   /// Replace the currency_in or currency_out based on the direction
   pub fn replace_currency(&mut self, in_or_out: &InOrOut, currency: Currency) {
      match in_or_out {
         InOrOut::In => {
            self.currency_in = currency;
         }
         InOrOut::Out => {
            self.currency_out = currency;
         }
      }
   }

   /// Give a default input currency based on the selected chain id
   pub fn default_currency_in(&mut self, id: u64) {
      let native = NativeCurrency::from(id);
      self.currency_in = Currency::from(native);
   }

   /// Give a default output currency based on the selected chain id
   pub fn default_currency_out(&mut self, id: u64) {
      self.currency_out = Currency::from(ERC20Token::wrapped_native_token(id));
   }

   fn swap_currencies(&mut self) {
      std::mem::swap(&mut self.currency_in, &mut self.currency_out);
      std::mem::swap(&mut self.amount_in, &mut self.amount_out);
   }

   fn select_version(&mut self, theme: &Theme, ui: &mut Ui) {
      let mut current_version = self.protocol_version;
      let versions = ProtocolVersion::all();

      let selected_text = RichText::new(current_version.to_str()).size(theme.text_sizes.normal);

      ComboBox::from_id_salt("protocol_version")
         .selected_text(selected_text)
         .show_ui(ui, |ui| {
            for version in versions {
               let text = RichText::new(version.to_str()).size(theme.text_sizes.normal);
               ui.selectable_value(&mut current_version, version, text);
            }
            self.protocol_version = current_version;
         });
   }

   /// Select the fee tier
   ///
   /// Returns if the fee tier was changed
   fn select_fee_tier(&mut self, theme: &Theme, pools: &Vec<AnyUniswapPool>, ui: &mut Ui) -> bool {
      if pools.is_empty() {
         return false;
      }

      if self.pool_data_syncing || self.syncing_pools {
         return false;
      }

      let mut changed = false;
      ui.horizontal(|ui| {
         ui.label(RichText::new("Fee Tier").size(theme.text_sizes.normal));
         ui.add_space(10.0);
         Grid::new("swap_ui_fee_tier_select")
            .spacing(vec2(15.0, 0.0))
            .show(ui, |ui| {
               for pool in pools {
                  let selected = self.pool.as_ref() == Some(pool);

                  let fee = pool.fee().fee_percent();
                  let text = RichText::new(format!("{fee}%")).size(theme.text_sizes.normal);
                  let mut button = Button::new(text);

                  if !selected {
                     button = button.fill(Color32::TRANSPARENT);
                  }

                  if ui.add(button).clicked() {
                     self.pool = Some(pool.clone());
                     changed = true;
                  }
               }

               ui.end_row();
            });
      });
      changed
   }

   pub fn show(
      &mut self,
      ctx: ZeusCtx,
      theme: &Theme,
      icons: Arc<Icons>,
      token_selection: &mut TokenSelectionWindow,
      settings: &UniswapSettingsUi,
      ui: &mut Ui,
   ) {
      if !self.open {
         return;
      }

      if self.should_update_pool_state() {
         self.update_pool_state(
            ctx.clone(),
            settings.swap_on_v2,
            settings.swap_on_v3,
            settings.swap_on_v4,
         );
      }

      let chain_id = ctx.chain().id();
      let owner = ctx.current_wallet().address;
      let currencies = ctx.get_currencies(chain_id);
      let simulate_mode = settings.simulate_mode;

      if simulate_mode {
         self.simulate_window.show(ctx.clone(), theme, settings, ui);
      }

      ui.vertical_centered(|ui| {
         ui.set_width(self.size.0);
         ui.set_height(self.size.1);
         ui.spacing_mut().item_spacing = vec2(0.0, 15.0);

         if simulate_mode {
            let text = RichText::new("You are on Simulate Mode")
               .size(theme.text_sizes.large)
               .strong();
            ui.label(text);
         }

         ui.horizontal(|ui| {
            if simulate_mode {
               ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                  self.select_version(theme, ui);
               });
            }

            // Force update pool state
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
               ui.spacing_mut().item_spacing.x = 10.0;
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);

               let refresh = Button::new(RichText::new("⟲").size(theme.text_sizes.normal));
               if ui.add(refresh).clicked() {
                  self.update_pool_state(
                     ctx.clone(),
                     settings.swap_on_v2,
                     settings.swap_on_v3,
                     settings.swap_on_v4,
                  );
                  self.sync_pools(ctx.clone(), settings, true);
               }

               if self.pool_data_syncing || self.syncing_pools {
                  ui.add(Spinner::new().size(17.0).color(Color32::WHITE));
               }
            });
         });

         if simulate_mode {
            let manager = ctx.pool_manager();
            let mut pools = manager.get_pools_from_pair(&self.currency_in, &self.currency_out);

            if self.protocol_version.is_v2() {
               pools.retain(|p| p.dex_kind().is_v2());
            }

            if self.protocol_version.is_v3() {
               pools.retain(|p| p.dex_kind().is_v3());
            }

            // sort pool by the lowest to highest fee
            pools.sort_by(|a, b| a.fee().fee().cmp(&b.fee().fee()));

            let changed = self.select_fee_tier(theme, &pools, ui);

            if changed {
               self.simulate_window.set_initial_pool(self.pool.clone());
               self.get_quote(ctx.clone(), settings);
            }

            if pools.is_empty() {
               ui.label(RichText::new("No pools found").size(theme.text_sizes.normal));
            }
         }

         // TODO: Show the correct usd values if we are in simulate mode

         // Sell
         let amount_changed = swap_section(
            ui,
            ctx.clone(),
            theme,
            icons.clone(),
            InOrOut::In,
            &self.currency_in,
            &mut self.amount_in,
            chain_id,
            owner,
            token_selection,
         );

         // Swap Currencies
         ui.add_space(5.0);
         ui.vertical_centered(|ui| {
            let swap_button = Button::image(icons.swap()).min_size(vec2(40.0, 40.0));

            if ui.add(swap_button).clicked() {
               self.swap_currencies();
               self.get_quote(ctx.clone(), settings);
            }
         });
         ui.add_space(5.0);

         // Buy
         swap_section(
            ui,
            ctx.clone(),
            theme,
            icons.clone(),
            InOrOut::Out,
            &self.currency_out,
            &mut self.amount_out,
            chain_id,
            owner,
            token_selection,
         );

         token_selection.show(
            ctx.clone(),
            theme,
            icons,
            chain_id,
            owner,
            &currencies,
            ui,
         );

         let selected_currency = token_selection.get_currency().cloned();
         let direction = token_selection.get_currency_direction();
         let changed_currency = selected_currency.is_some();
         let should_get_quote = self.should_get_quote(changed_currency, amount_changed);

         if let Some(currency) = selected_currency {
            self.replace_currency(&direction, currency.clone());
            token_selection.reset();
         }

         self.sync_pools(ctx.clone(), settings, changed_currency);

         if should_get_quote {
            self.get_quote(ctx.clone(), settings);
         }

         if simulate_mode {
            self.simulate_button(ctx.clone(), theme, settings, ui);
         }

         if !simulate_mode {
            self.swap_button(ctx.clone(), theme, settings, ui);
            self.swap_details(ctx, theme, settings, ui);
         }
      });
   }

   fn action(&self) -> Action {
      let should_wrap = self.currency_in.is_native() && self.currency_out.is_native_wrapped();
      let should_unwrap = self.currency_in.is_native_wrapped() && self.currency_out.is_native();

      if should_wrap {
         return Action::WrapETH;
      } else if should_unwrap {
         return Action::UnwrapWETH;
      } else {
         return Action::Swap;
      }
   }

   fn simulate_button(
      &mut self,
      ctx: ZeusCtx,
      theme: &Theme,
      settings: &UniswapSettingsUi,
      ui: &mut Ui,
   ) {
      let got_pool = self.pool.is_some();
      let enabled = !self.amount_in.is_empty() && got_pool;
      let button = Button::new(RichText::new("Simulate").size(theme.text_sizes.large))
         .min_size(vec2(ui.available_width() * 0.8, 45.0));

      if ui.add_enabled(enabled, button).clicked() {
         if let Some(pool) = &mut self.pool {
            let amount_in =
               NumericValue::parse_to_wei(&self.amount_in, self.currency_in.decimals());
            pool
               .simulate_swap_mut(&self.currency_in, amount_in.wei2())
               .unwrap_or_default();
            self.simulate_window.set_pool_after(Some(pool.clone()));

            self.get_quote(ctx.clone(), settings);
         }
      }
   }

   fn swap_button(
      &mut self,
      ctx: ZeusCtx,
      theme: &Theme,
      settings: &UniswapSettingsUi,
      ui: &mut Ui,
   ) {
      let valid = self.valid_inputs(ctx.clone());
      let has_routes = self.quote.route.is_some();
      let has_balance = self.sufficient_balance(ctx.clone());
      let has_entered_amount = !self.amount_in.is_empty();
      let action = self.action();

      let mut button_text = "Swap".to_string();

      if !has_entered_amount {
         button_text = "Enter Amount".to_string();
      }

      if valid && action.is_wrap() {
         button_text = format!("Wrap {}", self.currency_in.symbol());
      }

      if valid && action.is_unwrap() {
         button_text = format!("Unwrap {}", self.currency_in.symbol());
      }

      if valid && action.is_swap() && !has_routes {
         button_text = format!("No Routes Found");
      }

      if !has_balance {
         button_text = format!(
            "Insufficient {} Balance",
            self.currency_in.symbol()
         );
      }

      let swap_button = Button::new(
         RichText::new(button_text)
            .size(theme.text_sizes.large)
            .color(theme.colors.text_color),
      )
      .min_size(vec2(ui.available_width() * 0.8, 45.0));

      ui.vertical_centered(|ui| {
         if ui.add_enabled(valid, swap_button).clicked() {
            self.swap(ctx, settings);
         }
      });
   }

   fn valid_inputs(&self, ctx: ZeusCtx) -> bool {
      self.valid_amounts() && self.sufficient_balance(ctx)
   }

   fn valid_amounts(&self) -> bool {
      let amount_in = self.amount_in.parse().unwrap_or(0.0);
      let amount_out = self.amount_out.parse().unwrap_or(0.0);
      amount_in > 0.0 && amount_out > 0.0
   }

   fn sufficient_balance(&self, ctx: ZeusCtx) -> bool {
      let sender = ctx.current_wallet().address;
      let balance = ctx.get_currency_balance(ctx.chain().id(), sender, &self.currency_in);
      let amount = NumericValue::parse_to_wei(&self.amount_in, self.currency_in.decimals());
      balance.wei2() >= amount.wei2()
   }

   /// Sync pools for the first time for currency out
   fn sync_pools(&mut self, ctx: ZeusCtx, settings: &UniswapSettingsUi, changed_currency: bool) {
      if self.syncing_pools {
         return;
      }

      if !changed_currency {
         return;
      }

      // ETH -> WETH
      if self.currency_in.is_native() && self.currency_out.is_native_wrapped() {
         return;
      }

      let token_in = self.currency_in.to_erc20().into_owned();
      let token_out = self.currency_out.to_erc20().into_owned();
      tracing::info!(
         "Syncing pools for: {} -> {}",
         token_in.symbol,
         token_out.symbol
      );

      let chain_id = ctx.chain().id();
      let manager = ctx.pool_manager();
      let currency_out = self.currency_out.clone();
      tracing::info!("Token to sync pools for: {}", token_out.symbol);

      let swap_on_v2 = settings.swap_on_v2;
      let swap_on_v3 = settings.swap_on_v3;
      let swap_on_v4 = settings.swap_on_v4;

      self.syncing_pools = true;

      let ctx_clone = ctx.clone();
      RT.spawn(async move {
         let _ = eth::sync_pools_for_tokens(
            ctx_clone.clone(),
            chain_id,
            vec![token_out.clone()],
            false,
         )
         .await;

         SHARED_GUI.write(|gui| {
            gui.uniswap.swap_ui.syncing_pools = false;
            gui.uniswap.swap_ui.pool_data_syncing = true;
         });

         let pools = get_relevant_pools(
            ctx_clone.clone(),
            swap_on_v2,
            swap_on_v3,
            swap_on_v4,
            &currency_out,
         );

         match manager
            .update_state_for_pools(ctx_clone.clone(), chain_id, pools)
            .await
         {
            Ok(_) => {
               // tracing::info!("Updated pool state for token: {}", token.symbol);
               SHARED_GUI.write(|gui| {
                  gui.uniswap.swap_ui.last_pool_state_updated = Some(Instant::now());
                  gui.uniswap.swap_ui.pool_data_syncing = false;
               });
            }
            Err(_e) => {
               // tracing::error!("Error updating pool state: {:?}", e);
               SHARED_GUI.write(|gui| {
                  gui.uniswap.swap_ui.pool_data_syncing = false;
               });
            }
         }

         RT.spawn_blocking(move || {
            SHARED_GUI.write(|gui| {
               let settings = &gui.uniswap.settings;
               gui.uniswap.swap_ui.get_quote(ctx_clone.clone(), &settings);
            });

            match ctx_clone.save_pool_manager() {
               Ok(_) => {
                  tracing::info!("Pool Manager saved");
               }
               Err(e) => {
                  tracing::error!("Error saving pool manager: {:?}", e);
               }
            }
         });
      });
   }

   fn should_update_pool_state(&self) -> bool {
      if self.pool_data_syncing || self.syncing_pools {
         return false;
      }

      // ETH -> WETH
      if self.currency_in.is_native() && self.currency_out.is_native_wrapped() {
         return false;
      }

      // WETH -> WETH
      if self.currency_in.is_native_wrapped() && self.currency_out.is_native_wrapped() {
         return false;
      }

      if self.currency_in == self.currency_out {
         return false;
      }

      self.pool_state_expired()
   }

   fn pool_state_expired(&self) -> bool {
      let now = Instant::now();
      if let Some(last_updated) = self.last_pool_state_updated {
         let elapsed = now.duration_since(last_updated).as_secs();
         if elapsed < POOL_STATE_EXPIRY {
            return false;
         }
      }
      true
   }

   pub fn update_pool_state(
      &mut self,
      ctx: ZeusCtx,
      update_v2: bool,
      update_v3: bool,
      update_v4: bool,
   ) {
      let action = self.action();
      if action.is_wrap() || action.is_unwrap() {
         return;
      }

      let pools = get_relevant_pools(
         ctx.clone(),
         update_v2,
         update_v3,
         update_v4,
         &self.currency_out,
      );

      if pools.is_empty() {
         tracing::warn!(
            "Can't get quote, No pools found for {}-{}",
            self.currency_in.symbol(),
            self.currency_out.symbol()
         );
      }

      tracing::info!(
         "Updating pool state for {}-{}",
         self.currency_in.symbol(),
         self.currency_out.symbol()
      );

      let chain_id = ctx.chain().id();
      let manager = ctx.pool_manager();

      self.pool_data_syncing = true;
      let ctx_clone = ctx.clone();
      RT.spawn(async move {
         match manager
            .update_state_for_pools(ctx_clone.clone(), chain_id, pools)
            .await
         {
            Ok(_) => {
               SHARED_GUI.write(|gui| {
                  gui.uniswap.swap_ui.last_pool_state_updated = Some(Instant::now());
                  gui.uniswap.swap_ui.pool_data_syncing = false;
               });
            }
            Err(e) => {
               tracing::error!("Error updating pool state: {:?}", e);
               SHARED_GUI.write(|gui| {
                  gui.uniswap.swap_ui.pool_data_syncing = false;
               });
            }
         }

         // get a new quote
         SHARED_GUI.write(|gui| {
            let settings = &gui.uniswap.settings;
            gui.uniswap.swap_ui.get_quote(ctx_clone, &settings);
         });
      });
   }

   fn should_get_quote(&self, changed_currency: bool, changed_amount: bool) -> bool {
      if changed_amount || changed_currency {
         return true;
      } else {
         return false;
      }
   }

   pub fn get_quote(&mut self, ctx: ZeusCtx, settings: &UniswapSettingsUi) {
      if settings.simulate_mode {
         if let Some(pool) = &self.pool {
            let amount_in =
               NumericValue::parse_to_wei(&self.amount_in, self.currency_in.decimals());
            let amount_out = pool
               .simulate_swap(&self.currency_in, amount_in.wei2())
               .unwrap_or_default();
            let amount = NumericValue::format_wei(amount_out, self.currency_out.decimals());
            self.amount_out = amount.flatten();
            return;
         }
      }

      let action = self.action();
      if action == Action::WrapETH || action == Action::UnwrapWETH {
         self.amount_out = self.amount_in.clone();
         return;
      }

      let amount_in = NumericValue::parse_to_wei(&self.amount_in, self.currency_in.decimals());

      if amount_in.is_zero() {
         self.amount_out = String::new();
         return;
      }

      let currency_in = self.currency_in.clone();
      let currency_out = self.currency_out.clone();
      let chain = ctx.chain().id();

      self.getting_quote = true;
      let base_fee = ctx.get_base_fee(chain).unwrap_or_default().next;
      let priority_fee = ctx.get_priority_fee(chain).unwrap_or_default();
      let eth_price = ctx.get_token_price(&ERC20Token::wrapped_native_token(chain));
      let currency_out_price = ctx.get_currency_price(&currency_out);

      let max_hops = settings.max_hops;
      let split_routing_enabled = settings.split_routing_enabled;
      let max_split_routes = settings.max_split_routes;
      let swap_on_v2 = settings.swap_on_v2;
      let swap_on_v3 = settings.swap_on_v3;
      let swap_on_v4 = settings.swap_on_v4;

      RT.spawn_blocking(move || {
         let pools = get_relevant_pools(
            ctx.clone(),
            swap_on_v2,
            swap_on_v3,
            swap_on_v4,
            &currency_out,
         );

         let quote = if split_routing_enabled {
            get_quote_with_split_routing(
               amount_in.clone(),
               currency_in.clone(),
               currency_out.clone(),
               pools,
               eth_price,
               currency_out_price,
               base_fee,
               priority_fee.wei2(),
               max_hops,
               max_split_routes,
            )
         } else {
            get_quote(
               amount_in.clone(),
               currency_in.clone(),
               currency_out.clone(),
               pools,
               eth_price,
               currency_out_price,
               base_fee,
               priority_fee.wei2(),
               max_hops,
            )
         };

         tracing::info!(
            "Quote for {} {} -> {}",
            amount_in.format_abbreviated(),
            currency_in.symbol(),
            currency_out.symbol()
         );

         let swap_steps = quote.swap_steps.clone();
         let amount_out = quote.amount_out.clone();

         tracing::info!("Swap Steps Length: {}", swap_steps.len());

         for swap in &swap_steps {
            tracing::info!(
               "Swap Step: {} {} -> {} {} {} ({})",
               swap.amount_in.format_abbreviated(),
               swap.currency_in.symbol(),
               swap.amount_out.format_abbreviated(),
               swap.currency_out.symbol(),
               swap.pool.dex_kind().to_str(),
               swap.pool.fee().fee()
            );
         }

         SHARED_GUI.write(|gui| {
            if !quote.amount_out.is_zero() {
               gui.uniswap.swap_ui.amount_out = amount_out.flatten();

               gui.uniswap.swap_ui.getting_quote = false;
               gui.uniswap.swap_ui.quote = quote;
            } else {
               gui.uniswap.swap_ui.quote = Quote::default();
               gui.uniswap.swap_ui.amount_out = String::new();
               gui.uniswap.swap_ui.getting_quote = false;
            }
         });
      });
   }

   fn swap_details(&self, _ctx: ZeusCtx, theme: &Theme, settings: &UniswapSettingsUi, ui: &mut Ui) {
      // Slippage
      ui.horizontal(|ui| {
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.label(RichText::new("Slippage").size(theme.text_sizes.normal));
         });

         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            ui.label(
               RichText::new(format!("{}%", settings.slippage)).size(theme.text_sizes.normal),
            );
         });
      });

      // Minimum Received
      ui.horizontal(|ui| {
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.label(RichText::new("Minimum Received").size(theme.text_sizes.normal));
         });

         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            let mut amount_out = self.quote.amount_out.clone();
            let slippage: f64 = settings.slippage.parse().unwrap_or(0.5);
            amount_out.calc_slippage(slippage, self.currency_out.decimals());

            if self.valid_amounts() && self.action().is_swap() {
               ui.label(
                  RichText::new(format!(
                     "{} {}",
                     amount_out.formatted(),
                     self.currency_out.symbol()
                  ))
                  .size(theme.text_sizes.normal),
               );
            }
         });
      });
   }

   fn swap(&self, ctx: ZeusCtx, settings: &UniswapSettingsUi) {
      let action = self.action();
      let from = ctx.current_wallet().address;
      let chain = ctx.chain();

      if action.is_wrap() {
         let amount_in = self.amount_in.clone();
         let amount_in = NumericValue::parse_to_wei(&amount_in, self.currency_in.decimals());
         RT.spawn(async move {
            SHARED_GUI.write(|gui| {
               gui.loading_window.open("Wait while magic happens");
               gui.request_repaint();
            });

            match eth::wrap_eth(ctx.clone(), from, chain, amount_in.clone()).await {
               Ok(_) => {}
               Err(e) => {
                  SHARED_GUI.write(|gui| {
                     gui.progress_window.reset();
                     gui.loading_window.reset();
                     gui.msg_window.open("Transaction Error", e.to_string());
                     gui.request_repaint();
                  });
               }
            }
         });

         return;
      }

      if action.is_unwrap() {
         let amount_in = self.amount_in.clone();
         let amount_in = NumericValue::parse_to_wei(&amount_in, self.currency_in.decimals());
         RT.spawn(async move {
            SHARED_GUI.write(|gui| {
               gui.loading_window.open("Wait while magic happens");
               gui.request_repaint();
            });

            match eth::unwrap_weth(ctx.clone(), from, chain, amount_in.clone()).await {
               Ok(_) => {}
               Err(e) => {
                  SHARED_GUI.write(|gui| {
                     gui.progress_window.reset();
                     gui.loading_window.reset();
                     gui.msg_window.open("Transaction Error", e.to_string());
                     gui.request_repaint();
                  });
               }
            }
         });

         return;
      }

      let currency_in = self.quote.currency_in.clone();
      let currency_out = self.quote.currency_out.clone();
      let amount_in = self.quote.amount_in.clone();
      let amount_out = self.quote.amount_out.clone();
      let swap_steps = self.quote.swap_steps.clone();

      let mev_protect = settings.mev_protect;
      let slippage: f64 = settings.slippage.parse().unwrap_or(0.5);

      RT.spawn(async move {
         SHARED_GUI.open_loading("Wait while magic happens");
         SHARED_GUI.request_repaint();

         match eth::swap(
            ctx.clone(),
            chain,
            slippage,
            mev_protect,
            from,
            SwapType::ExactInput,
            amount_in,
            amount_out,
            currency_in,
            currency_out,
            swap_steps,
         )
         .await
         {
            Ok(_) => {
               tracing::info!("Transaction Sent");
            }
            Err(e) => {
               tracing::error!("Transaction Error: {:?}", e);
               SHARED_GUI.write(|gui| {
                  gui.progress_window.reset();
                  gui.loading_window.reset();
                  gui.tx_confirmation_window.reset();
                  gui.msg_window.open("Transaction Error", &e.to_string());
                  gui.request_repaint();
               });
            }
         }
      });
   }
}

/// Which pools to update the state for
pub fn get_relevant_pools(
   ctx: ZeusCtx,
   swap_on_v2: bool,
   swap_on_v3: bool,
   swap_on_v4: bool,
   currency_out: &Currency,
) -> Vec<AnyUniswapPool> {
   let manager = ctx.pool_manager();
   let all_pools = manager.get_pools_for_chain(currency_out.chain_id());
   let mut added_pools = HashSet::new();

   let mut good_pools = Vec::new();
   for pool in all_pools {
      if !swap_on_v2 && pool.dex_kind().is_v2() {
         continue;
      }

      if !swap_on_v3 && pool.dex_kind().is_v3() {
         continue;
      }

      if !swap_on_v4 && pool.dex_kind().is_v4() {
         continue;
      }

      let pool_key = (pool.chain_id(), pool.address(), pool.pool_id());

      if added_pools.contains(&pool_key) {
         continue;
      }

      // If the output currency is base, we only want to push pools that are paired with base tokens
      // This is to avoid pools that are paired with shit tokens
      let should_push = if currency_out.is_base() {
         pool.have(&currency_out) && pool.currency0().is_base() && pool.currency1().is_base()
      } else {
         pool.have(&currency_out)
      };

      // let base_pool = pool.currency0().is_base() && pool.currency1().is_base();

      if should_push {
         good_pools.push(pool);
         added_pools.insert(pool_key);
      }
   }

   good_pools
}

/// Helper function to draw one section of the swap UI
///
/// It draws the amount field, the currency selector button and the balance and max button
///
/// Returns if the amount field was changed
pub fn swap_section(
   ui: &mut Ui,
   ctx: ZeusCtx,
   theme: &Theme,
   icons: Arc<Icons>,
   direction: InOrOut,
   currency: &Currency,
   amount: &mut String,
   chain_id: u64,
   owner: Address,
   token_selection: &mut TokenSelectionWindow,
) -> bool {
   let frame = theme.frame1;

   let mut amount_changed = false;

   frame.show(ui, |ui| {
      ui.vertical(|ui| {
         ui.spacing_mut().item_spacing = vec2(0.0, 8.0);
         ui.horizontal(|ui| {
            ui.label(
               RichText::new(direction.to_string())
                  .size(theme.text_sizes.large)
                  .color(theme.colors.text_secondary),
            );
         });

         // Amount input
         ui.horizontal(|ui| {
            let amount_input = TextEdit::singleline(amount)
               .font(FontId::proportional(theme.text_sizes.heading))
               .hint_text(RichText::new("0").color(theme.colors.text_secondary))
               .background_color(theme.colors.text_edit_bg)
               .margin(Margin::same(10))
               .desired_width(ui.available_width() * 0.6)
               .min_size(vec2(0.0, 50.0));

            let res = ui.add(amount_input);
            if res.changed() {
               amount_changed = true;
            }

            // Currency Selector Button
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
               let icon = icons.currency_icon(currency);
               let button_text = RichText::new(currency.symbol()).size(theme.text_sizes.normal);

               let button = Button::image_and_text(icon, button_text).min_size(vec2(100.0, 40.0));

               if ui.add(button).clicked() {
                  token_selection.currency_direction = direction;
                  token_selection.open = true;
               }
            });
         });

         // USD Value
         ui.horizontal(|ui| {
            let amount = amount.parse().unwrap_or(0.0);
            let usd_value = ctx.get_currency_value_for_amount(amount, currency);
            ui.label(
               RichText::new(format!("${}", usd_value.formatted())).size(theme.text_sizes.normal),
            );
         });

         // Balance and Max Button
         ui.horizontal(|ui| {
            ui.spacing_mut().button_padding = vec2(10.0, 4.0);

            let balance = ctx.get_currency_balance(chain_id, owner, currency);
            let balance_text = format!("Balance: {}", balance.format_abbreviated());
            ui.label(
               RichText::new(balance_text)
                  .size(theme.text_sizes.normal)
                  .color(theme.colors.text_secondary),
            );

            ui.add_space(5.0);

            // Max button
            let max_text = RichText::new("Max").size(theme.text_sizes.small);
            let max_button = Button::new(max_text);

            if direction == InOrOut::In {
               if ui.add(max_button).clicked() {
                  *amount = balance.flatten();
                  amount_changed = true;
               }
            }
         });
      });
   });
   amount_changed
}
