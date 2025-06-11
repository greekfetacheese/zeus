use egui::{
   Align, Align2, Button, Color32, ComboBox, FontId, Frame, Grid, Layout, Margin, Order, RichText,
   ScrollArea, Slider, Spinner, TextEdit, Ui, Vec2, Window, vec2,
};
use zeus_eth::currency::{Currency, ERC20Token, NativeCurrency};
use zeus_eth::types::ChainId;
use zeus_eth::utils::NumericValue;

use super::{Settings, swap::InOrOut};
use crate::assets::icons::Icons;
use crate::core::{
   ZeusCtx,
   utils::{RT, eth, update},
};
use crate::gui::{SHARED_GUI, ui::TokenSelectionWindow};
use egui_theme::{Theme, utils::*};
use egui_widgets::LabelWithImage;
use std::sync::Arc;
use zeus_eth::{
   alloy_primitives::Address,
   amm::{
      AnyUniswapPool, UniswapPool, UniswapV3Pool,
      uniswap::v3::{
         fee_math::*,
         position::{PositionArgs, PositionResult2, simulate_position2},
      },
   },
   types::BlockTime,
};

use std::time::Instant;

/// Time in seconds to wait before updating the pool state again
const POOL_STATE_EXPIRY: u64 = 180;

const SIM_TIP: &str =
   "Simulate this position as if you were holding it for the specified number of days";

const SIM_TIP2: &str = "This does not guarantee that the earnings will be the same at the future but you can get a good idea of the potential earnings";

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ProtocolVersion {
   V3,
}

impl ProtocolVersion {
   pub fn to_str(&self) -> &'static str {
      match self {
         ProtocolVersion::V3 => "V3",
      }
   }

   pub fn all() -> Vec<Self> {
      vec![ProtocolVersion::V3]
   }
}

/// Ui to open a position for a specific pool
pub struct OpenPositionUi {
   pub open: bool,
   pub pair_selection_open: bool,
   pub size: (f32, f32),
   pub currency0: Currency,
   pub currency1: Currency,
   pub protocol_version: ProtocolVersion,
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
   pub sim_result: Option<PositionResult2>,
}

