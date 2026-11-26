use super::UniswapSettingsUi;
use crate::core::ZeusContext;
use crate::gui::ui::common::amount_field::{AmountField, AmountFieldParams};
use crate::gui::ui::common::{
   blocked_announcement, blocked_reason_line, currency_for_speech, name_picker, name_text,
   token_for_speech,
};
use crate::gui::ui::dapps::uniswap::ProtocolVersion;
use crate::gui::ui::token_selection::{InOrOut, TokenSelectionWindow};
use crate::utils::universal_router_v2::SwapType;
use crate::{assets::icons::Icons, gui::SHARED_GUI};
use egui::{
   Align, Grid, Id, Layout, RichText, Role, ScrollArea, Sense, Stroke, Ui, WidgetInfo, vec2,
};

use anyhow::anyhow;
use egui_elements::{Button, ComboBox, Label, Theme, widgets::Window};
use egui_lucide::Lucide;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::{collections::HashSet, time::Instant};
use zeus_eth::alloy_rpc_types::Block;
use zeus_eth::revm::context::ContextTr;

use crate::core::sim::diff::DiffProbe;
use crate::core::{
   AgentEnv, ApproveSimulation, DecodedEvent, DiffWatch, SendTxOptions, SendTxRequest, SwapParams,
   TransactionAnalysis, TransactionRich, UnwrapWETHParams, WrapETHParams, ZeusCtx,
   send_token_approve, send_transaction, sign_message, signature::Permit2Info, types::Dapp,
};
use crate::utils::{
   simulate::*,
   swap_quoter::*,
   universal_router_v2::{SwapRequest, encode_swap, swap_prefetch_accounts},
};

use zeus_eth::{
   alloy_primitives::{Address, Bytes, U256},
   alloy_rpc_types::BlockId,
   amm::uniswap::{AnyUniswapPool, UniswapPool, UniswapV4Pool},
   currency::{Currency, erc20::ERC20Token, native::NativeCurrency},
   revm_utils::{ForkDB, Host, new_evm, simulate},
   types::ChainId,
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
   fn is_wrap(self) -> bool {
      matches!(self, Self::WrapETH)
   }

   fn is_unwrap(self) -> bool {
      matches!(self, Self::UnwrapWETH)
   }

   fn is_wrap_or_unwrap(self) -> bool {
      matches!(self, Self::WrapETH | Self::UnwrapWETH)
   }

   fn is_swap(self) -> bool {
      matches!(self, Self::Swap)
   }
}

#[derive(Clone, Copy, PartialEq, Default)]
struct QuoteKey {
   chain: u64,
   amount_in_wei: U256,
   amount_out_wei: U256,
   quote_out_wei: U256,
   in_address: Address,
   out_address: Address,
   in_is_native: bool,
   out_is_native: bool,
   in_decimals: u8,
   out_decimals: u8,
   in_price_bits: u64,
   out_price_bits: u64,
   slippage_bits: u64,
   show_swap_metrics: bool,
}

#[derive(Clone, Default)]
struct QuoteCache {
   key: QuoteKey,
   amount_in_value: NumericValue,
   amount_out_value: NumericValue,
   min_received: NumericValue,
   price_impact: f64,
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

   pub fn show(
      &mut self,
      ctx: &mut ZeusContext,
      theme: &Theme,
      settings: &UniswapSettingsUi,
      ui: &mut Ui,
   ) {
      let window_frame = theme.window_frame;
      let title_frame = window_frame.stroke(Stroke::NONE);

      Window::new("Simulate")
         .id(Id::unique("swap_ui_simulate_window"))
         .resizable(true)
         .collapsible(true)
         .movable(true)
         .default_pos((1000.0, 70.0))
         .title_frame(title_frame)
         .frame(window_frame)
         .show(ui.ctx(), |ui| {
            ui.vertical(|ui| {
               ui.set_width(self.size.0);
               ui.set_height(self.size.1);
               ui.spacing_mut().item_spacing = vec2(0.0, theme.spacing.md);

               let button_visuals = theme.button_visuals();

               if self.pool_initial.is_none() {
                  let text = RichText::new("No Pool Selected").size(theme.typography.normal);
                  ui.label(text);
                  return;
               }

               ui.vertical_centered(|ui| {
                  ui.spacing_mut().button_padding = theme.button_padding;
                  let text = RichText::new("Reset Pool State").size(theme.typography.normal);
                  let button = Button::new(text).visuals(button_visuals);

                  if ui.add(button).clicked() {
                     self.set_pool_after(None);
                     let pool = self.pool_initial.clone();
                     let settings_clone = settings.clone();
                     crate::runtime::rt().spawn_blocking(move || {
                        SHARED_GUI.write(|gui| {
                           let ctx = gui.ctx.clone();
                           gui.uniswap.swap_ui.pool = pool;
                           gui.uniswap.swap_ui.get_quote(ctx, &settings_clone);
                        });
                     });
                  }

                  ui.label(RichText::new("Pool Info").size(theme.typography.normal));
               });

               let pool = self.pool_initial.as_ref().unwrap();

               ScrollArea::vertical().show(ui, |ui| {
                  // Pair
                  let token0 = pool.currency0();
                  let token1 = pool.currency1();
                  // Named: a pool's two tickers are read out one letter at a time.
                  let pair = format!(
                     "{} - {}",
                     currency_for_speech(token0).name,
                     currency_for_speech(token1).name
                  );
                  let text = RichText::new(pair).size(theme.typography.normal);
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
                  let text = RichText::new(price).size(theme.typography.normal);
                  ui.label(text);

                  // Base USD Price
                  let price = format!(
                     "{} ${}",
                     pool.base_currency().symbol(),
                     base_price.formatted(),
                  );
                  let text = RichText::new(price).size(theme.typography.normal);
                  ui.label(text);

                  Self::pool_balances(ui, theme, pool);

                  let Some(pool_after) = self.pool_after.as_ref() else {
                     return;
                  };

                  ui.vertical_centered(|ui| {
                     ui.label(
                        RichText::new("Pool State after swaps").size(theme.typography.normal),
                     );
                  });

                  let quote_price = pool_after.quote_price(base_price.f64()).unwrap_or_default();
                  let quote_price = NumericValue::currency_price(quote_price);

                  let price = format!(
                     "{} ${}",
                     pool.quote_currency().symbol(),
                     quote_price.formatted(),
                  );
                  ui.label(RichText::new(price).size(theme.typography.normal));

                  // TODO: Actually calculate the token balances for V3
                  Self::pool_balances(ui, theme, pool_after);
               });
            });
         });
   }

   fn pool_balances(ui: &mut Ui, theme: &Theme, pool: &AnyUniswapPool) {
      let token0 = pool.currency0();
      let token1 = pool.currency1();
      let (token0_balance, token1_balance) = pool.pool_balances();

      ui.label(RichText::new("Pool Balances").size(theme.typography.normal));
      ui.label(
         RichText::new(format!(
            "{} {}",
            token0.symbol(),
            token0_balance.abbreviated(),
         ))
         .size(theme.typography.normal),
      );
      ui.label(
         RichText::new(format!(
            "{} {}",
            token1.symbol(),
            token1_balance.abbreviated(),
         ))
         .size(theme.typography.normal),
      );
   }
}

/// A Swap UI for a DEX like Uniswap
pub struct SwapUi {
   open: bool,
   pub size: (f32, f32),
   pub currency_in: Currency,
   pub currency_out: Currency,
   pub amount_in_field: AmountField,
   pub amount_out_field: AmountField,
   /// Last time pool state was updated
   pub last_pool_state_updated: Option<Instant>,
   pub pool_data_syncing: bool,
   pub syncing_pools: bool,
   pub balance_syncing: bool,
   pub sending_tx: bool,
   pub quote: Quote,
   pub protocol_version: ProtocolVersion,

   /// Pool to simulate if simulate mode is on
   pub pool: Option<AnyUniswapPool>,
   pub simulate_window: SimulateWindow,
   /// Cached amount USD / min received / price impact — recomputed only when [QuoteKey] changes
   quote_cache: QuoteCache,

