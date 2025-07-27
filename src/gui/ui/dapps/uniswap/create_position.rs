use egui::{
   Align, Align2, Button, Color32, ComboBox, FontId, Frame, Grid, Layout, Margin, Order, RichText,
   ScrollArea, Slider, Spinner, TextEdit, Ui, Vec2, Window, vec2,
};
use zeus_eth::currency::{Currency, ERC20Token, NativeCurrency};
use zeus_eth::types::ChainId;
use zeus_eth::utils::NumericValue;

use super::{UniswapSettingsUi, swap::InOrOut};
use crate::assets::icons::Icons;
use crate::core::{
   ZeusCtx,
   utils::{RT, eth},
};
use crate::gui::ui::dapps::uniswap::currencies_amount_and_value;
use crate::gui::{SHARED_GUI, ui::TokenSelectionWindow};
use crate::utils::simulate_position::{PositionResult, SimPositionConfig, simulate_position};
use egui_theme::Theme;

use zeus_eth::{
   alloy_primitives::Address,
   amm::{
      AnyUniswapPool, DexKind, UniswapPool, UniswapV3Pool,
      uniswap::v3::{calculate_liquidity_amounts, calculate_liquidity_needed, get_tick_from_price},
      uniswap_v3_math,
   },
   types::BlockTime,
};

use std::sync::Arc;
use std::time::Instant;

/// Time in seconds to wait before updating the pool state again
const POOL_STATE_EXPIRY: u64 = 180;

const SIM_TIP: &str =
   "Simulate this position as if you were holding it for the specified number of days";

const SIM_TIP2: &str = "This does not guarantee that the earnings will be the same at the future but you can get a good idea of the potential earnings";

/// Ui to create a position
pub struct CreatePositionUi {
   pub open: bool,
   pub pair_selection_open: bool,
   pub size: (f32, f32),
   pub currency0: Currency,
   pub currency1: Currency,
   pub protocol: DexKind,
   pub selected_pool: Option<AnyUniswapPool>,
   pub set_price_range_ui: SetPriceRangeUi,
   pub syncing_pools: bool,
   pub pool_data_syncing: bool,
   pub last_pool_state_updated: Option<Instant>,

   // Simulations Window
   pub sim_window_open: bool,
   pub sim_window_size: (f32, f32),
   /// Days to go back for the [BlockTime]
   pub days_back: String,
   pub skip_simulating_mints: bool,
   pub skip_simulating_burns: bool,
   pub sim_result: Option<PositionResult>,
}

impl CreatePositionUi {
   pub fn new() -> Self {
      let native = Currency::from(NativeCurrency::from_chain_id(1).unwrap());
      let usdc = Currency::from(ERC20Token::usdc());
      Self {
         open: false,
         pair_selection_open: true,
         size: (600.0, 700.0),
         currency0: native,
         currency1: usdc,
         protocol: DexKind::UniswapV3,
         selected_pool: None,
         set_price_range_ui: SetPriceRangeUi::new(),
         syncing_pools: false,
         pool_data_syncing: false,
         last_pool_state_updated: None,
         sim_window_open: false,
         sim_window_size: (400.0, 550.0),
         days_back: String::new(),
         skip_simulating_mints: false,
         skip_simulating_burns: false,
         sim_result: None,
      }
   }

   pub fn replace_currency(&mut self, in_or_out: &InOrOut, currency: Currency) {
      match in_or_out {
         InOrOut::In => {
            self.currency0 = currency;
         }
         InOrOut::Out => {
            self.currency1 = currency;
         }
      }
   }

   pub fn default_currency0(&mut self, id: u64) {
      let native = NativeCurrency::from(id);
      self.currency0 = Currency::from(native);
   }

   pub fn default_currency1(&mut self, id: u64) {
      let chain: ChainId = id.into();
      let currency1 = match chain {
         ChainId::Ethereum(_) => Currency::from(ERC20Token::usdc()),
         ChainId::Optimism(_) => Currency::from(ERC20Token::usdc_optimism()),
         ChainId::Arbitrum(_) => Currency::from(ERC20Token::usdc_arbitrum()),
         ChainId::Base(_) => Currency::from(ERC20Token::usdc_base()),
         ChainId::BinanceSmartChain(_) => Currency::from(ERC20Token::usdc_bsc()),
      };
      self.currency1 = currency1;
   }

