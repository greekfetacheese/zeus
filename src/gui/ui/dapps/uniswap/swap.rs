use super::UniswapSettingsUi;
use crate::gui::ui::dapps::{AmountFieldWithCurrencySelect, uniswap::ProtocolVersion};
use crate::gui::ui::*;
use crate::utils::universal_router_v2::SwapType;
use crate::{assets::icons::Icons, gui::SHARED_GUI};
use egui::{
   Align, Button, Color32, ComboBox, Frame, Grid, Id, Layout, RichText, ScrollArea, Spinner, Ui,
   Window, vec2,
};

use anyhow::anyhow;
use std::sync::Arc;
use std::{collections::HashSet, time::Instant};
use zeus_eth::alloy_rpc_types::Block;
use zeus_eth::revm::context::ContextTr;
use zeus_theme::Theme;

use crate::core::{
   Dapp, TransactionAnalysis, ZeusCtx,
   transaction::{DecodedEvent, SwapParams, TokenApproveParams, UnwrapWETHParams, WrapETHParams},
};
use crate::utils::{
   RT,
   sign::sign_message,
   simulate::{fetch_accounts_info, simulate_transaction},
   tx::send_transaction,
};

use crate::utils::{Permit2Details, simulate::*, swap_quoter::*, universal_router_v2::encode_swap};