   /// What blocks the swap, in the words the button used to carry. Drawn above the button and said
   /// once when it changes.
   blocked: Option<String>,
   /// The quote sentence last said, so a refresh of the same quote is not said again.
   quote_announced: Option<String>,
   /// Whether the last frame that drew the form had swap steps: none to some is a quote arriving,
   /// some to none is the router answering with no route.
   quote_had_steps: bool,
   /// A quote request is out, so an empty quote this frame is a wait rather than an answer.
   quote_in_flight: bool,
   /// The words this page wants said, drained by the window right after it has drawn.
   notice: Option<String>,

   /// Bumped by every quote request; a result whose id is no longer current is dropped.
   quote_request: u64,
   /// One hook-pool request at a time, so dragging the amount slider cannot queue a call per frame.
   hook_fee_busy: Arc<AtomicBool>,
}

impl SwapUi {
   pub fn new() -> Self {
      let currency = NativeCurrency::from(1);
      let currency_in = Currency::from(currency);
      let currency_out = Currency::from(ERC20Token::wrapped_native_token(1));
      Self {
         open: true,
         size: (450.0, 550.0),
         currency_in,
         currency_out,
         amount_in_field: AmountField::new(),
         amount_out_field: AmountField::new(),
         last_pool_state_updated: None,
         pool_data_syncing: false,
         syncing_pools: false,
         balance_syncing: false,
         sending_tx: false,
         quote: Quote::default(),
         protocol_version: ProtocolVersion::V3,
         pool: None,
         simulate_window: SimulateWindow::new(),
         quote_cache: QuoteCache::default(),
         blocked: None,
         quote_announced: None,
         quote_had_steps: false,
         quote_in_flight: false,
         notice: None,
         quote_request: 0,
         hook_fee_busy: Arc::new(AtomicBool::new(false)),
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
      // A sentence about a form that no longer exists is not news about the next one, and the quote
      // that was announced belonged to the pair the form is no longer on.
      self.notice = None;
      self.blocked = None;
      self.quote_announced = None;
      self.quote_had_steps = false;
   }

   /// The words this page should say, drained by the window right after it is drawn.
   pub fn take_notice(&mut self) -> Option<String> {
      self.notice.take()
   }

   /// Announce why the swap is blocked, once, and only when the reason changes.
   ///
   /// The sentence is drawn beside the button either way; hearing it as well is what tells a reader
   /// who never left the amount field that the button they were heading for is not going to work,
   /// and what to fix.
   fn say_blocked(&mut self, reason: Option<&str>) {
      if let Some(text) = blocked_announcement(self.blocked.as_deref(), reason) {
         self.notice = Some(text);
      }

      self.blocked = reason.map(str::to_string);
   }

   /// Say a quote once, when it lands.
   ///
   /// The steps going from none to some is the arrival, and the sentence names the amount the Buy
   /// field was handed with it. A request that is still out says nothing: an empty quote then is a
   /// wait, not an answer.
   fn say_quote(&mut self) {
      if self.quote_in_flight {
         return;
      }

      let steps = self.quote.swap_steps.len();
      let had_steps = self.quote_had_steps;
      self.quote_had_steps = steps > 0;

      if steps > 0 {
         if had_steps {
            return;
         }

         let amount = self.amount_out_field.amount.trim();
         if amount.is_empty() {
            return;
         }

         let sentence = format!(
            "Quote ready: {} {}.",
            amount,
            self.currency_out.symbol()
         );
         if self.quote_announced.as_deref() != Some(sentence.as_str()) {
            self.notice = Some(sentence.clone());
            self.quote_announced = Some(sentence);
         }

         return;
      }

      // The router answered and there is no route: the quote that was drawn is gone.
      if had_steps && self.action().is_swap() {
         self.notice = Some("No route found for this pair.".to_string());
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
      std::mem::swap(
         &mut self.amount_in_field.amount,
         &mut self.amount_out_field.amount,
      );
      std::mem::swap(
         &mut self.amount_in_field.amount_wei,
         &mut self.amount_out_field.amount_wei,
      );
   }

   fn select_version(&mut self, theme: &Theme, ui: &mut Ui) {
      let current_version = self.protocol_version;
      let versions = ProtocolVersion::all();

      let combo_visuals = theme.combo_box_visuals();
      let label_visuals = theme.label_visuals();

      let selected_text = RichText::new(current_version.as_str()).size(theme.typography.normal);
      let label = Label::new(selected_text, None)
         .visuals(label_visuals)
         .sense(Sense::click())
         .expand(Some(6.0))
         .interactive(true)
         .fill_width(true);

      // The trigger's node is the next widget this ui allocates, and the crate keeps the popup under
      // an id it takes before the list is drawn, so the id is read here first.
      let trigger_id = ui.next_auto_id();

      ComboBox::new("protocol_version", label)
         .width(100.0)
         .visuals(combo_visuals)
         .show_ui(ui, |ui| {
            ui.spacing_mut().item_spacing.y = theme.spacing.sm;

            for version in versions {
               let text = RichText::new(version.as_str()).size(theme.typography.normal);
               let label = Label::new(text, None)
                  .sense(Sense::click())
                  .visuals(label_visuals)
                  .expand(Some(6.0))
                  .interactive(true)
                  .fill_width(true);

               let is_selected = version == current_version;
               let response = ui.add(label);

               // The rows are clickable labels that say nothing about the state they carry: a radio
               // button says which version this one is and whether it is the one in force.
               response.widget_info(|| {
                  WidgetInfo::selected(
                     Role::RadioButton,
                     ui.is_enabled(),
                     is_selected,
                     version.as_str(),
                  )
               });

               if response.clicked() {
                  self.protocol_version = version;
               }
            }
         });

      // The trigger draws the version alone, so a reader landing on it gets a bare "V3": the caption
      // the row shows names it, and it is read after the popup so a version picked this frame is the
      // one named.
      name_picker(
         ui,
         trigger_id,
         "Pool version",
         self.protocol_version.as_str(),
      );
   }

   /// Select the fee tier
   ///
   /// Returns if the fee tier was changed
   fn select_fee_tier(&mut self, theme: &Theme, pools: &[AnyUniswapPool], ui: &mut Ui) -> bool {
      if pools.is_empty() {
         return false;
      }

      if self.pool_data_syncing || self.syncing_pools {
         return false;
      }

      let visuals = theme.button_visuals();

      let mut changed = false;
      ui.horizontal(|ui| {
         let caption = ui.label(RichText::new("Fee Tier").size(theme.typography.normal));
         // The buttons say which tier they are: the caption beside them would be the same words
         // twice.
         ui.ctx().accesskit_node_builder(caption.id, |node| node.set_hidden());

         ui.add_space(10.0);
         Grid::new("swap_ui_fee_tier_select").spacing(vec2(15.0, 0.0)).show(ui, |ui| {
            for pool in pools {
               let selected = self.pool.as_ref() == Some(pool);

               let fee = pool.fee().fee_percent();
               let text = RichText::new(format!("{fee}%")).size(theme.typography.normal);
               let button = Button::new(text).visuals(visuals).selected(selected);
               let name = format!("Fee tier {fee}%");

               let response = ui.add(button);

               // The button draws the number alone, and the fill that shows the choice is invisible
               // to a reader: the node carries both the caption and the selected state.
               response.widget_info(|| {
                  WidgetInfo::selected(Role::Button, ui.is_enabled(), selected, &name)
               });

               if response.clicked() {
                  self.pool = Some(pool.clone());
                  changed = true;
               }
            }

            ui.end_row();
         });
      });
      changed
   }

   pub fn refresh(&mut self, settings: &UniswapSettingsUi) {
      self.update_pool_state(
         settings.swap_on_v2,
         settings.swap_on_v3,
         settings.swap_on_v4,
      );
      self.sync_pools(settings, false);
   }

   pub fn show(
      &mut self,
      ctx: &mut ZeusContext,
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
            settings.swap_on_v2,
            settings.swap_on_v3,
            settings.swap_on_v4,
         );
      }

      let chain_id = ctx.chain.id();
      let owner = ctx.current_wallet_info().address;
      let simulate_mode = settings.simulate_mode;

      if simulate_mode {
         self.simulate_window.show(ctx, theme, settings, ui);
      }

      ui.vertical_centered(|ui| {
         // The frame owns the width now — pinning it here would defeat the width transition.
         ui.set_height(self.size.1);
         ui.spacing_mut().item_spacing = vec2(0.0, theme.spacing.sm);

         if simulate_mode {
            let text =
               RichText::new("You are on Simulate Mode").size(theme.typography.large).strong();
            ui.label(text);

            ui.horizontal(|ui| {
               ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                  self.select_version(theme, ui);
               });
            });

            let manager = ctx.pool_manager.clone();
            let mut pools = manager.get_pools_from_pair(&self.currency_in, &self.currency_out);

            pools.retain(|p| match self.protocol_version {
               ProtocolVersion::V2 => p.dex_kind().is_v2(),
               ProtocolVersion::V3 => p.dex_kind().is_v3(),
               ProtocolVersion::V4 => p.dex_kind().is_v4(),
            });

            // sort pool by the lowest to highest fee
            pools.sort_by_key(|a| a.fee().fee());

            let changed = self.select_fee_tier(theme, &pools, ui);

            if changed {
               self.simulate_window.set_initial_pool(self.pool.clone());
               Self::spawn_get_quote(settings.clone());
            }

            if pools.is_empty() {
               ui.label(RichText::new("No pools found").size(theme.typography.normal));
            }
         }

         // TODO: Show the correct usd values if we are in simulate mode

         // Currency in
         let inner_frame = theme.frame2;
         let mut amount_changed = false;
         let balance = ctx.get_currency_balance(chain_id, owner, &self.currency_in);
         let max_amount = balance.clone();
         let amount_in_value = self.quote_cache.amount_in_value.clone();

         inner_frame.show(ui, |ui| {
            let changed = self.amount_in_field.show(
               AmountFieldParams::new(
                  theme,
                  icons.clone(),
                  &self.currency_in,
                  owner,
                  chain_id,
               )
               .balance(balance)
               .max_amount(max_amount)
               .value(amount_in_value)
               .label("Sell")
               .token_selection(token_selection, Some(InOrOut::In))
               .show_slider(true),
               ui,
            );
            amount_changed = changed;
         });

         // Swap Currencies
         ui.vertical_centered(|ui| {
            ui.spacing_mut().button_padding = vec2(theme.spacing.sm, theme.spacing.sm);

            let icon = Lucide::RefreshCw.size(20.0).color(theme.colors.text).image();
            // A picture of two arrows has no words of its own: the text alternative says what
            // pressing it exchanges.
            let swap_button = Button::image(icon).alt("Swap Sell and Buy tokens");

            let response = ui.add(swap_button);

            if response.clicked() {
               self.swap_currencies();
               Self::spawn_get_quote(settings.clone());
            }
         });

         // Currency out
         let balance = ctx.get_currency_balance(chain_id, owner, &self.currency_out);
         let amount_out_value = self.quote_cache.amount_out_value.clone();

         inner_frame.show(ui, |ui| {
            self.amount_out_field.show(
               AmountFieldParams::new(
                  theme,
                  icons.clone(),
                  &self.currency_out,
                  owner,
                  chain_id,
               )
               .balance(balance)
               .value(amount_out_value)
               .label("Buy")
               .token_selection(token_selection, Some(InOrOut::Out)),
               ui,
            )
         });

         let selected_currency = token_selection.get_selected_currency().cloned();
         let direction = token_selection.get_currency_direction();
         let changed_currency = selected_currency.is_some();
         let should_get_quote = changed_currency || amount_changed;

         if let Some(currency) = selected_currency {
            self.replace_currency(direction, currency.clone());
            self.update_currency_balance(currency);
            token_selection.reset();
         }

         if changed_currency {
            self.sync_pools(settings, true);
         }

         if should_get_quote {
            Self::spawn_get_quote(settings.clone());
         }

         self.refresh_quote_cache(ctx, settings);

         if simulate_mode {
            self.simulate_button(theme, settings, ui);
         } else {
            self.swap_button(ctx, theme, settings, ui);
            self.swap_details(theme, settings, ui);

            // The quote speaks last: when the router answers with no route, that sentence is fuller
            // than the button's own "No Routes Found" and is the one this pass carries.
            self.say_quote();
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

   fn spawn_get_quote(settings: UniswapSettingsUi) {
      crate::runtime::rt().spawn_blocking(move || {
         SHARED_GUI.write(|gui| {
            let ctx = gui.ctx.clone();
            gui.uniswap.swap_ui.get_quote(ctx, &settings);
         });
      });
   }

   fn finish_swap_tx(result: Result<(), anyhow::Error>, reset_confirm: bool) {
      SHARED_GUI.write(|gui| {
         gui.uniswap.swap_ui.sending_tx = false;
         if let Err(e) = result {
            gui.notification.reset();
            gui.loading_window.reset();
            // Only when no flow owns it: a confirmation still waiting on the user
            // belongs to another send, not to this failed swap.
            if reset_confirm && !gui.tx_confirmation_window.is_busy() {
               gui.tx_confirmation_window.reset();
            }
            gui.open_msg_window(format!("Transaction Error: {e}"));
            gui.request_repaint();
         }
      });
   }

   fn simulate_button(&mut self, theme: &Theme, settings: &UniswapSettingsUi, ui: &mut Ui) {
      let visuals = theme.button_visuals();
      let got_pool = self.pool.is_some();
      let enabled = !self.amount_in_field.amount.is_empty() && got_pool;
      let button = Button::new(RichText::new("Simulate").size(theme.typography.large))
         .min_size(vec2(ui.available_width() * 0.8, 45.0))
         .visuals(visuals);

      if ui.add_enabled(enabled, button).clicked()
         && let Some(pool) = &mut self.pool
      {
         let amount_in = NumericValue::parse_to_wei(
            &self.amount_in_field.amount,
            self.currency_in.decimals(),
         );
         pool.simulate_swap_mut(&self.currency_in, amount_in.wei()).unwrap_or_default();
         self.simulate_window.set_pool_after(Some(pool.clone()));
         Self::spawn_get_quote(settings.clone());
      }
   }

   /// What blocks the swap, in exactly the words the button used to draw and exactly the order it
   /// used to prefer them in: the balance was checked last, so it overrides every rule above it.
   fn blocked(&self, ctx: &mut ZeusContext) -> Option<String> {
      let has_entered_amount = !self.amount_in_field.amount.is_empty();
      let has_swap_steps = !self.quote.swap_steps.is_empty();
      let action = self.action();

      if !self.sufficient_balance(ctx) {
         return Some(format!(
            "Insufficient {} Balance",
            self.currency_in.symbol()
         ));
      }

      if has_entered_amount && action.is_swap() && !has_swap_steps {
         return Some("No Routes Found".to_string());
      }

      if !has_entered_amount {
         return Some("Enter Amount".to_string());
      }

      None
   }

   fn swap_button(
      &mut self,
      ctx: &mut ZeusContext,
      theme: &Theme,
      settings: &UniswapSettingsUi,
      ui: &mut Ui,
   ) {
      let sending_tx = self.sending_tx;
      let valid_inputs = self.valid_inputs(ctx);
      let has_swap_steps = !self.quote.swap_steps.is_empty();
      let action = self.action();

      let valid = if action.is_wrap_or_unwrap() {
         valid_inputs && !sending_tx
      } else {
         valid_inputs && has_swap_steps && !sending_tx
      };

      // The button's own words are the action, whatever is blocking it: the words it drew while it
      // could run are the ones that say what pressing it does. It draws the ticker, which a reader
      // would hear letter by letter, so the node is given the token's name.
      let (label, spoken) = if action.is_wrap() {
         (
            format!("Wrap {}", self.currency_in.symbol()),
            format!(
               "Wrap {}",
               currency_for_speech(&self.currency_in).name
            ),
         )
      } else if action.is_unwrap() {
         (
            format!("Unwrap {}", self.currency_in.symbol()),
            format!(
               "Unwrap {}",
               currency_for_speech(&self.currency_in).name
            ),
         )
      } else {
         ("Swap".to_string(), "Swap".to_string())
      };

      // Said once, then drawn directly above the button: a disabled button takes no focus, so a
      // reason that lived in its label could never be reached.
      let blocked = self.blocked(ctx);
      self.say_blocked(blocked.as_deref());

      if let Some(reason) = &blocked {
         blocked_reason_line(ui, theme, reason);
      }

      let visuals = theme.button_visuals();
      let swap_button = Button::new(
         RichText::new(label.clone())
            .size(theme.typography.large)
            .color(theme.colors.text),
      )
      .min_size(vec2(ui.available_width() * 0.8, 45.0))
      .visuals(visuals);

      ui.vertical_centered(|ui| {
         let response = ui.add_enabled(valid, swap_button);

         // The button keeps the name of what it does whatever is blocking it.
         response.widget_info(|| WidgetInfo::labeled(Role::Button, valid, &spoken));

         if response.clicked() {
            self.sending_tx = true;
            self.swap(ctx, settings);
         }
      });
   }

   fn valid_inputs(&self, ctx: &mut ZeusContext) -> bool {
      self.valid_amounts() && self.sufficient_balance(ctx)
   }

   fn valid_amounts(&self) -> bool {
      let amount_in = self.amount_in_field.amount.parse().unwrap_or(0.0);
      let amount_out = self.amount_out_field.amount.parse().unwrap_or(0.0);
      amount_in > 0.0 && amount_out > 0.0
   }

   fn sufficient_balance(&self, ctx: &mut ZeusContext) -> bool {
      let sender = ctx.current_wallet_info().address;
      let balance = ctx.get_currency_balance(ctx.chain.id(), sender, &self.currency_in);
      let amount = self.amount_in_field.amount_wei;
      balance.wei() >= amount
   }

   fn update_currency_balance(&self, currency: Currency) {
      crate::runtime::rt().spawn(async move {
         let ctx = SHARED_GUI.write(|gui| {
            gui.uniswap.swap_ui.balance_syncing = true;
            gui.ctx.clone()
         });

         let manager = ctx.balance_manager();
         let owner = ctx.current_wallet_info().address;
         let chain = currency.chain_id();

         let result = if currency.is_erc20() {
            let token = currency.to_erc20().into_owned();
            manager
               .update_tokens_balance(ctx.clone(), chain, owner, vec![token], false)
               .await
         } else {
            manager.update_eth_balance(ctx.clone(), chain, vec![owner], false).await
         };

         if let Err(e) = result {
            tracing::error!("Error updating currency balance: {:?}", e);
         }

         SHARED_GUI.write(|gui| {
            gui.uniswap.swap_ui.balance_syncing = false;
         });
      });
   }

   fn sync_pools(&mut self, settings: &UniswapSettingsUi, update_pool_state: bool) {
      if self.syncing_pools {
         return;
      }

      if self.currency_in == self.currency_out || self.action().is_wrap_or_unwrap() {
         return;
      }

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

      crate::runtime::rt().spawn(async move {
         let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
         let pool_manager = ctx.pool_manager();
         let chain_id = ctx.chain().id();

         match pool_manager.discover_pools_for_tokens(ctx.clone(), chain_id, tokens).await {
            Ok(_) => {}
            Err(e) => {
               tracing::error!("Failed to sync pools: {}", e);
            }
         }

         SHARED_GUI.write(|gui| {
            gui.uniswap.swap_ui.syncing_pools = false;
         });

         if update_pool_state {
            SHARED_GUI.write(|gui| {
               gui.uniswap.swap_ui.pool_data_syncing = true;
            });

            let pools = get_relevant_pools(
               ctx.clone(),
               swap_on_v2,
               swap_on_v3,
               swap_on_v4,
               &currency_in,
               &currency_out,
            );

            match pool_manager.update_state_for_pools(ctx.clone(), chain_id, pools).await {
               Ok(_) => {
                  SHARED_GUI.write(|gui| {
                     gui.uniswap.swap_ui.last_pool_state_updated = Some(Instant::now());
                     gui.uniswap.swap_ui.pool_data_syncing = false;
                  });
               }
               Err(_e) => {
                  SHARED_GUI.write(|gui| {
                     gui.uniswap.swap_ui.pool_data_syncing = false;
                  });
               }
            }
         }

         crate::runtime::rt().spawn_blocking(move || {
            SHARED_GUI.write(|gui| {
               let settings = &gui.uniswap.settings;
               gui.uniswap.swap_ui.get_quote(ctx.clone(), settings);
            });
         });
      });
   }

   fn should_update_pool_state(&self) -> bool {
      if self.pool_data_syncing || self.syncing_pools {
         return false;
      }

      if self.currency_in == self.currency_out || self.action().is_wrap_or_unwrap() {
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

   pub fn update_pool_state(&mut self, update_v2: bool, update_v3: bool, update_v4: bool) {
      let action = self.action();
      if action.is_wrap_or_unwrap() {
         return;
      }

      let currency_in = self.currency_in.clone();
      let currency_out = self.currency_out.clone();

      crate::runtime::rt().spawn(async move {
         let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());

         let pools = get_relevant_pools(
            ctx.clone(),
            update_v2,
            update_v3,
            update_v4,
            &currency_in,
            &currency_out,
         );

         let chain_id = ctx.chain().id();
         let manager = ctx.pool_manager();

         SHARED_GUI.write(|gui| {
            gui.uniswap.swap_ui.pool_data_syncing = true;
         });

         match manager.update_state_for_pools(ctx.clone(), chain_id, pools).await {
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
            gui.uniswap.swap_ui.get_quote(ctx, settings);
         });
      });
   }

   pub fn get_quote(&mut self, ctx: ZeusCtx, settings: &UniswapSettingsUi) {
      if settings.simulate_mode
         && let Some(pool) = &self.pool
      {
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

      if self.action().is_wrap_or_unwrap() {
         self.amount_out_field.amount = self.amount_in_field.amount.clone();
         self.amount_out_field.amount_wei = self.amount_in_field.amount_wei;
         self.quote = Quote::default();
         return;
      }

      let amount_in = self.amount_in_field.amount_wei;

      if amount_in.is_zero() {
         self.amount_out_field.amount = String::new();
         return;
      }

      // The hook pools are quoted for exactly this amount, before the local quoter runs.
      let amount_in_wei = amount_in;
      let amount_in = NumericValue::format_wei(amount_in, self.currency_in.decimals());

      let currency_in = self.currency_in.clone();
      let currency_out = self.currency_out.clone();
      let chain = ctx.chain().id();

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

      // Every request remembers its place in the queue: a quote that lands after a newer one must not
      // overwrite it.
      self.quote_request = self.quote_request.wrapping_add(1);
      let quote_request = self.quote_request;

      // An empty quote while this is out is a wait, not an answer the page should describe.
      self.quote_in_flight = true;
      let hook_fee_busy = self.hook_fee_busy.clone();

      let ctx_clone = ctx.clone();
      crate::runtime::rt().spawn(async move {
         let pools = get_relevant_pools(
            ctx.clone(),
            swap_on_v2,
            swap_on_v3,
            swap_on_v4,
            &currency_in,
            &currency_out,
         );

         let mut liquid_pools: Vec<AnyUniswapPool> = pools
            .into_iter()
            .filter(|pool| ctx_clone.pool_has_sufficient_liquidity(pool).unwrap_or(false))
            .collect();

         // Hook pools need their hook's cut fitted — or, when it cannot be modelled, the exact amount
         // quoted — before local math can price them.
         Self::prepare_hook_pools(
            ctx.clone(),
            &mut liquid_pools,
            &currency_in,
            amount_in_wei,
            &hook_fee_busy,
         )
         .await;

         let quote = crate::runtime::rt()
            .spawn_blocking(move || {
               if split_routing_enabled {
                  get_quote_with_split_routing(
                     amount_in,
                     currency_in,
                     currency_out,
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
                     amount_in,
                     currency_in,
                     currency_out,
                     liquid_pools,
                     eth_price,
                     currency_out_price,
                     base_fee,
                     priority_fee.wei(),
                     max_hops,
                  )
               }
            })
            .await
            .unwrap_or_default();

         SHARED_GUI.write(|gui| {
            if gui.uniswap.swap_ui.quote_request != quote_request {
               return;
            }

            gui.uniswap.swap_ui.quote_in_flight = false;

            if quote.amount_out.is_zero() {
               gui.uniswap.swap_ui.quote = Quote::default();
               gui.uniswap.swap_ui.amount_out_field.amount = String::new();
            } else {
               gui.uniswap.swap_ui.amount_out_field.amount = quote.amount_out.flatten();
               gui.uniswap.swap_ui.quote = quote;
            }
         });
      });
   }

   /// Get the hook pools ready for the local quoter.
   ///
   /// A pool whose hook cut is known is priced by the same fee-0 local math as any other pool, and the
   /// model lives on the pool — so dragging the amount slider costs no request at all, and the fit
   /// happens once per [`zeus_eth::amm::uniswap::v4::HOOK_FEE_TTL_SECS`].
   ///
   /// A hook that is not a flat cut of the input cannot be modelled; those pools get their exact amount
   /// quoted instead, which is the amount a direct hop asks for.
   pub(crate) async fn prepare_hook_pools(
      ctx: ZeusCtx,
      pools: &mut [AnyUniswapPool],
      currency_in: &Currency,
      amount_in: U256,
      busy: &AtomicBool,
   ) {
      if amount_in.is_zero() {
         return;
      }

      // `true` means the pool's hook is not a flat cut: only the chain can price this trade.
      let mut to_prepare: Vec<(usize, Currency, bool)> = Vec::new();

      for (index, pool) in pools.iter().enumerate() {
         let AnyUniswapPool::V4(v4_pool) = pool else {
            continue;
         };

         if !v4_pool.has_swap_hooks() {
            continue;
         }

         let Some(pool_currency_in) = pool_input_currency(v4_pool, currency_in) else {
            continue;
         };

         let zero_for_one = v4_pool.zero_for_one(&pool_currency_in);
         match v4_pool.hook_fee(zero_for_one) {
            Some(model) if !model.linear => to_prepare.push((index, pool_currency_in, true)),
            _ if !v4_pool.hook_fee_is_stale(zero_for_one) => {}
            _ => to_prepare.push((index, pool_currency_in, false)),
         }
      }

      if to_prepare.is_empty() {
         return;
      }

      // Dragging the amount slider fires a quote per frame. One round trip at a time is enough: the
      // next frame asks again, and an answer that arrives late is not worth queueing for.
      if busy.swap(true, Ordering::AcqRel) {
         return;
      }

      let client = ctx.get_client_manager();
      let chain = currency_in.chain_id();

      for (index, pool_currency_in, exact_only) in to_prepare {
         let AnyUniswapPool::V4(v4_pool) = &pools[index] else {
            continue;
         };

         let requested = v4_pool.clone();
         let pool_id = v4_pool.id();
         let zero_for_one = v4_pool.zero_for_one(&pool_currency_in);

         if !exact_only {
            let fitted = client
               .request(chain, |rpc| {
                  let requested = requested.clone();
                  let pool_currency_in = pool_currency_in.clone();
                  async move { requested.fit_hook_fee(rpc, &pool_currency_in, amount_in).await }
               })
               .await;

            match fitted {
               Ok(model) => {
                  if let AnyUniswapPool::V4(pool) = &mut pools[index] {
                     pool.hook_fees[usize::from(zero_for_one)] = Some(model);
                  }
                  ctx.pool_manager().set_hook_fee(chain, pool_id, zero_for_one, model);
                  continue;
               }
               // Nothing to model: fall back to the exact amount, which is how this pool was priced
               // before the fit existed.
               Err(e) => tracing::debug!(
                  "Could not fit hook fee for V4 pool {}: {e:?}",
                  pool_id
               ),
            }
         }

         let quoted = client
            .request(chain, |rpc| {
               let requested = requested.clone();
               let pool_currency_in = pool_currency_in.clone();
               async move { requested.quote_hook_swap(rpc, &pool_currency_in, amount_in).await }
            })
            .await;

         match quoted {
            Ok(amount_out) if !amount_out.is_zero() => {
               if let AnyUniswapPool::V4(pool) = &mut pools[index] {
                  pool.set_hook_quote(&pool_currency_in, amount_in, amount_out);
               }
            }
            Ok(_) => {}
            Err(e) => {
               tracing::debug!("No on-chain quote for V4 pool {}: {e:?}", pool_id);
            }
         }
      }

      busy.store(false, Ordering::Release);
   }

   fn swap_details(&self, theme: &Theme, settings: &UniswapSettingsUi, ui: &mut Ui) {
      let frame = theme.frame2;
      let text_size = theme.typography.large;

      frame.show(ui, |ui| {
         ui.spacing_mut().item_spacing = vec2(theme.spacing.sm, theme.spacing.sm);

         // Routing
         ui.horizontal(|ui| {
            let text = RichText::new("Routing").size(text_size);
            let info = Lucide::Info.size(20.0).color(theme.colors.text).image();
            let label = Label::new(text, Some(info)).interactive(false);

            let response = ui.add(label);

            // The hops live in a popup only a mouse can open: the count goes on the node's name and
            // the whole route on its description, so the trade can be read without hovering.
            let steps = &self.quote.swap_steps;
            let name = match steps.len() {
               0 => "Routing, no swaps yet".to_string(),
               1 => "Routing, 1 swap".to_string(),
               count => format!("Routing, {count} swaps"),
            };

            name_text(ui, &response, name);

            ui.ctx().accesskit_node_builder(response.id, |node| {
               node.set_description(route_description(steps));
            });

            response.on_hover_ui(|ui| {
               ui.set_width(350.0);
               ui.set_height(100.0);
               ScrollArea::vertical().show(ui, |ui| {
                  let swaps_len = steps.len();
                  let text = format!("Total swaps: {swaps_len}");
                  ui.label(RichText::new(text).size(theme.typography.very_small));
                  ui.add_space(5.0);

                  for step in steps {
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
                     ui.label(RichText::new(text).size(theme.typography.very_small));
                  }
               });
            });
         });

         // Slippage
         ui.horizontal(|ui| {
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
               let caption = ui.label(RichText::new("Slippage").size(text_size));
               // The value's node says both the word and the severity the colour carries: a bare
               // muted number tells a reader neither what it counts nor how bad it is.
               ui.ctx().accesskit_node_builder(caption.id, |node| node.set_hidden());
            });

            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
               let slippage = settings.slippage_f64();

               // The same thresholds the colour has always used: a percentage a reader cannot see is
               // the one that needs the word.
               let (color, severity) = if slippage < 1.0 {
                  (theme.colors.text, "")
               } else if slippage < 2.5 {
                  (theme.colors.warning, " (elevated)")
               } else {
                  (theme.colors.error, " (risky)")
               };

               let value =
                  ui.label(RichText::new(format!("{:.2}%", slippage)).size(text_size).color(color));

               name_text(
                  ui,
                  &value,
                  format!("Slippage {slippage:.2}%{severity}"),
               );
            });
         });

         // Minimum Received
         ui.horizontal(|ui| {
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
               let caption = ui.label(RichText::new("Minimum Received").size(text_size));
               ui.ctx().accesskit_node_builder(caption.id, |node| node.set_hidden());
            });

            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
               if self.quote_cache.key.show_swap_metrics {
                  let floor = format!(
                     "{} {}",
                     self.quote_cache.min_received.formatted(),
                     self.currency_out.symbol()
                  );

                  let value = ui.label(RichText::new(&floor).size(text_size));
                  name_text(ui, &value, format!("Minimum Received {floor}"));
               } else {
                  // An empty row is a row a reader lands on and gets nothing from: the quiet words
                  // say there is no quote yet, and the node says which row they belong to.
                  let text =
                     RichText::new("Not quoted yet").size(text_size).color(theme.colors.text_muted);
                  let value = ui.label(text);

                  name_text(ui, &value, "Minimum Received: not quoted yet");
               }
            });
         });

         // Price Impact
         ui.horizontal(|ui| {
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
               let caption = ui.label(RichText::new("Price Impact").size(text_size));
               ui.ctx().accesskit_node_builder(caption.id, |node| node.set_hidden());
            });

            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
               let price_impact = self.quote_cache.price_impact;

               // The colour says which way the number went: positive is a cost, negative is a gain
               // the pool is handing over.
               let color = if price_impact == 0.0 {
                  theme.colors.text
               } else if price_impact.is_sign_positive() {
                  theme.colors.error
               } else {
                  theme.colors.success
               };

               let value = ui.label(
                  RichText::new(format!("{:.2}%", price_impact)).size(text_size).color(color),
               );

               name_text(
                  ui,
                  &value,
                  format!("Price impact {price_impact:.2}%"),
               );
            });
         });
      });
   }

   fn quote_key(&self, ctx: &ZeusContext, settings: &UniswapSettingsUi) -> QuoteKey {
      let chain = ctx.chain.id();
      QuoteKey {
         chain,
         amount_in_wei: self.amount_in_field.amount_wei,
         amount_out_wei: self.amount_out_field.amount_wei,
         quote_out_wei: self.quote.amount_out.wei(),
         in_address: self.currency_in.address(),
         out_address: self.currency_out.address(),
         in_is_native: self.currency_in.is_native(),
         out_is_native: self.currency_out.is_native(),
         in_decimals: self.currency_in.decimals(),
         out_decimals: self.currency_out.decimals(),
         in_price_bits: ctx.get_currency_price(&self.currency_in).f64().to_bits(),
         out_price_bits: ctx.get_currency_price(&self.currency_out).f64().to_bits(),
         slippage_bits: settings.slippage_f64().to_bits(),
         show_swap_metrics: self.valid_amounts() && self.action().is_swap(),
      }
   }

   fn refresh_quote_cache(&mut self, ctx: &mut ZeusContext, settings: &UniswapSettingsUi) {
      let key = self.quote_key(ctx, settings);
      if key == self.quote_cache.key {
         return;
      }

      let amount_in: f64 = self.amount_in_field.amount.parse().unwrap_or(0.0);
      let amount_in_value = ctx.get_currency_value_for_amount(amount_in, &self.currency_in);
      let amount_out_value = if self.action().is_wrap_or_unwrap() {
         amount_in_value.clone()
      } else {
         let amount_out: f64 = self.amount_out_field.amount.parse().unwrap_or(0.0);
         ctx.get_currency_value_for_amount(amount_out, &self.currency_out)
      };

      let min_received = if key.show_swap_metrics {
         self.quote.amount_out.calc_slippage(
            settings.slippage_f64(),
            self.currency_out.decimals(),
         )
      } else {
         NumericValue::default()
      };

      let price_impact = if key.show_swap_metrics {
         Self::calc_price_impact(amount_in_value.f64(), amount_out_value.f64())
      } else {
         0.0
      };

      self.quote_cache = QuoteCache {
         key,
         amount_in_value,
         amount_out_value,
         min_received,
         price_impact,
      };
   }

   fn calc_price_impact(amount_in_usd: f64, amount_out_usd: f64) -> f64 {
      if !amount_in_usd.is_finite() || amount_in_usd == 0.0 {
         return 0.0;
      }

      let impact = (1.0 - (amount_out_usd / amount_in_usd)) * 100.0;
      if impact.is_finite() { impact } else { 0.0 }
   }

   fn swap(&self, ctx: &mut ZeusContext, settings: &UniswapSettingsUi) {
      let action = self.action();
      let from = ctx.current_wallet_info().address;
      let chain = ctx.chain;

      if action.is_wrap_or_unwrap() {
         let amount_in = NumericValue::format_wei(
            self.amount_in_field.amount_wei,
            self.currency_in.decimals(),
         );
         let wrap = action.is_wrap();
         crate::runtime::rt().spawn(async move {
            let ctx = Self::open_loading();
            let result = if wrap {
               wrap_eth(ctx, AgentEnv::user(), from, chain, amount_in).await
            } else {
               unwrap_weth(ctx, AgentEnv::user(), from, chain, amount_in).await
            };
            Self::finish_swap_tx(result.map(|_| ()), false);
         });
         return;
      }

      let currency_in = self.quote.currency_in.clone();
      let currency_out = self.quote.currency_out.clone();
      let amount_in = self.quote.amount_in.clone();
      let swap_steps = self.quote.swap_steps.clone();

      let mev_protect = settings.mev_protect;
      let deadline = settings.deadline;
      // The sanitised accessor, not the raw field: the value enforced here is the one the
      // settings widget displayed. Never re-parse `settings.slippage` in the swap flow.
      let slippage = settings.slippage_f64();

      crate::runtime::rt().spawn(async move {
         let ctx = Self::open_loading();
         let result = swap_via_ur(
            ctx,
            AgentEnv::user(),
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
         .await;
         Self::finish_swap_tx(result.map(|_| ()), true);
      });
   }

   fn open_loading() -> ZeusCtx {
      SHARED_GUI.write(|gui| {
         gui.loading_window.open("Preparing your transaction…");
         gui.request_repaint();
         gui.ctx.clone()
      })
   }

   /// Stand a quote up without the network: the hops, and the two rows the details pane reads from
   /// the cache. The cache is keyed to the form as it stands so the next draw keeps it instead of
   /// recomputing over it.
   #[cfg(test)]
   pub(crate) fn seed_quote_for_test(
      &mut self,
      ctx: &mut ZeusContext,
      settings: &UniswapSettingsUi,
      steps: Vec<SwapStep<AnyUniswapPool>>,
      min_received: NumericValue,
      price_impact: f64,
   ) {
      let key = QuoteKey {
         show_swap_metrics: true,
         ..self.quote_key(ctx, settings)
      };

      self.quote.swap_steps = steps;
      self.quote_cache = QuoteCache {
         key,
         amount_in_value: self.quote_cache.amount_in_value.clone(),
         amount_out_value: self.quote_cache.amount_out_value.clone(),
         min_received,
         price_impact,
      };
   }

   /// Recompute what blocks the swap and say it, the way the frame does before it draws the button.
   #[cfg(test)]
   pub(crate) fn refresh_blocked_for_test(&mut self, ctx: &mut ZeusContext) {
      let blocked = self.blocked(ctx);
      self.say_blocked(blocked.as_deref());
   }
}

