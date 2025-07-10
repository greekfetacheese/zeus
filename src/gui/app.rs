use crate::assets::icons::Icons;
use crate::core::{
   ZeusCtx,
   utils::{RT, load_theme_kind, update},
};
use crate::gui::{GUI, SHARED_GUI, window::window_frame};
use crate::server::run_server;
use eframe::{
   CreationContext,
   egui::{self, Frame},
};
use egui_theme::{Theme, ThemeKind};
use std::sync::Arc;

pub struct ZeusApp {
   pub set_style_on_startup: bool,
   pub updated_started: bool,
   pub ctx: ZeusCtx,
}

impl ZeusApp {
   pub fn new(cc: &CreationContext) -> Self {
      let time = std::time::Instant::now();
      let egui_ctx = cc.egui_ctx.clone();

      let theme_kind = if let Ok(kind) = load_theme_kind() {
         kind
      } else {
         ThemeKind::Nord
      };

      let theme = Theme::new(theme_kind);
      egui_ctx.set_style(theme.style.clone());

      // Load the icons
      let icons = Icons::new(&cc.egui_ctx).unwrap();
      let icons = Arc::new(icons);

      // Update the shared GUI with the current GUI state

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
      Self {
         set_style_on_startup: true,
         updated_started: false,
         ctx,
      }
   }

   fn set_style(&mut self, ctx: &egui::Context) {
      let ctx = ctx.clone();
      let style = SHARED_GUI.read(|shared_gui| shared_gui.theme.style.clone());
      ctx.set_style(style);
   }

   fn start_update(&mut self) {
      let ctx = self.ctx.clone();
      if !self.updated_started {
         let ctx_clone = ctx.clone();
         RT.spawn(async move {
            update::on_startup(ctx_clone).await;
         });
         let ctx_clone = ctx.clone();
         RT.spawn(async move {
            let _ = run_server(ctx_clone).await;
         });
         self.updated_started = true;
      }
   }

   fn on_shutdown(&mut self, ctx: &egui::Context, gui: &GUI) {
      if ctx.input(|i| i.viewport().close_requested()) {
         let clear_clipboard = gui
            .wallet_ui
            .export_key_ui
            .exporter
            .key_copied_time
            .is_some();
         if clear_clipboard {
            ctx.copy_text("".to_string());
         }
         let zeus_ctx = gui.ctx.clone();
         zeus_ctx.write_account(|account| {
            account.credentials_mut().erase();
            for wallet in account.wallets_mut() {
               wallet.key.erase();
            }
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

      if !self.updated_started {
         self.start_update();
      }

      if self.set_style_on_startup {
         self.set_style(ctx);
         self.set_style_on_startup = false;
      }

      SHARED_GUI.write(|gui| {
         self.on_shutdown(ctx, gui);

         let bg_color = gui.theme.colors.bg_color;
         let bg_frame = Frame::new().fill(bg_color);

         window_frame(ctx, "Zeus", bg_frame.clone(), |ui| {
            egui_theme::utils::apply_theme_changes(&gui.theme, ui);

            // Paint the Ui that belongs to the top panel
            egui::TopBottomPanel::top("top_panel")
               .exact_height(150.0)
               .resizable(false)
               .show_separator_line(true)
               .frame(bg_frame.clone())
               .show_inside(ui, |ui| {
                  if gui.ctx.logged_in() {
                     gui.show_top_panel(ui);
                  }
               });

            // Paint the Ui that belongs to the left panel
            egui::SidePanel::left("left_panel")
               .exact_width(150.0)
               .resizable(false)
               .show_separator_line(true)
               .frame(bg_frame.clone())
               .show_inside(ui, |ui| {
                  if gui.ctx.logged_in() {
                     gui.show_left_panel(ui);
                  }
               });

            if gui.should_show_right_panel() {
               // Paint the Ui that belongs to the left panel
               egui::SidePanel::right("right_panel")
                  .exact_width(250.0)
                  .resizable(false)
                  .show_separator_line(true)
                  .frame(bg_frame.clone())
                  .show_inside(ui, |ui| {
                     if gui.ctx.logged_in() {
                        gui.show_right_panel(ui);
                     }
                  });
            }

            // Paint the Ui that belongs to the central panel
            egui::CentralPanel::default()
               .frame(bg_frame.clone())
               .show_inside(ui, |ui| {
                  ui.with_layout(
                     egui::Layout::top_down(egui::Align::Center),
                     |ui| {
                        ui.set_max_width(900.0);
                        gui.show_central_panel(ui);
                     },
                  );
               });
         });

         #[cfg(feature = "dev")]
         gui.fps_metrics.update(time.elapsed().as_nanos());
      });
   }
}