   // Only V3 for now
   fn select_version(&mut self, _chain: u64, theme: &Theme, ui: &mut Ui) {
      let mut current_protocol = self.protocol;
      let protocol_kinds = vec![DexKind::UniswapV3];

      let selected_text = RichText::new(current_protocol.as_str()).size(theme.text_sizes.normal);

      ComboBox::from_id_salt("protocol_version")
         .selected_text(selected_text)
         .show_ui(ui, |ui| {
            for protocol in protocol_kinds {
               let text = RichText::new(protocol.as_str()).size(theme.text_sizes.normal);
               ui.selectable_value(&mut current_protocol, protocol, text);
            }
            self.protocol = current_protocol;
         });
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

      ui.vertical_centered(|ui| {
         ui.set_width(self.size.0);
         ScrollArea::vertical().show(ui, |ui| {
            self.pair_selection(
               ctx.clone(),
               theme,
               icons.clone(),
               token_selection,
               ui,
            );

            self.set_price_range_ui.show(ctx.clone(), theme, icons.clone(), ui);

            ui.add_space(20.0);

            // button size
            let size = vec2(ui.available_width() * 0.5, 45.0);

            ui.horizontal(|ui| {
               ui.spacing_mut().item_spacing.x = 20.0;
               ui.set_max_width(ui.available_width() * 0.9);
               self.add_liquidity_button(ctx.clone(), theme, settings, size, ui);
               self.simulate_button(theme, size, ui);
            });
         });
      });

      self.sim_window(ctx.clone(), theme, ui);
   }

