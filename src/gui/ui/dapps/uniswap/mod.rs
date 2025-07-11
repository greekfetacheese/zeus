pub mod create_position;
pub mod pool;
pub mod swap;
pub mod view_positions;

use create_position::CreatePositionUi;
use egui_widgets::LabelWithImage;
use pool::PoolsUi;
use swap::SwapUi;
use view_positions::ViewPositionsUi;

use egui::{Align, Frame, Button, FontId, Layout, Margin, RichText, Slider, TextEdit, Ui, vec2};
use zeus_eth::alloy_primitives::Address;
use zeus_eth::currency::Currency;
use zeus_eth::utils::NumericValue;

use crate::assets::icons::Icons;
use crate::core::{ZeusCtx, utils::RT};
use crate::gui::SHARED_GUI;
use crate::gui::ui::TokenSelectionWindow;
use egui_theme::Theme;
use std::sync::Arc;

#[derive(Clone, Default, Copy, Debug, PartialEq)]
pub enum ProtocolVersion {
   V2,
   #[default]
   V3,
   V4,
}

impl ProtocolVersion {
   pub fn is_v2(&self) -> bool {
      matches!(self, Self::V2)
   }

   pub fn is_v3(&self) -> bool {
      matches!(self, Self::V3)
   }

   pub fn is_v4(&self) -> bool {
      matches!(self, Self::V4)
   }

   pub fn to_str(&self) -> &'static str {
      match self {
         ProtocolVersion::V2 => "V2",
         ProtocolVersion::V3 => "V3",
         ProtocolVersion::V4 => "V4",
      }
   }

   pub fn from_str(s: &str) -> Option<Self> {
      match s {
         "V2" => Some(Self::V2),
         "V3" => Some(Self::V3),
         "V4" => Some(Self::V4),
         _ => None,
      }
   }

   pub fn all() -> Vec<Self> {
      vec![ProtocolVersion::V2, ProtocolVersion::V3, ProtocolVersion::V4]
   }
}

#[derive(Clone)]
pub struct UniswapSettingsUi {
   open: bool,
   pub swap_on_v2: bool,
   pub swap_on_v3: bool,
   pub swap_on_v4: bool,
   pub split_routing_enabled: bool,
   pub max_hops: usize,
   pub max_split_routes: usize,
   pub mev_protect: bool,
   pub slippage: String,
   /// Applies only to [SwapUi]
   pub simulate_mode: bool,
   /// Days to go back to sync positions
   /// Applies only to [ViewPositionsUi]
   pub days: String,
}

impl UniswapSettingsUi {
   pub fn new() -> Self {
      Self {
         open: false,
         swap_on_v2: true,
         swap_on_v3: true,
         swap_on_v4: false,
         split_routing_enabled: false,
         max_hops: 4,
         max_split_routes: 5,
         mev_protect: true,
         slippage: "0.5".to_string(),
         simulate_mode: false,
         days: String::new(),
      }
   }

   pub fn open(&mut self) {
      self.open = true;
   }

