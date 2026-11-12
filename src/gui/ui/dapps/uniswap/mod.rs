use egui::{
   Align, CornerRadius, CursorIcon, Layout, Rect, RichText, Sense, Ui, UiBuilder, pos2, vec2,
};

use crate::assets::icons::Icons;
use crate::core::ZeusContext;
use crate::gui::ui::TokenSelectionWindow;
use crate::gui::ui::common::notices;
use crate::gui::ui::show_with_fade;
use egui_elements::{Button, Spinner, Tabs, Theme, visuals::ButtonVisuals};
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

/// Which page of the Uniswap window is visible.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Page {
   Pools,
   #[default]
   Swap,
   Settings,
}

impl Page {
   fn index(self) -> usize {
      match self {
         Page::Pools => 0,
         Page::Swap => 1,
         Page::Settings => 2,
      }
   }

   fn from_index(index: usize) -> Self {
      match index {
         0 => Page::Pools,
         1 => Page::Swap,
         _ => Page::Settings,
      }
   }

   fn labels() -> [&'static str; 3] {
      ["Pools", "Swap", "Settings"]
   }
}

const HEADER_HEIGHT: f32 = 30.0;

/// A UI for a dex like Uniswap
pub struct UniswapUi {
   open: bool,
   page: Page,
   pub size: (f32, f32),
   pub settings: UniswapSettingsUi,
   pub swap_ui: SwapUi,
   pub pools_ui: PoolsUi,
}

impl UniswapUi {
   pub fn new() -> Self {
      let mut swap_ui = SwapUi::new();
      swap_ui.open();

      Self {
         open: false,
         page: Page::Swap,
         size: (400.0, 500.0),
         settings: UniswapSettingsUi::new(),
         swap_ui,
         pools_ui: PoolsUi::new(),
      }
   }

   pub fn open(&mut self) {
      self.open = true;
   }

   pub fn close(&mut self) {
      self.open = false;
      self.page = Page::Swap;
      self.swap_ui.open();
      self.swap_ui.amount_in_field.reset();
      self.swap_ui.amount_out_field.reset();
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   fn set_page(&mut self, page: Page) {
      if self.page == page {
         return;
      }

      self.page = page;

      match page {
         Page::Swap => self.swap_ui.open(),
         // Leaving for the pool table clears the swap form, exactly as the old Pools button did.
         Page::Pools => self.swap_ui.close(),
         // Crossing to settings keeps whatever the user typed.
         Page::Settings => {}
      }
   }

   fn header(&mut self, theme: &Theme, ui: &mut Ui) {
      let (row, _) = ui.allocate_exact_size(
         vec2(ui.available_width(), HEADER_HEIGHT),
         Sense::hover(),
      );

      // Pools / Swap / Settings, centred on the header's own centre line.
      ui.scope_builder(
         UiBuilder::new().max_rect(row).layout(Layout::top_down(Align::Center)),
         |ui| {
            let mut selected = self.page.index();
            let tabs = Tabs::new("uniswap_pages", &mut selected, Page::labels())
               .fill_width(false)
               .height(HEADER_HEIGHT);
            if tabs.show(ui).changed() {
               self.set_page(Page::from_index(selected));
            }
         },
      );

      // Refresh (or the sync spinner) stays on the far edge, where it has always been.
      let edge = Rect::from_min_max(
         pos2(row.right() - 2.0 * HEADER_HEIGHT, row.top()),
         row.right_bottom(),
      );
      ui.scope_builder(
         UiBuilder::new().max_rect(edge).layout(Layout::right_to_left(Align::Center)),
         |ui| {
            let button_visuals = theme.button_visuals();

            let icon = Lucide::RefreshCw.size(20.0).color(theme.colors.text).image();

            let syncing = self.swap_ui.pool_data_syncing
               || self.swap_ui.syncing_pools
               || self.swap_ui.balance_syncing;

            if !syncing {
               let visuals = ButtonVisuals {
                  bg_hover: button_visuals.bg_hover,
                  corner_radius: CornerRadius::same(25),
                  ..Default::default()
               };
               let button = Button::image(icon).small().visuals(visuals);
               let res = ui.add(button).on_hover_cursor(CursorIcon::PointingHand);

               if res.clicked() && self.page == Page::Swap {
                  self.swap_ui.refresh(&self.settings);
               }
            } else {
               ui.add(Spinner::new().size(17.0).color(theme.colors.text));
            }
         },
      );
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

               self.header(theme, ui);

               match self.page {
                  Page::Swap => self.swap_ui.show(
                     ctx,
                     theme,
                     icons.clone(),
                     token_selection,
                     &self.settings,
                     ui,
                  ),
                  Page::Pools => self.pools_ui.show(ctx, theme, icons.clone(), ui),
                  Page::Settings => self.show_settings_page(theme, ui),
               }
            });
         });
      });
   }

   fn show_settings_page(&mut self, theme: &Theme, ui: &mut Ui) {
      ui.vertical_centered(|ui| {
         ui.set_width(self.size.0);
         ui.set_max_height(self.size.1);
         ui.spacing_mut().item_spacing = vec2(theme.spacing.sm, theme.spacing.md);
         ui.spacing_mut().button_padding = theme.button_padding;

         // Settings is its own view now, so every swap knob belongs on it.
         self.settings.show(true, false, theme, ui);
      });
   }
}