   fn sim_window(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      let mut open = self.sim_window_open;

      Window::new("Simulate Position")
         .open(&mut open)
         .resizable(false)
         .order(Order::Foreground)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .collapsible(false)
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_width(self.sim_window_size.0);
            ui.set_height(self.sim_window_size.1);

            ui.spacing_mut().item_spacing.y = 10.0;
            ui.spacing_mut().button_padding = vec2(10.0, 8.0);

            ui.vertical_centered(|ui| {
               let tip1 = RichText::new(SIM_TIP).size(theme.text_sizes.small);
               ui.label(tip1);

               let tip2 = RichText::new(SIM_TIP2).size(theme.text_sizes.small);
               ui.label(tip2);

               ui.add_space(10.0);

               let text = RichText::new("Days to go back").size(theme.text_sizes.normal);
               ui.label(text);

               TextEdit::singleline(&mut self.days_back)
                  .font(FontId::proportional(theme.text_sizes.normal))
                  .background_color(theme.colors.text_edit_bg)
                  .margin(Margin::same(10))
                  .show(ui);

               ui.add_space(20.0);

               ui.checkbox(
                  &mut self.skip_simulating_mints,
                  "Skip simulating Mint Events",
               );
               ui.checkbox(
                  &mut self.skip_simulating_burns,
                  "Skip simulating Burn Events",
               );

               let text = RichText::new("Simulate").size(theme.text_sizes.large);
               let button = Button::new(text).min_size(vec2(ui.available_width() * 0.5, 45.0));

               if ui.add(button).clicked() {
                  let days = self.days_back.parse::<u64>().unwrap_or(0);
                  let mut position_config = self.set_price_range_ui.sim_position_config.clone();
                  position_config.skip_simulating_mints = self.skip_simulating_mints;
                  position_config.skip_simulating_burns = self.skip_simulating_burns;

                  if days == 0 {
                     RT.spawn_blocking(move || {
                        SHARED_GUI.write(|gui| {
                           gui.msg_window.open(
                              "Invalid Days",
                              format!("Days must be greater than 0"),
                           );
                           gui.request_repaint();
                        });
                     });
                     return;
                  }

                  let block_time = BlockTime::Days(days);
                  let pool = self.selected_pool.clone().unwrap();
                  let pool: UniswapV3Pool = pool.try_into().unwrap();

                  RT.spawn(async move {
                     SHARED_GUI.write(|gui| {
                        gui.loading_window.open("Wait while magic happens");
                        gui.request_repaint();
                     });

                     let client = ctx.get_client(ctx.chain().id()).await.unwrap();

                     match simulate_position(client, block_time, position_config, pool).await {
                        Ok(result) => {
                           SHARED_GUI.write(|gui| {
                              gui.uniswap.create_position_ui.sim_result = Some(result);
                              gui.loading_window.reset();
                              gui.request_repaint();
                           });
                        }
                        Err(e) => {
                           SHARED_GUI.write(|gui| {
                              gui.msg_window.open("Error", e.to_string());
                              gui.loading_window.reset();
                              gui.request_repaint();
                           });
                        }
                     }
                  });
               }

               if self.sim_result.is_some() {
                  ScrollArea::vertical().show(ui, |ui| {
                     ui.vertical_centered(|ui| {
                        ui.spacing_mut().item_spacing = vec2(10.0, 10.0);

                        let result = self.sim_result.clone().unwrap();

                        let earned0 = result.token0_earned;
                        let earned1 = result.token1_earned;
                        let earned0_usd = result.earned0_usd;
                        let earned1_usd = result.earned1_usd;
                        let token0 = result.token0.symbol;
                        let token1 = result.token1.symbol;
                        let active_swaps = result.active_swaps;
                        let total_swaps = result.total_swaps;
                        let apr = result.apr;

                        let text = RichText::new(format!("Forked Block: {}", result.forked_block))
                           .size(theme.text_sizes.normal);
                        ui.label(text);

                        let lower_tick_text = format!("Lower Tick {}", result.lower_tick);
                        let text = RichText::new(lower_tick_text).size(theme.text_sizes.normal);
                        ui.label(text);

                        let upper_tick_text = format!("Upper Tick {}", result.upper_tick);
                        let text = RichText::new(upper_tick_text).size(theme.text_sizes.normal);
                        ui.label(text);

                        let amount0_text = format!(
                           "Amount0 in position: {} {}",
                           result.amount0.format_abbreviated(),
                           token0
                        );
                        let text = RichText::new(amount0_text).size(theme.text_sizes.normal);
                        ui.label(text);

                        let amount1_text = format!(
                           "Amount1 in position: {} {}",
                           result.amount1.format_abbreviated(),
                           token1
                        );
                        let text = RichText::new(amount1_text).size(theme.text_sizes.normal);
                        ui.label(text);

                        let text = RichText::new("Total Volume").size(theme.text_sizes.normal);
                        ui.label(text);

                        let text = RichText::new(format!(
                           "${}",
                           result.total_volume_usd.format_abbreviated()
                        ))
                        .size(theme.text_sizes.normal);
                        ui.label(text);

                        let text = RichText::new("Total Earned").size(theme.text_sizes.normal);
                        ui.label(text);

                        let token0_earned = format!(
                           "{} (${}) {}",
                           earned0.format_abbreviated(),
                           earned0_usd.format_abbreviated(),
                           token0
                        );
                        let token1_earned = format!(
                           "{} (${}) {}",
                           earned1.format_abbreviated(),
                           earned1_usd.format_abbreviated(),
                           token1
                        );
                        let text = RichText::new(token0_earned).size(theme.text_sizes.normal);
                        ui.label(text);

                        let text = RichText::new(token1_earned).size(theme.text_sizes.normal);
                        ui.label(text);

                        let text = format!("APR: {:.2}%", apr);
                        ui.label(RichText::new(text).size(theme.text_sizes.normal));

                        let text = format!(
                           "Your position was active {} times out of {} total swaps",
                           active_swaps, total_swaps
                        );
                        let text = RichText::new(text).size(theme.text_sizes.normal);
                        ui.label(text);

                        let text = format!("Failed Swaps: {}", result.failed_swaps);
                        ui.label(RichText::new(text).size(theme.text_sizes.normal));

                        let config = &self.set_price_range_ui.sim_position_config;

                        if !config.skip_simulating_mints {
                           let text = format!("Total Mint Events: {}", result.total_mints);
                           ui.label(RichText::new(text).size(theme.text_sizes.normal));

                           let text = format!("Failed Mint Simulations: {}", result.failed_mints);
                           ui.label(RichText::new(text).size(theme.text_sizes.normal));
                        }

                        if !config.skip_simulating_burns {
                           let text = format!("Total Burn Events: {}", result.total_burns);
                           ui.label(RichText::new(text).size(theme.text_sizes.normal));

                           let text = format!("Failed Burn Simulations: {}", result.failed_burns);
                           ui.label(RichText::new(text).size(theme.text_sizes.normal));
                        }
                     });
                  });
               }
            });
         });

      self.sim_window_open = open;
   }

   fn simulate_button(&mut self, theme: &Theme, size: Vec2, ui: &mut Ui) {
      let amount0_needed = &self.set_price_range_ui.amount0_needed;
      let amount1_needed = &self.set_price_range_ui.amount1_needed;

      let selected_pool = self.set_price_range_ui.selected_pool.is_some();
      let valid_amounts = amount0_needed.f64() > 0.0 && amount1_needed.f64() > 0.0;
      let enabled = selected_pool && valid_amounts;

      let button =
         Button::new(RichText::new("Simulate").size(theme.text_sizes.large)).min_size(size);

      if ui.add_enabled(enabled, button).clicked() {
         self.sim_window_open = true;
      }
   }

   fn add_liquidity_button(
      &mut self,
      ctx: ZeusCtx,
      theme: &Theme,
      settings: &UniswapSettingsUi,
      size: Vec2,
      ui: &mut Ui,
   ) {
      let amount0_needed = &self.set_price_range_ui.amount0_needed;
      let amount1_needed = &self.set_price_range_ui.amount1_needed;
      let owner = ctx.current_wallet().address;

      let selected_pool = self.set_price_range_ui.selected_pool.is_some();
      let valid_amounts = amount0_needed.f64() > 0.0 && amount1_needed.f64() > 0.0;

      let has_balance_a = self.sufficient_balance_a(ctx.clone(), owner);
      let has_balance_b = self.sufficient_balance_b(ctx.clone(), owner);

      let enabled = selected_pool && valid_amounts && has_balance_a && has_balance_b;

      let mut button_text = "Add Liquidity".to_string();

      if !has_balance_a {
         button_text = format!("Insufficient {} Balance", self.currency0.symbol());
      }

      if !has_balance_b {
         button_text = format!("Insufficient {} Balance", self.currency1.symbol());
      }

      if !valid_amounts {
         button_text = "Invalid Amounts".to_string();
      }

      let button =
         Button::new(RichText::new(button_text).size(theme.text_sizes.large)).min_size(size);

      if ui.add_enabled(enabled, button).clicked() {
         let chain = ctx.chain();
         let from = owner;
         let pool = self.selected_pool.clone().unwrap();
         let slippage = settings.slippage.clone();
         let mev_protect = settings.mev_protect;

         let position_args = self.set_price_range_ui.sim_position_config.clone();
         let pool: UniswapV3Pool = pool.try_into().unwrap();

         RT.spawn(async move {
            SHARED_GUI.write(|gui| {
               gui.loading_window.open("Wait while magic happens");
               gui.request_repaint();
            });

            match eth::mint_new_liquidity_position_v3(
               ctx,
               chain,
               from,
               pool,
               position_args.lower_range,
               position_args.upper_range,
               position_args.deposit_amount,
               slippage,
               mev_protect,
            )
            .await
            {
               Ok(_) => {
                  tracing::info!("Minted new liquidity position");
               }
               Err(e) => {
                  tracing::error!("Error minting new liquidity position: {:?}", e);
                  SHARED_GUI.write(|gui| {
                     gui.progress_window.reset();
                     gui.loading_window.reset();
                     gui.msg_window.open("Transaction Error", e.to_string());
                     gui.request_repaint();
                  });
               }
            }
         });
      }
   }

   fn sufficient_balance_a(&self, ctx: ZeusCtx, owner: Address) -> bool {
      let balance = ctx.get_currency_balance(ctx.chain().id(), owner, &self.currency0);
      balance.wei() >= self.set_price_range_ui.amount0_needed.wei()
   }

   fn sufficient_balance_b(&self, ctx: ZeusCtx, owner: Address) -> bool {
      let balance = ctx.get_currency_balance(ctx.chain().id(), owner, &self.currency1);
      balance.wei() >= self.set_price_range_ui.amount1_needed.wei()
   }

   pub fn pair_selection(
      &mut self,
      ctx: ZeusCtx,
      theme: &Theme,
      icons: Arc<Icons>,
      token_selection: &mut TokenSelectionWindow,
      ui: &mut Ui,
   ) {
      let chain_id = ctx.chain().id();
      let owner = ctx.current_wallet().address;
      let currencies = ctx.get_currencies(chain_id);

      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         self.select_version(chain_id, theme, ui);
      });

      ui.spacing_mut().item_spacing.y = 10.0;
      ui.spacing_mut().button_padding = vec2(10.0, 8.0);
      let ui_width = ui.available_width();

      if self.syncing_pools || self.pool_data_syncing {
         ui.add(Spinner::new().size(17.0).color(Color32::WHITE));
      }

      ui.add_space(20.0);

      // Pair Selection
      let text = RichText::new("Select Pair").size(theme.text_sizes.very_large);
      ui.label(text);

      ui.add_space(10.0);

      let size = vec2(ui_width * 0.3, 40.0);
      ui.allocate_ui(size, |ui| {
         ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 20.0;
            ui.spacing_mut().button_padding = vec2(10.0, 8.0);

            let icon0 = icons.currency_icon(&self.currency0);
            let icon1 = icons.currency_icon(&self.currency1);

            let text0 = RichText::new(self.currency0.symbol()).size(theme.text_sizes.normal);
            let text1 = RichText::new(self.currency1.symbol()).size(theme.text_sizes.normal);

            let button0 = Button::image_and_text(icon0, text0).min_size(vec2(100.0, 40.0));
            let button1 = Button::image_and_text(icon1, text1).min_size(vec2(100.0, 40.0));

            if ui.add(button0).clicked() {
               token_selection.currency_direction = InOrOut::In;
               token_selection.open = true;
            }

            if ui.add(button1).clicked() {
               token_selection.currency_direction = InOrOut::Out;
               token_selection.open = true;
            }
         });
      });

      ui.add_space(10.0);

      let manager = ctx.pool_manager();
      let mut pools = manager.get_pools_from_pair(&self.currency0, &self.currency1);

      if self.protocol == DexKind::UniswapV3 {
         pools.retain(|p| p.dex_kind().is_v3());
      }

      // sort pool by the lowest to highest fee
      pools.sort_by(|a, b| a.fee().fee().cmp(&b.fee().fee()));

      // Fee Tier
      let text = RichText::new("Fee Tier").size(theme.text_sizes.very_large);
      ui.label(text);
      ui.add_space(10.0);

      if pools.is_empty() {
         ui.label(RichText::new("No pools found").size(theme.text_sizes.very_large));
      }

      ui.horizontal(|ui| {
         ui.add_space(ui_width * 0.25);
         Grid::new("fee_tier").spacing(vec2(15.0, 0.0)).show(ui, |ui| {
            for pool in pools {
               let selected = self.selected_pool.as_ref() == Some(&pool);
               let current_pool = self.selected_pool.as_ref();

               let fee = pool.fee().fee_percent();
               let text = RichText::new(format!("{fee}%")).size(theme.text_sizes.normal);
               let mut button = Button::new(text);

               if !selected {
                  button = button.fill(Color32::TRANSPARENT);
               }

               let same_pair = if current_pool.is_some() {
                  let current_pool = current_pool.unwrap();
                  pool.have(current_pool.currency0()) && pool.have(current_pool.currency1())
               } else {
                  false
               };

               if ui.add(button).clicked() {
                  self.selected_pool = Some(pool.clone());
                  self.currency0 = pool.currency0().clone();
                  self.currency1 = pool.currency1().clone();

                  // Only reset the price range if we select a different pair
                  if !same_pair {
                     self.set_price_range_ui.set_values(Some(pool.clone()), self.protocol);
                  }
               }
            }

            ui.end_row();
         });
      });

      ui.add_space(20.0);

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
      let changed_currency = selected_currency.is_some();
      let direction = token_selection.get_currency_direction();

      if let Some(currency) = selected_currency {
         self.replace_currency(&direction, currency.clone());
         token_selection.reset();

         // update token balances
         let ctx_clone = ctx.clone();
         let token_a = self.currency0.to_erc20().into_owned();
         let token_b = self.currency1.to_erc20().into_owned();
         RT.spawn(async move {
            let manager = ctx_clone.balance_manager();
            let _ = manager
               .update_tokens_balance(
                  ctx_clone.clone(),
                  chain_id,
                  owner,
                  vec![token_a, token_b],
               )
               .await;
            ctx_clone.save_balance_manager();
         });
      }

      self.sync_pools(ctx.clone(), changed_currency);
      if self.should_update_pool_state() {
         self.update_pool_state(ctx);
      }
   }
}