/// The route as one sentence, for the node a reader lands on.
///
/// The popup that draws these lines is opened by a mouse, so the same hops go on the trigger's
/// description: the total first, then one sentence per step in the order the popup lists them.
fn route_description(steps: &[SwapStep<AnyUniswapPool>]) -> String {
   let mut text = format!("Total swaps: {}.", steps.len());

   for step in steps {
      text.push_str(&format!(
         " {} {} to {} {} in the {}/{} pool, fee {} percent",
         step.amount_in.abbreviated(),
         step.currency_in.symbol(),
         step.amount_out.abbreviated(),
         step.currency_out.symbol(),
         step.pool.currency0().symbol(),
         step.pool.currency1().symbol(),
         step.pool.fee().fee_percent()
      ));
   }

   text
}

/// The pool-side currency for a trade currency.
///
/// WETH and native are one asset to the quoter, but the hook and the quoter need the address the pool
/// actually holds.
fn pool_input_currency(pool: &UniswapV4Pool, currency: &Currency) -> Option<Currency> {
   if same_asset(pool.currency0(), currency) {
      Some(pool.currency0().clone())
   } else if same_asset(pool.currency1(), currency) {
      Some(pool.currency1().clone())
   } else {
      None
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

   // If we are swapping from base to a quote token we only include pools
   // that have currency out and base pools
   let is_base_to_quote_swap = currency_in.is_base() && !currency_out.is_base();

   // If we are swapping from a quote token to base we only include pools
   // that have currency in and base pools
   let is_quote_to_base_swap = !currency_in.is_base() && currency_out.is_base();

   // If we are swapping from a quote token to a quote token we only include pools
   // that have currency in or currency out
   let is_quote_to_quote_swap = !currency_in.is_base() && !currency_out.is_base();

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
         } else if is_base_to_quote_swap {
            if pool.have(currency_out) {
               add_pool = true;
            }

            if is_base_pool {
               add_pool = true;
            }
         } else if is_quote_to_base_swap {
            if pool.have(currency_in) {
               add_pool = true;
            }

            // Also include base pools that have the currency out
            if is_base_pool && pool.have(currency_out) {
               add_pool = true;
            }
         } else if is_quote_to_quote_swap && (pool.have(currency_in) || pool.have(currency_out)) {
            add_pool = true;
         }

         if add_pool {
            relevant_pools.push(pool);
            added_pools.insert(pool_key);
         }
      }
   }

   relevant_pools
}

