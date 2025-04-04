#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use eframe::egui;
use gui::app::ZeusApp;

pub mod assets;
pub mod core;
pub mod gui;

use core::utils::trace::*;
use std::panic;

fn main() -> eframe::Result {
   panic::set_hook(Box::new(|panic_info| {
      let message = panic_info
         .payload()
         .downcast_ref::<&str>()
         .map_or("Unknown panic", |s| s);
      let location = panic_info
         .location()
         .map_or("Unknown location".to_string(), |loc| {
            format!("{}:{}:{}", loc.file(), loc.line(), loc.column())
         });
      tracing::error!("Panic occurred: '{}' at {}", message, location);
   }));

   // only use wgpu for windows
   let renderer = if cfg!(target_os = "windows") {
      eframe::Renderer::Wgpu
   } else {
      eframe::Renderer::Glow
   };

   let options = eframe::NativeOptions {
      renderer,
      viewport: egui::ViewportBuilder::default()
         .with_decorations(false) // Hide the OS-specific "chrome" around the window
         .with_inner_size([1280.0, 900.0])
         .with_min_inner_size([1280.0, 900.0])
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