pub struct SetPriceRangeUi {
   pub size: (f32, f32),
   pub protocol: DexKind,
   pub selected_pool: Option<AnyUniswapPool>,
   /// Deposit amount in Token0
   pub deposit_amount: String,
   pub sim_position_config: SimPositionConfig,

   /// Amount0 needed to mint the position
   pub amount0_needed: NumericValue,
   /// Amount1 needed to mint the position
   pub amount1_needed: NumericValue,

   // Slider values
   pub min_price: f64,
   pub max_price: f64,
   pub min_price_slider_min_value: f64,
   pub min_price_slider_max_value: f64,
   pub max_price_slider_min_value: f64,
   pub max_price_slider_max_value: f64,
}

impl SetPriceRangeUi {
   pub fn new() -> Self {
      Self {
         size: (500.0, 500.0),
         protocol: DexKind::UniswapV3,
         selected_pool: None,
         deposit_amount: String::new(),
         sim_position_config: SimPositionConfig::default(),
         amount0_needed: NumericValue::default(),
         amount1_needed: NumericValue::default(),
         min_price: 0.0,
         max_price: 0.0,
         min_price_slider_min_value: 0.0,
         min_price_slider_max_value: 0.0,
         max_price_slider_min_value: 0.0,
         max_price_slider_max_value: 0.0,
      }
   }

