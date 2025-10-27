use egui::{FontData, FontDefinitions, FontFamily};

use crate::assets::{INTER_BOLD_18, icons::Icons};
use crate::core::{ZeusCtx, context::load_theme_kind, utils::update};
use crate::gui::{GUI, SHARED_GUI};
use crate::server::run_server;
use crate::utils::RT;
use eframe::{
   CreationContext,
   egui::{self, Frame},
};
use std::sync::Arc;
use zeus_theme::{Theme, ThemeKind, window::window_frame};

pub struct ZeusApp {
   pub style_has_been_set: bool,
   pub ctx: ZeusCtx,
}

impl ZeusApp {
   pub fn new(cc: &CreationContext) -> Self {
      let time = std::time::Instant::now();
      let egui_ctx = cc.egui_ctx.clone();

      setup_fonts(&egui_ctx);

      let theme_kind = if let Ok(kind) = load_theme_kind() {
         kind
      } else {
         ThemeKind::Dark
      };

      let theme = Theme::new(theme_kind);
      egui_ctx.set_style(theme.style.clone());

      let icons = Icons::new(&cc.egui_ctx).unwrap();
      let icons = Arc::new(icons);

      SHARED_GUI.write(|shared_gui| {
         shared_gui.icons = icons.clone();
         shared_gui.theme = theme.clone();
         shared_gui.egui_ctx = egui_ctx;
      });

      let ctx = SHARED_GUI.read(|shared_gui| shared_gui.ctx.clone());

      tracing::info!(
         "ZeusApp loaded in {}ms",
         time.elapsed().as_millis()
      );

      let ctx_clone = ctx.clone();
      RT.spawn(async move {
         update::test_and_measure_rpcs(ctx_clone).await;
      });

      let ctx_clone = ctx.clone();
      RT.spawn(async move {
         if ctx_clone.vault_exists() {
            update::on_startup(ctx_clone).await;
         }
      });

      let ctx_clone = ctx.clone();
      RT.spawn(async move {
         let _r = run_server(ctx_clone).await;
      });

      Self {
         style_has_been_set: false,
         ctx,
      }
   }

   fn on_shutdown(&mut self, ctx: &egui::Context, gui: &GUI) {
      if ctx.input(|i| i.viewport().close_requested()) {
         let zeus_ctx = gui.ctx.clone();
         zeus_ctx.write_vault(|vault| {
            vault.erase();
         });
      }
   }
}

impl eframe::App for ZeusApp {
   fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
      egui::Rgba::TRANSPARENT.to_array()
   }

   fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
      #[cfg(feature = "dev")]
      let time = std::time::Instant::now();

      SHARED_GUI.write(|gui| {
         self.on_shutdown(ctx, gui);

         // This is needed for Windows
         if !self.style_has_been_set {
            let style = gui.theme.style.clone();
            ctx.set_style(style);
            self.style_has_been_set = true;
         }

         let theme = gui.theme.clone();
         let bg_color = theme.colors.bg;
         let panel_frame = Frame::new().fill(bg_color);

         window_frame(ctx, "Zeus", theme, |ui| {
            #[cfg(feature = "dev")]
            zeus_theme::utils::apply_theme_changes(&mut gui.theme, ui);

            // Paint the Ui that belongs to the top panel
            egui::TopBottomPanel::top("top_panel")
               .min_height(150.0)
               .resizable(false)
               .show_separator_line(false)
               .frame(panel_frame)
               .show_inside(ui, |ui| {
                  if gui.ctx.vault_unlocked() {
                     gui.show_top_panel(ui);
                  }
               });

            // Paint the Ui that belongs to the left panel
            egui::SidePanel::left("left_panel")
               .min_width(150.0)
               .max_width(150.0)
               .resizable(false)
               .show_separator_line(true)
               .show_inside(ui, |ui| {
                  if gui.ctx.vault_unlocked() {
                     ui.add_space(40.0);
                     gui.show_left_panel(ui);
                  }
               });

            if gui.should_show_right_panel() {
               // Paint the Ui that belongs to the left panel
               egui::SidePanel::right("right_panel")
                  .min_width(150.0)
                  .resizable(false)
                  .show_separator_line(true)
                  .frame(panel_frame)
                  .show_inside(ui, |ui| {
                     if gui.ctx.vault_unlocked() {
                        gui.show_right_panel(ui);
                     }
                  });
            }

            // Paint the Ui that belongs to the central panel
            egui::CentralPanel::default().frame(panel_frame).show_inside(ui, |ui| {
               gui.show_central_panel(ui);
            });
         });

         #[cfg(feature = "dev")]
         gui.fps_metrics.update(time.elapsed().as_nanos());
      });
   }
}

pub fn setup_fonts(ctx: &egui::Context) {
   let mut fonts = FontDefinitions::default();

   let inter_bold = FontData::from_static(INTER_BOLD_18);
   fonts.font_data.insert("inter_bold".to_owned(), Arc::new(inter_bold));

   let mut newfam = std::collections::BTreeMap::new();
   newfam.insert(
      FontFamily::Name("inter_bold".into()),
      vec!["inter_bold".to_owned()],
   );
   fonts.families.append(&mut newfam);

   ctx.set_fonts(fonts);
}
