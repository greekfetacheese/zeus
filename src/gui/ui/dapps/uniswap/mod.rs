use egui::{
   Align, CornerRadius, CursorIcon, Layout, Rect, RichText, Role, Sense, Ui, UiBuilder, WidgetInfo,
   pos2, vec2,
};

use crate::assets::icons::Icons;
use crate::core::ZeusContext;
use crate::gui::ui::TokenSelectionWindow;
use crate::gui::ui::common::notices;
use crate::gui::ui::show_with_fade;
use egui_elements::{
   Button, Easing, Motion, Spinner, Tabs, Theme, animate_from, visuals::ButtonVisuals,
};
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
   fn name(self) -> &'static str {
      match self {
         Page::Pools => "Pools",
         Page::Swap => "Swap",
         Page::Settings => "Settings",
      }
   }

   /// The pages the tab strip offers.
   ///
   /// The pool table is a list of pools to hunt through and import from by eye — a search field,
   /// a version filter and a table of pairs — so a reader is not walked through it. The two pages
   /// it used to sit between keep their places.
   fn tabs(reader_attached: bool) -> &'static [Page] {
      match reader_attached {
         false => &[Page::Pools, Page::Swap, Page::Settings],
         true => &[Page::Swap, Page::Settings],
      }
   }
}

/// Header row height. The tab strip fills it, so the row and the strip are one measurement.
const HEADER_HEIGHT: f32 = 36.0;

/// The tab strip's width, the same on every page. A strip sized from the frame widened and narrowed
/// with it, which read as a layout jump on every tab switch.
const HEADER_STRIP_WIDTH: f32 = 300.0;

/// A UI for a dex like Uniswap
pub struct UniswapUi {
   open: bool,
   page: Page,
   /// Whether an assistive technology is attached. The pool table is a list of pools to hunt
   /// through and import from by eye, so a reader is not offered it.
   reader_attached: bool,
   /// Input time the visible page was selected at, so the page body fades in from it.
   page_shown_at: Option<f64>,
   /// The sentence the window wants said, drained by the frame right after the window has drawn.
   notice: Option<String>,
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
         reader_attached: false,
         page_shown_at: None,
         notice: None,
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
      // A sentence raised on the page that was left is not about the page the window opens on.
      self.notice = None;
      self.swap_ui.open();
      self.swap_ui.amount_in_field.reset();
      self.swap_ui.amount_out_field.reset();
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   /// Tell the window whether a screen reader is attached: the pool table is not offered to one.
   pub fn set_reader_attached(&mut self, attached: bool) {
      self.reader_attached = attached;
   }

   /// Put the pool table in front, for a test that draws the page the window opens on.
   #[cfg(test)]
   pub fn open_pools_for_test(&mut self) {
      self.set_page(Page::Pools);
   }

   /// The words the frame should say, drained by it right after the window has drawn.
   pub fn take_notice(&mut self) -> Option<String> {
      self.notice.take()
   }

   fn set_page(&mut self, page: Page) {
      if self.page == page {
         return;
      }

      self.page = page;
      self.page_shown_at = None;

      match page {
         Page::Swap => self.swap_ui.open(),
         // Leaving for the pool table clears the swap form, exactly as the old Pools button did.
         Page::Pools => self.swap_ui.close(),
         // Crossing to settings keeps whatever the user typed.
         Page::Settings => {}
      }
   }

   /// Width the frame eases to for the visible page: the pool table is wider than the swap form.
   fn page_width(&self) -> f32 {
      match self.page {
         Page::Pools => self.pools_ui.size.0,
         Page::Swap | Page::Settings => self.size.0,
      }
   }

