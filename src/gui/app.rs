use std::sync::Arc;
use std::path::PathBuf;

use egui::Frame;
use egui_theme::{ Theme, ThemeKind };
use crate::assets::{ icons::Icons, fonts::get_fonts };
use crate::core::utils::{RT, sync_pools};
use crate::core::ZeusCtx;
use crate::gui::{ GUI, SHARED_GUI };
use eframe::{ egui::{ self }, CreationContext };
use crate::gui::window::window_frame;

pub struct ZeusApp {
    pub on_startup: bool,
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
            let theme = Theme::new(ThemeKind::Midnight);
            theme
        };

        theme.style.animation_time = 0.5;
        ctx.set_style(theme.style.clone());
        ctx.set_fonts(get_fonts());

        // Load the icons
        let icons = Icons::new(&cc.egui_ctx).unwrap();
        let icons = Arc::new(icons);

        let gui = GUI::new(icons.clone(), theme.clone(), ctx_clone);

        // Update the shared GUI with the current GUI state
        let mut shared_gui = SHARED_GUI.write().unwrap();

        shared_gui.icons = gui.icons.clone();
        shared_gui.theme = gui.theme.clone();
        shared_gui.egui_ctx = gui.egui_ctx.clone();

        Self {
            on_startup: true,
            ctx: gui.ctx.clone(),
        }
    }

    fn start_up(&mut self, ctx: &egui::Context, zeus_ctx: ZeusCtx) {
        if self.on_startup {
            let ctx = ctx.clone();

            let gui = SHARED_GUI.read().unwrap();
            ctx.set_style(gui.theme.style.clone());

            /* 
            RT.spawn(async move {
                match sync_pools(zeus_ctx).await {
                    Ok(_) => {
                        tracing::info!("Synced all the pools");
                    }
                    Err(e) => {
                        tracing::error!("Error syncing pools: {:?}", e);
                    }
                }
            });
            */

            self.on_startup = false;
        }
    }
}

impl eframe::App for ZeusApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        egui::Rgba::TRANSPARENT.to_array()
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.start_up(ctx, self.ctx.clone());

        let mut gui = SHARED_GUI.write().unwrap();

        let bg_color = if gui.show_overlay {
            gui.theme.colors.overlay_color
        } else {
            gui.theme.colors.bg_color
        };

        let bg_frame = Frame::none().fill(bg_color);

        window_frame(ctx, "Zeus", bg_frame.clone(), |ui| {
            egui_theme::utils::apply_theme_changes(&gui.theme, ui);

            // Paint the Ui that belongs to the top panel
            egui::TopBottomPanel
                ::top("top_panel")
                .exact_height(200.0)
                .resizable(false)
                .show_separator_line(false)
                .frame(bg_frame.clone())
                .show_inside(ui, |ui| {
                    if gui.ctx.logged_in() {
                        gui.show_top_panel(ui);
                    }
                });

            // Paint the Ui that belongs to the left panel
            egui::SidePanel
                ::left("left_panel")
                .exact_width(100.0)
                .resizable(false)
                .show_separator_line(false)
                .frame(bg_frame.clone())
                .show_inside(ui, |ui| {
                    if gui.ctx.logged_in() {
                        gui.show_left_panel(ui);
                    }
                });

            egui::SidePanel
                ::right("right_panel")
                .resizable(false)
                .exact_width(100.0)
                .show_separator_line(false)
                .frame(bg_frame.clone())
                .show_inside(
                    ui,
                    |_ui| {
                        // nothing for now just occupy the space
                    }
                );

            // Paint the Ui that belongs to the central panel
            egui::CentralPanel
                ::default()
                .frame(bg_frame.clone())
                .show_inside(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        gui.show_central_panel(ui);
                    });
                });
        });
    }
}

/* 
/// Request repaint every 32ms (30 FPS) only if the Viewport is not minimized.
fn request_repaint(ctx: egui::Context) {
    let duration = Duration::from_millis(32);

    loop {
        let is_minimized = ctx.input(|i| i.viewport().minimized.unwrap_or(false));

        if !is_minimized {
            ctx.request_repaint();
        }
        std::thread::sleep(duration);
    }
}
    */