   pub fn set_values(&mut self, pool: Option<AnyUniswapPool>, protocol: DexKind) {
      self.selected_pool = pool;
      self.protocol = protocol;
      self.set_slider_values();
   }

   pub fn set_pool(&mut self, pool: Option<AnyUniswapPool>) {
      self.selected_pool = pool;
   }

   pub fn set_slider_values(&mut self) {
      if self.selected_pool.is_none() {
         return;
      }

      let pool = self.selected_pool.clone().unwrap();
      let price = pool.calculate_price(pool.currency0()).unwrap_or(0.0);
      let stable_pair = pool.currency0().is_stablecoin() && pool.currency1().is_stablecoin();

      // Calculate the min and max possible values for the sliders
      let (min_price, max_price) = if stable_pair {
         let min = price * 0.95; // -5% off the current price
         let max = price * 1.05; // +5% off the current price
         (min, max)
      } else {
         let min = price * 0.01; // -99% off the current price
         let max = price * 2.0; // +100% off the current price
         (min, max)
      };

      self.min_price = min_price;
      self.max_price = max_price;
      self.min_price_slider_min_value = min_price;
      self.min_price_slider_max_value = max_price;
      self.max_price_slider_min_value = price;
      self.max_price_slider_max_value = max_price;
   }

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      ui.spacing_mut().item_spacing.y = 10.0;
      ui.spacing_mut().button_padding = vec2(10.0, 8.0);

