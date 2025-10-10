use egui::{
   Align, Align2, Button, Color32, FontId, Frame, Id, Layout, Margin, RichText, ScrollArea,
   Spinner, TextEdit, Ui, Window, vec2,
};
use zeus_eth::{
   abi,
   alloy_primitives::{Address, FixedBytes, U256},
   alloy_provider::Provider,
   alloy_rpc_types::Log,
   alloy_sol_types::SolEvent,
   amm::uniswap::{
      AnyUniswapPool, DexKind, UniswapPool, UniswapV3Pool, nft_position_manager_creation_block,
      uniswap_v3_math,
      v3::{calculate_liquidity_amounts, calculate_liquidity_needed, get_price_from_tick},
   },
   currency::{Currency, ERC20Token},
   types::BlockTime,
   utils::{NumericValue, address_book::uniswap_nft_position_manager, get_logs_for},
};

use crate::{
   assets::icons::Icons, core::db::V3Position, gui::ui::dapps::uniswap::currencies_amount_and_value,
};
use crate::{core::utils::eth, gui::SHARED_GUI};
use crate::{
   core::{ZeusCtx, utils::RT},
   gui::ui::dapps::uniswap::UniswapSettingsUi,
};
use zeus_theme::Theme;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

const POOL_STATE_EXPIRY: u64 = 300;

pub struct CollectFees {
   open: bool,
   pub size: (f32, f32),
   pub position: Option<V3Position>,
}

impl CollectFees {
   pub fn new() -> Self {
      Self {
         open: false,
         size: (400.0, 500.0),
         position: None,
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self, position: Option<V3Position>) {
      self.open = true;
      self.position = position;
   }

   pub fn close(&mut self) {
      self.open = false;
      self.position = None;
   }

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      let mut open = self.open;

      let id = Id::new("collect_fees_window");
      let title = RichText::new("Collect Fees").size(theme.text_sizes.very_large);

      Window::new(title)
         .id(id)
         .open(&mut open)
         .resizable(false)
         .collapsible(false)
         .movable(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.vertical_centered(|ui| {
               ui.set_width(self.size.0);
               ui.set_height(self.size.1);
               ui.spacing_mut().item_spacing = vec2(10.0, 15.0);

               let position = self.position.as_ref();
               if position.is_none() {
                  ui.label(RichText::new("No position selected").size(theme.text_sizes.very_large));
                  return;
               }

               let position = position.unwrap();
               let manager = ctx.pool_manager();
               let chain = ctx.chain();
               let pool = manager.get_v3_pool_from_address(chain.id(), position.pool_address);

               if pool.is_none() {
                  ui.label(RichText::new("Pool not found").size(theme.text_sizes.very_large));
                  return;
               }

               let pool: UniswapV3Pool = pool.unwrap().try_into().unwrap();

               let price0_usd = ctx.get_currency_price(pool.currency0());
               let price1_usd = ctx.get_currency_price(pool.currency1());

               let size = vec2(ui.available_width() * 0.9, ui.available_height());
               let frame = theme.frame2;
               ui.allocate_ui(size, |ui| {
                  currencies_amount_and_value(
                     ctx.clone(),
                     chain.id(),
                     position.owner,
                     pool.currency0(),
                     pool.currency1(),
                     &position.tokens_owed0,
                     &position.tokens_owed1,
                     &price0_usd,
                     &price1_usd,
                     theme,
                     icons.clone(),
                     frame,
                     ui,
                  );
               });

               let button_size = vec2(ui.available_width() * 0.7, 45.0);
               let button = Button::new(RichText::new("Collect").size(theme.text_sizes.large))
                  .min_size(button_size);

               if ui.add(button).clicked() {
                  let ctx_clone = ctx.clone();
                  let owner = position.owner;
                  let position = position.clone();

                  RT.spawn(async move {
                     SHARED_GUI.write(|gui| {
                        gui.loading_window.open("Wait while magic happens");
                        gui.request_repaint();
                     });

                     match eth::collect_fees_position_v3(
                        ctx_clone,
                        chain,
                        owner,
                        position,
                        pool.currency0.clone(),
                        pool.currency1.clone(),
                     )
                     .await
                     {
                        Ok(_) => {
                           tracing::info!("Collected Fees");
                        }
                        Err(e) => {
                           tracing::error!("Error collecting fees: {:?}", e);
                           SHARED_GUI.write(|gui| {
                              gui.notification.reset();
                              gui.loading_window.reset();
                              gui.msg_window.open("Transaction Error", e.to_string());
                              gui.request_repaint();
                           });
                        }
                     }
                  });
               }
            });
         });
      self.open = open;
      if !self.open {
         self.close();
      }
   }
}

pub struct RemoveLiquidity {
   open: bool,
   pub size: (f32, f32),
   pub position: Option<V3Position>,
   pub withdraw_amount: String,
   pub slippage: String,
}

