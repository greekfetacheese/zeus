use egui::{Align, Layout, RichText, Slider, Ui, vec2};

use crate::gui::SHARED_GUI;
use crate::gui::ui::REFRESH;
use crate::utils::RT;
use egui_elements::{Button, SecureTextEdit, Theme};

const DEFAULT_SLIPPAGE: f64 = 0.05;
const MAX_SLIPPAGE: f64 = 20.0;

const MIN_DEADLINE: u64 = 1; // minutes
const MAX_DEADLINE: u64 = 60; // minutes

const SLIPPAGE_TIP: &str =
   "Your transaction will revert if the price changes unfavorably by more than this percentage.";

const DEADLINE_TIP: &str = "The transaction will revert if it is pending for more than this time.";

#[derive(Clone)]
pub struct UniswapSettingsUi {
   open: bool,
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
   pub fn new() -> Self {
      Self {
         open: false,
         swap_on_v2: true,
         swap_on_v3: true,
         swap_on_v4: true,
         split_routing_enabled: false,
         max_hops: 5,
         max_split_routes: 5,
         deadline: 5,
         mev_protect: true,
         slippage: DEFAULT_SLIPPAGE.to_string(),
         slippage_f64: DEFAULT_SLIPPAGE,
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

   pub fn slippage_f64(&self) -> f64 {
      self.slippage_f64
   }

   pub fn show(
      &mut self,
      swap_ui_open: bool,
      view_position_open: bool,
      theme: &Theme,
      ui: &mut Ui,
   ) {
      let button_visuals = theme.button_visuals();
      ui.spacing_mut().item_spacing = vec2(10.0, 15.0);

      // Slippage
      ui.horizontal(|ui| {
         let text = RichText::new("Slippage").size(theme.typography.normal);
         ui.label(text).on_hover_text(SLIPPAGE_TIP);

         let text = RichText::new(REFRESH).size(theme.typography.very_small);
         let button = Button::new(text).visuals(button_visuals).small();

         if ui.add(button).clicked() {
            self.slippage_f64 = DEFAULT_SLIPPAGE;
            self.slippage = DEFAULT_SLIPPAGE.to_string();
         }

         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            let output =
               SecureTextEdit::singleline(&mut self.slippage).desired_width(50.0).show(ui);

            if output.response.changed() {
               let adjusted_slippage = self.slippage.parse().unwrap_or(DEFAULT_SLIPPAGE);

               let new_slippage = if adjusted_slippage > MAX_SLIPPAGE {
                  MAX_SLIPPAGE
               } else {
                  adjusted_slippage
               };

               self.slippage_f64 = new_slippage;
               self.slippage = new_slippage.to_string();
            }
         });
      });

      // Swap deadline
      ui.horizontal(|ui| {
         let text = RichText::new("Deadline (minutes)").size(theme.typography.normal);
         ui.label(text).on_hover_text(DEADLINE_TIP);

         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            ui.label(RichText::new(self.deadline.to_string()).size(theme.typography.normal));
            ui.add(Slider::new(&mut self.deadline, MIN_DEADLINE..=MAX_DEADLINE).show_value(false));
         });
      });

      if swap_ui_open {
         // Max Hops
         ui.horizontal(|ui| {
            ui.label(RichText::new("Max Hops").size(theme.typography.normal));

            let res = ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
               ui.label(RichText::new(self.max_hops.to_string()).size(theme.typography.normal));
               ui.add(Slider::new(&mut self.max_hops, 1..=10).show_value(false))
            });

            if res.inner.changed() {
               RT.spawn_blocking(move || {
                  SHARED_GUI.write(|gui| {
                     let ctx = gui.ctx.clone();
                     let settings = &gui.uniswap.settings;
                     gui.uniswap.swap_ui.get_quote(ctx, settings);
                  });
               });
            }
         });

         // Max Split Routes
         ui.horizontal(|ui| {
            ui.label(RichText::new("Max Routes").size(theme.typography.normal));

            let res = ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
               ui.label(
                  RichText::new(self.max_split_routes.to_string()).size(theme.typography.normal),
               );
               ui.add(Slider::new(&mut self.max_split_routes, 1..=10).show_value(false))
            });

            if res.inner.changed() {
               RT.spawn_blocking(move || {
                  SHARED_GUI.write(|gui| {
                     let ctx = gui.ctx.clone();
                     let settings = &gui.uniswap.settings;
                     gui.uniswap.swap_ui.get_quote(ctx, settings);
                  });
               });
            }
         });

         let text = RichText::new("MEV Protect").size(theme.typography.normal);
         ui.checkbox(&mut self.mev_protect, text);

         let text = RichText::new("Split Routing").size(theme.typography.normal);
         let res = ui.checkbox(&mut self.split_routing_enabled, text);
         if res.changed() {
            RT.spawn_blocking(move || {
               SHARED_GUI.write(|gui| {
                  let ctx = gui.ctx.clone();
                  let settings = &gui.uniswap.settings;
                  gui.uniswap.swap_ui.get_quote(ctx, settings);
               });
            });
         }

         let text = RichText::new("Swap on V2").size(theme.typography.normal);
         let v2_was_on = self.swap_on_v2;
         let v2_res = ui.checkbox(&mut self.swap_on_v2, text);

         let text = RichText::new("Swap on V3").size(theme.typography.normal);
         let v3_was_on = self.swap_on_v3;
         let v3_res = ui.checkbox(&mut self.swap_on_v3, text);

         let text = RichText::new("Swap on V4").size(theme.typography.normal);
         let v4_was_on = self.swap_on_v4;
         let v4_res = ui.checkbox(&mut self.swap_on_v4, text);

         if v2_res.changed() || v3_res.changed() || v4_res.changed() {
            let update_v2 = self.swap_on_v2 && !v2_was_on;
            let update_v3 = self.swap_on_v3 && !v3_was_on;
            let update_v4 = self.swap_on_v4 && !v4_was_on;
            RT.spawn_blocking(move || {
               SHARED_GUI.write(|gui| {
                  gui.uniswap.swap_ui.update_pool_state(update_v2, update_v3, update_v4);
               });
            });
         }

         let text = RichText::new("Simulate Mode").size(theme.typography.normal);
         ui.checkbox(&mut self.simulate_mode, text);
      }

      if view_position_open {
         let visuals = theme.text_edit_visuals();
         let text = RichText::new("Number of Days to go back").size(theme.typography.normal);
         ui.label(text);
         ui.add(SecureTextEdit::singleline(&mut self.days).desired_width(25.0).visuals(visuals));
      }
   }
}
