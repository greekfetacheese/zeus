//! App-wide ambient particle pass.
//!
//! Painted once per frame on the main viewport, after the panels, so the
//! effects cover the whole app. The master switch lives in
//! [`AppearanceSettings::ambient_enabled`]; with it off this pass makes no crate
//! call at all, so no effect can request a repaint.

use crate::gui::appearance::AppearanceSettings;
use egui::Ui;
use egui_elements::{Light, Lightning, Snow};

/// Stable id for the app-wide snow effect.
///
/// The id carries the effect's animation state in egui's temporary data, so it
/// must be the same every frame — a fresh id per frame would animate nothing.
pub const AMBIENT_SNOW_ID: &str = "zeus_ambient_snow";

/// Stable id for the app-wide glow effect.
pub const AMBIENT_LIGHT_ID: &str = "zeus_ambient_light";

/// Stable id for the app-wide lightning effect.
pub const AMBIENT_LIGHTNING_ID: &str = "zeus_ambient_lightning";

pub struct Ambient;

impl Ambient {
   pub fn show(ui: &mut Ui, settings: &AppearanceSettings) {
      if !settings.ambient_enabled {
         return;
      }

      if settings.snow.active {
         Snow::from_settings(AMBIENT_SNOW_ID, &settings.snow).show(ui);
      }
      if settings.light.active {
         Light::from_settings(AMBIENT_LIGHT_ID, &settings.light).show(ui);
      }
      if settings.lightning.active {
         Lightning::from_settings(AMBIENT_LIGHTNING_ID, &settings.lightning).show(ui);
      }
   }
}

#[cfg(test)]
mod tests {
   use super::*;
   use std::time::Duration;

   /// Run one headless pass and return the repaint delay it asked for.
   fn repaint_delay(ctx: &egui::Context, settings: &AppearanceSettings) -> Duration {
      let output = ctx.run_ui(
         egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
               egui::Pos2::ZERO,
               egui::vec2(800.0, 600.0),
            )),
            ..Default::default()
         },
         |ui| Ambient::show(ui, settings),
      );
      let delay = output
         .viewport_output
         .get(&egui::ViewportId::ROOT)
         .map_or(Duration::MAX, |viewport| viewport.repaint_delay);
      output.drop_without_applying_deltas();
      delay
   }

   /// The performance promise: the master switch off means Zeus never calls into
   /// the crate, so nothing asks for a repaint; with snow on, the effect does.
   #[test]
   fn a_disabled_ambient_requests_no_repaint() {
      let ctx = egui::Context::default();
      let off = AppearanceSettings::new();
      let mut on = AppearanceSettings::new();
      on.ambient_enabled = true;
      // Isolate the continuous effect: lightning's default scheduler is off and
      // the glow would request a repaint for its own reason.
      on.light.active = false;
      on.lightning.active = false;

      // egui's first passes queue setup commands of their own; let them settle
      // so the measurement below is the pass's own request.
      repaint_delay(&ctx, &off);
      repaint_delay(&ctx, &off);

      assert_eq!(
         repaint_delay(&ctx, &off),
         Duration::MAX,
         "a disabled ambient pass must not ask for frames"
      );
      assert!(
         repaint_delay(&ctx, &on) < Duration::MAX,
         "active snow must keep the frame loop going"
      );
   }
}