impl RemoveLiquidity {
   pub fn new() -> Self {
      Self {
         open: false,
         size: (400.0, 500.0),
         position: None,
         withdraw_amount: String::new(),
         slippage: String::new(),
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self, position: Option<V3Position>) {
      self.open = true;
      self.position = position;
   }

   pub fn close(&mut self) {
      self.open = false;
      self.position = None;
   }

   pub fn show(
      &mut self,
      ctx: ZeusCtx,
      theme: &Theme,
      icons: Arc<Icons>,
      mev_protect: bool,
      ui: &mut Ui,
   ) {
      let mut open = self.open;

      let id = Id::new("remove_liquidity_window");
      let title = RichText::new("Remove Liquidity").size(theme.text_sizes.large);

      Window::new(title)
         .id(id)
         .open(&mut open)
         .resizable(false)
         .collapsible(false)
         .movable(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.vertical_centered(|ui| {
               ui.set_width(self.size.0);
               ui.set_height(self.size.1);
               ui.spacing_mut().item_spacing = vec2(10.0, 15.0);

               let position = self.position.as_ref();
               if position.is_none() {
                  ui.label(RichText::new("No position selected").size(theme.text_sizes.very_large));
                  return;
               }

               let position = position.unwrap();
               let manager = ctx.pool_manager();
               let chain = ctx.chain();
               let pool = manager.get_v3_pool_from_address(chain.id(), position.pool_address);

               if pool.is_none() {
                  ui.label(RichText::new("Pool not found").size(theme.text_sizes.very_large));
                  return;
               }

               let pool: UniswapV3Pool = pool.unwrap().try_into().unwrap();
               let state = pool.state().v3_state();
               if state.is_none() {
                  ui.label(
                     RichText::new("Pool state not initialized").size(theme.text_sizes.very_large),
                  );
                  return;
               }

               let state = state.unwrap();

               let text = RichText::new("Withdraw Amount (%)").size(theme.text_sizes.very_large);
               ui.label(text);

               TextEdit::singleline(&mut self.withdraw_amount)
                  .hint_text("0%")
                  .font(FontId::proportional(theme.text_sizes.normal))
                  .margin(Margin::same(10))
                  .show(ui);

               let percentage = self.withdraw_amount.parse().unwrap_or(0.0);

               let liquidity_to_remove = if percentage == 100.0 {
                  position.liquidity
               } else {
                  (position.liquidity as f64 * (percentage / 100.0)) as u128
               };

               let sqrt_price_lower =
                  uniswap_v3_math::tick_math::get_sqrt_ratio_at_tick(position.tick_lower)
                     .unwrap_or_default();
               let sqrt_price_upper =
                  uniswap_v3_math::tick_math::get_sqrt_ratio_at_tick(position.tick_upper)
                     .unwrap_or_default();

               let (amount0_to_remove, amount1_to_remove) = calculate_liquidity_amounts(
                  state.sqrt_price,
                  sqrt_price_lower,
                  sqrt_price_upper,
                  liquidity_to_remove,
               )
               .unwrap_or_default();

               let amount0_to_remove =
                  NumericValue::format_wei(amount0_to_remove, pool.token0().decimals);
               let amount1_to_remove =
                  NumericValue::format_wei(amount1_to_remove, pool.token1().decimals);

               let price0_usd = ctx.get_currency_price(pool.currency0());
               let price1_usd = ctx.get_currency_price(pool.currency1());

               let size = vec2(ui.available_width() * 0.9, ui.available_height());
               let frame = theme.frame2;
               ui.allocate_ui(size, |ui| {
                  currencies_amount_and_value(
                     ctx.clone(),
                     chain.id(),
                     position.owner,
                     pool.currency0(),
                     pool.currency1(),
                     &amount0_to_remove,
                     &amount1_to_remove,
                     &price0_usd,
                     &price1_usd,
                     theme,
                     icons.clone(),
                     frame,
                     ui,
                  );
               });

               let text = RichText::new("Slippage").size(theme.text_sizes.normal);
               ui.label(text);

               TextEdit::singleline(&mut self.slippage)
                  .hint_text("0.5")
                  .font(FontId::proportional(theme.text_sizes.normal))
                  .margin(Margin::same(10))
                  .desired_width(25.0)
                  .show(ui);

               let button_size = vec2(ui.available_width() * 0.7, 45.0);
               let button =
                  Button::new(RichText::new("Remove Liquidity").size(theme.text_sizes.large))
                     .min_size(button_size);

               if ui.add(button).clicked() {
                  let chain = ctx.chain();
                  let ctx_clone = ctx.clone();
                  let owner = ctx.current_wallet_address();
                  let position = position.clone();
                  let slippage = self.slippage.clone();

                  RT.spawn(async move {
                     SHARED_GUI.write(|gui| {
                        gui.loading_window.open("Wait while magic happens");
                        gui.request_repaint();
                     });

                     match eth::decrease_liquidity_position_v3(
                        ctx_clone,
                        chain,
                        owner,
                        position,
                        pool,
                        liquidity_to_remove,
                        slippage,
                        mev_protect,
                     )
                     .await
                     {
                        Ok(_) => {
                           tracing::info!("Removed liquidity");
                        }
                        Err(e) => {
                           tracing::error!("Error removing liquidity: {:?}", e);
                           SHARED_GUI.write(|gui| {
                              gui.notification.reset();
                              gui.loading_window.reset();
                              gui.msg_window.open("Transaction Error", e.to_string());
                              gui.request_repaint();
                           });
                        }
                     }
                  });
               }
            });
         });
      self.open = open;
      if !self.open {
         self.close();
      }
   }
}

