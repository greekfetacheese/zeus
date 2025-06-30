#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use eframe::{
   egui,
   egui_wgpu::{WgpuConfiguration, WgpuSetup, WgpuSetupCreateNew},
   wgpu::{self, MemoryHints, PowerPreference, PresentMode},
};
use std::sync::Arc;
use gui::app::ZeusApp;

pub mod assets;
pub mod core;
pub mod gui;
pub mod server;

use core::utils::trace::*;
use std::panic;

// use mimalloc::MiMalloc;

// #[global_allocator]
// static GLOBAL: MiMalloc = MiMalloc;

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

   let wgpu_setup = WgpuSetup::CreateNew(WgpuSetupCreateNew {
      power_preference: PowerPreference::HighPerformance,
      device_descriptor: Arc::new(|adapter| {
         let base_limits = if adapter.get_info().backend == wgpu::Backend::Gl {
            wgpu::Limits::downlevel_webgl2_defaults()
         } else {
            wgpu::Limits::default()
         };

         wgpu::DeviceDescriptor {
            label: Some("egui wgpu device"),
            required_features: wgpu::Features::default(),
            required_limits: wgpu::Limits {
               max_texture_dimension_2d: 8192,
               ..base_limits
            },
            memory_hints: MemoryHints::MemoryUsage,
         }
      }),
      ..Default::default()
   });

   let wgpu_config = WgpuConfiguration {
      present_mode: PresentMode::AutoVsync,
      desired_maximum_frame_latency: None,
      wgpu_setup,
      ..Default::default()
   };

   let options = eframe::NativeOptions {
      renderer: eframe::Renderer::Wgpu,
      wgpu_options: wgpu_config,
      viewport: egui::ViewportBuilder::default()
         .with_decorations(false)
         .with_inner_size([1280.0, 900.0])
         .with_min_inner_size([1280.0, 900.0])
         .with_transparent(true)
         .with_resizable(true),

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