pub async fn wrap_eth(
   ctx: ZeusCtx,
   env: AgentEnv,
   from: Address,
   chain: ChainId,
   amount: NumericValue,
) -> Result<TransactionRich, anyhow::Error> {
   let client = ctx.get_client_manager();

   let (block, block_id) = pinned_head(ctx.clone(), chain, BlockId::latest()).await?;

   let eth_balance_before = native_balance_at(ctx.clone(), chain, from, block_id).await?;

   let weth = ERC20Token::wrapped_native_token(chain.id());

   let weth_balance_before = client
      .request_with(
         chain.id(),
         weth.clone(),
         |client, weth| async move {
            weth
               .balance_of(client, from, Some(block_id))
               .await
               .map_err(|e| anyhow!("{:?}", e))
         },
      )
      .await?;

   let weth_balance_before = NumericValue::format_wei(weth_balance_before, weth.decimals);

   let call_data = weth.encode_deposit();
   let interact_to = weth.address;
   let value = amount.wei();

   let accounts = vec![
      AccountPrefetch::eoa(from),
      AccountPrefetch::contract(interact_to),
      AccountPrefetch::contract(block.header.beneficiary),
   ];

   let (sim, weth_balance_after) = simulate_on_fork_with(
      ctx.clone(),
      chain,
      ForkPrefetch {
         block,
         accounts,
         storage: StoragePrefetch::None,
      },
      ForkSimRequest {
         from,
         interact_to,
         call_data: call_data.clone(),
         value,
         gas_limit: None,
         authorization_list: vec![],
      },
      |evm, _| simulate::erc20_balance(evm, weth.address, from),
   )
   .await?;

   let ForkSim {
      sim_res,
      logs,
      balance_after: eth_balance_after,
      block_number,
      ..
   } = sim;

   let weth_balance_after = NumericValue::format_wei(weth_balance_after?, weth.decimals);

   let weth_received = if weth_balance_after.wei() > weth_balance_before.wei() {
      weth_balance_after.wei() - weth_balance_before.wei()
   } else {
      U256::ZERO
   };

   let weth_received = NumericValue::format_wei(weth_received, weth.decimals);

   if weth_received.wei() < amount.wei() {
      return Err(anyhow!(
         "Received less WETH than requested (expected {}, got {})",
         amount.abbreviated(),
         weth_received.abbreviated()
      ));
   }

   let weth_usd = ctx.get_currency_value_for_amount(amount.f64(), &weth.clone().into());

   let contract_interact = Some(true);
   let auth_list = Vec::new();

   let params = WrapETHParams {
      chain: chain.id(),
      recipient: from,
      eth_wrapped: amount,
      eth_wrapped_usd: Some(weth_usd),
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
      sim_res.tx_gas_used(),
      eth_balance_before,
      eth_balance_after,
      auth_list.clone(),
   )
   .await?;

   let main_event = DecodedEvent::WrapETH(params.clone());
   tx_analysis.set_block_number(block_number);
   tx_analysis.set_main_event(main_event);

   let mev_protect = false;
   let source_is_zeus = true;

   let (_, rich) = send_transaction(
      ctx.clone(),
      source_is_zeus,
      SendTxRequest::new(chain, from, interact_to)
         .call_data(call_data)
         .value(value)
         .authorization_list(auth_list)
         .analysis(tx_analysis),
      SendTxOptions {
         mev_protect,
         priority: env.priority,
         authority: env.authority,
         ..Default::default()
      },
   )
   .await?;

   // update balances
   crate::runtime::rt().spawn(async move {
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

      ctx.update_public_data(chain.id(), from);
   });

   Ok(rich)
}

