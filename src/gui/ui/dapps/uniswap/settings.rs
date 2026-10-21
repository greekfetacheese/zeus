use egui::{Align, Layout, RichText, Slider, Ui, vec2};

use crate::gui::SHARED_GUI;
use egui_elements::{Button, SecureTextEdit, Theme};
use egui_lucide::Lucide;

const DEFAULT_SLIPPAGE: f64 = 0.05;
const MAX_SLIPPAGE: f64 = 20.0;

const MIN_DEADLINE: u64 = 1; // minutes
const MAX_DEADLINE: u64 = 60; // minutes

const SLIPPAGE_TIP: &str =
   "Your transaction will revert if the price changes unfavorably by more than this percentage.";

const DEADLINE_TIP: &str = "The transaction will revert if it is pending for more than this time.";

/// Single sanitisation point for the swap slippage.
///
/// The raw text a user types is parsed here and only here. Anything that is not a finite
/// number is rejected in favour of [DEFAULT_SLIPPAGE]; a finite value is clamped into
/// `[0.0, MAX_SLIPPAGE]`. The settings widget writes this value back into both the displayed
/// string and the numeric field, and the swap flow reads that field, so the slippage the user
/// reads is the slippage that is enforced.
fn sanitise_slippage(raw: &str) -> f64 {
   let parsed = match raw.parse::<f64>() {
      Ok(value) if value.is_finite() => value,
      _ => return DEFAULT_SLIPPAGE,
   };

   parsed.clamp(0.0, MAX_SLIPPAGE)
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
         split_routing_enabled: true,
         max_hops: 2,
         max_split_routes: 3,
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

   /// Sanitise [Self::slippage] and write the result back into both the displayed string and
   /// the numeric field the swap flow enforces, so the two can never disagree.
   fn apply_slippage_input(&mut self) {
      let sanitised = sanitise_slippage(&self.slippage);
      self.slippage_f64 = sanitised;
      self.slippage = sanitised.to_string();
   }

   pub fn show(
      &mut self,
      swap_ui_open: bool,
      view_position_open: bool,
      theme: &Theme,
      ui: &mut Ui,
   ) {
      let button_visuals = theme.button_visuals();
      ui.spacing_mut().item_spacing = vec2(theme.spacing.sm, theme.spacing.md);

      // Slippage
      ui.horizontal(|ui| {
         let text = RichText::new("Slippage").size(theme.typography.normal);
         ui.label(text).on_hover_text(SLIPPAGE_TIP);

         let icon = Lucide::RefreshCw.size(14.0).color(theme.colors.text).image();
         let button = Button::image(icon).small().visuals(button_visuals);

         if ui.add(button).clicked() {
            self.slippage_f64 = DEFAULT_SLIPPAGE;
            self.slippage = DEFAULT_SLIPPAGE.to_string();
         }

         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            let output =
               SecureTextEdit::singleline(&mut self.slippage).desired_width(50.0).show(ui);

            if output.response.changed() {
               self.apply_slippage_input();
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
               crate::runtime::rt().spawn_blocking(move || {
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
               crate::runtime::rt().spawn_blocking(move || {
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
            crate::runtime::rt().spawn_blocking(move || {
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
            crate::runtime::rt().spawn_blocking(move || {
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

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn sanitise_slippage_clamps_and_rejects_invalid() {
      // Negative tolerance is clamped to zero, not kept and not turned into a 100%-of-quote
      // floor while the screen reports it.
      assert_eq!(sanitise_slippage("-1"), 0.0);
      assert_eq!(sanitise_slippage("0"), 0.0);
      assert_eq!(sanitise_slippage("0.5"), 0.5);

      // Non-finite input is rejected in favour of the default.
      assert_eq!(sanitise_slippage("nan"), DEFAULT_SLIPPAGE);
      assert_eq!(sanitise_slippage("inf"), DEFAULT_SLIPPAGE);

      // Above the cap is clamped to the cap.
      let above_max = (MAX_SLIPPAGE + 1.0).to_string();
      assert_eq!(sanitise_slippage(&above_max), MAX_SLIPPAGE);

      // Non-numeric text falls back to the default.
      assert_eq!(sanitise_slippage("abc"), DEFAULT_SLIPPAGE);
      assert_eq!(sanitise_slippage(""), DEFAULT_SLIPPAGE);
   }

   /// The bug: a negative entry left the field displaying "-1" while the swap enforced a
   /// zero-tolerance (100%-of-quote) floor. After the write-back both agree.
   #[test]
   fn negative_entry_keeps_displayed_and_enforced_in_sync() {
      let mut settings = UniswapSettingsUi::new();
      settings.slippage = "-1".to_string();

      settings.apply_slippage_input();

      assert_eq!(settings.slippage, "0");
      assert_eq!(settings.slippage_f64(), 0.0);

      // The displayed string round-trips through the same sanitiser that produced the field.
      assert_eq!(
         sanitise_slippage(&settings.slippage),
         settings.slippage_f64()
      );
   }
}