      if self.selected_pool.is_none() {
         let text = RichText::new("No selected pool found").size(theme.text_sizes.very_large);
         ui.label(text);
         return;
      }

      let chain = ctx.chain();
      let owner = ctx.current_wallet().address;
      let pool = self.selected_pool.clone().unwrap();
      let currency0 = pool.currency0();
      let currency1 = pool.currency1();

      // Deposit Amount
      let text = format!("Deposit Amount in {}", currency0.symbol());
      let text = RichText::new(text).size(theme.text_sizes.very_large);
      ui.label(text);

      TextEdit::singleline(&mut self.deposit_amount)
         .hint_text("0")
         .font(FontId::proportional(theme.text_sizes.normal))
         .background_color(theme.colors.text_edit_bg)
         .margin(Margin::same(10))
         .show(ui);

      let deposit_amount =
         NumericValue::parse_to_wei(&self.deposit_amount, pool.currency0().decimals());
      self.sim_position_config.deposit_amount = deposit_amount.clone();

      ui.add_space(20.0);

      let text = RichText::new("Current Price").size(theme.text_sizes.very_large);
      ui.label(text);

      // Price is expressed Token0 in terms of Token1
      // Aka how much Token1 per Token0
      let price = pool.calculate_price(&currency0).unwrap_or(0.0);
      let price0_usd = ctx.get_currency_price(&currency0);
      let price1_usd = ctx.get_currency_price(&currency1);