pub async fn unwrap_weth(
   ctx: ZeusCtx,
   env: AgentEnv,
   from: Address,
   chain: ChainId,
   amount: NumericValue,
) -> Result<TransactionRich, anyhow::Error> {
   let (block, block_id) = pinned_head(ctx.clone(), chain, BlockId::latest()).await?;

   let eth_balance_before = native_balance_at(ctx.clone(), chain, from, block_id).await?;

   let weth = ERC20Token::wrapped_native_token(chain.id());

   let call_data = weth.encode_withdraw(amount.wei());
   let interact_to = weth.address;
   let value = U256::ZERO;

   let accounts = vec![
      AccountPrefetch::eoa(from),
      AccountPrefetch::contract(interact_to),
      AccountPrefetch::contract(block.header.beneficiary),
   ];

   let sim = simulate_on_fork(
      ctx.clone(),
      chain,
      ForkPrefetch {
         block,
         accounts,
         storage: StoragePrefetch::None,
      },
      ForkSimRequest {
         from,
         interact_to,
         call_data: call_data.clone(),
         value,
         gas_limit: None,
         authorization_list: vec![],
      },
   )
   .await?;

   let ForkSim {
      sim_res,
      logs,
      balance_after: eth_balance_after,
      block_number,
      ..
   } = sim;

   let eth_received = if eth_balance_after > eth_balance_before {
      NumericValue::format_wei(
         eth_balance_after - eth_balance_before,
         weth.decimals,
      )
   } else {
      NumericValue::default()
   };

   if eth_received.wei() < amount.wei() {
      return Err(anyhow!(
         "Received less ETH than requested (expected {}, got {})",
         amount.abbreviated(),
         eth_received.abbreviated()
      ));
   }

   let eth_received_usd = ctx.get_token_value_for_amount(eth_received.f64(), &weth);

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
      sim_res.tx_gas_used(),
      eth_balance_before,
      eth_balance_after,
      auth_list.clone(),
   )
   .await?;

   let main_event = DecodedEvent::UnwrapWETH(params.clone());
   tx_analysis.set_block_number(block_number);
   tx_analysis.set_main_event(main_event);

   let mev_protect = false;
   let source_is_zeus = true;

   let (_, rich) = send_transaction(
      ctx.clone(),
      source_is_zeus,
      SendTxRequest::new(chain, from, interact_to)
         .call_data(call_data)
         .value(value)
         .authorization_list(auth_list)
         .analysis(tx_analysis),
      SendTxOptions {
         mev_protect,
         priority: env.priority,
         authority: env.authority,
         ..Default::default()
      },
   )
   .await?;

   // update balances
   crate::runtime::rt().spawn(async move {
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

      ctx.update_public_data(chain.id(), from);
   });

   Ok(rich)
}

