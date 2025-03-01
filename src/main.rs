//#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use eframe::egui;
use gui::app::ZeusApp;

pub mod assets;
pub mod core;
pub mod gui;

use core::utils::trace::*;

fn main() -> eframe::Result {
   let options = eframe::NativeOptions {
      renderer: eframe::Renderer::Wgpu,
      viewport: egui::ViewportBuilder::default()
         .with_decorations(false) // Hide the OS-specific "chrome" around the window
         .with_inner_size([1440.0, 900.0])
         .with_min_inner_size([1440.0, 900.0])
         .with_transparent(true), // To have rounded corners we need transparency

      ..Default::default()
   };

   let _tracing_guard = setup_tracing();

   eframe::run_native(
      "Zeus",
      options,
      Box::new(|cc| {
         egui_extras::install_image_loaders(&cc.egui_ctx);

         let app = ZeusApp::new(&cc);

         Ok(Box::new(app))
      }),
   )
}