      let state = pool.state().v3_state();
      if state.is_none() {
         let text = RichText::new("Pool State Not Initialized").size(theme.text_sizes.very_large);
         ui.label(text);

         let manager = ctx.pool_manager();
         let pool = manager.get_v3_pool_from_address(chain.id(), pool.address());
         self.selected_pool = pool;
         return;
      }

      let state = state.unwrap();

      let lower_tick = get_tick_from_price(self.sim_position_config.lower_range);
      let upper_tick = get_tick_from_price(self.sim_position_config.upper_range);

      let sqrt_price_lower =
         uniswap_v3_math::tick_math::get_sqrt_ratio_at_tick(lower_tick).unwrap_or_default();
      let sqrt_price_upper =
         uniswap_v3_math::tick_math::get_sqrt_ratio_at_tick(upper_tick).unwrap_or_default();

      // Calculate the liquidity based on the desired amount of token0
      let liquidity = calculate_liquidity_needed(
         state.sqrt_price,
         sqrt_price_lower,
         sqrt_price_upper,
         deposit_amount.wei(),
         true,
      )
      .unwrap_or_default();

      let (amount0, amount1) = calculate_liquidity_amounts(
         state.sqrt_price,
         sqrt_price_lower,
         sqrt_price_upper,
         liquidity,
      )
      .unwrap_or_default();

      let amount0_needed = NumericValue::format_wei(amount0, pool.currency0().decimals());
      let amount1_needed = NumericValue::format_wei(amount1, pool.currency1().decimals());

      let text = format!(
         "1 {} = {:.4} {}",
         currency0.symbol(),
         price,
         currency1.symbol(),
      );

      ui.label(RichText::new(text).size(theme.text_sizes.normal));
      ui.add_space(20.0);

      let size = vec2(ui.available_width() * 0.9, ui.available_height());
      let frame = theme.frame2;
      ui.allocate_ui(size, |ui| {
         currencies_amount_and_value(
            ctx.clone(),
            chain.id(),
            owner,
            currency0,
            currency1,
            &amount0_needed,
            &amount1_needed,
            &price0_usd,
            &price1_usd,
            theme,
            icons.clone(),
            frame,
            ui,
         );
      });

      self.amount0_needed = amount0_needed;
      self.amount1_needed = amount1_needed;

      ui.add_space(20.0);

      let frame = theme.frame1;

      // Price Range

      frame.show(ui, |ui| {
         self.min_price(theme, currency0, currency1, ui);
      });

      ui.add_space(20.0);

      frame.show(ui, |ui| {
         self.max_price(theme, currency0, currency1, ui);
      });

      ui.add_space(20.0);

      self.sim_position_config.lower_range = self.min_price;
      self.sim_position_config.upper_range = self.max_price;
   }

   fn min_price(&mut self, theme: &Theme, currency0: &Currency, currency1: &Currency, ui: &mut Ui) {
      ui.set_max_width(ui.available_width() * 0.8);
      let text = RichText::new("Min Price").size(theme.text_sizes.normal);
      ui.label(text);

      let range = self.min_price_slider_min_value..=self.min_price_slider_max_value;
      let slider = Slider::new(&mut self.min_price, range).min_decimals(10);
      ui.horizontal(|ui| {
         ui.add_space(ui.available_width() * 0.2);
         ui.add(slider);
      });

      // Currency 1 per Currency 0
      let text = format!(
         "{} per {}",
         currency1.symbol(),
         currency0.symbol()
      );
      ui.label(RichText::new(text).size(theme.text_sizes.normal));
   }

   fn max_price(&mut self, theme: &Theme, currency0: &Currency, currency1: &Currency, ui: &mut Ui) {
      ui.set_max_width(ui.available_width() * 0.8);
      let text = RichText::new("Max Price").size(theme.text_sizes.normal);
      ui.label(text);

      let range = self.max_price_slider_min_value..=self.max_price_slider_max_value;
      let slider = Slider::new(&mut self.max_price, range).min_decimals(10);
      ui.horizontal(|ui| {
         ui.add_space(ui.available_width() * 0.2);
         ui.add(slider);
      });

      // Currency 1 per Currency 0
      let text = format!(
         "{} per {}",
         currency1.symbol(),
         currency0.symbol()
      );
      ui.label(RichText::new(text).size(theme.text_sizes.normal));
   }
}