async fn handle_approve(
   ctx: ZeusCtx,
   env: AgentEnv,
   chain: ChainId,
   signer_address: Address,
   token: &ERC20Token,
   eth_balance_before: U256,
   block: Block,
   permit_info: &Permit2Info,
   fork_db: ForkDB,
) -> Result<ForkDB, anyhow::Error> {
   let mut new_fork_db = fork_db.clone();

   if permit_info.needs_approval {
      let approval_logs;
      let approval_gas_used;
      let eth_balance_after;

      let permit2 = address_book::permit2_contract(chain.id())?;

      {
         let spec = ctx.node_spec(chain).await;
         let mut evm = new_evm(chain, Some(&block), fork_db, spec);
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

         approval_gas_used = res.tx_gas_used();
         approval_logs = res.logs().to_vec();

         let state = evm.balance(signer_address);
         eth_balance_after = if let Some(state) = state {
            state.data
         } else {
            U256::ZERO
         };

         new_fork_db = evm.db().clone();
      }

      // The approval is a second transaction the swap form never mentions, and it opens its own
      // prompt: say it before that prompt appears, so nobody confirms a fee they were not warned
      // about.
      SHARED_GUI.write(|gui| {
         gui.uniswap.swap_ui.notice = Some(format!(
            "Approving {} first.",
            token_for_speech(&token.name, &token.symbol).name
         ));
      });

      let receipt = send_token_approve(
         ctx.clone(),
         env.clone(),
         chain,
         signer_address,
         token,
         permit2,
         U256::MAX,
         Some(ApproveSimulation {
            logs: approval_logs,
            gas_used: approval_gas_used,
            eth_balance_before,
            eth_balance_after,
            block_number: block.header.number,
         }),
         "",
         false, // mev protect not needed for approval
      )
      .await?;

      if !receipt.status() {
         return Err(anyhow!("Token Approval Failed"));
      }
   }

   Ok(new_fork_db)
}

