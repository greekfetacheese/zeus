use egui::{
   Align, Align2, CornerRadius, CursorIcon, Frame, Layout, Order, RichText, Spinner, Ui, Window,
   vec2,
};
use zeus_eth::alloy_primitives::Address;
use zeus_eth::currency::Currency;
use zeus_eth::utils::NumericValue;

use crate::assets::icons::Icons;
use crate::core::ZeusCtx;
use crate::gui::ui::TokenSelectionWindow;
use std::str::FromStr;
use std::sync::Arc;
use zeus_theme::{ButtonVisuals, OverlayManager, Theme};
use zeus_widgets::{Button, Label};

pub mod pool;
pub mod settings;
pub mod swap;

use pool::PoolsUi;
use settings::UniswapSettingsUi;
use swap::SwapUi;

#[derive(Clone, Default, Copy, Debug, PartialEq)]
pub enum ProtocolVersion {
   V2,
   #[default]
   V3,
   V4,
}

impl FromStr for ProtocolVersion {
   type Err = anyhow::Error;

   fn from_str(s: &str) -> Result<Self, Self::Err> {
      match s {
         "V2" => Ok(Self::V2),
         "V3" => Ok(Self::V3),
         "V4" => Ok(Self::V4),
         _ => Err(anyhow::anyhow!("Invalid protocol version")),
      }
   }
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

   pub fn as_str(&self) -> &'static str {
      match self {
         ProtocolVersion::V2 => "V2",
         ProtocolVersion::V3 => "V3",
         ProtocolVersion::V4 => "V4",
      }
   }

   pub fn all() -> Vec<Self> {
      vec![
         ProtocolVersion::V2,
         ProtocolVersion::V3,
         ProtocolVersion::V4,
      ]
   }
}

/// A UI for a dex like Uniswap
pub struct UniswapUi {
   open: bool,
   pub size: (f32, f32),
   pub settings: UniswapSettingsUi,
   pub swap_ui: SwapUi,
   pub pools_ui: PoolsUi,
   // pub create_position_ui: CreatePositionUi,
   // pub view_positions_ui: ViewPositionsUi,
}

impl UniswapUi {
   pub fn new(overlay: OverlayManager) -> Self {
      Self {
         open: false,
         size: (500.0, 900.0),
         settings: UniswapSettingsUi::new(overlay),
         swap_ui: SwapUi::new(),
         pools_ui: PoolsUi::new(),
         // create_position_ui: CreatePositionUi::new(),
         // view_positions_ui: ViewPositionsUi::new(),
      }
   }

   pub fn open(&mut self) {
      self.open = true;
   }