impl CreatePositionUi {
   fn sync_pools(&mut self, ctx: ZeusCtx, changed_currency: bool) {
      if self.syncing_pools {
         return;
      }

      if !changed_currency {
         return;
      }

      // ETH -> WETH
      if self.currency0.is_native() && self.currency1.is_native_wrapped() {
         return;
      }

      let token_in = self.currency0.to_erc20().into_owned();
      let token_out = self.currency1.to_erc20().into_owned();
      tracing::info!(
         "Syncing pools for: {}-{}",
         token_in.symbol,
         token_out.symbol
      );

      let chain_id = ctx.chain().id();
      let manager = ctx.pool_manager();
      let currency_in = self.currency0.clone();
      let currency_out = self.currency1.clone();

      self.syncing_pools = true;

      let ctx_clone = ctx.clone();
      RT.spawn(async move {
         let _ = eth::sync_pools_for_tokens(
            ctx_clone.clone(),
            chain_id,
            vec![token_in, token_out],
            false,
         )
         .await;

         SHARED_GUI.write(|gui| {
            gui.uniswap.create_position_ui.syncing_pools = false;
            gui.uniswap.create_position_ui.pool_data_syncing = true;
         });

         let pools = manager.get_pools_from_pair(&currency_in, &currency_out);
         match manager.update_state_for_pools(ctx_clone, chain_id, pools).await {
            Ok(_) => {
               // tracing::info!("Updated pool state for token: {}", token.symbol);
               SHARED_GUI.write(|gui| {
                  gui.uniswap.create_position_ui.last_pool_state_updated = Some(Instant::now());
                  gui.uniswap.create_position_ui.pool_data_syncing = false;
               });
            }
            Err(_e) => {
               // tracing::error!("Error updating pool state: {:?}", e);
               SHARED_GUI.write(|gui| {
                  gui.uniswap.create_position_ui.pool_data_syncing = false;
               });
            }
         }
      });
   }

   fn should_update_pool_state(&self) -> bool {
      if self.pool_data_syncing || self.syncing_pools {
         return false;
      }

      // ETH -> WETH
      if self.currency0.is_native() && self.currency1.is_native_wrapped() {
         return false;
      }

      // WETH -> WETH
      if self.currency0.is_native_wrapped() && self.currency1.is_native_wrapped() {
         return false;
      }

      if self.currency0 == self.currency1 {
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
      let chain_id = ctx.chain().id();
      let manager = ctx.pool_manager();

      let pools = manager.get_pools_from_pair(&self.currency0, &self.currency1);

      tracing::info!(
         "Updating pool state for{}-{}",
         self.currency0.symbol(),
         self.currency1.symbol()
      );

      self.pool_data_syncing = true;
      let ctx_clone = ctx.clone();
      RT.spawn(async move {
         match manager.update_state_for_pools(ctx_clone, chain_id, pools).await {
            Ok(_) => {
               SHARED_GUI.write(|gui| {
                  gui.uniswap.create_position_ui.last_pool_state_updated = Some(Instant::now());
                  gui.uniswap.create_position_ui.pool_data_syncing = false;
               });
            }
            Err(e) => {
               tracing::error!("Error updating pool state: {:?}", e);
               SHARED_GUI.write(|gui| {
                  gui.uniswap.create_position_ui.pool_data_syncing = false;
               });
            }
         }
      });
   }
}