   fn header(&mut self, theme: &Theme, ui: &mut Ui) {
      let (row, _) = ui.allocate_exact_size(
         vec2(ui.available_width(), HEADER_HEIGHT),
         Sense::hover(),
      );

      // Pools / Swap / Settings, centred on the header's own centre line.
      let strip = Rect::from_center_size(
         row.center(),
         vec2(HEADER_STRIP_WIDTH, HEADER_HEIGHT),
      );
      ui.scope_builder(
         UiBuilder::new().max_rect(strip).layout(Layout::top_down(Align::Center)),
         |ui| {
            let tabs = Page::tabs(self.reader_attached);
            let mut selected = tabs.iter().position(|page| *page == self.page).unwrap_or(0);
            let labels: Vec<&str> = tabs.iter().map(|page| page.name()).collect();

            let strip = Tabs::new("uniswap_pages", &mut selected, labels)
               .fill_width(true)
               .height(HEADER_HEIGHT);
            if strip.show(ui).changed() {
               self.set_page(tabs[selected]);
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
               // The button is a picture of a refresh arrow: its text alternative says what
               // pressing it refreshes.
               let button =
                  Button::image(icon).small().visuals(visuals).alt("Refresh pools and quote");
               let res = ui.add(button).on_hover_cursor(CursorIcon::PointingHand);

               if res.clicked() && self.page == Page::Swap {
                  self.swap_ui.refresh(&self.settings);
               }
            } else {
               let spinner = ui.add(Spinner::new().size(17.0));

               // A spinner is a picture with no words: it says what is being fetched, the way the
               // loading window's spinner says it is working.
               spinner.widget_info(|| {
                  WidgetInfo::labeled(
                     Role::ProgressIndicator,
                     ui.is_enabled(),
                     "Refreshing pools and quote",
                  )
               });
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

         // The page a reader is not offered cannot stay in front: its segment is gone from the
         // strip, so a window left on the pool table when a reader attaches goes back to the swap
         // form instead of stranding them on a page with no way out.
         if self.reader_attached && self.page == Page::Pools {
            self.set_page(Page::Swap);
         }

         ui.vertical_centered(|ui| {
            // The frame eases to the page's width on the host's own animation time, so a switch runs
            // at the same speed as every other transition in the GUI. `animate_from` seeds the first
            // sight settled at the target, so opening the window does not grow the frame from zero.
            let target = self.page_width();
            let width = animate_from(
               ui,
               "uniswap_frame_width",
               target,
               target,
               Motion::from_ui(ui),
            );
            let settled = (width - target).abs() < 0.5;

            frame.show(ui, |ui| {
               ui.set_width(width);
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

               // Nothing in a page is laid out while the frame is resizing. The measuring containers
               // both cache on the available size: the pool `Table` and the amount field's `Flex`
               // row re-measure on every width change, and each re-measure asks egui for another
               // layout pass — that is the `request_discard … frames in a row` perf warning, and the
               // frame-rate dip behind it. The body is painted once the width has landed.
               if !settled {
                  return;
               }

               let now = ui.input(|i| i.time);
               let fade = match self.page_shown_at {
                  Some(shown_at) => {
                     let seconds = ui.style().animation_time as f64;

                     if seconds <= 0.0 {
                        1.0
                     } else {
                        Easing::EaseOut.apply(((now - shown_at) / seconds) as f32)
                     }
                  }
                  None => {
                     self.page_shown_at = Some(now);
                     0.0
                  }
               };

               if fade < 1.0 {
                  ui.ctx().request_repaint();
               }

               ui.scope(|ui| {
                  ui.multiply_opacity(fade);

                  match self.page {
                     Page::Swap => {
                        self.swap_ui.show(
                           ctx,
                           theme,
                           icons.clone(),
                           token_selection,
                           &self.settings,
                           ui,
                        );

                        // The form raises its sentences from inside its own draw, so it hands them
                        // up to the window's channel the way the pool table does.
                        if let Some(text) = self.swap_ui.take_notice() {
                           self.notice = Some(text);
                        }
                     }
                     Page::Pools => {
                        self.pools_ui.show(ctx, theme, icons.clone(), ui);

                        // The table raises its count from inside its own draw, so it hands the
                        // sentence up to the window's channel like the settings page does.
                        if let Some(text) = self.pools_ui.take_notice() {
                           self.notice = Some(text);
                        }
                     }
                     Page::Settings => self.show_settings_page(theme, ui),
                  }
               });
            });
         });
      });
   }

   fn show_settings_page(&mut self, theme: &Theme, ui: &mut Ui) {
      // A left-aligned column: the frame centres its children, which centred every checkbox on its
      // own width and left the group ragged.
      ui.with_layout(Layout::top_down(Align::Min), |ui| {
         ui.set_max_height(self.size.1);
         ui.spacing_mut().item_spacing = vec2(theme.spacing.sm, theme.spacing.md);
         ui.spacing_mut().button_padding = theme.button_padding;

         // The tab strip owns the top of the page; keep the first row off it.
         ui.add_space(theme.spacing.lg);

         // Settings is its own view now, so every swap knob belongs on it.
         self.settings.show(true, false, theme, ui);

         // The page has no channel of its own, so the sentence it raised comes up to the window's.
         if let Some(text) = self.settings.take_notice() {
            self.notice = Some(text);
         }
      });
   }
}