use zeus_eth::{
   abi,
   alloy_primitives::{U256, address},
   alloy_provider::Provider,
   alloy_rpc_types::BlockId,
   amm::uniswap::{AnyUniswapPool, UniswapPool},
   currency::{Currency, erc20::ERC20Token, native::NativeCurrency},
   revm_utils::{ForkDB, ForkFactory, Host, new_evm, simulate},
   utils::{NumericValue, address_book},
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
                     token0_balance.abbreviated(),
                  );
                  let text = RichText::new(token0_balance).size(theme.text_sizes.normal);
                  ui.label(text);

                  let token1_balance = format!(
                     "{} {}",
                     token1.symbol(),
                     token1_balance.abbreviated(),
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
                     token0_balance.abbreviated(),
                  );
                  let text = RichText::new(token0_balance).size(theme.text_sizes.normal);
                  ui.label(text);

                  let token1_balance = format!(
                     "{} {}",
                     token1.symbol(),
                     token1_balance.abbreviated(),
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
   open: bool,
   pub size: (f32, f32),
   pub currency_in: Currency,
   pub currency_out: Currency,
   pub amount_in_field: AmountFieldWithCurrencySelect,
   pub amount_out_field: AmountFieldWithCurrencySelect,
   /// Last time pool state was updated
   pub last_pool_state_updated: Option<Instant>,
   /// Last time quote was updated
   pub last_quote_updated: Option<Instant>,
   pub pool_data_syncing: bool,
   pub syncing_pools: bool,
   pub balance_syncing: bool,
   pub getting_quote: bool,
   pub sending_tx: bool,
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
         amount_in_field: AmountFieldWithCurrencySelect::new(),
         amount_out_field: AmountFieldWithCurrencySelect::new(),
         last_pool_state_updated: None,
         last_quote_updated: None,
         pool_data_syncing: false,
         syncing_pools: false,
         balance_syncing: false,
         getting_quote: false,
         sending_tx: false,
         quote: Quote::default(),
         protocol_version: ProtocolVersion::V3,
         pool: None,
         simulate_window: SimulateWindow::new(),
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
      self.amount_in_field.reset();
      self.amount_out_field.reset();
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
      std::mem::swap(
         &mut self.amount_in_field.amount,
         &mut self.amount_out_field.amount,
      );
   }

   fn select_version(&mut self, theme: &Theme, ui: &mut Ui) {
      let mut current_version = self.protocol_version;
      let versions = ProtocolVersion::all();

      let selected_text = RichText::new(current_version.as_str()).size(theme.text_sizes.normal);

      ComboBox::from_id_salt("protocol_version")
         .selected_text(selected_text)
         .show_ui(ui, |ui| {
            for version in versions {
               let text = RichText::new(version.as_str()).size(theme.text_sizes.normal);
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
         Grid::new("swap_ui_fee_tier_select").spacing(vec2(15.0, 0.0)).show(ui, |ui| {
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

   pub fn refresh(&mut self, ctx: ZeusCtx, settings: &UniswapSettingsUi) {
      self.update_pool_state(
         ctx.clone(),
         settings.swap_on_v2,
         settings.swap_on_v3,
         settings.swap_on_v4,
      );
      self.sync_pools(ctx.clone(), settings, true);
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
      let owner = ctx.current_wallet_info().address;
      let simulate_mode = settings.simulate_mode;

      if simulate_mode {
         self.simulate_window.show(ctx.clone(), theme, settings, ui);
      }

      ui.vertical_centered(|ui| {
         ui.set_width(self.size.0);
         ui.set_height(self.size.1);
         ui.spacing_mut().item_spacing = vec2(0.0, 15.0);

         if simulate_mode {
            let text =
               RichText::new("You are on Simulate Mode").size(theme.text_sizes.large).strong();
            ui.label(text);
         }

         ui.horizontal(|ui| {
            if simulate_mode {
               ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                  self.select_version(theme, ui);
               });
            }

            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
               if self.pool_data_syncing || self.syncing_pools || self.balance_syncing {
                  ui.add(Spinner::new().size(17.0).color(theme.colors.text));
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
            pools.sort_by_key(|a| a.fee().fee());

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

         // Currency in
         let frame = theme.frame1;
         let mut amount_changed = false;
         let label = String::from("Sell");
         let balance = || ctx.get_currency_balance(chain_id, owner, &self.currency_in);
         let max_amount = || ctx.get_currency_balance(chain_id, owner, &self.currency_in);
         let amount = self.amount_in_field.amount.parse().unwrap_or(0.0);
         let value = || ctx.get_currency_value_for_amount(amount, &self.currency_in);

         frame.show(ui, |ui| {
            let changed = self.amount_in_field.show(
               ctx.clone(),
               theme,
               icons.clone(),
               Some(label),
               owner,
               &self.currency_in,
               Some(token_selection),
               Some(InOrOut::In),
               balance,
               max_amount,
               value,
               false,
               true,
               ui,
            );
            amount_changed = changed;
         });

         // Swap Currencies
         ui.vertical_centered(|ui| {
            let tint = theme.image_tint_recommended;
            let swap_button = Button::image(icons.swap(tint)).min_size(vec2(40.0, 40.0));

            if ui.add(swap_button).clicked() {
               self.swap_currencies();
               self.get_quote(ctx.clone(), settings);
            }
         });

         // Currency out
         let label = String::from("Buy");
         let balance = || ctx.get_currency_balance(chain_id, owner, &self.currency_out);
         let max_amount = || NumericValue::default();
         let amount = self.amount_out_field.amount.parse().unwrap_or(0.0);
         let value = || ctx.get_currency_value_for_amount(amount, &self.currency_out);
         frame.show(ui, |ui| {
            self.amount_out_field.show(
               ctx.clone(),
               theme,
               icons.clone(),
               Some(label),
               owner,
               &self.currency_out,
               Some(token_selection),
               Some(InOrOut::Out),
               balance,
               max_amount,
               value,
               false,
               false,
               ui,
            )
         });

         token_selection.show(
            ctx.clone(),
            theme,
            icons.clone(),
            chain_id,
            owner,
            ui,
         );

         let selected_currency = token_selection.get_currency().cloned();
         let direction = token_selection.get_currency_direction();
         let changed_currency = selected_currency.is_some();
         let should_get_quote = changed_currency || amount_changed;

         if let Some(currency) = selected_currency {
            self.replace_currency(direction, currency.clone());
            self.update_currency_balance(ctx.clone(), currency);
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
            self.swap_details(ctx, theme, icons, settings, ui);
         }
      });
   }

   fn action(&self) -> Action {
      let should_wrap = self.currency_in.is_native() && self.currency_out.is_native_wrapped();
      let should_unwrap = self.currency_in.is_native_wrapped() && self.currency_out.is_native();

      if should_wrap {
         Action::WrapETH
      } else if should_unwrap {
         Action::UnwrapWETH
      } else {
         Action::Swap
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
      let enabled = !self.amount_in_field.amount.is_empty() && got_pool;
      let button = Button::new(RichText::new("Simulate").size(theme.text_sizes.large))
         .min_size(vec2(ui.available_width() * 0.8, 45.0));

      if ui.add_enabled(enabled, button).clicked() {
         if let Some(pool) = &mut self.pool {
            let amount_in = NumericValue::parse_to_wei(
               &self.amount_in_field.amount,
               self.currency_in.decimals(),
            );
            pool.simulate_swap_mut(&self.currency_in, amount_in.wei()).unwrap_or_default();
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
      let sending_tx = self.sending_tx;
      let valid_inputs = self.valid_inputs(ctx.clone());
      let has_swap_steps = !self.quote.swap_steps.is_empty();
      let has_balance = self.sufficient_balance(ctx.clone());
      let has_entered_amount = !self.amount_in_field.amount.is_empty();
      let action = self.action();

      let valid = if action.is_wrap() || action.is_unwrap() {
         valid_inputs && !sending_tx
      } else {
         valid_inputs && has_swap_steps && !sending_tx
      };

      let mut button_text = "Swap".to_string();

      if !has_entered_amount {
         button_text = "Enter Amount".to_string();
      }

      if valid_inputs && action.is_wrap() {
         button_text = format!("Wrap {}", self.currency_in.symbol());
      }

      if valid_inputs && action.is_unwrap() {
         button_text = format!("Unwrap {}", self.currency_in.symbol());
      }

      if valid_inputs && action.is_swap() && !has_swap_steps {
         button_text = "No Routes Found".to_string();
      }

      if !has_balance {
         button_text = format!(
            "Insufficient {} Balance",
            self.currency_in.symbol()
         );
      }

      let swap_button = Button::new(
         RichText::new(button_text).size(theme.text_sizes.large).color(theme.colors.text),
      )
      .min_size(vec2(ui.available_width() * 0.8, 45.0));

      ui.vertical_centered(|ui| {
         if ui.add_enabled(valid, swap_button).clicked() {
            self.sending_tx = true;
            self.swap(ctx, settings);
         }
      });
   }

   fn valid_inputs(&self, ctx: ZeusCtx) -> bool {
      self.valid_amounts() && self.sufficient_balance(ctx)
   }

   fn valid_amounts(&self) -> bool {
      let amount_in = self.amount_in_field.amount.parse().unwrap_or(0.0);
      let amount_out = self.amount_out_field.amount.parse().unwrap_or(0.0);
      amount_in > 0.0 && amount_out > 0.0
   }

   fn sufficient_balance(&self, ctx: ZeusCtx) -> bool {
      let sender = ctx.current_wallet_info().address;
      let balance = ctx.get_currency_balance(ctx.chain().id(), sender, &self.currency_in);
      let amount = NumericValue::parse_to_wei(
         &self.amount_in_field.amount,
         self.currency_in.decimals(),
      );
      balance.wei() >= amount.wei()
   }

   fn update_currency_balance(&self, ctx: ZeusCtx, currency: Currency) {
      RT.spawn(async move {
         SHARED_GUI.write(|gui| {
            gui.uniswap.swap_ui.balance_syncing = true;
         });

         let manager = ctx.balance_manager();
         let owner = ctx.current_wallet_info().address;
         let chain = currency.chain_id();

         if currency.is_erc20() {
            let token = currency.to_erc20().into_owned();

            match manager
               .update_tokens_balance(ctx.clone(), chain, owner, vec![token], false)
               .await
            {
               Ok(_) => {
                  SHARED_GUI.write(|gui| {
                     gui.uniswap.swap_ui.balance_syncing = false;
                  });
               }
               Err(e) => {
                  tracing::error!("Error updating token balance: {:?}", e);
                  SHARED_GUI.write(|gui| {
                     gui.uniswap.swap_ui.balance_syncing = false;
                  });
               }
            }
         } else {
            match manager.update_eth_balance(ctx.clone(), chain, vec![owner], false).await {
               Ok(_) => {
                  SHARED_GUI.write(|gui| {
                     gui.uniswap.swap_ui.balance_syncing = false;
                  });
               }
               Err(e) => {
                  tracing::error!("Error updating ETH balance: {:?}", e);
                  SHARED_GUI.write(|gui| {
                     gui.uniswap.swap_ui.balance_syncing = false;
                  });
               }
            }
         }

         if currency.is_erc20() {
            let token = currency.to_erc20().into_owned();
            if ctx.portfolio_has_token(chain, owner, token.address) {
               let mut portfolio = ctx.get_portfolio(chain, owner);
               portfolio.add_token(token);
               ctx.write(|ctx| {
                  ctx.portfolio_db.insert_portfolio(chain, owner, portfolio);
               });

               ctx.calculate_portfolio_value(chain, owner);
               ctx.save_portfolio_db();
            }
         }
      });
   }

   fn sync_pools(&mut self, ctx: ZeusCtx, settings: &UniswapSettingsUi, changed_currency: bool) {
      if self.syncing_pools {
         return;
      }

      if !changed_currency {
         return;
      }

      if self.currency_in == self.currency_out {
         return;
      }

      // ETH -> WETH
      if self.currency_in.is_native() && self.currency_out.is_native_wrapped() {
         return;
      }

      // WETH -> WETH
      if self.currency_in.is_native_wrapped() && self.currency_out.is_native_wrapped() {
         return;
      }

      // WETH -> ETH
      if self.currency_in.is_native_wrapped() && self.currency_out.is_native() {
         return;
      }

      let chain_id = ctx.chain().id();
      let pool_manager = ctx.pool_manager();
      let currency_in = self.currency_in.clone();
      let currency_out = self.currency_out.clone();

      let tokens = vec![
         currency_in.to_erc20().into_owned(),
         currency_out.to_erc20().into_owned(),
      ];

      let swap_on_v2 = settings.swap_on_v2;
      let swap_on_v3 = settings.swap_on_v3;
      let swap_on_v4 = settings.swap_on_v4;

      self.syncing_pools = true;

      let ctx_clone = ctx.clone();
      RT.spawn(async move {
         match pool_manager.sync_pools_for_tokens(ctx_clone.clone(), chain_id, tokens).await {
            Ok(_) => {}
            Err(e) => {
               tracing::error!("Failed to sync pools: {}", e);
            }
         }

         SHARED_GUI.write(|gui| {
            gui.uniswap.swap_ui.syncing_pools = false;
            gui.uniswap.swap_ui.pool_data_syncing = true;
         });

         let pools = get_relevant_pools(
            ctx_clone.clone(),
            swap_on_v2,
            swap_on_v3,
            swap_on_v4,
            &currency_in,
            &currency_out,
         );

         match pool_manager.update_state_for_pools(ctx_clone.clone(), chain_id, pools).await {
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
               gui.uniswap.swap_ui.get_quote(ctx_clone.clone(), settings);
            });

            ctx_clone.save_pool_manager();
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

      // WETH -> ETH
      if self.currency_in.is_native_wrapped() && self.currency_out.is_native() {
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
         &self.currency_in,
         &self.currency_out,
      );

      if pools.is_empty() {
         tracing::warn!(
            "Can't get quote, No pools found for {}-{}",
            self.currency_in.symbol(),
            self.currency_out.symbol()
         );
      }

      let chain_id = ctx.chain().id();
      let manager = ctx.pool_manager();

      self.pool_data_syncing = true;
      let ctx_clone = ctx.clone();
      RT.spawn(async move {
         match manager.update_state_for_pools(ctx_clone.clone(), chain_id, pools).await {
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
            gui.uniswap.swap_ui.get_quote(ctx_clone, settings);
         });
      });
   }

   pub fn get_quote(&mut self, ctx: ZeusCtx, settings: &UniswapSettingsUi) {
      if settings.simulate_mode {
         if let Some(pool) = &self.pool {
            let amount_in = NumericValue::parse_to_wei(
               &self.amount_in_field.amount,
               self.currency_in.decimals(),
            );
            let amount_out =
               pool.simulate_swap(&self.currency_in, amount_in.wei()).unwrap_or_default();
            let amount = NumericValue::format_wei(amount_out, self.currency_out.decimals());
            self.amount_out_field.amount = amount.flatten();
            return;
         }
      }

      let action = self.action();
      if action == Action::WrapETH || action == Action::UnwrapWETH {
         self.amount_out_field.amount = self.amount_in_field.amount.clone();
         return;
      }

      let amount_in = NumericValue::parse_to_wei(
         &self.amount_in_field.amount,
         self.currency_in.decimals(),
      );

      if amount_in.is_zero() {
         self.amount_out_field.amount = String::new();
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

      let ctx_clone = ctx.clone();
      RT.spawn_blocking(move || {
         let pools = get_relevant_pools(
            ctx.clone(),
            swap_on_v2,
            swap_on_v3,
            swap_on_v4,
            &currency_in,
            &currency_out,
         );

         let mut liquid_pools = Vec::new();
         for pool in pools.iter() {
            let has_liquidity = ctx_clone.pool_has_sufficient_liquidity(pool).unwrap_or(false);

            if has_liquidity {
               liquid_pools.push(pool.clone());
            }
         }

         let quote = if split_routing_enabled {
            get_quote_with_split_routing(
               ctx_clone.clone(),
               amount_in.clone(),
               currency_in.clone(),
               currency_out.clone(),
               liquid_pools,
               eth_price,
               currency_out_price,
               base_fee,
               priority_fee.wei(),
               max_hops,
               max_split_routes,
            )
         } else {
            get_quote(
               ctx_clone.clone(),
               amount_in.clone(),
               currency_in.clone(),
               currency_out.clone(),
               liquid_pools,
               eth_price,
               currency_out_price,
               base_fee,
               priority_fee.wei(),
               max_hops,
            )
         };

         let amount_out = quote.amount_out.clone();

         SHARED_GUI.write(|gui| {
            if !quote.amount_out.is_zero() {
               gui.uniswap.swap_ui.amount_out_field.amount = amount_out.flatten();

               gui.uniswap.swap_ui.getting_quote = false;
               gui.uniswap.swap_ui.quote = quote;
            } else {
               gui.uniswap.swap_ui.quote = Quote::default();
               gui.uniswap.swap_ui.amount_out_field.amount = String::new();
               gui.uniswap.swap_ui.getting_quote = false;
            }
         });
      });
   }

   fn swap_details(
      &self,
      ctx: ZeusCtx,
      theme: &Theme,
      icons: Arc<Icons>,
      settings: &UniswapSettingsUi,
      ui: &mut Ui,
   ) {
      let frame = theme.frame2;
      let text_size = theme.text_sizes.large;
      let tint = theme.image_tint_recommended;

      frame.show(ui, |ui| {
         // Routing
         ui.horizontal(|ui| {
            let text = RichText::new("Routing").size(text_size);
            let info = icons.info(tint);
            let label = Label::new(text, Some(info));

            ui.add(label).on_hover_ui(|ui| {
               ui.set_width(350.0);
               ui.set_height(100.0);
               ScrollArea::vertical().show(ui, |ui| {
                  let swaps_len = self.quote.swap_steps.len();
                  let text = format!("Total swaps {}", swaps_len);
                  ui.label(RichText::new(text).size(theme.text_sizes.very_small));
                  ui.add_space(5.0);

                  for step in &self.quote.swap_steps {
                     let text = format!(
                        "{} {} -> {} {} ({}/{} {} {}%)",
                        step.amount_in.abbreviated(),
                        step.currency_in.symbol(),
                        step.amount_out.abbreviated(),
                        step.currency_out.symbol(),
                        step.pool.currency0().symbol(),
                        step.pool.currency1().symbol(),
                        step.pool.dex_kind().version_str(),
                        step.pool.fee().fee_percent()
                     );
                     ui.label(RichText::new(text).size(theme.text_sizes.very_small));
                  }
               });
            });
         });

         // Slippage
         ui.horizontal(|ui| {
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
               ui.label(RichText::new("Slippage").size(text_size));
            });

            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
               ui.label(RichText::new(format!("{:.1}%", settings.slippage_f64)).size(text_size));
            });
         });

         // Minimum Received
         ui.horizontal(|ui| {
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
               ui.label(RichText::new("Minimum Received").size(text_size));
            });

            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
               let slippage: f64 = settings.slippage.parse().unwrap_or(0.5);
               let amount_out_min =
                  self.quote.amount_out.calc_slippage(slippage, self.currency_out.decimals());

               if self.valid_amounts() && self.action().is_swap() {
                  ui.label(
                     RichText::new(format!(
                        "{} {}",
                        amount_out_min.formatted(),
                        self.currency_out.symbol()
                     ))
                     .size(text_size),
                  );
               }
            });
         });

         // Price Impact
         ui.horizontal(|ui| {
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
               ui.label(RichText::new("Price Impact").size(text_size));
            });

            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
               let price_impact = self.calc_price_impact(ctx.clone());
               let color = if price_impact == 0.0 {
                  theme.colors.text
               } else if price_impact.is_sign_positive() {
                  theme.colors.error
               } else {
                  theme.colors.success
               };

               ui.label(
                  RichText::new(format!("{:.2}%", price_impact)).size(text_size).color(color),
               );
            });
         });
      });
   }

   fn calc_price_impact(&self, ctx: ZeusCtx) -> f64 {
      if !self.valid_amounts() || !self.action().is_swap() {
         return 0.0;
      }

      // Currency in USD value
      let amount: f64 = self.amount_in_field.amount.parse().unwrap_or(0.0);
      let amount_in_usd = ctx.get_currency_value_for_amount(amount, &self.currency_in);

      // Currency out USD value
      let amount_out: f64 = self.amount_out_field.amount.parse().unwrap_or(0.0);
      let amount_out_usd = ctx.get_currency_value_for_amount(amount_out, &self.currency_out);

      let price_impact = (1.0 - (amount_out_usd.f64() / amount_in_usd.f64())) * 100.0;
      price_impact
   }

   fn swap(&self, ctx: ZeusCtx, settings: &UniswapSettingsUi) {
      let action = self.action();
      let from = ctx.current_wallet_info().address;
      let chain = ctx.chain();

      if action.is_wrap() {
         let amount_in = self.amount_in_field.amount.clone();
         let amount_in = NumericValue::parse_to_wei(&amount_in, self.currency_in.decimals());
         RT.spawn(async move {
            SHARED_GUI.write(|gui| {
               gui.loading_window.open("Wait while magic happens");
               gui.request_repaint();
            });

            match wrap_eth(ctx.clone(), from, chain, amount_in.clone()).await {
               Ok(_) => {
                  SHARED_GUI.write(|gui| {
                     gui.uniswap.swap_ui.sending_tx = false;
                  });
               }
               Err(e) => {
                  SHARED_GUI.write(|gui| {
                     gui.uniswap.swap_ui.sending_tx = false;
                     gui.notification.reset();
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
         let amount_in = self.amount_in_field.amount.clone();
         let amount_in = NumericValue::parse_to_wei(&amount_in, self.currency_in.decimals());
         RT.spawn(async move {
            SHARED_GUI.write(|gui| {
               gui.loading_window.open("Wait while magic happens");
               gui.request_repaint();
            });

            match unwrap_weth(ctx.clone(), from, chain, amount_in.clone()).await {
               Ok(_) => {
                  SHARED_GUI.write(|gui| {
                     gui.uniswap.swap_ui.sending_tx = false;
                  });
               }
               Err(e) => {
                  SHARED_GUI.write(|gui| {
                     gui.uniswap.swap_ui.sending_tx = false;
                     gui.notification.reset();
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
      let deadline = settings.deadline;
      let slippage: f64 = settings.slippage.parse().unwrap_or(0.5);

      RT.spawn(async move {
         SHARED_GUI.open_loading("Wait while magic happens");
         SHARED_GUI.request_repaint();

         match swap(
            ctx.clone(),
            chain,
            slippage,
            mev_protect,
            deadline,
            from,
            amount_in,
            amount_out,
            currency_in,
            currency_out,
            swap_steps,
         )
         .await
         {
            Ok(_) => {
               SHARED_GUI.write(|gui| {
                  gui.uniswap.swap_ui.sending_tx = false;
               });
               tracing::info!("Transaction Sent");
            }
            Err(e) => {
               tracing::error!("Transaction Error: {:?}", e);
               SHARED_GUI.write(|gui| {
                  gui.uniswap.swap_ui.sending_tx = false;
                  gui.notification.reset();
                  gui.loading_window.reset();
                  gui.tx_confirmation_window.reset(ctx);
                  gui.msg_window.open("Transaction Error", e.to_string());
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
   currency_in: &Currency,
   currency_out: &Currency,
) -> Vec<AnyUniswapPool> {
   let manager = ctx.pool_manager();
   let all_pools = manager.get_pools_for_chain(currency_out.chain_id());
   let mut relevant_pools = Vec::new();
   let mut added_pools = HashSet::new();

   // Handle ETH/WETH
   let weth = Currency::wrapped_native(currency_in.chain_id());

   // If we are swapping between two "safe"
   // base currencies (e.g., DAI to USDC), we should avoid routing through
   // a non-base token (e.g., PEPE).
   let is_base_pair_swap = currency_in.is_base() && currency_out.is_base();

   // If we are swapping from base to a shit token we only include pools
   // that have currency out
   let is_base_to_shit_token_swap = currency_in.is_base() && !currency_out.is_base();

   // If we are swapping from a shit token to base we only include pools
   // that have currency in
   let is_shit_token_to_base_swap = !currency_in.is_base() && currency_out.is_base();

   // If we are swapping from a shit token to a shit token we only include pools
   // that have currency in or currency out
   let is_shit_token_to_shit_token_swap = !currency_in.is_base() && !currency_out.is_base();

   for pool in all_pools {
      if (!swap_on_v2 && pool.dex_kind().is_v2())
         || (!swap_on_v3 && pool.dex_kind().is_v3())
         || (!swap_on_v4 && pool.dex_kind().is_v4())
      {
         continue;
      }

      let pool_key = (pool.chain_id(), pool.address(), pool.id());
      if added_pools.contains(&pool_key) {
         continue;
      }

      let is_base_pool = pool.currency0().is_base() && pool.currency1().is_base();

      // A pool is relevant if it contains either our starting or ending currency.
      let has_currency_in = pool.have(currency_in) || pool.have(&weth);
      let has_currency_out = pool.have(currency_out) || pool.have(&weth);

      if has_currency_in || has_currency_out {
         let mut add_pool = false;

         if is_base_pair_swap {
            if is_base_pool {
               add_pool = true;
            }
         } else if is_base_to_shit_token_swap {
            if pool.have(currency_out) {
               add_pool = true;
            }
         } else if is_shit_token_to_base_swap {
            if pool.have(currency_in) {
               add_pool = true;
            }

            // Also include base pools that have the currency out
            if is_base_pool && pool.have(currency_out) {
               add_pool = true;
            }
         } else if is_shit_token_to_shit_token_swap {
            if pool.have(currency_in) || pool.have(currency_out) {
               add_pool = true;
            }
         }

         if add_pool {
            relevant_pools.push(pool);
            added_pools.insert(pool_key);
         }
      }
   }

   relevant_pools
}

async fn swap(
   ctx: ZeusCtx,
   chain: ChainId,
   slippage: f64,
   mev_protect: bool,
   deadline: u64,
   from: Address,
   amount_in: NumericValue,
   _amount_out: NumericValue,
   currency_in: Currency,
   currency_out: Currency,
   swap_steps: Vec<SwapStep<impl UniswapPool + Clone>>,
) -> Result<(), anyhow::Error> {
   swap_via_ur(
      ctx,
      chain,
      slippage,
      mev_protect,
      deadline,
      from,
      amount_in,
      currency_in,
      currency_out,
      swap_steps,
   )
   .await?;

   Ok(())
}

pub async fn wrap_eth(
   ctx: ZeusCtx,
   from: Address,
   chain: ChainId,
   amount: NumericValue,
) -> Result<(), anyhow::Error> {
   let client = ctx.get_zeus_client();

   let eth_balance_before_fut = client.request(chain.id(), |client| async move {
      client
         .get_balance(from)
         .block_id(BlockId::latest())
         .await
         .map_err(|e| anyhow!("{:?}", e))
   });

   let block = client
      .request(chain.id(), |client| async move {
         client.get_block(BlockId::latest()).await.map_err(|e| anyhow!("{:?}", e))
      })
      .await?;

   let block = if let Some(block) = block {
      block
   } else {
      return Err(anyhow!(
         "No block found, this is usally a provider issue"
      ));
   };

   let block_id = BlockId::number(block.header.number);

   let weth = ERC20Token::wrapped_native_token(chain.id());

   let call_data = weth.encode_deposit();
   let interact_to = weth.address;
   let value = amount.wei();

   let mut accounts = Vec::new();
   accounts.push(from);
   accounts.push(interact_to);
   accounts.push(block.header.beneficiary);
   accounts.push(weth.address);

   let accounts_info = fetch_accounts_info(ctx.clone(), chain.id(), block_id, accounts).await;

   let fork_client = ctx.get_client(chain.id()).await?;
   let mut factory =
      ForkFactory::new_sandbox_factory(fork_client, chain.id(), None, Some(block_id));

   for info in accounts_info {
      factory.insert_account_info(info.address, info.info);
   }

   let fork_db = factory.new_sandbox_fork();

   let eth_balance_after;
   let sim_res;
   {
      let mut evm = new_evm(chain, Some(&block), fork_db.clone());

      let time = Instant::now();
      sim_res = simulate_transaction(
         &mut evm,
         from,
         interact_to,
         call_data.clone(),
         value,
         vec![],
      )?;
      tracing::info!(
         "Simulate Transaction took {} ms",
         time.elapsed().as_millis()
      );

      let state = evm.balance(from);
      eth_balance_after = if let Some(state) = state {
         state.data
      } else {
         U256::ZERO
      };
   }

   let logs = sim_res.clone().into_logs();

   let wrapped: Currency = weth.clone().into();
   let eth_balance_before = eth_balance_before_fut.await?;
   let eth_wrapped_usd = ctx.get_currency_value_for_amount(amount.f64(), &wrapped);
   let mut weth_received = None;

   for log in &logs {
      if let Ok(decoded) = abi::weth9::decode_deposit_log(log) {
         weth_received = Some(decoded.wad);
         break;
      }
   }

   if weth_received.is_none() {
      return Err(anyhow::anyhow!(
         "Failed to decode weth deposit log"
      ));
   }

   let weth_received = NumericValue::format_wei(weth_received.unwrap(), wrapped.decimals());
   let weth_received_usd = ctx.get_currency_value_for_amount(weth_received.f64(), &wrapped);

   let contract_interact = Some(true);
   let auth_list = Vec::new();

   let params = WrapETHParams {
      chain: chain.id(),
      recipient: from,
      eth_wrapped: amount,
      eth_wrapped_usd: Some(eth_wrapped_usd),
      weth_received,
      weth_received_usd: Some(weth_received_usd),
   };

   let mut tx_analysis = TransactionAnalysis::new(
      ctx.clone(),
      chain.id(),
      from,
      interact_to,
      contract_interact,
      call_data.clone(),
      value,
      logs,
      sim_res.gas_used(),
      eth_balance_before,
      eth_balance_after,
      auth_list.clone(),
   )
   .await?;

   let main_event = DecodedEvent::WrapETH(params.clone());
   tx_analysis.set_main_event(main_event);

   let mev_protect = false;

   let (_, _) = send_transaction(
      ctx.clone(),
      "".to_string(),
      Some(tx_analysis),
      chain,
      mev_protect,
      from,
      interact_to,
      call_data,
      value,
      auth_list,
   )
   .await?;

   // update balances
   RT.spawn(async move {
      let manager = ctx.balance_manager();
      match manager
         .update_tokens_balance(ctx.clone(), chain.id(), from, vec![weth], true)
         .await
      {
         Ok(_) => {}
         Err(e) => tracing::error!("Error updating weth balance: {:?}", e),
      }

      match manager.update_eth_balance(ctx.clone(), chain.id(), vec![from], true).await {
         Ok(_) => {}
         Err(e) => tracing::error!("Error updating eth balance: {:?}", e),
      }

      ctx.save_balance_manager();
   });

   Ok(())
}

pub async fn unwrap_weth(
   ctx: ZeusCtx,
   from: Address,
   chain: ChainId,
   amount: NumericValue,
) -> Result<(), anyhow::Error> {
   let client = ctx.get_zeus_client();

   let eth_balance_before_fut = client.request(chain.id(), |client| async move {
      client
         .get_balance(from)
         .block_id(BlockId::latest())
         .await
         .map_err(|e| anyhow!("{:?}", e))
   });

   let block = client
      .request(chain.id(), |client| async move {
         client.get_block(BlockId::latest()).await.map_err(|e| anyhow!("{:?}", e))
      })
      .await?;

   let block = if let Some(block) = block {
      block
   } else {
      return Err(anyhow!(
         "No block found, this is usally a provider issue"
      ));
   };

   let block_id = BlockId::number(block.header.number);
   let weth = ERC20Token::wrapped_native_token(chain.id());

   let call_data = weth.encode_withdraw(amount.wei());
   let interact_to = weth.address;
   let value = U256::ZERO;

   let mut accounts = Vec::new();
   accounts.push(from);
   accounts.push(interact_to);
   accounts.push(block.header.beneficiary);
   accounts.push(weth.address);

   let accounts_info = fetch_accounts_info(ctx.clone(), chain.id(), block_id, accounts).await;

   let fork_client = ctx.get_client(chain.id()).await?;
   let mut factory =
      ForkFactory::new_sandbox_factory(fork_client, chain.id(), None, Some(block_id));

   for info in accounts_info {
      factory.insert_account_info(info.address, info.info);
   }

   let fork_db = factory.new_sandbox_fork();

   let eth_balance_after;
   let sim_res;
   {
      let mut evm = new_evm(chain, Some(&block), fork_db.clone());

      let time = Instant::now();
      sim_res = simulate_transaction(
         &mut evm,
         from,
         interact_to,
         call_data.clone(),
         value,
         vec![],
      )?;

      tracing::info!(
         "Simulate Transaction took {} ms",
         time.elapsed().as_millis()
      );

      let state = evm.balance(from);
      eth_balance_after = if let Some(state) = state {
         state.data
      } else {
         U256::ZERO
      };
   }

   let logs = sim_res.clone().into_logs();

   let eth_balance_before = eth_balance_before_fut.await?;
   let mut eth_received = None;

   for log in &logs {
      if let Ok(decoded) = abi::weth9::decode_withdraw_log(log) {
         eth_received = Some(decoded.wad);
         break;
      }
   }

   if eth_received.is_none() {
      return Err(anyhow::anyhow!(
         "Failed to decode weth withdraw log"
      ));
   }

   let eth_received = NumericValue::format_wei(eth_received.unwrap(), weth.decimals);
   let wrapped_c: Currency = weth.clone().into();
   let eth_received_usd = ctx.get_currency_value_for_amount(eth_received.f64(), &wrapped_c);

   let contract_interact = Some(true);
   let auth_list = Vec::new();

   let params = UnwrapWETHParams {
      chain: chain.id(),
      src: from,
      weth_unwrapped: amount,
      weth_unwrapped_usd: Some(eth_received_usd.clone()),
      eth_received,
      eth_received_usd: Some(eth_received_usd),
   };

   let mut tx_analysis = TransactionAnalysis::new(
      ctx.clone(),
      chain.id(),
      from,
      interact_to,
      contract_interact,
      call_data.clone(),
      value,
      logs,
      sim_res.gas_used(),
      eth_balance_before,
      eth_balance_after,
      auth_list.clone(),
   )
   .await?;

   let main_event = DecodedEvent::UnwrapWETH(params.clone());
   tx_analysis.set_main_event(main_event);

   let mev_protect = false;

   let (_, _) = send_transaction(
      ctx.clone(),
      "".to_string(),
      Some(tx_analysis),
      chain,
      mev_protect,
      from,
      interact_to,
      call_data,
      value,
      auth_list,
   )
   .await?;

   // update balances
   RT.spawn(async move {
      let manager = ctx.balance_manager();
      match manager
         .update_tokens_balance(ctx.clone(), chain.id(), from, vec![weth], true)
         .await
      {
         Ok(_) => {}
         Err(e) => tracing::error!("Error updating weth balance: {:?}", e),
      }

      match manager.update_eth_balance(ctx.clone(), chain.id(), vec![from], true).await {
         Ok(_) => {}
         Err(e) => tracing::error!("Error updating eth balance: {:?}", e),
      }

      ctx.save_balance_manager();
   });

   Ok(())
}

async fn handle_approve(
   ctx: ZeusCtx,
   chain: ChainId,
   signer_address: Address,
   token: &ERC20Token,
   eth_balance_before: U256,
   block: Block,
   permit_details: &Permit2Details,
   fork_db: ForkDB,
) -> Result<ForkDB, anyhow::Error> {
   let mut new_fork_db = fork_db.clone();

   if permit_details.permit2_needs_approval {
      let approval_logs;
      let approval_gas_used;
      let eth_balance_after;

      let permit2 = address_book::permit2_contract(chain.id())?;

      {
         let mut evm = new_evm(chain, Some(&block), fork_db);
         let time = Instant::now();

         let res = simulate::approve_token(
            &mut evm,
            token.address,
            signer_address,
            permit2,
            U256::MAX,
         )?;

         tracing::info!(
            "Approval simulation took {} ms",
            time.elapsed().as_millis()
         );

         approval_gas_used = res.gas_used();
         approval_logs = res.logs().to_vec();

         let state = evm.balance(signer_address);
         eth_balance_after = if let Some(state) = state {
            state.data
         } else {
            U256::ZERO
         };

         new_fork_db = evm.db().clone();
      }

      let call_data = token.encode_approve(permit2, U256::MAX);
      let dapp = "".to_string();
      let interact_to = token.address;
      let value = U256::ZERO;
      let amount = NumericValue::format_wei(U256::MAX, token.decimals);
      let auth_list = Vec::new();
      let contract_interact = Some(true);

      let params = TokenApproveParams {
         token: vec![token.clone()],
         amount: vec![amount],
         amount_usd: vec![None],
         owner: signer_address,
         spender: permit2,
      };

      let mut analysis = TransactionAnalysis::new(
         ctx.clone(),
         chain.id(),
         signer_address,
         interact_to,
         contract_interact,
         call_data.clone(),
         value,
         approval_logs,
         approval_gas_used,
         eth_balance_before,
         eth_balance_after,
         auth_list.clone(),
      )
      .await?;

      let main_event = DecodedEvent::TokenApprove(params.clone());
      analysis.set_main_event(main_event);

      let (receipt, _) = send_transaction(
         ctx.clone(),
         dapp,
         Some(analysis),
         chain,
         false, // mev protect not needed for approval
         signer_address,
         interact_to,
         call_data,
         value,
         auth_list,
      )
      .await?;

      if !receipt.status() {
         return Err(anyhow!("Token Approval Failed"));
      }
   }

   Ok(new_fork_db)
}

/// Execute a swap through the Universal Router
async fn swap_via_ur(
   ctx: ZeusCtx,
   chain: ChainId,
   slippage: f64,
   mev_protect: bool,
   deadline: u64,
   signer_address: Address,
   amount_in: NumericValue,
   currency_in: Currency,
   currency_out: Currency,
   swap_steps: Vec<SwapStep<impl UniswapPool + Clone>>,
) -> Result<(), anyhow::Error> {
   let client = ctx.get_zeus_client();

   let block_fut = client.request(chain.id(), |client| async move {
      client.get_block(BlockId::latest()).await.map_err(|e| anyhow!("{:?}", e))
   });

   let balance_fut = client.request(chain.id(), |client| async move {
      client
         .get_balance(signer_address)
         .block_id(BlockId::latest())
         .await
         .map_err(|e| anyhow!("{:?}", e))
   });

   let token_out = currency_out.to_erc20().into_owned();
   let token_out_balance_fut = client.request(chain.id(), |client| {
      let token = token_out.clone();
      async move { token.balance_of(client.clone(), signer_address, None).await }
   });

   let (block, eth_balance_before) = tokio::try_join!(block_fut, balance_fut)?;

   let block = if let Some(block) = block.as_ref() {
      block
   } else {
      return Err(anyhow!(
         "No block found, this is usally a provider issue"
      ));
   };

   // Prefetch account and storage info

   let router_addr = address_book::universal_router_v2(chain.id())?;
   let permit2_addr = address_book::permit2_contract(chain.id())?;
   let first_pool = &swap_steps.first().unwrap().pool;
   let last_pool = &swap_steps.last().unwrap().pool;
   let burn_addr = address!("0x0000000000000000000000000000000000000001");

   let mut accounts = Vec::new();
   accounts.push(signer_address);
   accounts.push(router_addr);
   accounts.push(permit2_addr);
   accounts.push(block.header.beneficiary);

   if currency_in.is_erc20() {
      accounts.push(currency_in.address());

      if chain.is_base() || chain.is_optimism() {
         accounts.push(burn_addr);
      }
   }

   if currency_in.is_native() && !first_pool.dex_kind().is_v4() {
      accounts.push(currency_in.to_erc20().address)
   }

   if currency_out.is_erc20() {
      accounts.push(currency_out.address());
   }

   if currency_out.is_native() && !last_pool.dex_kind().is_v4() {
      accounts.push(currency_out.to_erc20().address)
   }

   let pools_addr = swap_steps.iter().map(|s| s.pool.address()).collect::<Vec<_>>();
   for pool in pools_addr {
      if !pool.is_zero() {
         accounts.push(pool);
      }
   }

   let block_id = BlockId::number(block.number());

   let accounts_info_fut = fetch_accounts_info(
      ctx.clone(),
      chain.id(),
      block_id,
      accounts.clone(),
   );

   let pools = swap_steps.iter().map(|s| s.pool.clone()).collect::<Vec<_>>();
   let storage_fut = fetch_storage_for_pools(ctx.clone(), chain.id(), block_id, pools);

   let time = Instant::now();

   let accounts_info = accounts_info_fut.await;
   let storage_info = storage_fut.await;

   tracing::info!(
      "Fetched accounts & storage info in {} ms",
      time.elapsed().as_millis()
   );

   let fork_client = ctx.get_client(chain.id()).await?;

   let mut factory =
      ForkFactory::new_sandbox_factory(fork_client, chain.id(), None, Some(block_id));

   for info in accounts_info {
      factory.insert_account_info(info.address, info.info);
   }

   for storage in storage_info {
      let _r = factory.insert_account_storage(storage.address, storage.slot, storage.value);
   }

   // Handle the token approval if needed

   let mut new_fork_db = None;
   let mut permit2_details_opt = None;

   if currency_in.is_erc20() {
      let token = currency_in.to_erc20();
      let fork_db = factory.new_sandbox_fork();

      let permit2_details = Permit2Details::new(
         ctx.clone(),
         chain.id(),
         &token,
         amount_in.wei(),
         signer_address,
         router_addr,
      )
      .await?;

      let new_db = handle_approve(
         ctx.clone(),
         chain,
         signer_address,
         &token,
         eth_balance_before,
         block.clone(),
         &permit2_details,
         fork_db,
      )
      .await?;

      new_fork_db = Some(new_db);
      permit2_details_opt = Some(permit2_details);
   }

   let signer = ctx.get_wallet(signer_address).ok_or(anyhow!("Wallet not found"))?.key;

   // Do a simulation to get the real amount out

   let params = encode_swap(
      ctx.clone(),
      permit2_details_opt.clone(),
      chain.id(),
      swap_steps.clone(),
      SwapType::ExactInput,
      amount_in.wei(),
      U256::ZERO,
      slippage,
      currency_in.clone(),
      currency_out.clone(),
      signer.clone(),
      signer_address,
      deadline,
   )
   .await?;

   let fork_db = new_fork_db.unwrap_or(factory.new_sandbox_fork());

   let token_out_balance_after;
   let eth_balance_after;
   let sim_res;

   {
      let mut evm = new_evm(chain, Some(&block), fork_db);

      let time = Instant::now();

      sim_res = simulate_transaction(
         &mut evm,
         signer_address,
         router_addr,
         params.call_data.clone(),
         params.value,
         vec![],
      )?;

      tracing::info!(
         "Swap Simulation took {} ms",
         time.elapsed().as_millis()
      );

      let state = evm.balance(signer_address);
      eth_balance_after = if let Some(state) = state {
         state.data
      } else {
         U256::ZERO
      };

      token_out_balance_after = if currency_out.is_native() {
         eth_balance_after
      } else {
         let b = simulate::erc20_balance(&mut evm, currency_out.address(), signer_address)?;
         b
      };
   }

   let token_out_balance_before = if currency_out.is_erc20() {
      token_out_balance_fut.await?
   } else {
      eth_balance_before
   };

   // Calculate the real amount out
   let real_amount_out = if token_out_balance_after > token_out_balance_before {
      let amount_out = token_out_balance_after - token_out_balance_before;
      NumericValue::format_wei(amount_out, currency_out.decimals())
   } else {
      return Err(anyhow!("No tokens received from the swap"));
   };

   // Prompt the user to sign a message if needed
   if let Some(permit2_details) = &permit2_details_opt {
      if permit2_details.needs_new_signature {
         let msg = permit2_details.msg.clone().ok_or(anyhow!("No permit message found"))?;
         let _sig = sign_message(
            ctx.clone(),
            "".to_string(),
            chain,
            Some(msg),
            None,
         )
         .await?;
      }
   }

   let amount_out_min = real_amount_out.calc_slippage(slippage, currency_out.decimals());

   // Build the call data again with the real_amount_out and slippage applied
   let execute_params = encode_swap(
      ctx.clone(),
      permit2_details_opt,
      chain.id(),
      swap_steps.clone(),
      SwapType::ExactInput,
      amount_in.wei(),
      amount_out_min.wei(),
      slippage,
      currency_in.clone(),
      currency_out.clone(),
      signer,
      signer_address,
      deadline,
   )
   .await?;

   let amount_in_usd = ctx.get_currency_value_for_amount(amount_in.f64(), &currency_in);
   let received_usd = ctx.get_currency_value_for_amount(real_amount_out.f64(), &currency_out);
   let min_received_usd = ctx.get_currency_value_for_amount(amount_out_min.f64(), &currency_out);

   let swap_params = SwapParams {
      dapp: Dapp::Uniswap,
      input_currency: currency_in.clone(),
      output_currency: currency_out.clone(),
      amount_in: amount_in.clone(),
      amount_in_usd: Some(amount_in_usd),
      received: real_amount_out,
      received_usd: Some(received_usd),
      min_received: Some(amount_out_min),
      min_received_usd: Some(min_received_usd),
      sender: signer_address,
      recipient: Some(signer_address),
   };

   let contract_interact = Some(true);
   let logs = sim_res.logs().to_vec();
   let gas_used = sim_res.gas_used();
   let auth_list = Vec::new();

   let mut swap_tx_analysis = TransactionAnalysis::new(
      ctx.clone(),
      chain.id(),
      signer_address,
      router_addr,
      contract_interact,
      execute_params.call_data.clone(),
      execute_params.value,
      logs,
      gas_used,
      eth_balance_before,
      eth_balance_after,
      auth_list,
   )
   .await?;

   let main_event = DecodedEvent::SwapToken(swap_params.clone());
   swap_tx_analysis.set_main_event(main_event);

   // Now we can proceed with the swap
   let call_data = execute_params.call_data.clone();
   let value = execute_params.value;
   let dapp = "".to_string();
   let auth_list = Vec::new();

   let (_, _) = send_transaction(
      ctx.clone(),
      dapp,
      Some(swap_tx_analysis),
      chain,
      mev_protect,
      signer_address,
      router_addr,
      call_data,
      value,
      auth_list,
   )
   .await?;

   let mut tokens = Vec::new();

   if currency_in.is_erc20() {
      tokens.push(currency_in.to_erc20().into_owned());
   }

   if currency_out.is_erc20() {
      tokens.push(currency_out.to_erc20().into_owned());
   }

   // Update balances
   RT.spawn(async move {
      let manager = ctx.balance_manager();
      match manager
         .update_tokens_balance(
            ctx.clone(),
            chain.id(),
            signer_address,
            tokens,
            true,
         )
         .await
      {
         Ok(_) => {}
         Err(e) => {
            tracing::error!("Failed to update balances: {}", e);
         }
      }

      match manager
         .update_eth_balance(
            ctx.clone(),
            chain.id(),
            vec![signer_address],
            true,
         )
         .await
      {
         Ok(_) => {}
         Err(e) => {
            tracing::error!("Failed to update ETH balance: {}", e);
         }
      }

      // Update the portfolio
      let mut portfolio = ctx.get_portfolio(chain.id(), signer_address);

      if currency_out.is_erc20() {
         portfolio.add_token(currency_out.to_erc20().into_owned());
         ctx.write(|ctx| ctx.portfolio_db.insert_portfolio(chain.id(), signer_address, portfolio));
      }

      ctx.calculate_portfolio_value(chain.id(), signer_address);
      ctx.save_balance_manager();
      ctx.save_portfolio_db();
   });

   Ok(())
}