   pub fn close(&mut self) {
      self.open = false;
      self.settings.close();
      self.swap_ui.amount_in_field.reset();
      self.swap_ui.amount_out_field.reset();
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

      ui.vertical_centered(|ui| {
         ui.spacing_mut().button_padding = vec2(10.0, 8.0);

         // TODO: Add support for BSC, There is an issue with batch calls
         if ctx.chain().is_bsc() {
            let text = RichText::new("Swap feature is not available on Binance Smart Chain")
               .size(theme.text_sizes.large)
               .color(theme.colors.error);
            ui.label(text);
         } else {
            ui.add_space(15.0);
         }

         let size = vec2(ui.available_width() * 0.45, 50.0);
         let button_visuals = theme.button_visuals();

         // Header
         ui.allocate_ui(size, |ui| {
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
               // Swap - Pool - Position Buttons
               ui.set_width(self.size.0);
               ui.spacing_mut().item_spacing.x = 10.0;

               ui.horizontal(|ui| {
                  let tint = theme.image_tint_recommended;
                  let icon = match theme.dark_mode {
                     true => icons.gear_white_x24(tint),
                     false => icons.gear_dark_x24(tint),
                  };

                  let mut visuals = ButtonVisuals::default();
                  visuals.bg_hover = button_visuals.bg_hover;
                  visuals.corner_radius = CornerRadius::same(25);
                  let button = Button::image(icon).small().visuals(visuals);
                  let res = ui.add(button).on_hover_cursor(CursorIcon::PointingHand);

                  if res.clicked() {
                     self.settings.open();
                  }

                  let icon = match theme.dark_mode {
                     true => icons.refresh_white_x22(tint),
                     false => icons.refresh_dark_x22(tint),
                  };

                  let syncing = self.swap_ui.pool_data_syncing
                     || self.swap_ui.syncing_pools
                     || self.swap_ui.balance_syncing;

                  if !syncing {
                     let mut visuals = ButtonVisuals::default();
                     visuals.bg_hover = button_visuals.bg_hover;
                     visuals.corner_radius = CornerRadius::same(25);
                     let button = Button::image(icon).small().visuals(visuals);
                     let res = ui.add(button).on_hover_cursor(CursorIcon::PointingHand);

                     if res.clicked() {
                        if self.swap_ui.is_open() {
                           self.swap_ui.refresh(ctx.clone(), &self.settings);
                        }
                     }
                  } else {
                     ui.add(Spinner::new().size(17.0).color(theme.colors.text));
                  }

                  #[cfg(feature = "dev")]
                  {
                     let text = RichText::new("Swap").size(theme.text_sizes.large);
                     let swap_button = Button::new(text).visuals(button_visuals);
                     if ui.add(swap_button).clicked() {
                        self.swap_ui.open();
                        self.pools_ui.open = false;
                        // self.create_position_ui.open = false;
                        // self.view_positions_ui.open = false;
                     }

                     let text = RichText::new("Pools").size(theme.text_sizes.large);
                     let pools_button = Button::new(text).visuals(button_visuals);
                     if ui.add(pools_button).clicked() {
                        self.pools_ui.open = true;
                        self.swap_ui.close();
                        // self.create_position_ui.open = false;
                        // self.view_positions_ui.open = false;
                     }
                  }

                  /*
                  #[cfg(feature = "dev")]
                  {
                     let text = RichText::new("Create Position").size(theme.text_sizes.large);
                     let positions_button = Button::new(text);
                     if ui.add(positions_button).clicked() {
                        // self.create_position_ui.open = true;
                        // self.swap_ui.close();
                        // self.pools_ui.open = false;
                        // self.view_positions_ui.open = false;
                     }
                  }
                  */

                  /*
                                    #[cfg(feature = "dev")]
                                    {
                                       let text = RichText::new("View Positions").size(theme.text_sizes.large);
                                       let positions_button = Button::new(text);
                                       if ui.add(positions_button).clicked() {
                                          // self.view_positions_ui.open = true;
                                          // self.swap_ui.close();
                                          // self.pools_ui.open = false;
                                          // self.create_position_ui.open = false;
                                       }
                                    }
                  */
               });

               /*
               if self.view_positions_ui.open {
                  let owner = ctx.current_wallet_address();
                  let chain = ctx.chain();
                  let positions = ctx.get_v3_positions(chain.id(), owner);
                  if !positions.is_empty() {
                     self.view_positions_ui.sync_pool_state(ctx.clone(), owner, positions);
                  }
               }
               */
            });
         });

         self.swap_ui.show(
            ctx.clone(),
            theme,
            icons.clone(),
            token_selection,
            &self.settings,
            ui,
         );

         self.pools_ui.show(ctx.clone(), theme, icons.clone(), ui);

         self.show_settings(ctx.clone(), theme, ui);

         /*
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
         */
      });
   }

   pub fn show_settings(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      if !self.settings.is_open() {
         return;
      }

      Window::new("Uniswap_Settings")
         .title_bar(false)
         .movable(false)
         .order(Order::Foreground)
         .resizable(false)
         .frame(Frame::window(ui.style()))
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .show(ui.ctx(), |ui| {
            ui.set_width(300.0);
            ui.set_height(400.0);
            ui.spacing_mut().button_padding = vec2(10.0, 8.0);

            let swap_ui_open = self.swap_ui.is_open();
            let view_positions_open = false;

            ui.vertical_centered(|ui| {
               self.settings.show(
                  ctx.clone(),
                  swap_ui_open,
                  view_positions_open,
                  theme,
                  ui,
               );

               ui.add_space(10.0);

               let text = RichText::new("Close").size(theme.text_sizes.normal);
               let visuals = theme.button_visuals();
               if ui.add(Button::new(text).visuals(visuals)).clicked() {
                  self.settings.close();
               }
            });
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
   let tint = theme.image_tint_recommended;

   ui.vertical(|ui| {
      // Currency 0
      frame.show(ui, |ui| {
         ui.horizontal(|ui| {
            ui.vertical(|ui| {
               ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                  let text = RichText::new(token0.symbol()).size(theme.text_sizes.large);
                  let icon = icons.currency_icon(token0, tint);
                  let label = Label::new(text, Some(icon)).image_on_left().interactive(false);
                  ui.add(label);
               });

               ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                  let balance = ctx.get_currency_balance(chain, owner, token0);
                  let b_text = format!("(Balance: {})", balance.abbreviated());
                  let text = RichText::new(b_text).size(theme.text_sizes.normal);
                  let label = Label::new(text, None).interactive(false);
                  ui.add(label);
               });
            });

            // Currency 0 Amount & Value
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
               let value = NumericValue::value(amount0.f64(), price0_usd.f64());
               let text = RichText::new(format!("(${})", value.abbreviated()))
                  .size(theme.text_sizes.normal);
               ui.label(text);

               ui.add_space(5.0);

               let text = RichText::new(amount0.abbreviated()).size(theme.text_sizes.normal);
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
                  let icon = icons.currency_icon(token1, tint);
                  let label = Label::new(text, Some(icon)).image_on_left().interactive(false);
                  ui.add(label);
               });

               ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                  let balance = ctx.get_currency_balance(chain, owner, token1);
                  let b_text = format!("(Balance: {})", balance.abbreviated());
                  let text = RichText::new(b_text).size(theme.text_sizes.normal);
                  let label = Label::new(text, None).interactive(false);
                  ui.add(label);
               });
            });

            // Currency B Amount & Value
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
               let value = NumericValue::value(amount1.f64(), price1_usd.f64());
               let text = RichText::new(format!("(${})", value.abbreviated()))
                  .size(theme.text_sizes.normal);
               ui.label(text);

               ui.add_space(5.0);

               let text = RichText::new(amount1.abbreviated()).size(theme.text_sizes.normal);
               ui.label(text);
            });
         });
      });
   });
}
