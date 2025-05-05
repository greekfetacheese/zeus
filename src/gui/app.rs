use crate::assets::icons::Icons;
use crate::core::{
   ZeusCtx,
   utils::{RT, update},
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
   pub on_startup: bool,
   pub updated_started: bool,
   pub ctx: ZeusCtx,
}

impl ZeusApp {
   pub fn new(cc: &CreationContext) -> Self {
      let ctx = cc.egui_ctx.clone();
      let ctx_clone = ctx.clone();

      let mut theme = Theme::new(ThemeKind::Mocha);
      theme.style.animation_time = 0.3;
      ctx.set_style(theme.style.clone());

      // Load the icons
      let icons = Icons::new(&cc.egui_ctx).unwrap();
      let icons = Arc::new(icons);

      let gui = GUI::new(icons.clone(), theme.clone(), ctx_clone);

      // Update the shared GUI with the current GUI state

      SHARED_GUI.write(|shared_gui| {
         shared_gui.icons = gui.icons.clone();
         shared_gui.theme = gui.theme.clone();
         shared_gui.egui_ctx = gui.egui_ctx.clone();
         shared_gui.ctx = gui.ctx.clone();
      });

      Self {
         on_startup: true,
         updated_started: false,
         ctx: gui.ctx.clone(),
      }
   }

   fn start_up(&mut self, ctx: &egui::Context) {
      let ctx = ctx.clone();
      let style = SHARED_GUI.read(|shared_gui| shared_gui.theme.style.clone());
      ctx.set_style(style);
   }

   fn start_update(&mut self) {
      let ctx = self.ctx.clone();
      let logged_in = self.ctx.logged_in();
      if logged_in && !self.updated_started {
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
      }
   }
}

impl eframe::App for ZeusApp {
   fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
      egui::Rgba::TRANSPARENT.to_array()
   }

   fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
      if !self.updated_started {
         self.start_update();
      }

      if self.on_startup {
         self.start_up(ctx);
         self.on_startup = false;
      }

      SHARED_GUI.write(|gui| {
         self.on_shutdown(ctx, gui);

         let bg_color = if gui.show_overlay {
            gui.theme.colors.overlay_color
         } else {
            gui.theme.colors.bg_color
         };

         let bg_frame = Frame::new().fill(bg_color);

         window_frame(ctx, "Zeus", bg_frame.clone(), |ui| {
            egui_theme::utils::apply_theme_changes(&gui.theme, ui);

            // Paint the Ui that belongs to the top panel
            egui::TopBottomPanel::top("top_panel")
               .exact_height(180.0)
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

            // Paint the Ui that belongs to the central panel
            egui::CentralPanel::default()
               .frame(bg_frame.clone())
               .show_inside(ui, |ui| {
                  ui.vertical_centered(|ui| {
                     gui.show_central_panel(ui);
                  });
               });
         });
      });
   }
}
