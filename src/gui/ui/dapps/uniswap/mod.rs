use egui::{Align, CornerRadius, CursorIcon, Layout, Order, RichText, Spinner, Ui, vec2};

use crate::assets::icons::Icons;
use crate::core::ZeusContext;
use crate::gui::ui::TokenSelectionWindow;
use crate::gui::ui::show_with_fade;
use egui_elements::{Button, Modal, Theme, visuals::ButtonVisuals};
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
            ui.spacing_mut().item_spacing.x = theme.spacing.sm;

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

   fn show_railgun_not_supported(&self, theme: &Theme, ui: &mut Ui) {
      let frame = theme.frame1;
      ui.vertical_centered(|ui| {
         frame.show(ui, |ui| {
            ui.set_width(self.size.0);
            ui.set_max_height(self.size.1);
            ui.spacing_mut().item_spacing = vec2(0.0, theme.spacing.sm);

            let text =
               RichText::new("Private swaps are not supported").size(theme.typography.very_large);
            ui.label(text);
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
      let frame = theme.frame1;

      show_with_fade(ui, "swap_ui_fade", self.open, |ui| {
         if ctx.privacy_mode {
            self.show_railgun_not_supported(theme, ui);
            return;
         }

         ui.vertical_centered(|ui| {
            frame.show(ui, |ui| {
               ui.set_width(self.size.0);
               ui.set_height(self.size.1);

               ui.spacing_mut().item_spacing = vec2(0.0, theme.spacing.sm);
               ui.spacing_mut().button_padding = theme.button_padding;

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
            ui.spacing_mut().button_padding = theme.button_padding;

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