   pub fn close(&mut self) {
      self.open = false;
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn show(
      &mut self,
      ctx: ZeusCtx,
      swap_ui_open: bool,
      view_position_open: bool,
      theme: &Theme,
      ui: &mut Ui,
   ) {
      if !self.open {
         return;
      }

      ui.spacing_mut().item_spacing = vec2(5.0, 15.0);

      // For some fucking reason cargo fmt doesnt work here
      ui.vertical_centered(|ui| {
         let slider_size = vec2(ui.available_width() * 0.5, 25.0);

                  let text = RichText::new("MEV Protect").size(theme.text_sizes.normal);
                  ui.checkbox(&mut self.mev_protect, text);

                  let text = RichText::new("Slippage").size(theme.text_sizes.normal);
                  ui.label(text).on_hover_text("Your transaction will revert if the price changes unfavorably by more than this percentage");

               TextEdit::singleline(&mut self.slippage)
                  .hint_text("0.5")
                  .font(FontId::proportional(theme.text_sizes.small))
                  .desired_width(25.0)
                  .background_color(theme.colors.text_edit_bg)
                  .margin(Margin::same(10))
                  .show(ui);

               if swap_ui_open {

               ui.label(RichText::new("Routing").size(theme.text_sizes.large));
               
               let text = RichText::new("Split Routing").size(theme.text_sizes.normal);
               let res = ui.checkbox(&mut self.split_routing_enabled, text);
               if res.changed() {
                  let ctx_clone = ctx.clone();
                  RT.spawn_blocking(move || {
                     SHARED_GUI.write(|gui| {
                        let settings = &gui.uniswap.settings;
                        gui.uniswap.swap_ui.get_quote(ctx_clone, settings);
                     });
                  });
               }

               ui.label(RichText::new("Max Hops").size(theme.text_sizes.normal));
               ui.allocate_ui(slider_size, |ui| {
               let res = ui.add(Slider::new(&mut self.max_hops, 1..=10));
               if res.changed() {
                  let ctx_clone = ctx.clone();
                  RT.spawn_blocking(move || {
                     SHARED_GUI.write(|gui| {
                        let settings = &gui.uniswap.settings;
                        gui.uniswap.swap_ui.get_quote(ctx_clone, settings);
                     });
                  });
               }
            });

            ui.label(RichText::new("Max Split Routes").size(theme.text_sizes.normal));
            ui.allocate_ui(slider_size, |ui| {
               let res = ui.add(Slider::new(&mut self.max_split_routes, 1..=10));
               if res.changed() {
                  let ctx_clone = ctx.clone();
                  RT.spawn_blocking(move || {
                     SHARED_GUI.write(|gui| {
                        let settings = &gui.uniswap.settings;
                        gui.uniswap.swap_ui.get_quote(ctx_clone, settings);
                     });
                  });
               }
            });

                let text = RichText::new("Swap on V2").size(theme.text_sizes.normal);
                let swap_on_v2_before = self.swap_on_v2;
                let v2_res = ui.checkbox(&mut self.swap_on_v2, text);

                let text = RichText::new("Swap on V3").size(theme.text_sizes.normal);
                let swap_on_v3_before = self.swap_on_v3;
                let v3_res = ui.checkbox(&mut self.swap_on_v3, text);

                
                let text = RichText::new("Swap on V4").size(theme.text_sizes.normal);
                let swap_on_v4_before = self.swap_on_v4;
                let v4_res = ui.checkbox(&mut self.swap_on_v4, text);

                if v2_res.changed() || v3_res.changed() || v4_res.changed() {
                   let ctx_clone = ctx.clone();
                   RT.spawn_blocking(move || {
                      SHARED_GUI.write(|gui| {
                        let update_v2 = !swap_on_v2_before;
                         let update_v3 = !swap_on_v3_before;
                         let update_v4 = !swap_on_v4_before;
                         gui.uniswap.swap_ui.update_pool_state(ctx_clone, update_v2, update_v3, update_v4);
                      });
                   });
                }
               
                  let text = RichText::new("Simulate Mode").size(theme.text_sizes.normal);
                  ui.checkbox(&mut self.simulate_mode, text);
               }

               if view_position_open {
                  let text = RichText::new("Number of Days to go back").size(theme.text_sizes.normal);
                  ui.label(text);
                  ui.add(TextEdit::singleline(&mut self.days).desired_width(25.0));
               }
         });
   }
}

pub struct UniswapUi {
   open: bool,
   pub size: (f32, f32),
   pub settings: UniswapSettingsUi,
   pub swap_ui: SwapUi,
   pub pools_ui: PoolsUi,
   pub create_position_ui: CreatePositionUi,
   pub view_positions_ui: ViewPositionsUi,
}

impl UniswapUi {
   pub fn new() -> Self {
      Self {
         open: false,
         size: (500.0, 900.0),
         settings: UniswapSettingsUi::new(),
         swap_ui: SwapUi::new(),
         pools_ui: PoolsUi::new(),
         create_position_ui: CreatePositionUi::new(),
         view_positions_ui: ViewPositionsUi::new(),
      }
   }

   pub fn open(&mut self) {
      self.open = true;
      self.settings.open();
   }

   pub fn close(&mut self) {
      self.open = false;
      self.settings.close();
   }

