use egui::{Align, CornerRadius, CursorIcon, Layout, Order, RichText, Ui, vec2};

use crate::assets::icons::Icons;
use crate::core::ZeusContext;
use crate::gui::SHARED_GUI;
use crate::gui::ui::TokenSelectionWindow;
use crate::gui::ui::common::notices;
use crate::gui::ui::show_with_fade;
use egui_elements::{Button, Modal, Spinner, Tabs, Theme, visuals::ButtonVisuals};
use egui_lucide::Lucide;
use std::str::FromStr;
use std::sync::Arc;

pub mod pool;
pub mod settings;
pub mod swap;

use pool::PoolsUi;
use settings::UniswapSettingsUi;
use swap::SwapUi;

/// The Uniswap card's views, in the order the tab strip shows them.
const TAB_POOLS: usize = 0;
const TAB_SWAP: usize = 1;
const TAB_SETTINGS: usize = 2;

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
      // The card is as wide as the view it holds: 600 for the pools table, 400 for the swap form.
      // A strip centred inside a narrower block than the card reads as off-centre, which is what
      // the old right-aligned button pair looked like.
      let width = if self.pools_ui.open {
         self.pools_ui.size.0
      } else {
         self.size.0
      };
      // One button's height, so the strip and the icon beside it share a line.
      let height = theme.typography.normal + 2.0 * theme.button_padding.y;
      // Three even segments, centred, with the refresh outside them on the right.
      let strip = (width * 0.7).min(360.0);

      ui.allocate_ui_with_layout(
         vec2(width, height),
         Layout::left_to_right(Align::Center),
         |ui| {
            ui.spacing_mut().item_spacing.x = theme.spacing.sm;
            ui.add_space(((width - strip) * 0.5).max(0.0));

            ui.allocate_ui_with_layout(
               vec2(strip, height),
               Layout::left_to_right(Align::Center),
               |ui| {
                  // The strip carries its own motion: the pill travels to the segment that is picked
                  // and stretches between the two while it is in flight, so switching view reads as
                  // one move instead of a repaint.
                  let mut selected = if self.settings.is_open() {
                     TAB_SETTINGS
                  } else if self.pools_ui.open {
                     TAB_POOLS
                  } else {
                     TAB_SWAP
                  };

                  let response = Tabs::new(
                     "uniswap_view_tabs",
                     &mut selected,
                     ["Pools", "Swap", "Settings"],
                  )
                  .fill_width(true)
                  .height(height)
                  .show(ui);

                  if !response.changed() {
                     return;
                  }

                  match selected {
                     TAB_POOLS => {
                        self.settings.close();
                        self.swap_ui.close();
                        self.pools_ui.open = true;
                     }
                     TAB_SWAP => {
                        self.settings.close();
                        self.pools_ui.open = false;
                        self.swap_ui.open();
                     }
                     _ => self.settings.open(),
                  }
               },
            );

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
               let button_visuals = theme.button_visuals();
               let visuals = ButtonVisuals {
                  bg_hover: button_visuals.bg_hover,
                  corner_radius: CornerRadius::same(25),
                  ..Default::default()
               };

               let syncing = self.swap_ui.pool_data_syncing
                  || self.swap_ui.syncing_pools
                  || self.swap_ui.balance_syncing;

               if syncing {
                  ui.add(Spinner::new().size(17.0).color(theme.colors.text));
                  return;
               }

               let icon = Lucide::RefreshCw.size(20.0).color(theme.colors.text).image();
               let button = Button::image(icon).small().visuals(visuals);
               let response = ui.add(button).on_hover_cursor(CursorIcon::PointingHand);

               if response.clicked() && self.swap_ui.is_open() {
                  self.swap_ui.refresh(&self.settings);
               }
            });
         },
      );

      // A little more air between the strip and the first line the view puts under it.
      ui.add_space(theme.spacing.md);
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
            notices::notice_panel(
               theme,
               self.size,
               "Private swaps are not supported",
               None,
               ui,
            );
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

               self.show_settings(ctx, theme, ui);
            });
         });
      });
   }

   pub fn show_settings(&mut self, ctx: &mut ZeusContext, theme: &Theme, ui: &mut Ui) {
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
            let pools_ui_open = self.pools_ui.open;

            ui.vertical_centered(|ui| {
               self.settings.show(
                  ctx,
                  swap_ui_open,
                  view_positions_open,
                  pools_ui_open,
                  theme,
                  ui,
               );

               ui.add_space(10.0);

               let text = RichText::new("Close").size(theme.typography.normal);
               let visuals = theme.button_visuals();
               if ui.add(Button::new(text).visuals(visuals)).clicked() {
                  self.settings.close();

                  // The V4 sync span lives in the pool catalog, so persist it when the panel
                  // closes instead of waiting for the shutdown save.
                  let _ = crate::runtime::rt().spawn_blocking(move || {
                     let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
                     ctx.save_pool_manager();
                  });
               }
            });
         });
   }
}
