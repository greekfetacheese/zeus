#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use eframe::{
   egui,
   egui_wgpu::{WgpuConfiguration, WgpuSetup, WgpuSetupCreateNew},
   wgpu::{self, MemoryHints, Trace},
};
use gui::app::ZeusApp;
use std::sync::Arc;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::Registry;

use tracing_subscriber::{
   EnvFilter, fmt, layer::SubscriberExt, prelude::*, util::SubscriberInitExt,
};

pub mod assets;
pub mod core;
pub mod gui;
pub mod server;
mod tests;
pub mod utils;

use std::panic;

fn main() -> eframe::Result {
   let _tracing_guard = setup_tracing();

   panic::set_hook(Box::new(|panic_info| {
      let message = panic_info.payload().downcast_ref::<&str>().map_or("Unknown panic", |s| s);
      let location = panic_info.location().map_or("Unknown location".to_string(), |loc| {
         format!("{}:{}:{}", loc.file(), loc.line(), loc.column())
      });
      tracing::error!("Panic occurred: '{}' at {}", message, location);
   }));

   let wgpu_setup = WgpuSetup::CreateNew(WgpuSetupCreateNew {
      device_descriptor: Arc::new(|_adapter| wgpu::DeviceDescriptor {
         memory_hints: MemoryHints::MemoryUsage,
         trace: Trace::Off,
         ..Default::default()
      }),
      ..Default::default()
   });

   let wgpu_config = WgpuConfiguration {
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

   eframe::run_native(
      "Zeus",
      options,
      Box::new(|cc| {
         egui_extras::install_image_loaders(&cc.egui_ctx);

         let app = ZeusApp::new(cc);

         Ok(Box::new(app))
      }),
   )
}

pub fn setup_tracing() -> (WorkerGuard, WorkerGuard) {
   // Setup for file appenders
   let trace_appender = tracing_appender::rolling::daily("./logs", "trace.log");
   let output_appender = tracing_appender::rolling::daily("./logs", "output.log");

   // Creating non-blocking writers
   let (trace_writer, trace_guard) = tracing_appender::non_blocking(trace_appender);
   let (output_writer, output_guard) = tracing_appender::non_blocking(output_appender);

   // Use different filters for trace logs and other levels
   let console_filter = EnvFilter::new("zeus=info,error,warn,zeus_eth=info,error,warn");
   let trace_filter = EnvFilter::new("zeus=trace,zeus_eth=trace");
   let output_filter = EnvFilter::new("zeus=info,error,warn,zeus_eth=info,error,warn");

   // Setting up layers
   let console_layer = fmt::layer().with_writer(std::io::stdout).with_filter(console_filter);

   let trace_layer = fmt::layer().with_writer(trace_writer).with_filter(trace_filter);

   let output_layer = fmt::layer().with_writer(output_writer).with_filter(output_filter);

   // Applying configuration
   Registry::default()
      .with(trace_layer)
      .with(console_layer)
      .with(output_layer)
      .init();

   (trace_guard, output_guard)
}
