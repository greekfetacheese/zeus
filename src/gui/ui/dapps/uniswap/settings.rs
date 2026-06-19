use egui::{Align, Layout, RichText, Slider, Ui, vec2};

use crate::core::ZeusCtx;
use crate::gui::SHARED_GUI;
use crate::gui::ui::REFRESH;
use crate::utils::RT;
use zeus_theme::{OverlayManager, Theme};
use zeus_widgets::{Button, SecureTextEdit};

const MIN_SLIPPAGE: f64 = 0.1;
const MAX_SLIPPAGE: f64 = 20.0;
const DEFAULT_SLIPPAGE: f64 = 0.5;

const MIN_DEADLINE: u64 = 1; // minutes
const MAX_DEADLINE: u64 = 60; // minutes

const SLIPPAGE_TIP: &str =
   "Your transaction will revert if the price changes unfavorably by more than this percentage.";

const DEADLINE_TIP: &str = "The transaction will revert if it is pending for more than this time.";

#[derive(Clone)]
pub struct UniswapSettingsUi {
   open: bool,
   overlay: OverlayManager,
   pub swap_on_v2: bool,
   pub swap_on_v3: bool,
   pub swap_on_v4: bool,
   pub split_routing_enabled: bool,
   pub max_hops: usize,
   pub max_split_routes: usize,
   /// Deadline in minutes
   pub deadline: u64,
   pub mev_protect: bool,
   pub slippage: String,
   slippage_f64: f64,
   /// Applies only to [SwapUi]
   pub simulate_mode: bool,
   /// Days to go back to sync positions
   /// Applies only to [ViewPositionsUi]
   pub days: String,
}

impl UniswapSettingsUi {
   pub fn new(overlay: OverlayManager) -> Self {
      Self {
         open: false,
         overlay,
         swap_on_v2: true,
         swap_on_v3: true,
         swap_on_v4: true,
         split_routing_enabled: false,
         max_hops: 5,
         max_split_routes: 5,
         deadline: 5,
         mev_protect: true,
         slippage: "0.5".to_string(),
         slippage_f64: 0.5,
         simulate_mode: false,
         days: String::new(),
      }
   }

   pub fn open(&mut self) {
      if !self.open {
         self.overlay.window_opened();
         self.open = true;
      }
   }

   pub fn close(&mut self) {
      self.overlay.window_closed();
      self.open = false;
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn slippage_f64(&self) -> f64 {
      self.slippage_f64
   }

   pub fn show(
      &mut self,
      ctx: ZeusCtx,
      swap_ui_open: bool,
      view_position_open: bool,
      theme: &Theme,
      ui: &mut Ui,
   ) {
      let button_visuals = theme.button_visuals();
      ui.spacing_mut().item_spacing = vec2(10.0, 15.0);

      // Slippage
      ui.horizontal(|ui| {
         let text = RichText::new("Slippage").size(theme.text_sizes.normal);
         ui.label(text).on_hover_text(SLIPPAGE_TIP);

         let text = RichText::new(REFRESH).size(theme.text_sizes.very_small);
         let button = Button::new(text).visuals(button_visuals).small();

         if ui.add(button).clicked() {
            self.slippage_f64 = DEFAULT_SLIPPAGE;
            self.slippage = DEFAULT_SLIPPAGE.to_string();
         }

         let res = ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            let slippage = format!("{:.1}", self.slippage_f64);
            ui.label(RichText::new(slippage).size(theme.text_sizes.normal));

            ui.add(
               Slider::new(
                  &mut self.slippage_f64,
                  MIN_SLIPPAGE..=MAX_SLIPPAGE,
               )
               .show_value(false),
            )
         });

         if res.inner.changed() {
            self.slippage = self.slippage_f64.to_string();
         }
      });

      // Swap deadline
      ui.horizontal(|ui| {
         let text = RichText::new("Deadline (minutes)").size(theme.text_sizes.normal);
         ui.label(text).on_hover_text(DEADLINE_TIP);

         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            ui.label(RichText::new(self.deadline.to_string()).size(theme.text_sizes.normal));
            ui.add(Slider::new(&mut self.deadline, MIN_DEADLINE..=MAX_DEADLINE).show_value(false));
         });
      });

      if swap_ui_open {
         // Max Hops
         ui.horizontal(|ui| {
            ui.label(RichText::new("Max Hops").size(theme.text_sizes.normal));

            let res = ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
               ui.label(RichText::new(self.max_hops.to_string()).size(theme.text_sizes.normal));
               ui.add(Slider::new(&mut self.max_hops, 1..=10).show_value(false))
            });

            if res.inner.changed() {
               let ctx_clone = ctx.clone();
               RT.spawn_blocking(move || {
                  SHARED_GUI.write(|gui| {
                     let settings = &gui.uniswap.settings;
                     gui.uniswap.swap_ui.get_quote(ctx_clone, settings);
                  });
               });
            }
         });

         // Max Split Routes
         ui.horizontal(|ui| {
            ui.label(RichText::new("Max Routes").size(theme.text_sizes.normal));

            let res = ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
               ui.label(
                  RichText::new(self.max_split_routes.to_string()).size(theme.text_sizes.normal),
               );
               ui.add(Slider::new(&mut self.max_split_routes, 1..=10).show_value(false))
            });

            if res.inner.changed() {
               let ctx_clone = ctx.clone();
               RT.spawn_blocking(move || {
                  SHARED_GUI.write(|gui| {
                     let settings = &gui.uniswap.settings;
                     gui.uniswap.swap_ui.get_quote(ctx_clone, settings);
                  });
               });
            }
         });

         let text = RichText::new("MEV Protect").size(theme.text_sizes.normal);
         ui.checkbox(&mut self.mev_protect, text);

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

         let text = RichText::new("Swap on V2").size(theme.text_sizes.normal);
         let v2_was_on = self.swap_on_v2;
         let v2_res = ui.checkbox(&mut self.swap_on_v2, text);

         let text = RichText::new("Swap on V3").size(theme.text_sizes.normal);
         let v3_was_on = self.swap_on_v3;
         let v3_res = ui.checkbox(&mut self.swap_on_v3, text);

         let text = RichText::new("Swap on V4").size(theme.text_sizes.normal);
         let v4_was_on = self.swap_on_v4;
         let v4_res = ui.checkbox(&mut self.swap_on_v4, text);

         if v2_res.changed() || v3_res.changed() || v4_res.changed() {
            let ctx_clone = ctx.clone();
            let update_v2 = self.swap_on_v2 && !v2_was_on;
            let update_v3 = self.swap_on_v3 && !v3_was_on;
            let update_v4 = self.swap_on_v4 && !v4_was_on;
            RT.spawn_blocking(move || {
               SHARED_GUI.write(|gui| {
                  gui.uniswap
                     .swap_ui
                     .update_pool_state(ctx_clone, update_v2, update_v3, update_v4);
               });
            });
         }

         let text = RichText::new("Simulate Mode").size(theme.text_sizes.normal);
         ui.checkbox(&mut self.simulate_mode, text);
      }

      if view_position_open {
         let visuals = theme.text_edit_visuals();
         let text = RichText::new("Number of Days to go back").size(theme.text_sizes.normal);
         ui.label(text);
         ui.add(SecureTextEdit::singleline(&mut self.days).desired_width(25.0).visuals(visuals));
      }
   }
}