pub struct AddLiquidity {
   open: bool,
   pub size: (f32, f32),
   pub position: Option<V3Position>,
   pub deposit_amount: String,
   pub slippage: String,
}

impl AddLiquidity {
   pub fn new() -> Self {
      Self {
         open: false,
         size: (400.0, 500.0),
         position: None,
         deposit_amount: String::new(),
         slippage: "0.5".to_string(),
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self, position: Option<V3Position>) {
      self.open = true;
      self.position = position;
   }

   pub fn close(&mut self) {
      self.open = false;
      self.position = None;
      self.deposit_amount = String::new();
   }

   pub fn show(
      &mut self,
      ctx: ZeusCtx,
      theme: &Theme,
      icons: Arc<Icons>,
      mev_protect: bool,
      ui: &mut Ui,
   ) {
      let mut open = self.open;

      let id = Id::new("add_liquidity_window");
      let title = RichText::new("Add Liquidity").size(theme.text_sizes.large);

      Window::new(title)
         .id(id)
         .open(&mut open)
         .resizable(false)
         .collapsible(false)
         .movable(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.vertical_centered(|ui| {
               ui.set_width(self.size.0);
               ui.set_height(self.size.1);
               ui.spacing_mut().item_spacing = vec2(10.0, 15.0);

               let position = self.position.as_ref();
               if position.is_none() {
                  ui.label(RichText::new("No position selected").size(theme.text_sizes.very_large));
                  return;
               }

               let position = position.unwrap();
               let manager = ctx.pool_manager();
               let chain = ctx.chain();
               let pool = manager.get_v3_pool_from_address(chain.id(), position.pool_address);

               if pool.is_none() {
                  ui.label(RichText::new("Pool not found").size(theme.text_sizes.very_large));
                  return;
               }

               let pool = pool.unwrap();
               let state = pool.state().v3_state();
               if state.is_none() {
                  ui.label(
                     RichText::new("Pool state not initialized").size(theme.text_sizes.very_large),
                  );
                  return;
               }

               let state = state.unwrap();

               let token0 = pool.currency0();
               let token1 = pool.currency1();

               let text = format!("Deposit Amount in {}", token0.symbol());
               let text = RichText::new(text).size(theme.text_sizes.very_large);
               ui.label(text);

               TextEdit::singleline(&mut self.deposit_amount)
                  .hint_text("0")
                  .font(FontId::proportional(theme.text_sizes.normal))
                  .margin(Margin::same(10))
                  .show(ui);

               let sqrt_price_lower =
                  uniswap_v3_math::tick_math::get_sqrt_ratio_at_tick(position.tick_lower)
                     .unwrap_or_default();
               // tracing::info!("Sqrt price lower {}", sqrt_price_lower);

               let sqrt_price_upper =
                  uniswap_v3_math::tick_math::get_sqrt_ratio_at_tick(position.tick_upper)
                     .unwrap_or_default();
               // tracing::info!("Sqrt price upper {}", sqrt_price_upper);

               let deposit_amount =
                  NumericValue::parse_to_wei(&self.deposit_amount, token0.decimals());
               // tracing::info!("Deposit amount {} {}", token0.symbol(), deposit_amount.format_abbreviated());
               // tracing::info!("Pool SqrtPrice {}", state.sqrt_price);

               // Calculate the liquidity based on the desired amount of token0
               let liquidity = calculate_liquidity_needed(
                  state.sqrt_price,
                  sqrt_price_lower,
                  sqrt_price_upper,
                  deposit_amount.wei(),
                  true,
               )
               .unwrap_or_default();
               // tracing::info!("Liquidity {}", liquidity);

               let (amount0, amount1) = calculate_liquidity_amounts(
                  state.sqrt_price,
                  sqrt_price_lower,
                  sqrt_price_upper,
                  liquidity,
               )
               .unwrap_or_default();
               // tracing::info!("Amount0 {} Amount1 {}", amount0, amount1);

               let amount0_needed = NumericValue::format_wei(amount0, token0.decimals());
               let amount1_needed = NumericValue::format_wei(amount1, token1.decimals());
               let price0_usd = ctx.get_currency_price(token0);
               let price1_usd = ctx.get_currency_price(token1);

               let size = vec2(ui.available_width() * 0.9, ui.available_height());
               let frame = theme.frame2;
               ui.allocate_ui(size, |ui| {
                  currencies_amount_and_value(
                     ctx.clone(),
                     chain.id(),
                     position.owner,
                     token0,
                     token1,
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

               let text = RichText::new("Slippage").size(theme.text_sizes.normal);
               ui.label(text);

               TextEdit::singleline(&mut self.slippage)
                  .hint_text("0.5")
                  .font(FontId::proportional(theme.text_sizes.normal))
                  .margin(Margin::same(10))
                  .desired_width(ui.available_width() * 0.3)
                  .show(ui);

               let button_size = vec2(ui.available_width() * 0.7, 45.0);
               let button =
                  Button::new(RichText::new("Add Liquidity").size(theme.text_sizes.large))
                     .min_size(button_size);

               if ui.add(button).clicked() {
                  let chain = ctx.chain();
                  let ctx_clone = ctx.clone();
                  let owner = ctx.current_wallet_address();
                  let position = position.clone();
                  let slippage = self.slippage.clone();

                  RT.spawn(async move {
                     SHARED_GUI.write(|gui| {
                        gui.loading_window.open("Wait while magic happens");
                        gui.request_repaint();
                     });

                     match eth::increase_liquidity_position_v3(
                        ctx_clone,
                        chain,
                        owner,
                        position,
                        pool.try_into().unwrap(),
                        deposit_amount,
                        slippage,
                        mev_protect,
                     )
                     .await
                     {
                        Ok(_) => {
                           tracing::info!("Added liquidity");
                        }
                        Err(e) => {
                           tracing::error!("Error adding liquidity: {:?}", e);
                           SHARED_GUI.write(|gui| {
                              gui.notification.reset();
                              gui.loading_window.reset();
                              gui.msg_window.open("Transaction Error", e.to_string());
                              gui.request_repaint();
                           });
                        }
                     }
                  });
               }
            });
         });

      self.open = open;
      if !self.open {
         self.close();
      }
   }
}

// TODO: Add other DEXes

pub struct PositionDetails {
   open: bool,
   pub size: (f32, f32),
   pub position: Option<V3Position>,
   pub pool: Option<AnyUniswapPool>,
}

impl PositionDetails {
   pub fn new() -> Self {
      Self {
         open: false,
         size: (600.0, 300.0),
         position: None,
         pool: None,
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self, position: Option<V3Position>, pool: Option<AnyUniswapPool>) {
      self.open = true;
      self.position = position;
      self.pool = pool;
   }

   pub fn close(&mut self) {
      self.open = false;
      self.position = None;
      self.pool = None;
   }

   pub fn show(&mut self, _ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      let mut open = self.open;

      let id = Id::new("view_positions_ui_position_details_window");
      let title = RichText::new("Position Details").size(theme.text_sizes.heading);

      Window::new(title)
         .id(id)
         .open(&mut open)
         .resizable(false)
         .collapsible(false)
         .movable(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.vertical_centered(|ui| {
               ui.set_width(self.size.0);
               ui.set_height(self.size.1);
               ui.spacing_mut().item_spacing = vec2(10.0, 15.0);

               ui.add_space(20.0);

               let position = self.position.as_ref();
               let pool = self.pool.as_ref();

               if position.is_none() {
                  ui.label(RichText::new("No position selected").size(theme.text_sizes.very_large));
                  return;
               }

               if pool.is_none() {
                  ui.label(
                     RichText::new("Position found, but no pool selected")
                        .size(theme.text_sizes.very_large),
                  );
                  return;
               }

               let position = position.unwrap();
               let pool = pool.unwrap();

               let token0 = pool.currency0();
               let token1 = pool.currency1();

               let min_price = get_price_from_tick(
                  position.tick_lower,
                  token0.decimals(),
                  token1.decimals(),
               );

               let max_price = get_price_from_tick(
                  position.tick_upper,
                  token0.decimals(),
                  token1.decimals(),
               );

               let current_price = pool.calculate_price(token0).unwrap_or_default();

               let frame = theme.frame2;
               let size = vec2(ui.available_width() * 0.5, 40.0);
               let size2 = vec2(ui.available_width() * 0.7, 40.0);

               // First row - Token ID, Lower Tick, Upper Tick
               ui.allocate_ui(size, |ui| {
                  ui.horizontal(|ui| {
                     // Token ID
                     frame.show(ui, |ui| {
                        ui.vertical(|ui| {
                           ui.label(RichText::new("Token ID").size(theme.text_sizes.normal));
                           ui.label(
                              RichText::new(position.id.to_string()).size(theme.text_sizes.normal),
                           );
                        });
                     });

                     // Lower Tick
                     frame.show(ui, |ui| {
                        ui.vertical(|ui| {
                           ui.label(RichText::new("Lower Tick").size(theme.text_sizes.normal));
                           ui.label(
                              RichText::new(position.tick_lower.to_string())
                                 .size(theme.text_sizes.normal),
                           );
                        });
                     });

                     // Upper Tick
                     frame.show(ui, |ui| {
                        ui.vertical(|ui| {
                           ui.label(RichText::new("Upper Tick").size(theme.text_sizes.normal));
                           ui.label(
                              RichText::new(position.tick_upper.to_string())
                                 .size(theme.text_sizes.normal),
                           );
                        });
                     });
                  });
               });

               ui.add_space(20.0);

               // Second row - Min Price, Current Price, Max Price
               ui.allocate_ui(size2, |ui| {
                  ui.horizontal(|ui| {
                     // Min Price
                     frame.show(ui, |ui| {
                        ui.vertical(|ui| {
                           ui.label(RichText::new("Min Price").size(theme.text_sizes.normal));
                           ui.label(
                              RichText::new(format!("{:.6}", min_price))
                                 .size(theme.text_sizes.normal),
                           );
                           ui.label(
                              RichText::new(format! {"{} / {}", token1.symbol(), token0.symbol()})
                                 .size(theme.text_sizes.normal),
                           );
                        });
                     });

                     // Current Price
                     frame.show(ui, |ui| {
                        ui.vertical(|ui| {
                           ui.label(RichText::new("Current Price").size(theme.text_sizes.normal));
                           ui.label(
                              RichText::new(format!("{:.6}", current_price))
                                 .size(theme.text_sizes.normal),
                           );
                           ui.label(
                              RichText::new(format! {"{} / {}", token1.symbol(), token0.symbol()})
                                 .size(theme.text_sizes.normal),
                           );
                        });
                     });

                     // Max Price
                     frame.show(ui, |ui| {
                        ui.vertical(|ui| {
                           ui.label(RichText::new("Max Price").size(theme.text_sizes.normal));
                           ui.label(
                              RichText::new(format!("{:.6}", max_price))
                                 .size(theme.text_sizes.normal),
                           );
                           ui.label(
                              RichText::new(format! {"{} / {}", token1.symbol(), token0.symbol()})
                                 .size(theme.text_sizes.normal),
                           );
                        });
                     });
                  });
               });
            });
         });
      self.open = open;

      if !self.open {
         self.close();
      }
   }
}

pub struct ViewPositionsUi {
   pub open: bool,
   pub size: (f32, f32),
   pub syncing: bool,
   pub state_syncing: bool,
   pub last_state_sync: Option<Instant>,
   pub position_details: PositionDetails,
   pub add_liquidity: AddLiquidity,
   pub remove_liquidity: RemoveLiquidity,
   pub collect_fees: CollectFees,
}

impl ViewPositionsUi {
   pub fn new() -> Self {
      Self {
         open: false,
         size: (600.0, 700.0),
         syncing: false,
         state_syncing: false,
         last_state_sync: None,
         position_details: PositionDetails::new(),
         add_liquidity: AddLiquidity::new(),
         remove_liquidity: RemoveLiquidity::new(),
         collect_fees: CollectFees::new(),
      }
   }

   pub fn show(
      &mut self,
      ctx: ZeusCtx,
      theme: &Theme,
      icons: Arc<Icons>,
      settings: &UniswapSettingsUi,
      ui: &mut Ui,
   ) {
      if !self.open {
         return;
      }

      let mev_protect = settings.mev_protect;

      self.position_details.show(ctx.clone(), theme, ui);

      self.add_liquidity.show(ctx.clone(), theme, icons.clone(), mev_protect, ui);

      self.remove_liquidity.show(ctx.clone(), theme, icons.clone(), mev_protect, ui);

      self.collect_fees.show(ctx.clone(), theme, icons, ui);

      let owner = ctx.current_wallet_address();
      let chain = ctx.chain();
      let positions = ctx.get_v3_positions(chain.id(), owner);

      ui.vertical_centered(|ui| {
         ui.set_width(self.size.0);
         ui.spacing_mut().item_spacing = vec2(0.0, 15.0);
         ui.spacing_mut().button_padding = vec2(10.0, 8.0);

         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            ui.spacing_mut().item_spacing = vec2(5.0, 15.0);

            let text = RichText::new("Sync Positions").size(theme.text_sizes.normal);
            let button = Button::new(text);
            if ui.add(button).clicked() {
               let days: u64 = settings.days.parse().unwrap_or(0);
               let ctx_clone = ctx.clone();
               self.syncing = true;

               RT.spawn(async move {
                  match sync_v3_positions(ctx_clone, days).await {
                     Ok(_) => {
                        tracing::info!("Synced V3 Positions");
                        SHARED_GUI.write(|gui| {
                           gui.uniswap.view_positions_ui.syncing = false;
                        });
                     }
                     Err(e) => {
                        tracing::error!("Error syncing V3 positions: {:?}", e);
                        SHARED_GUI.write(|gui| {
                           gui.uniswap.view_positions_ui.syncing = false;
                           gui.confirm_window.open("Error syncing V3 positions");
                           gui.confirm_window.set_msg2(format!("{:?}", e));
                           gui.request_repaint();
                        });
                     }
                  }
               });
            }

            if self.syncing || self.state_syncing {
               ui.add(Spinner::new().size(20.0).color(Color32::WHITE));
            }
         });

         if positions.is_empty() {
            let text = RichText::new("No positions found").size(theme.text_sizes.normal);
            ui.label(text);
            return;
         }

         if self.should_sync_state() {
            self.sync_pool_state(ctx.clone(), owner, positions.clone());
         }

         let frame = theme.frame1;

         ScrollArea::vertical().show(ui, |ui| {
            ui.vertical_centered(|ui| {
               for position in &positions {
                  frame.show(ui, |ui| {
                     ui.set_width(ui.available_width());
                     ui.spacing_mut().item_spacing = vec2(5.0, 15.0);

                     ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                           // In range - Pair - Protocol Version - Pool Fee
                           ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                              let in_range = self.is_in_range(ctx.clone(), position);
                              let text = if in_range { "In Range" } else { "Out of Range" };

                              let color = if in_range {
                                 Color32::GREEN
                              } else {
                                 Color32::RED
                              };

                              ui.label(
                                 RichText::new(text).color(color).size(theme.text_sizes.normal),
                              );

                              let pair = RichText::new(format!(
                                 "{} / {}",
                                 position.token0.symbol(),
                                 position.token1.symbol()
                              ))
                              .size(theme.text_sizes.normal);
                              ui.label(pair);

                              ui.label(RichText::new("v3").size(theme.text_sizes.normal));

                              let fee = RichText::new(format!("{}%", position.fee.fee_percent()))
                                 .size(theme.text_sizes.normal);
                              ui.label(fee);
                           });

                           // Details Button
                           ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                              let details = Button::new(
                                 RichText::new("Details").size(theme.text_sizes.normal),
                              );

                              if ui.add(details).clicked() {
                                 let manager = ctx.pool_manager();
                                 let pool = manager.get_v3_pool_from_address(
                                    ctx.chain().id(),
                                    position.pool_address,
                                 );
                                 self.position_details.open(Some(position.clone()), pool);
                              }
                           });
                        });

                        ui.horizontal(|ui| {
                           // Position $ Value
                           ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                              let value = self.position_value(ctx.clone(), position);
                              ui.label(
                                 RichText::new(format!("Position ${:.2}", value.f64()))
                                    .size(theme.text_sizes.normal),
                              );
                           });

                           // Add liquidity button
                           ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                              let add_liquidity = Button::new(
                                 RichText::new("Add Liquidity").size(theme.text_sizes.normal),
                              );
                              if ui.add(add_liquidity).clicked() {
                                 self.add_liquidity.open(Some(position.clone()));
                              }
                           });
                        });

                        // Uncollected Fees
                        ui.horizontal(|ui| {
                           ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                              let uncollected_fees =
                                 self.uncollected_fees_value(ctx.clone(), position);
                              ui.label(
                                 RichText::new(format!(
                                    "Uncollected Fees ${:.2}",
                                    uncollected_fees.f64()
                                 ))
                                 .size(theme.text_sizes.normal),
                              );
                           });

                           // Remove liquidity button
                           ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                              let remove_liquidity = Button::new(
                                 RichText::new("Remove Liquidity").size(theme.text_sizes.normal),
                              );
                              if ui.add(remove_liquidity).clicked() {
                                 self.remove_liquidity.open(Some(position.clone()));
                              }
                           });
                        });

                        // APR
                        ui.horizontal(|ui| {
                           ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                              ui.label(
                                 RichText::new(format!("APR {}%", position.apr))
                                    .size(theme.text_sizes.normal),
                              );
                           });

                           // Collect fees button
                           ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                              let collect_fees = Button::new(
                                 RichText::new("Collect Fees").size(theme.text_sizes.normal),
                              );
                              if ui.add(collect_fees).clicked() {
                                 self.collect_fees.open(Some(position.clone()));
                              }
                           });
                        });
                     });
                  });
               }
            });
         });
      });
   }

   fn position_value(&self, ctx: ZeusCtx, position: &V3Position) -> NumericValue {
      let value0 = ctx.get_currency_value_for_amount(position.amount0.f64(), &position.token0);
      let value1 = ctx.get_currency_value_for_amount(position.amount1.f64(), &position.token1);
      NumericValue::from_f64(value0.f64() + value1.f64())
   }

   fn uncollected_fees_value(&self, ctx: ZeusCtx, position: &V3Position) -> NumericValue {
      let value0 = ctx.get_currency_value_for_amount(position.tokens_owed0.f64(), &position.token0);
      let value1 = ctx.get_currency_value_for_amount(position.tokens_owed1.f64(), &position.token1);
      NumericValue::from_f64(value0.f64() + value1.f64())
   }

   fn is_in_range(&self, ctx: ZeusCtx, position: &V3Position) -> bool {
      let manager = ctx.pool_manager();
      let pool = manager.get_v3_pool_from_address(ctx.chain().id(), position.pool_address);

      if pool.is_some() {
         let pool = pool.unwrap();
         let state = pool.state().v3_state();
         if state.is_none() {
            return false;
         }
         let state = state.unwrap();
         let current_tick = state.tick;
         current_tick >= position.tick_lower && current_tick <= position.tick_upper
      } else {
         false
      }
   }

   fn should_sync_state(&self) -> bool {
      if self.syncing || self.state_syncing {
         return false;
      }

      let now = Instant::now();
      if let Some(last_updated) = self.last_state_sync {
         let elapsed = now.duration_since(last_updated).as_secs();
         if elapsed < POOL_STATE_EXPIRY {
            return false;
         }
      }

      true
   }

   pub fn sync_pool_state(&mut self, ctx: ZeusCtx, owner: Address, mut positions: Vec<V3Position>) {
      let chain_id = ctx.chain().id();
      let nft_contract = uniswap_nft_position_manager(chain_id).unwrap();
      let manager = ctx.pool_manager();

      let mut pools = Vec::new();
      for position in &positions {
         if position.liquidity == 0 {
            ctx.write(|ctx| {
               ctx.v3_positions_db.remove(chain_id, owner, position);
            });
            continue;
         }

         let pool = manager.get_v3_pool_from_address(chain_id, position.pool_address);
         if let Some(pool) = pool {
            pools.push(pool);
         }
      }

      self.state_syncing = true;
      let ctx_clone = ctx.clone();

      RT.spawn(async move {
         let client = match ctx_clone.get_client(chain_id).await {
            Ok(client) => client,
            Err(e) => {
               SHARED_GUI.write(|gui| {
                  gui.uniswap.view_positions_ui.state_syncing = false;
               });
               tracing::error!(
                  "Error getting client for chain {}: {:?}",
                  chain_id,
                  e
               );
               return;
            }
         };

         match manager.update_state_for_pools(ctx_clone.clone(), chain_id, pools).await {
            Ok(_) => {
               SHARED_GUI.write(|gui| {
                  gui.uniswap.view_positions_ui.state_syncing = false;
                  gui.uniswap.view_positions_ui.last_state_sync = Some(Instant::now());
               });
            }
            Err(e) => {
               SHARED_GUI.write(|gui| {
                  gui.uniswap.view_positions_ui.state_syncing = false;
               });
               tracing::error!("Error syncing pool state: {:?}", e);
            }
         }

         // Update the positions
         for position in positions.iter_mut() {
            let pool = manager.get_v3_pool_from_address(chain_id, position.pool_address);
            if pool.is_none() {
               continue;
            }

            let updated_position = match abi::uniswap::nft_position::positions(
               client.clone(),
               nft_contract,
               position.id,
            )
            .await
            {
               Ok(updated_position) => updated_position,
               Err(e) => {
                  tracing::error!("Error updating position: {:?}", e);
                  continue;
               }
            };

            let pool = pool.unwrap();
            let state = pool.state().v3_state().unwrap();

            let sqrt_price_lower =
               uniswap_v3_math::tick_math::get_sqrt_ratio_at_tick(position.tick_lower).unwrap();
            let sqrt_price_upper =
               uniswap_v3_math::tick_math::get_sqrt_ratio_at_tick(position.tick_upper).unwrap();

            let (amount0, amount1) = calculate_liquidity_amounts(
               state.sqrt_price,
               sqrt_price_lower,
               sqrt_price_upper,
               position.liquidity,
            )
            .unwrap();

            position.amount0 = NumericValue::format_wei(amount0, pool.currency0().decimals());
            position.amount1 = NumericValue::format_wei(amount1, pool.currency1().decimals());
            position.tokens_owed0 = NumericValue::format_wei(
               updated_position.tokens_owed0,
               pool.currency0().decimals(),
            );
            position.tokens_owed1 = NumericValue::format_wei(
               updated_position.tokens_owed1,
               pool.currency1().decimals(),
            );

            ctx.write(|ctx| {
               ctx.v3_positions_db.insert(chain_id, owner, position.clone());
            });
         }

         ctx.save_v3_positions_db();
      });
   }
}