/// Execute a swap through the Universal Router
pub(crate) async fn swap_via_ur(
   ctx: ZeusCtx,
   env: AgentEnv,
   chain: ChainId,
   slippage: f64,
   mev_protect: bool,
   deadline: u64,
   signer_address: Address,
   amount_in: NumericValue,
   currency_in: Currency,
   currency_out: Currency,
   swap_steps: Vec<SwapStep<AnyUniswapPool>>,
) -> Result<TransactionRich, anyhow::Error> {
   let client = ctx.get_client_manager();

   let (block, block_id) = pinned_head(ctx.clone(), chain, BlockId::latest()).await?;

   let eth_balance_before = native_balance_at(ctx.clone(), chain, signer_address, block_id).await?;

   let token_out = currency_out.to_erc20().into_owned();
   let token_out_balance_fut = client.request_with(
      chain.id(),
      token_out.clone(),
      |client, token| async move {
         token.balance_of(client.clone(), signer_address, Some(block_id)).await
      },
   );

   // Prefetch account and storage info
   let router_addr = address_book::universal_router_v2(chain.id())?;
   let permit2_addr = address_book::permit2_contract(chain.id())?;

   // What the swap moves, read before the fork runs. The calldata does not exist yet — it needs the
   // Permit2 signature and is encoded twice — so the probe starts without it: the pre-state is the
   // portfolio plus the router, and everything the swap touches is added from its logs after the
   // simulation. Without this the analysis reaches the confirmation with no diffs at all.
   // The contracts a swap can move are the ones it names: the two currencies of the trade, and the two
   // allowances `handle_approve` can write below — the token's to Permit2 and Permit2's to the router.
   // Nothing else can reach the signer's balance through Zeus's own calldata.
   let watch = DiffWatch::exact([currency_in.address(), currency_out.address()]).with_approvals([
      (currency_in.address(), permit2_addr),
      (currency_in.address(), router_addr),
   ]);

   let probe = DiffProbe::start(
      ctx.clone(),
      chain,
      block_id,
      signer_address,
      router_addr,
      Bytes::default(),
      &watch,
   );

   let accounts = swap_prefetch_accounts(
      chain,
      signer_address,
      router_addr,
      permit2_addr,
      block.header.beneficiary,
      &currency_in,
      &currency_out,
      &swap_steps,
   );

   let pools = swap_steps.iter().map(|s| s.pool.clone()).collect::<Vec<_>>();

   let factory = prepare_fork(
      ctx.clone(),
      chain,
      &block,
      accounts,
      StoragePrefetch::Pools(pools),
   )
   .await?;

   // Handle the token approval if needed

   let mut new_fork_db = None;
   let mut permit2_info_opt = None;

   if currency_in.is_erc20() {
      let token = currency_in.to_erc20();
      let fork_db = factory.new_sandbox_fork();

      let permit2_info = Permit2Info::new(
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
         env.clone(),
         chain,
         signer_address,
         &token,
         eth_balance_before,
         block.clone(),
         &permit2_info,
         fork_db,
      )
      .await?;

      new_fork_db = Some(new_db);
      permit2_info_opt = Some(permit2_info);
   }

   SHARED_GUI.write(|gui| {
      gui.loading_window.open("Preparing your transaction…");
      gui.request_repaint();
   });

   let signer = ctx.get_wallet(signer_address).ok_or(anyhow!("Wallet not found"))?.key;

   // Do a simulation to get the real amount out

   let params = encode_swap(
      ctx.clone(),
      SwapRequest {
         chain_id: chain.id(),
         currency_in: currency_in.clone(),
         currency_out: currency_out.clone(),
         amount_in: amount_in.wei(),
         // No floor yet: this encode is what the fork simulation below runs,
         // its output is what the real `amount_out_min` is derived from.
         amount_out_min: U256::ZERO,
         slippage,
         swap_type: SwapType::ExactInput,
         steps: swap_steps.clone(),
         signer: signer.clone(),
         recipient: signer_address,
         deadline_minutes: deadline,
         permit2_info: permit2_info_opt.clone(),
      },
   )
   .await?;

   let fork_db = new_fork_db.unwrap_or(factory.new_sandbox_fork());

   // Simulate on the fork the approval was committed into, if there was one, so
   // the swap sees the allowance.
   let (sim, (token_out_balance_after, measured)) = simulate_on_fork_db(
      chain,
      &block,
      fork_db,
      ctx.node_spec(chain).await,
      ForkSimRequest {
         from: signer_address,
         interact_to: router_addr,
         call_data: params.call_data.clone(),
         value: params.value,
         gas_limit: None,
         authorization_list: vec![],
      },
      |evm, sim| {
         let balance = if currency_out.is_native() {
            Ok(sim.balance_after)
         } else {
            simulate::erc20_balance(evm, currency_out.address(), signer_address)
         };

         // The probe reads the post-simulation EVM here: it cannot be handed across the await.
         let measured = probe.measure(ctx.clone(), chain, block_id, &sim.logs, evm);

         (balance, measured)
      },
   )?;

   let ForkSim {
      sim_res,
      logs,
      balance_after: eth_balance_after,
      block_number,
      ..
   } = sim;

   let token_out_balance_after = token_out_balance_after?;

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
   if let Some(permit2_info) = &permit2_info_opt
      && permit2_info.needs_new_signature
   {
      let msg = permit2_info.msg.clone().ok_or(anyhow!("No permit message found"))?;
      let _sig = sign_message(
         ctx.clone(),
         "".to_string(),
         chain,
         Some(msg),
         None,
         None,
      )
      .await?;
      SHARED_GUI.write(|gui| {
         gui.request_repaint();
      });
   }

   SHARED_GUI.write(|gui| {
      gui.loading_window.open("Sending the transaction…");
      gui.request_repaint();
   });

   let amount_out_min = real_amount_out.calc_slippage(slippage, currency_out.decimals());

   // Build the call data again with the real_amount_out and slippage applied
   let execute_params = encode_swap(
      ctx.clone(),
      SwapRequest {
         chain_id: chain.id(),
         currency_in: currency_in.clone(),
         currency_out: currency_out.clone(),
         amount_in: amount_in.wei(),
         amount_out_min: amount_out_min.wei(),
         slippage,
         swap_type: SwapType::ExactInput,
         steps: swap_steps.clone(),
         signer,
         recipient: signer_address,
         deadline_minutes: deadline,
         permit2_info: permit2_info_opt,
      },
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

   let (sim_balance_diff, sim_approval_diff) = measured
      .resolve(
         ctx.clone(),
         chain,
         eth_balance_before,
         eth_balance_after,
      )
      .await?;

   let contract_interact = Some(true);
   let gas_used = sim_res.tx_gas_used();
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

   swap_tx_analysis.set_diffs(sim_balance_diff, sim_approval_diff);

   let main_event = DecodedEvent::SwapToken(swap_params.clone());
   swap_tx_analysis.set_block_number(block_number);
   swap_tx_analysis.set_main_event(main_event);

   // Now we can proceed with the swap
   let call_data = execute_params.call_data.clone();
   let value = execute_params.value;
   let dapp = "".to_string();
   let auth_list = Vec::new();
   let source_is_zeus = true;

   let (_, rich) = send_transaction(
      ctx.clone(),
      source_is_zeus,
      SendTxRequest::new(chain, signer_address, router_addr)
         .call_data(call_data)
         .value(value)
         .authorization_list(auth_list)
         .analysis(swap_tx_analysis),
      SendTxOptions {
         dapp,
         mev_protect,
         priority: env.priority,
         authority: env.authority,
         ..Default::default()
      },
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
   crate::runtime::rt().spawn(async move {
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
         ctx.write_wallet_state(|ws| {
            ws.portfolio_db.insert_portfolio(chain.id(), signer_address, portfolio)
         });
      }

      ctx.update_public_data(chain.id(), signer_address);
   });

   Ok(rich)
}
