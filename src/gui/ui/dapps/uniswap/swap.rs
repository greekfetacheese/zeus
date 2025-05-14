use crate::core::ZeusCtx;
use crate::gui::ui::*;
use crate::{assets::icons::Icons, gui::SHARED_GUI};
use egui::{
   Align, Align2, Button, Color32, FontId, Frame, Layout, Margin, Order, RichText, Spinner,
   TextEdit, Ui, Window, vec2,
};
use egui_theme::Theme;
use std::sync::Arc;
use std::time::Instant;
use zeus_eth::amm::{
   DexKind,
   uniswap::{quoter::*, router::SwapType},
};
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

pub struct SwapUi {
   pub open: bool,
   pub settings_open: bool,
   pub currency_in: Currency,
   pub currency_out: Currency,
   pub amount_in: String,
   pub amount_out: String,
   pub mev_protect: bool,
   /// Percent
   pub slippage: String,
   /// Last time pool state was updated
   pub last_pool_state_updated: Option<Instant>,
   /// Last time quote was updated
   pub last_quote_updated: Option<Instant>,
   pub pool_data_syncing: bool,
   pub syncing_pools: bool,
   pub getting_quote: bool,
   pub quote_routes: QuoteRoutes,
}

impl SwapUi {
   pub fn new() -> Self {
      let currency = NativeCurrency::from(1);
      let currency_in = Currency::from(currency);
      let currency_out = Currency::from(ERC20Token::wrapped_native_token(1));
      Self {
         open: false,
         settings_open: false,
         currency_in,
         currency_out,
         amount_in: "".to_string(),
         amount_out: "".to_string(),
         mev_protect: true,
         slippage: "0.5".to_string(),
         last_pool_state_updated: None,
         last_quote_updated: None,
         pool_data_syncing: false,
         syncing_pools: false,
         getting_quote: false,
         quote_routes: QuoteRoutes::default(),
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

   pub fn show(
      &mut self,
      ctx: ZeusCtx,
      icons: Arc<Icons>,
      theme: &Theme,
      token_selection: &mut TokenSelectionWindow,
      ui: &mut Ui,
   ) {
      if !self.open {
         return;
      }

      if self.should_update_pool_state() {
         self.update_pool_state(ctx.clone());
      }

      let chain_id = ctx.chain().id();
      let owner = ctx.current_wallet().address;
      let currencies = ctx.get_currencies(chain_id);

      let mut open = self.open;
      let window_frame = Frame::new().inner_margin(Margin::symmetric(15, 20));

      Window::new("Swap")
         .open(&mut open)
         .title_bar(false)
         .resizable(false)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(window_frame)
         .show(ui.ctx(), |ui| {
            ui.vertical(|ui| {
               ui.set_width(450.0);
               ui.spacing_mut().item_spacing = vec2(0.0, 15.0);

               ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                  ui.spacing_mut().item_spacing.x = 10.0;
                  ui.spacing_mut().button_padding = vec2(10.0, 8.0);

                  // Force update pool state
                  let refresh = Button::new(RichText::new("Refresh").size(theme.text_sizes.normal));
                  if ui.add(refresh).clicked() {
                     self.update_pool_state(ctx.clone());
                  }

                  let button = Button::new(RichText::new("Settings").size(theme.text_sizes.normal));
                  if ui.add(button).clicked() {
                     self.settings_open = true;
                  }

                  if self.pool_data_syncing || self.syncing_pools {
                     ui.add(Spinner::new().size(17.0).color(Color32::WHITE));
                  }
               });

               self.settings(theme, ui);

               // --- Sell Section ---
               let amount_changed = self.swap_section(
                  ui,
                  ctx.clone(),
                  theme,
                  icons.clone(),
                  token_selection,
                  InOrOut::In,
                  chain_id,
                  owner,
               );

               // Swap Currencies
               ui.add_space(5.0);
               ui.vertical_centered(|ui| {
                  let swap_button =
                     Button::new(RichText::new("🡫").size(theme.text_sizes.large).strong())
                        .min_size(vec2(40.0, 40.0));

                  if ui.add(swap_button).clicked() {
                     self.swap_currencies();
                     self.get_quote(ctx.clone());
                  }
               });
               ui.add_space(5.0);

               // Buy Section
               self.swap_section(
                  ui,
                  ctx.clone(),
                  theme,
                  icons.clone(),
                  token_selection,
                  InOrOut::Out,
                  chain_id,
                  owner,
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

               self.sync_pools(ctx.clone(), changed_currency);

               if should_get_quote {
                  self.get_quote(ctx.clone());
               }

               self.swap_button(ctx.clone(), theme, ui);
               self.swap_details(ctx, theme, ui);
            });
         });

      self.open = open;
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

   fn swap_button(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      let valid = self.valid_inputs(ctx.clone());
      let has_routes = !self.quote_routes.routes.is_empty();
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
            self.swap(ctx);
         }
      });
   }

   /// Helper function to draw one section (Sell or Buy) of the swap UI
   ///
   /// Returns if the amount field was changed
   fn swap_section(
      &mut self,
      ui: &mut Ui,
      ctx: ZeusCtx,
      theme: &Theme,
      icons: Arc<Icons>,
      token_selection: &mut TokenSelectionWindow,
      direction: InOrOut,
      chain_id: u64,
      owner: Address,
   ) -> bool {
      let frame = theme.frame1;
      let _frame_bg_color = frame.fill;

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

            let mut amount = match direction {
               InOrOut::In => self.amount_in.clone(),
               InOrOut::Out => self.amount_out.clone(),
            };

            let currency = match direction {
               InOrOut::In => &self.currency_in.clone(),
               InOrOut::Out => &self.currency_out.clone(),
            };

            // Amount input
            ui.horizontal(|ui| {
               let amount_input = TextEdit::singleline(&mut amount)
                  .font(FontId::proportional(theme.text_sizes.heading))
                  .hint_text(RichText::new("0").color(theme.colors.text_secondary))
                  .background_color(theme.colors.text_edit_bg2)
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

                  let button =
                     Button::image_and_text(icon, button_text).min_size(vec2(100.0, 40.0));

                  if ui.add(button).clicked() {
                     token_selection.currency_direction = direction.clone();
                     token_selection.open = true;
                  }
               });
            });

            match direction {
               InOrOut::In => self.amount_in = amount.clone(),
               InOrOut::Out => self.amount_out = amount.clone(),
            }

            // USD Value
            ui.horizontal(|ui| {
               let amount = amount.parse().unwrap_or(0.0);
               let usd_value = ctx.get_currency_value2(amount, currency);
               ui.label(
                  RichText::new(format!("${}", usd_value.formatted()))
                     .size(theme.text_sizes.normal),
               );
            });

            // Balance and Max Button
            ui.horizontal(|ui| {
               let balance = ctx.get_currency_balance(chain_id, owner, currency);
               let balance_text = format!("Balance: {}", balance.format_abbreviated());
               ui.label(
                  RichText::new(balance_text)
                     .size(theme.text_sizes.normal)
                     .color(theme.colors.text_secondary),
               );

               // Max button
               let max_text = RichText::new("Max").size(theme.text_sizes.small);
               let max_button = Button::new(max_text).min_size(vec2(40.0, 20.0));

               if direction == InOrOut::In {
                  if ui.add(max_button).clicked() {
                     self.amount_in = balance.flatten();
                     self.get_quote(ctx);
                  }
               }
            });
         });
      });
      amount_changed
   }

   fn settings(&mut self, theme: &Theme, ui: &mut Ui) {
      if !self.settings_open {
         return;
      }

      let frame = Frame::window(ui.style()).inner_margin(Margin::same(10));
      Window::new("swap_settings")
         .title_bar(false)
         .resizable(false)
         .order(Order::Foreground)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(frame)
         .show(ui.ctx(), |ui| {
            ui.set_width(150.0);
            ui.set_height(100.0);
            ui.spacing_mut().item_spacing = vec2(0.0, 15.0);
            ui.spacing_mut().button_padding = vec2(10.0, 8.0);
            ui.vertical(|ui| {

               let text = RichText::new("MEV Protect").size(theme.text_sizes.normal);
               ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                  ui.label(text).on_hover_text("Protect against front-running");
                  ui.add_space(10.0);
                  ui.checkbox(&mut self.mev_protect, "");
               });

               ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                  let text = RichText::new("Slippage").size(theme.text_sizes.normal);
                  ui.label(text).on_hover_text("Your transaction will revert if the price changes unfavorably by more than this percentage");
                  ui.add_space(10.0);
               TextEdit::singleline(&mut self.slippage)
                  .hint_text("0.5")
                  .font(FontId::proportional(theme.text_sizes.small))
                  .desired_width(25.0)
                  .background_color(theme.colors.text_edit_bg)
                  .margin(Margin::same(10))
                  .show(ui);
            });

            ui.vertical_centered(|ui| {
            let btn = Button::new(RichText::new("Close").size(theme.text_sizes.normal));
            if ui.add(btn).clicked() {
               self.settings_open = false;
            }
            });

         });


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

   fn _valid_slippage(&self) -> bool {
      let slippage = self.slippage.parse().unwrap_or(0.0);
      slippage > 0.0 && slippage < 100.0
   }

   fn sufficient_balance(&self, ctx: ZeusCtx) -> bool {
      let sender = ctx.current_wallet().address;
      let balance = ctx.get_currency_balance(ctx.chain().id(), sender, &self.currency_in);
      let amount = NumericValue::parse_to_wei(&self.amount_in, self.currency_in.decimals());
      balance.wei2() >= amount.wei2()
   }

   /// Sync pools for the first time for currency out
   fn sync_pools(&mut self, ctx: ZeusCtx, changed_currency: bool) {
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
      let currency_to_update_pools_from = self.currency_to_update_pools_from().clone();

      let dexes = DexKind::main_dexes(chain_id);
      self.syncing_pools = true;

      let ctx2 = ctx.clone();
      RT.spawn(async move {
         let client = ctx2.get_client_with_id(chain_id).await.unwrap();
         match manager
            .sync_pools_for_tokens(
               client.clone(),
               vec![token_in.clone(), token_out.clone()],
               dexes,
            )
            .await
         {
            Ok(_) => {
               // tracing::info!("Synced pools for token: {}", token.symbol);
               SHARED_GUI.write(|gui| {
                  gui.swap_ui.syncing_pools = false;
                  gui.swap_ui.pool_data_syncing = true;
               });
            }
            Err(e) => {
               tracing::error!("Error syncing pools: {:?}", e);
               SHARED_GUI.write(|gui| {
                  gui.swap_ui.syncing_pools = false;
               });
               return;
            }
         };

         let pools_to_update = manager.get_pools_from_currency(&currency_to_update_pools_from);
         match manager
            .update_state_for_pools(client, chain_id, pools_to_update)
            .await
         {
            Ok(_) => {
               // tracing::info!("Updated pool state for token: {}", token.symbol);
               SHARED_GUI.write(|gui| {
                  gui.swap_ui.last_pool_state_updated = Some(Instant::now());
                  gui.swap_ui.pool_data_syncing = false;
               });
            }
            Err(_e) => {
               // tracing::error!("Error updating pool state: {:?}", e);
               SHARED_GUI.write(|gui| {
                  gui.swap_ui.pool_data_syncing = false;
               });
            }
         }

         RT.spawn_blocking(move || match ctx2.save_pool_manager() {
            Ok(_) => {
               tracing::info!("Pool Manager saved");
            }
            Err(e) => {
               tracing::error!("Error saving pool manager: {:?}", e);
            }
         });
      });
   }

   /// Currency to use to pull pools from
   fn currency_to_update_pools_from(&self) -> &Currency {
      // Do not consider WETH or as the token to get pools from because overtime we will accumulate a lot of pools
      // Since it's the most common paired token
      let currency = if self.currency_in.is_weth_or_eth() && !self.currency_out.is_weth_or_eth() {
         &self.currency_out
      } else {
         &self.currency_in
      };
      currency
   }

   /// Which pools to update the state for
   fn pools_to_update(&self, ctx: ZeusCtx, currency: &Currency) -> Vec<AnyUniswapPool> {
      let manager = ctx.pool_manager();
      let pools = manager.get_pools_from_currency(currency);
      if pools.is_empty() {
         tracing::warn!(
            "No pools found for currency: {}",
            currency.symbol()
         );
      }
      pools
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

      let now = Instant::now();
      if let Some(last_updated) = self.last_pool_state_updated {
         let elapsed = now.duration_since(last_updated).as_secs();
         if elapsed < POOL_STATE_EXPIRY {
            return false;
         }
      }

      true
   }

   fn update_pool_state(&mut self, ctx: ZeusCtx) {
      let action = self.action();
      if action.is_wrap() || action.is_unwrap() {
         return;
      }

      let currency = self.currency_to_update_pools_from();
      let pools = self.pools_to_update(ctx.clone(), currency);

      if pools.is_empty() {
         tracing::warn!(
            "Can't get quote, No pools found for currency: {}",
            currency.symbol()
         );
         return;
      }

      tracing::info!(
         "Updating pool state for currency: {}",
         currency.symbol()
      );

      let chain_id = ctx.chain().id();
      let manager = ctx.pool_manager();

      self.pool_data_syncing = true;
      let ctx2 = ctx.clone();
      RT.spawn(async move {
         let client = ctx2.get_client_with_id(chain_id).await.unwrap();
         match manager
            .update_state_for_pools(client, chain_id, pools)
            .await
         {
            Ok(_) => {
               SHARED_GUI.write(|gui| {
                  gui.swap_ui.last_pool_state_updated = Some(Instant::now());
                  gui.swap_ui.pool_data_syncing = false;
               });
            }
            Err(e) => {
               tracing::error!("Error updating pool state: {:?}", e);
               SHARED_GUI.write(|gui| {
                  gui.swap_ui.pool_data_syncing = false;
               });
            }
         }

         // get a new quote
         SHARED_GUI.write(|gui| {
            gui.swap_ui.get_quote(ctx2);
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

   fn get_quote(&mut self, ctx: ZeusCtx) {
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
      let manager = ctx.pool_manager();
      let pools = manager.get_pools_for_chain(chain);
      let base_fee = ctx.get_base_fee(chain).unwrap_or_default().next;
      let priority_fee = ctx.get_priority_fee(chain).unwrap_or_default();
      let eth_price = ctx.get_eth_price();
      let currency_out_price = ctx.get_currency_price(&currency_out);

      RT.spawn_blocking(move || {
         let quote = get_quote(
            amount_in.wei2(),
            currency_in,
            currency_out,
            pools,
            eth_price,
            currency_out_price,
            base_fee,
            priority_fee.wei2(),
         );

         // tracing::info!("Swap Route Length: {}", quote.swaps_len());
         // tracing::info!("Route {}", quote.currency_path_str());
         // tracing::info!("Gas Used: {}", quote.total_gas_used());
         // tracing::info!("Gas Cost USD: {}", quote.total_gas_cost_usd());

         let swap_steps = quote.get_swap_steps();
         tracing::info!("Swap Steps Length: {}", swap_steps.len());
         for swap in &swap_steps {
            tracing::info!(
               "Swap Step: {} {} -> {} {} {} ({})",
               swap.amount_in.f64(),
               swap.currency_in.symbol(),
               swap.amount_out.f64(),
               swap.currency_out.symbol(),
               swap.pool.address(),
               swap.pool.fee().fee()
            );
         }

         SHARED_GUI.write(|gui| {
            if !quote.routes.is_empty() {
               gui.swap_ui.amount_out = quote.total_amount_out().flatten();

               gui.swap_ui.getting_quote = false;
               gui.swap_ui.quote_routes = quote;
            } else {
               gui.swap_ui.quote_routes = QuoteRoutes::default();
               gui.swap_ui.amount_out = String::new();
               gui.swap_ui.getting_quote = false;
            }
         });
      });
   }

   fn swap_details(&self, _ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      let quote = &self.quote_routes;

      // Slippage
      ui.horizontal(|ui| {
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.label(RichText::new("Slippage").size(theme.text_sizes.normal));
         });

         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            ui.label(RichText::new(format!("{}%", self.slippage)).size(theme.text_sizes.normal));
         });
      });

      // Minimum Received
      ui.horizontal(|ui| {
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.label(RichText::new("Minimum Received").size(theme.text_sizes.normal));
         });

         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            let mut amount_out = quote.total_amount_out();
            let slippage: f64 = self.slippage.parse().unwrap_or(0.5);
            amount_out.calc_slippage(slippage, self.currency_out.decimals());

            ui.label(
               RichText::new(format!(
                  "{} {}",
                  amount_out.formatted(),
                  self.currency_out.symbol()
               ))
               .size(theme.text_sizes.normal),
            );
         });
      });
   }

   fn swap(&self, ctx: ZeusCtx) {
      let action = self.action();
      let from = ctx.current_wallet().address;
      let chain = ctx.chain();

      if action.is_wrap() || action.is_unwrap() {
         let amount_in = self.amount_in.clone();
         let amount_in = NumericValue::parse_to_wei(&amount_in, self.currency_in.decimals());
         RT.spawn(async move {
            SHARED_GUI.write(|gui| {
               gui.loading_window.open("Wait while magic happens");
               gui.request_repaint();
            });

            match eth::wrap_or_unwrap_eth(
               ctx.clone(),
               from,
               chain,
               amount_in.clone(),
               action.is_wrap(),
            )
            .await
            {
               Ok(_) => {}
               Err(e) => {
                  tracing::error!("Error wrapping/unwrapping: {:?}", e);
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

      let quote = self.quote_routes.clone();
      let amount_in = quote.total_amount_in();
      let mev_protect = self.mev_protect;
      let currency_in = quote.currency_in.clone();
      let currency_out = quote.currency_out.clone();
      let amount_out = quote.total_amount_out();
      let slippage: f64 = self.slippage.parse().unwrap_or(0.5);

      // set no slippage on intermidiate swaps to make sure they don't fail
      let mut swap_steps = quote.get_swap_steps();
      let len = swap_steps.len();
      for (i, swap) in swap_steps.iter_mut().enumerate() {
         if i < len - 1 {
            swap.amount_out = NumericValue::default()
         }
      }

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
                  gui.tx_confirm_window.reset();
                  gui.msg_window.open("Transaction Error", &e.to_string());
                  gui.request_repaint();
               });
            }
         }
      });
   }
}