impl OpenPositionUi {
   pub fn new() -> Self {
      let native = Currency::from(NativeCurrency::from_chain_id(1).unwrap());
      let usdc = Currency::from(ERC20Token::usdc());
      Self {
         open: false,
         pair_selection_open: true,
         size: (600.0, 700.0),
         currency0: native,
         currency1: usdc,
         protocol_version: ProtocolVersion::V3,
         selected_pool: None,
         set_price_range_ui: SetPriceRangeUi::new(),
         syncing_pools: false,
         pool_data_syncing: false,
         last_pool_state_updated: None,
         sim_window_open: false,
         sim_window_size: (400.0, 550.0),
         days_back: String::new(),
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

   fn select_version(&mut self, theme: &Theme, ui: &mut Ui) {
      let mut current_version = self.protocol_version;
      let versions = ProtocolVersion::all();
      widget_visuals(
         ui,
         theme.get_widget_visuals(theme.colors.bg_color),
      );

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

   pub fn show(
      &mut self,
      ctx: ZeusCtx,
      theme: &Theme,
      icons: Arc<Icons>,
      token_selection: &mut TokenSelectionWindow,
      settings: &Settings,
      ui: &mut Ui,
   ) {
      if !self.open {
         return;
      }

      ScrollArea::vertical().show(ui, |ui| {
         ui.vertical_centered(|ui| {
            ui.set_width(self.size.0);

            self.pair_selection(
               ctx.clone(),
               theme,
               icons.clone(),
               token_selection,
               ui,
            );

            self
               .set_price_range_ui
               .show(ctx.clone(), theme, icons.clone(), ui);

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

               let text = RichText::new("Simulate").size(theme.text_sizes.large);
               let button = Button::new(text).min_size(vec2(ui.available_width() * 0.5, 45.0));
               

               if ui.add(button).clicked() {
                  let days = self.days_back.parse::<u64>().unwrap_or(0);

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
                  let position_args = self.set_price_range_ui.position_args.clone();
                  let pool: UniswapV3Pool = pool.try_into().unwrap();

                  RT.spawn(async move {
                     SHARED_GUI.write(|gui| {
                        gui.loading_window.open("Wait while magic happens");
                        gui.request_repaint();
                     });

                     let client = ctx.get_client(ctx.chain().id()).await.unwrap();

                     match simulate_position2(client, block_time, position_args, pool, 2).await {
                        Ok(mut result) => {
                           result.swap_debugs.clear();
                           SHARED_GUI.write(|gui| {
                              gui.uniswap.open_position_ui.sim_result = Some(result);
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

                        let earned0 = result.earned0;
                        let earned1 = result.earned1;
                        let earned0_usd = result.earned0_usd;
                        let earned1_usd = result.earned1_usd;
                        let token0 = result.token0.symbol;
                        let token1 = result.token1.symbol;
                        let active_swaps = result.active_swaps;
                        let total_swaps = result.total_swaps;
                        let apr = result.apr;

                        let pool = self.selected_pool.clone().unwrap();
                        let pool: UniswapV3Pool = pool.try_into().unwrap();

                        let lower_tick_text = format!("Lower Tick {}", result.position.tick_lower);
                        let text = RichText::new(lower_tick_text).size(theme.text_sizes.normal);
                        ui.label(text);

                        let upper_tick_text = format!("Upper Tick {}", result.position.tick_upper);
                        let text = RichText::new(upper_tick_text).size(theme.text_sizes.normal);
                        ui.label(text);


                        let text = RichText::new("Total Volume").size(theme.text_sizes.normal);
                        ui.label(text);

                        let volume = result.buy_volume_usd + result.sell_volume_usd;
                        let text =
                           RichText::new(format!("${:.4}", volume)).size(theme.text_sizes.normal);
                        ui.label(text);

                        let text = RichText::new("Total Earned").size(theme.text_sizes.normal);
                        ui.label(text);

                        let token0_earned =
                           format!("{:.4} (${:.4}) {}", earned0, earned0_usd, token0);
                        let token1_earned =
                           format!("{:.4} (${:.4}) {}", earned1, earned1_usd, token1);
                        let text = RichText::new(token0_earned).size(theme.text_sizes.normal);
                        ui.label(text);

                        let text = RichText::new(token1_earned).size(theme.text_sizes.normal);
                        ui.label(text);

                        let text = format!(
                           "Your position was active {} times out of {} total swaps",
                           active_swaps, total_swaps
                        );
                        let text = RichText::new(text).size(theme.text_sizes.normal);
                        ui.label(text);

                        let text = format!("APR: {:.2}%", apr);
                        ui.label(RichText::new(text).size(theme.text_sizes.normal));

                        let token0 = pool.token0();
                        let token1 = pool.token1();

                        let debugs = result.swap_debugs;
                        for (i, swap) in debugs.iter().enumerate() {
                           ui.label(
                              RichText::new("=============================")
                                 .size(theme.text_sizes.normal),
                           );

                           let text =
                              RichText::new(format!("Swap {}", i)).size(theme.text_sizes.normal);
                           ui.label(text);

                           let token_in = swap.swap_data.token_in.clone();
                           let amount_in = swap.swap_data.amount_in;
                           let token_out = swap.swap_data.token_out.clone();
                           let amount_out = swap.swap_data.amount_out;

                           let amount_in = NumericValue::format_wei(amount_in, token_in.decimals);
                           let amount_out =
                              NumericValue::format_wei(amount_out, token_out.decimals);

                           let block = RichText::new(format!("Block: {}", swap.swap_data.block)).size(theme.text_sizes.normal);
                           ui.label(block);

                           let token_in_text =
                              format!("{} {}", token_in.symbol, amount_in.formatted());
                           let text = RichText::new(format!("Amount In: {}", token_in_text))
                              .size(theme.text_sizes.normal);
                           ui.label(text);

                           let token_out_text =
                              format!("{} {}", token_out.symbol, amount_out.formatted());
                           let text = RichText::new(format!("Amount Out: {}", token_out_text))
                              .size(theme.text_sizes.normal);
                           ui.label(text);

                           let total_earned0 = format!(
                              "{} {}",
                              token0.symbol,
                              swap.total_fees_earned0.format_abbreviated()
                           );
                           let text = RichText::new(format!(
                              "Token0 Owned After {}",
                              total_earned0
                           ))
                           .size(theme.text_sizes.normal);
                           ui.label(text);

                           let total_earned1 = format!(
                              "{} {}",
                              token1.symbol,
                              swap.total_fees_earned1.format_abbreviated()
                           );
                           let text = RichText::new(format!(
                              "Token1 Owned After {}",
                              total_earned1
                           ))
                           .size(theme.text_sizes.normal);
                           ui.label(text);

                           let text = RichText::new(format!(
                              "In Range: {}",
                              if swap.in_range { "Yes" } else { "No" }
                           ));
                           ui.label(text);
                        }
                     });
                  });
               }
            });
         });

      self.sim_window_open = open;
   }

   fn simulate_button(&mut self, theme: &Theme, size: Vec2, ui: &mut Ui) {
      let deposit_amounts = self.set_price_range_ui.deposit_amounts.clone();

      let selected_pool = self.set_price_range_ui.selected_pool.is_some();
      let valid_amounts = deposit_amounts.amount0 > 0.0 && deposit_amounts.amount1 > 0.0;
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
      settings: &Settings,
      size: Vec2,
      ui: &mut Ui,
   ) {
      let deposit_amounts = self.set_price_range_ui.deposit_amounts.clone();
      let owner = ctx.current_wallet().address;

      let selected_pool = self.set_price_range_ui.selected_pool.is_some();
      let valid_amounts = deposit_amounts.amount0 > 0.0 && deposit_amounts.amount1 > 0.0;

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
         let token_a = pool.currency0().to_erc20().into_owned();
         let token_b = pool.currency1().to_erc20().into_owned();
         let slippage = settings.slippage.clone();
         let mev_protect = settings.mev_protect;

         let deposit_amounts = self.set_price_range_ui.deposit_amounts.clone();
         let position_args = self.set_price_range_ui.position_args.clone();

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
               token_a,
               token_b,
               deposit_amounts,
               position_args,
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
      let amount_in = &self.set_price_range_ui.deposit_amounts.amount0.to_string();
      let amount = NumericValue::parse_to_wei(amount_in, self.currency0.decimals());
      balance.wei2() >= amount.wei2()
   }

   fn sufficient_balance_b(&self, ctx: ZeusCtx, owner: Address) -> bool {
      let balance = ctx.get_currency_balance(ctx.chain().id(), owner, &self.currency1);
      let amount_in = &self.set_price_range_ui.deposit_amounts.amount1.to_string();
      let amount = NumericValue::parse_to_wei(amount_in, self.currency1.decimals());
      balance.wei2() >= amount.wei2()
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
         self.select_version(theme, ui);
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

      ui.horizontal(|ui| {
         ui.add_space(ui_width * 0.35);
         ui.spacing_mut().item_spacing.x = 20.0;

         let icon0 = icons.currency_icon_x24(&self.currency0);
         let icon1 = icons.currency_icon_x24(&self.currency1);

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

      ui.add_space(10.0);

      let manager = ctx.pool_manager();
      let mut pools = manager.get_pools_from_pair(&self.currency0, &self.currency1);

      if self.protocol_version == ProtocolVersion::V3 {
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
         Grid::new("fee_tier")
            .spacing(vec2(15.0, 0.0))
            .show(ui, |ui| {
               for pool in pools {
                  let selected = self.selected_pool.as_ref() == Some(&pool);

                  let fee = pool.fee().fee_percent();
                  let text = RichText::new(format!("{fee}%")).size(theme.text_sizes.normal);
                  let mut button = Button::new(text);

                  if !selected {
                     button = button.fill(Color32::TRANSPARENT);
                  }

                  if ui.add(button).clicked() {
                     self.selected_pool = Some(pool.clone());
                     self.currency0 = pool.currency0().clone();
                     self.currency1 = pool.currency1().clone();

                     self
                        .set_price_range_ui
                        .set_values(Some(pool.clone()), self.protocol_version.clone());
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
            let _ = update::update_tokens_balance_for_chain(
               ctx_clone.clone(),
               chain_id,
               owner,
               vec![token_a, token_b],
            )
            .await;
            ctx_clone.save_balance_db();
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
   pub protocol_version: ProtocolVersion,
   pub selected_pool: Option<AnyUniswapPool>,
   pub deposit_amount: String,
   pub deposit_amounts: DepositAmounts,
   pub position_args: PositionArgs,

   // Slider values
   pub min_price: f64,
   pub max_price: f64,
   pub min_price_slider_min_value: f64,
   pub min_price_slider_max_value: f64,
   pub max_price_slider_min_value: f64,
   pub max_price_slider_max_value: f64,
   pub price_assumption: f64,
   pub price_assumption_slider_min_value: f64,
   pub price_assumption_slider_max_value: f64,
}

impl SetPriceRangeUi {
   pub fn new() -> Self {
      Self {
         size: (500.0, 500.0),
         protocol_version: ProtocolVersion::V3,
         selected_pool: None,
         deposit_amount: String::new(),
         deposit_amounts: DepositAmounts::default(),
         position_args: PositionArgs::default(),
         min_price: 0.0,
         max_price: 0.0,
         min_price_slider_min_value: 0.0,
         min_price_slider_max_value: 0.0,
         max_price_slider_min_value: 0.0,
         max_price_slider_max_value: 0.0,
         price_assumption: 0.0,
         price_assumption_slider_min_value: 0.0,
         price_assumption_slider_max_value: 0.0,
      }
   }

   pub fn set_values(&mut self, pool: Option<AnyUniswapPool>, version: ProtocolVersion) {
      self.selected_pool = pool;
      self.protocol_version = version;
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
      self.price_assumption = price;
      self.price_assumption_slider_min_value = min_price;
      self.price_assumption_slider_max_value = max_price;
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
      let text = RichText::new("$ Deposit Amount").size(theme.text_sizes.very_large);
      ui.label(text);

      TextEdit::singleline(&mut self.deposit_amount)
         .hint_text("$0")
         .font(FontId::proportional(theme.text_sizes.normal))
         .background_color(theme.colors.text_edit_bg)
         .margin(Margin::same(10))
         .show(ui);

      let deposit_amount = self.deposit_amount.parse::<f64>().unwrap_or(0.0);
      self.position_args.deposit_amount = deposit_amount;

      ui.add_space(20.0);

      let text = RichText::new("Current Price").size(theme.text_sizes.very_large);
      ui.label(text);

      // Price is expressed Token0 in terms of Token1
      // Aka how much Token1 per Token0
      let price = pool.calculate_price(&currency0).unwrap_or(0.0);
      let price_a_usd = ctx.get_currency_price(&currency0);
      let price_b_usd = ctx.get_currency_price(&currency1);

      let text = format!(
         "1 {} = {:.4} {}",
         currency0.symbol(),
         price,
         currency1.symbol(),
      );

      ui.label(RichText::new(text).size(theme.text_sizes.normal));
      ui.add_space(20.0);

      let frame = theme.frame1;

      // Currencies Amount and value
      ui.vertical(|ui| {
         ui.set_max_width(ui.available_width() * 0.8);

         // Currency A
         frame.show(ui, |ui| {
            ui.horizontal(|ui| {
               ui.vertical(|ui| {
                  ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                     let text = RichText::new(currency0.symbol()).size(theme.text_sizes.normal);
                     let icon = icons.currency_icon_x24(&currency0);
                     let label = LabelWithImage::new(text, Some(icon)).image_on_left();
                     ui.add(label);
                  });

                  ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                     let balance = ctx.get_currency_balance(chain.id(), owner, &currency0);
                     let b_text = format!("(Balance: {})", balance.format_abbreviated());
                     let text = RichText::new(b_text).size(theme.text_sizes.small);
                     let label = LabelWithImage::new(text, None);
                     ui.add(label);
                  });
               });

               // Currency A Amount & Value
               ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                  let value = NumericValue::value(self.deposit_amounts.amount0, price_a_usd.f64());
                  let text =
                     RichText::new(format!("(${:.2})", value.f64())).size(theme.text_sizes.normal);
                  ui.label(text);

                  ui.add_space(10.0);

                  let text = RichText::new(format!("{:.2}", self.deposit_amounts.amount0))
                     .size(theme.text_sizes.normal);
                  ui.label(text);
               });
            });
         });

         // Currency B
         frame.show(ui, |ui| {
            ui.horizontal(|ui| {
               ui.vertical(|ui| {
                  ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                     let text = RichText::new(currency1.symbol()).size(theme.text_sizes.normal);
                     let icon = icons.currency_icon_x24(&currency1);
                     let label = LabelWithImage::new(text, Some(icon)).image_on_left();
                     ui.add(label);
                  });

                  ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                     let balance = ctx.get_currency_balance(chain.id(), owner, &currency1);
                     let b_text = format!("(Balance: {})", balance.format_abbreviated());
                     let text = RichText::new(b_text).size(theme.text_sizes.small);
                     let label = LabelWithImage::new(text, None);
                     ui.add(label);
                  });
               });

               // Currency B Amount & Value
               ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                  let value = NumericValue::value(self.deposit_amounts.amount1, price_b_usd.f64());
                  let text =
                     RichText::new(format!("(${:.2})", value.f64())).size(theme.text_sizes.normal);
                  ui.label(text);

                  ui.add_space(10.0);

                  let text = RichText::new(format!("{:.2}", self.deposit_amounts.amount1))
                     .size(theme.text_sizes.normal);
                  ui.label(text);
               });
            });
         });
      });

      ui.add_space(20.0);

      // Price Range

      frame.show(ui, |ui| {
         self.min_price(theme, currency0, currency1, ui);
      });

      ui.add_space(20.0);

      frame.show(ui, |ui| {
         self.max_price(theme, currency0, currency1, ui);
      });

      ui.add_space(20.0);

      // Most active price assumption
      frame.show(ui, |ui| {
         self.price_assumption(theme, currency0, currency1, ui);
      });

      let price0_usd = ctx.get_currency_price(&currency0);
      let price1_usd = ctx.get_currency_price(&currency1);

      let deposit_amount = self.deposit_amount.parse::<f64>().unwrap_or(0.0);
      let deposit_amounts = get_tokens_deposit_amount(
         self.price_assumption,
         self.min_price,
         self.max_price,
         price0_usd.f64(),
         price1_usd.f64(),
         deposit_amount,
      );

      let position_args = PositionArgs::new(
         self.min_price,
         self.max_price,
         self.price_assumption,
         deposit_amount,
      );

      self.position_args = position_args;
      self.deposit_amounts = deposit_amounts;
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

      // Currency 0 per Currency 1
      let text = format!(
         "{} per {}",
         currency0.symbol(),
         currency1.symbol()
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

      // Currency 0 per Currency 1
      let text = format!(
         "{} per {}",
         currency0.symbol(),
         currency1.symbol()
      );
      ui.label(RichText::new(text).size(theme.text_sizes.normal));
   }

   /// Most active price assumption
   fn price_assumption(
      &mut self,
      theme: &Theme,
      currency0: &Currency,
      currency1: &Currency,
      ui: &mut Ui,
   ) {
      ui.set_max_width(ui.available_width() * 0.8);
      let text = RichText::new("Price Assumption").size(theme.text_sizes.normal);
      ui.label(text);

      let range = self.price_assumption_slider_min_value..=self.price_assumption_slider_max_value;
      let slider = Slider::new(&mut self.price_assumption, range).min_decimals(10);
      ui.horizontal(|ui| {
         ui.add_space(ui.available_width() * 0.2);
         ui.add(slider);
      });

      // Currency 0 per Currency 1
      let text = format!(
         "{} per {}",
         currency0.symbol(),
         currency1.symbol()
      );
      ui.label(RichText::new(text).size(theme.text_sizes.normal));
   }
}

impl OpenPositionUi {
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

      let ctx2 = ctx.clone();
      RT.spawn(async move {
         let client = ctx2.get_client(chain_id).await.unwrap();
         let _ = eth::sync_pools_for_tokens(
            ctx2.clone(),
            chain_id,
            vec![token_in, token_out],
            false,
         )
         .await;

         SHARED_GUI.write(|gui| {
            gui.uniswap.open_position_ui.syncing_pools = false;
            gui.uniswap.open_position_ui.pool_data_syncing = true;
         });

         let pools = manager.get_pools_from_pair(&currency_in, &currency_out);
         match manager
            .update_state_for_pools(client, chain_id, pools)
            .await
         {
            Ok(_) => {
               // tracing::info!("Updated pool state for token: {}", token.symbol);
               SHARED_GUI.write(|gui| {
                  gui.uniswap.open_position_ui.last_pool_state_updated = Some(Instant::now());
                  gui.uniswap.open_position_ui.pool_data_syncing = false;
               });
            }
            Err(_e) => {
               // tracing::error!("Error updating pool state: {:?}", e);
               SHARED_GUI.write(|gui| {
                  gui.uniswap.open_position_ui.pool_data_syncing = false;
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
      let ctx2 = ctx.clone();
      RT.spawn(async move {
         let client = ctx2.get_client(chain_id).await.unwrap();
         match manager
            .update_state_for_pools(client, chain_id, pools)
            .await
         {
            Ok(_) => {
               SHARED_GUI.write(|gui| {
                  gui.uniswap.open_position_ui.last_pool_state_updated = Some(Instant::now());
                  gui.uniswap.open_position_ui.pool_data_syncing = false;
               });
            }
            Err(e) => {
               tracing::error!("Error updating pool state: {:?}", e);
               SHARED_GUI.write(|gui| {
                  gui.uniswap.open_position_ui.pool_data_syncing = false;
               });
            }
         }
      });
   }
}
