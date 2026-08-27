use egui::{
   Align, Align2, CornerRadius, CursorIcon, Frame, Layout, Order, RichText, Spinner, Ui, Window,
   vec2,
};
use zeus_eth::alloy_primitives::Address;
use zeus_eth::currency::Currency;
use zeus_eth::utils::NumericValue;

use crate::assets::icons::Icons;
use crate::core::{ZeusContext, ZeusCtx};
use crate::gui::ui::TokenSelectionWindow;
use egui_elements::{Button, Label, Modal, Theme, visuals::ButtonVisuals};
use egui_lucide::Lucide;
use std::str::FromStr;
use std::sync::Arc;

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
}

impl UniswapUi {
   pub fn new() -> Self {
      Self {
         open: false,
         size: (400.0, 500.0),
         settings: UniswapSettingsUi::new(),
         swap_ui: SwapUi::new(),
         pools_ui: PoolsUi::new(),
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

   fn header(&mut self, _ctx: &mut ZeusContext, theme: &Theme, ui: &mut Ui) {
      let size = vec2(ui.available_width() * 0.95, 30.0);

      ui.allocate_ui(size, |ui| {
         ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            // Swap - Pool - Settings Buttons
            ui.set_width(self.size.0);
            ui.spacing_mut().item_spacing.x = 10.0;

            let button_visuals = theme.button_visuals();

            let icon = Lucide::Settings.size(20.0).color(theme.colors.text).image();

            let mut visuals = ButtonVisuals::default();
            visuals.bg_hover = button_visuals.bg_hover;
            visuals.corner_radius = CornerRadius::same(25);
            let button = Button::image(icon).small().visuals(visuals);
            let res = ui.add(button).on_hover_cursor(CursorIcon::PointingHand);

            if res.clicked() {
               self.settings.open();
            }

            let icon = Lucide::RefreshCw.size(20.0).color(theme.colors.text).image();

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
                     self.swap_ui.refresh(&self.settings);
                  }
               }
            } else {
               ui.add(Spinner::new().size(17.0).color(theme.colors.text));
            }

            #[cfg(feature = "dev")]
            {
               let text = RichText::new("Swap").size(theme.typography.large);
               let swap_button = Button::new(text).visuals(button_visuals);
               if ui.add(swap_button).clicked() {
                  self.swap_ui.open();
                  self.pools_ui.open = false;
               }

               let text = RichText::new("Pools").size(theme.typography.large);
               let pools_button = Button::new(text).visuals(button_visuals);
               if ui.add(pools_button).clicked() {
                  self.pools_ui.open = true;
                  self.swap_ui.close();
               }
            }
         });
      });
   }

   pub fn show(
      &mut self,
      ctx: &mut ZeusContext,
      theme: &Theme,
      icons: Arc<Icons>,
      token_selection: &mut TokenSelectionWindow,
      ui: &mut Ui,
   ) {
      if !self.open {
         return;
      }

      let window_frame = theme.frame1;

      Window::new("uniswap_ui")
         .title_bar(false)
         .resizable(false)
         .collapsible(false)
         .order(Order::Background)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 100.0))
         .frame(window_frame)
         .show(ui.ctx(), |ui| {
            ui.vertical_centered(|ui| {
               ui.set_width(self.size.0);
               ui.set_height(self.size.1);

               ui.spacing_mut().item_spacing = vec2(0.0, 10.0);
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);

               // TODO: Add support for BSC, There is an issue with batch calls
               if ctx.chain.is_bsc() {
                  let text = RichText::new("Swap feature is not available on Binance Smart Chain")
                     .size(theme.typography.large)
                     .color(theme.colors.error);
                  ui.label(text);
               }

               self.header(ctx, theme, ui);

               self.swap_ui.show(
                  ctx,
                  theme,
                  icons.clone(),
                  token_selection,
                  &self.settings,
                  ui,
               );

               self.pools_ui.show(ctx, theme, icons.clone(), ui);

               self.show_settings(theme, ui);
            });
         });
   }

   pub fn show_settings(&mut self, theme: &Theme, ui: &mut Ui) {
      if !self.settings.is_open() {
         return;
      }

      let mut open = self.settings.is_open();

      Modal::new("Uniswap_Settings", &mut open)
         .backdrop_order(Order::Middle)
         .content_order(Order::Foreground)
         .closable(false)
         .show(ui.ctx(), |ui| {
            ui.set_width(300.0);
            ui.set_height(400.0);
            ui.spacing_mut().button_padding = vec2(10.0, 8.0);

            let swap_ui_open = self.swap_ui.is_open();
            let view_positions_open = false;

            ui.vertical_centered(|ui| {
               self.settings.show(swap_ui_open, view_positions_open, theme, ui);

               ui.add_space(10.0);

               let text = RichText::new("Close").size(theme.typography.normal);
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
                  let text = RichText::new(token0.symbol()).size(theme.typography.large);
                  let icon = icons.currency_icon_x32(token0, tint);
                  let label = Label::new(text, Some(icon)).image_on_left().interactive(false);
                  ui.add(label);
               });

               ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                  let balance = ctx.get_currency_balance(chain, owner, token0);
                  let b_text = format!("(Balance: {})", balance.abbreviated());
                  let text = RichText::new(b_text).size(theme.typography.normal);
                  let label = Label::new(text, None).interactive(false);
                  ui.add(label);
               });
            });

            // Currency 0 Amount & Value
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
               let value = NumericValue::value(amount0.f64(), price0_usd.f64());
               let text = RichText::new(format!("(${})", value.abbreviated()))
                  .size(theme.typography.normal);
               ui.label(text);

               ui.add_space(5.0);

               let text = RichText::new(amount0.abbreviated()).size(theme.typography.normal);
               ui.label(text);
            });
         });
      });

      // Currency 1
      frame.show(ui, |ui| {
         ui.horizontal(|ui| {
            ui.vertical(|ui| {
               ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                  let text = RichText::new(token1.symbol()).size(theme.typography.large);
                  let icon = icons.currency_icon_x32(token1, tint);
                  let label = Label::new(text, Some(icon)).image_on_left().interactive(false);
                  ui.add(label);
               });

               ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                  let balance = ctx.get_currency_balance(chain, owner, token1);
                  let b_text = format!("(Balance: {})", balance.abbreviated());
                  let text = RichText::new(b_text).size(theme.typography.normal);
                  let label = Label::new(text, None).interactive(false);
                  ui.add(label);
               });
            });

            // Currency B Amount & Value
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
               let value = NumericValue::value(amount1.f64(), price1_usd.f64());
               let text = RichText::new(format!("(${})", value.abbreviated()))
                  .size(theme.typography.normal);
               ui.label(text);

               ui.add_space(5.0);

               let text = RichText::new(amount1.abbreviated()).size(theme.typography.normal);
               ui.label(text);
            });
         });
      });
   });
}