   pub fn is_open(&self) -> bool {
      self.open
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

      // ui.add_space(50.0);

      ui.vertical_centered(|ui| {
         ui.spacing_mut().item_spacing = vec2(0.0, 15.0);
         ui.spacing_mut().button_padding = vec2(10.0, 8.0);

         ui.scope(|ui| {
            ui.set_width(self.size.0);

            // Header
            ui.horizontal(|ui| {
               // Swap - Pool - Position Buttons
               ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                  ui.spacing_mut().item_spacing.x = 10.0;

                  let text = RichText::new("Swap").size(theme.text_sizes.large);
                  let swap_button = Button::new(text);
                  if ui.add(swap_button).clicked() {
                     self.swap_ui.open = true;
                     self.pools_ui.open = false;
                     self.create_position_ui.open = false;
                     self.view_positions_ui.open = false;
                  }

                  let text = RichText::new("Pools").size(theme.text_sizes.large);
                  let pools_button = Button::new(text);
                  if ui.add(pools_button).clicked() {
                     self.pools_ui.open = true;
                     self.swap_ui.open = false;
                     self.create_position_ui.open = false;
                     self.view_positions_ui.open = false;
                  }

                  let text = RichText::new("Create Position").size(theme.text_sizes.large);
                  let positions_button = Button::new(text);
                  if ui.add(positions_button).clicked() {
                     self.create_position_ui.open = true;
                     self.swap_ui.open = false;
                     self.pools_ui.open = false;
                     self.view_positions_ui.open = false;
                  }

                  let text = RichText::new("View Positions").size(theme.text_sizes.large);
                  let positions_button = Button::new(text);
                  if ui.add(positions_button).clicked() {
                     self.view_positions_ui.open = true;
                     self.swap_ui.open = false;
                     self.pools_ui.open = false;
                     self.create_position_ui.open = false;
                  }
               });
            });
         });

         ui.add_space(40.0);

         self.swap_ui.show(
            ctx.clone(),
            theme,
            icons.clone(),
            token_selection,
            &self.settings,
            ui,
         );

         self.pools_ui.show(ctx.clone(), theme, icons.clone(), ui);

         self.create_position_ui.show(
            ctx.clone(),
            theme,
            icons.clone(),
            token_selection,
            &self.settings,
            ui,
         );

         self.view_positions_ui.show(
            ctx.clone(),
            theme,
            icons.clone(),
            &self.settings,
            ui,
         );
      });
   }
}




pub fn currencies_amount_and_value(
   ctx: ZeusCtx,
   chain: u64,
   owner: Address,
   token0: &Currency,
   token1: &Currency,
   amount0: &NumericValue,
   amount1: &NumericValue,
   price0_usd: &NumericValue,
   price1_usd: &NumericValue,
   theme: &Theme,
   icons: Arc<Icons>,
   frame: Frame,
   ui: &mut Ui,
) {

   ui.vertical(|ui| {

      // Currency 0
      frame.show(ui, |ui| {
         ui.horizontal(|ui| {
            ui.vertical(|ui| {
               ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                  let text = RichText::new(token0.symbol()).size(theme.text_sizes.large);
                  let icon = icons.currency_icon(&token0);
                  let label = LabelWithImage::new(text, Some(icon)).image_on_left();
                  ui.add(label);
               });

               ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                  let balance = ctx.get_currency_balance(chain, owner, &token0);
                  let b_text = format!("(Balance: {})", balance.format_abbreviated());
                  let text = RichText::new(b_text).size(theme.text_sizes.normal);
                  let label = LabelWithImage::new(text, None);
                  ui.add(label);
               });
            });

            // Currency 0 Amount & Value
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
               let value = NumericValue::value(amount0.f64(), price0_usd.f64());
               let text = RichText::new(format!("(${})", value.format_abbreviated()))
                  .size(theme.text_sizes.normal);
               ui.label(text);

               ui.add_space(5.0);

               let text = RichText::new(format!("{}", amount0.format_abbreviated()))
                  .size(theme.text_sizes.normal);
               ui.label(text);
            });
         });
      });

      // Currency 1
      frame.show(ui, |ui| {
         ui.horizontal(|ui| {
            ui.vertical(|ui| {
               ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                  let text = RichText::new(token1.symbol()).size(theme.text_sizes.large);
                  let icon = icons.currency_icon(&token1);
                  let label = LabelWithImage::new(text, Some(icon)).image_on_left();
                  ui.add(label);
               });

               ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                  let balance = ctx.get_currency_balance(chain, owner, &token1);
                  let b_text = format!("(Balance: {})", balance.format_abbreviated());
                  let text = RichText::new(b_text).size(theme.text_sizes.normal);
                  let label = LabelWithImage::new(text, None);
                  ui.add(label);
               });
            });

            // Currency B Amount & Value
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
               let value = NumericValue::value(amount1.f64(), price1_usd.f64());
               let text = RichText::new(format!("(${})", value.format_abbreviated()))
                  .size(theme.text_sizes.normal);
               ui.label(text);

               ui.add_space(5.0);

               let text = RichText::new(format!("{}", amount1.format_abbreviated()))
                  .size(theme.text_sizes.normal);
               ui.label(text);
            });
         });
      });
   });
}