struct TokenIdWithBlock {
   id: U256,
   timestamp: u64,
   hash: FixedBytes<32>,
}

// TODO: Ideally need a way to get the block number from the first tx of the EOA
async fn sync_v3_positions(ctx: ZeusCtx, days: u64) -> Result<(), anyhow::Error> {
   let chain = ctx.chain();
   let client = ctx.get_archive_client(chain.id(), false).await?;

   let latest_block = client.get_block_number().await?;

   let wallets = ctx.get_all_wallets_info();
   let wallet_addresses = wallets.iter().map(|w| w.address).collect::<Vec<_>>();

   let nft_contract = uniswap_nft_position_manager(chain.id())?;
   let creation_block = nft_position_manager_creation_block(chain.id())?;

   let block_time = if days > 0 {
      BlockTime::Days(days)
   } else {
      BlockTime::Block(creation_block)
   };

   let from_block = block_time.go_back(chain.id(), latest_block)?;

   let event = abi::uniswap::nft_position::INonfungiblePositionManager::Transfer::SIGNATURE;
   let events = vec![event];

   let logs = get_logs_for(
      client.clone(),
      vec![nft_contract],
      events,
      from_block,
      1,
      50_000,
   )
   .await?;

   // Map with all the token ids and their owners
   let mut token_ids: HashMap<Address, Vec<TokenIdWithBlock>> = HashMap::new();

   for log in logs {
      if let Ok(decoded) = abi::uniswap::nft_position::decode_transfer_log(log.data()) {
         let nft_owner = decoded.to;
         for wallet in &wallet_addresses {
            if *wallet == nft_owner {
               let id = decoded.tokenId;
               let timestamp = log.block_timestamp.unwrap_or_default();
               let hash = log.transaction_hash;
               if hash.is_none() {
                  tracing::error!("Transaction hash not found for log {:?}", log);
                  continue;
               }
               token_ids.entry(*wallet).or_default().push(TokenIdWithBlock {
                  id,
                  timestamp,
                  hash: hash.unwrap(),
               });
            }
         }
      }
   }

   let token_ids = token_ids.into_iter().collect::<Vec<_>>();

   for (owner, token_ids) in token_ids {
      for token_id in token_ids {
         let id = token_id.id;
         let hash = token_id.hash;
         let timestamp = token_id.timestamp;

         let position =
            abi::uniswap::nft_position::positions(client.clone(), nft_contract, id).await?;

         let position_exists = ctx.read(|ctx| {
            ctx.v3_positions_db.get(chain.id(), owner).iter().find(|p| p.id == id).cloned()
         });

         if position_exists.is_some() {
            continue;
         }

         let tx = client.get_transaction_receipt(hash).await?;
         if tx.is_none() {
            tracing::error!("Transaction not found for hash {}", hash);
            continue;
         }

         let tx = tx.unwrap();
         let mut amount0_minted = U256::ZERO;
         let mut amount1_minted = U256::ZERO;

         let logs: Vec<Log> = tx.logs().to_vec();

         for log in logs {
            if let Ok(decoded) = abi::uniswap::v3::pool::decode_mint_log(log.data()) {
               amount0_minted = decoded.amount0;
               amount1_minted = decoded.amount1;
               break;
            }
         }

         let (cached_token0, cached_token1) = ctx.read(|ctx| {
            (
               ctx.currency_db.get_erc20_token(chain.id(), position.token0),
               ctx.currency_db.get_erc20_token(chain.id(), position.token1),
            )
         });

         let token0 = if let Some(token) = cached_token0 {
            token
         } else {
            let token = ERC20Token::new(client.clone(), position.token0, chain.id()).await?;
            ctx.write(|ctx| {
               ctx.currency_db.insert_currency(chain.id(), Currency::from(token.clone()))
            });
            token
         };

         let token1 = if let Some(token) = cached_token1 {
            token
         } else {
            let token = ERC20Token::new(client.clone(), position.token1, chain.id()).await?;
            ctx.write(|ctx| {
               ctx.currency_db.insert_currency(chain.id(), Currency::from(token.clone()))
            });
            token
         };

         let cached_pool = ctx.read(|ctx| {
            ctx.pool_manager.get_v3_pool_from_token_addresses_and_fee(
               chain.id(),
               position.fee,
               position.token0,
               position.token1,
            )
         });

         let amount0_minted = NumericValue::format_wei(amount0_minted, token0.decimals);
         let amount1_minted = NumericValue::format_wei(amount1_minted, token1.decimals);
         let tokens_owed0 = NumericValue::format_wei(position.tokens_owed0, token0.decimals);
         let tokens_owed1 = NumericValue::format_wei(position.tokens_owed1, token1.decimals);

         let pool = if let Some(pool) = cached_pool {
            pool
         } else {
            let dex = DexKind::UniswapV3;
            UniswapV3Pool::from_components(
               client.clone(),
               chain.id(),
               position.fee,
               token0,
               token1,
               dex,
            )
            .await?
            .into()
         };

         let v3_position = V3Position {
            chain_id: chain.id(),
            owner,
            dex: pool.dex_kind(),
            block: tx.block_number.unwrap_or_default(),
            timestamp,
            id,
            nonce: position.nonce,
            operator: position.operator,
            token0: pool.currency0().clone(),
            token1: pool.currency1().clone(),
            fee: pool.fee(),
            pool_address: pool.address(),
            tick_lower: position.tick_lower,
            tick_upper: position.tick_upper,
            liquidity: position.liquidity,
            fee_growth_inside0_last_x128: position.fee_growth_inside0_last_x128,
            fee_growth_inside1_last_x128: position.fee_growth_inside1_last_x128,
            amount0: amount0_minted,
            amount1: amount1_minted,
            tokens_owed0,
            tokens_owed1,
            apr: 0.0,
         };

         ctx.write(|ctx| {
            ctx.v3_positions_db.insert(chain.id(), owner, v3_position);
         });
         ctx.save_v3_positions_db();
      }
   }

   Ok(())
}
