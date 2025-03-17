use std::path::PathBuf;
use std::sync::Arc;

use crate::assets::icons::Icons;
use crate::core::{
   ZeusCtx,
   utils::{RT, update},
};
use crate::gui::{GUI, SHARED_GUI, window::window_frame};
use eframe::{
   CreationContext,
   egui::{self, Frame},
};
use egui_theme::{Theme, ThemeKind};

pub struct ZeusApp {
   pub on_startup: bool,
   pub updated_started: bool,
   pub ctx: ZeusCtx,
}

impl ZeusApp {
   pub fn new(cc: &CreationContext) -> Self {
      let ctx = cc.egui_ctx.clone();
      let ctx_clone = ctx.clone();

      let path_exists = PathBuf::from("my-custom-theme.json").exists();
      let mut theme = if path_exists {
         let path = PathBuf::from("my-custom-theme.json");
         let theme = Theme::from_custom(path).unwrap();
         theme
      } else {
         let theme = Theme::new(ThemeKind::Mocha);
         theme
      };

      theme.style.animation_time = 0.5;
      ctx.set_style(theme.style.clone());

      // Load the icons
      let icons = Icons::new(&cc.egui_ctx).unwrap();
      let icons = Arc::new(icons);

      let gui = GUI::new(icons.clone(), theme.clone(), ctx_clone);

      // Update the shared GUI with the current GUI state
      let mut shared_gui = SHARED_GUI.write().unwrap();

      shared_gui.icons = gui.icons.clone();
      shared_gui.theme = gui.theme.clone();
      shared_gui.egui_ctx = gui.egui_ctx.clone();
      shared_gui.ctx = gui.ctx.clone();

      Self {
         on_startup: true,
         updated_started: false,
         ctx: gui.ctx.clone(),
      }
   }

   fn start_up(&mut self, ctx: &egui::Context) {
      let ctx = ctx.clone();
      let gui = SHARED_GUI.read().unwrap();
      ctx.set_style(gui.theme.style.clone());
   }

   fn start_update(&mut self) {
      let ctx = self.ctx.clone();
      let logged_in = self.ctx.logged_in();
      if logged_in && !self.updated_started {
         RT.spawn(async move {
           // update::on_startup(ctx).await;
         });
         self.updated_started = true;
      }
   }
}

impl eframe::App for ZeusApp {
   fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
      egui::Rgba::TRANSPARENT.to_array()
   }

   fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
      self.start_update();

      if self.on_startup {
         self.start_up(ctx);
         self.on_startup = false;
      }

      let mut gui = SHARED_GUI.write().unwrap();

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
            .exact_width(100.0)
            .resizable(false)
            .show_separator_line(true)
            .frame(bg_frame.clone())
            .show_inside(ui, |ui| {
               if gui.ctx.logged_in() {
                  gui.show_left_panel(ui);
               }
            });

         egui::SidePanel::right("right_panel")
            .resizable(false)
            .exact_width(100.0)
            .show_separator_line(true)
            .frame(bg_frame.clone())
            .show_inside(ui, |_ui| {
               // nothing for now just occupy the space
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
   }
}
