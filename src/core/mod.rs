use std::time::Duration;
use std::sync::Arc;
use std::path::{PathBuf, Path};

use egui::Frame;
use egui_theme::{Theme, ThemeKind};
use crate::assets::icons::Icons;
use crate::gui::{GUI, SHARED_GUI};
use crate::assets::fonts::get_fonts;
use data::app_data::APP_DATA;
use eframe::{egui::{self}, CreationContext};
use utils::on_startup;
use crate::gui::window::window_frame;

pub mod data;
pub mod user;
pub mod utils;

pub struct ZeusApp {
    pub on_startup: bool,
}

impl ZeusApp

{
    pub fn new(cc: &CreationContext) -> Self {
        let ctx = cc.egui_ctx.clone();
         
        let ctx_clone = ctx.clone(); 
        std::thread::spawn(move || {
            request_repaint(ctx_clone);
        });

        let path_exists = PathBuf::from("zeus-theme.json").exists();
        let theme = if path_exists {
            let path = PathBuf::from("zeus-theme.json");
            let theme = Theme::from_custom(path).unwrap();
            theme
        } else {
            let theme = Theme::new(ThemeKind::Midnight);
            theme
        };
        
        ctx.set_style(theme.style.clone());
        ctx.set_fonts(get_fonts());  

        // Load the icons
        let icons = Icons::new(&cc.egui_ctx).unwrap();
        let icons = Arc::new(icons);

        {
            let mut data = APP_DATA.write().unwrap();
            data.icons = Some(icons.clone());
        }

        let gui = GUI::new(icons, theme);
        
        // Update the shared GUI with the current GUI state
        let mut shared_gui = SHARED_GUI.write().unwrap();

        shared_gui.icons = gui.icons.clone();
        shared_gui.theme = gui.theme.clone();

        Self {
            on_startup: true,
        }
    }

    fn start_up(&mut self, ctx: egui::Context) {
        if self.on_startup {

            {
                let gui = SHARED_GUI.read().unwrap();
                ctx.set_style(gui.theme.style.clone());
            }

            std::thread::spawn(move || {
            match on_startup() {
                Ok(_) => {},
                Err(e) => {
                    let mut gui = SHARED_GUI.write().unwrap();
                    gui.open_msg_window("Failed to load important data", e.to_string());
                }
            }
        });
            self.on_startup = false;
        }
    }

}

impl eframe::App for ZeusApp

{
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        egui::Rgba::TRANSPARENT.to_array()
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.start_up(ctx.clone());


        let logged_in;
        let bg_frame;
        {
            let app_data = APP_DATA.read().unwrap();
            logged_in = app_data.logged_in;

            let gui = SHARED_GUI.write().unwrap();
            bg_frame = Frame::none().fill(gui.theme.colors.bg_color);
        }

        window_frame(ctx, "Zeus", bg_frame.clone(), |ui| {

            {
                let gui = SHARED_GUI.read().unwrap();
                egui_theme::utils::apply_theme_changes(&gui.theme, ui);
            }

            // Paint the Ui that belongs to the top panel
            egui::TopBottomPanel::top("top_panel")
                .exact_height(200.0)
                .frame(bg_frame.clone())
                .show_inside(ui, |ui| {
                    if logged_in {
                    let mut gui = SHARED_GUI.write().unwrap();
                    gui.show_top_panel(ui);
                    }
                });

            // Paint the Ui that belongs to the left panel
            egui::SidePanel::left("left_panel")
                .exact_width(100.0)
                .frame(bg_frame.clone())
                .show_inside(ui, |ui| {
                    if logged_in {
                    let mut gui = SHARED_GUI.write().unwrap();
                    gui.show_left_panel(ui);
                    }
                });

            egui::SidePanel::right("right_panel")
                .exact_width(100.0)
                .frame(bg_frame.clone())
                .show_inside(ui, |_ui| {
                    // nothing for now just occupy the space
                });

            // Paint the Ui that belongs to the central panel
            egui::CentralPanel::default().frame(bg_frame.clone()).show_inside(ui, |ui| {
                ui.vertical_centered(|ui| {
                    let mut gui = SHARED_GUI.write().unwrap();
                    gui.show_central_panel(ui);
                });
            });
        });
    }
}

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