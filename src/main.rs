#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use eframe::{
   egui,
   egui_wgpu::{WgpuConfiguration, WgpuSetup, WgpuSetupCreateNew},
   wgpu::{self, InstanceDescriptor, MemoryHints, Trace},
};
use gui::app::ZeusApp;
use std::sync::Arc;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::Registry;

use tracing_subscriber::{
   EnvFilter, fmt, layer::SubscriberExt, prelude::*, util::SubscriberInitExt,
};

pub mod assets;
pub mod connector;
pub mod core;
pub mod embedded;
pub mod gui;
pub mod server;
mod tests;
pub mod utils;

use std::panic;

fn main() -> eframe::Result {
   // Native messaging uses stdin/stdout. Must run before any tracing to stdout,
   // and must NEVER fall through into the GUI, Brave/Chrome spawn this process
   // on every sendNativeMessage and kill it when the handshake ends.
   if connector::is_native_messaging_invocation() {
      if let Err(e) = connector::run_native_messaging_host() {
         eprintln!("zeus connector host: {e}");
         std::process::exit(1);
      }
      return Ok(());
   }

   let _tracing_guard = setup_tracing();

   cleanup_old_logs();

   panic::set_hook(Box::new(|panic_info| {
      let message = panic_info.payload().downcast_ref::<&str>().map_or("Unknown panic", |s| s);
      let location = panic_info.location().map_or("Unknown location".to_string(), |loc| {
         format!("{}:{}:{}", loc.file(), loc.line(), loc.column())
      });
      tracing::error!("Panic occurred: '{}' at {}", message, location);
   }));

   let wgpu_setup = WgpuSetup::CreateNew(WgpuSetupCreateNew {
      device_descriptor: Arc::new(wgpu_device_descriptor),
      instance_descriptor: InstanceDescriptor::new_without_display_handle(),
      display_handle: None,
      native_adapter_selector: None,
      power_preference: wgpu::PowerPreference::None,
   });

   let wgpu_config = WgpuConfiguration {
      wgpu_setup,
      ..Default::default()
   };

   let options = eframe::NativeOptions {
      renderer: eframe::Renderer::Wgpu,
      wgpu_options: wgpu_config,
      viewport: egui::ViewportBuilder::default()
         .with_decorations(true)
         .with_inner_size([1280.0, 900.0])
         .with_min_inner_size([1280.0, 900.0])
         .with_transparent(false)
         .with_resizable(true),

      ..Default::default()
   };

   let current_version = self_update::cargo_crate_version!().to_string();

   #[cfg(feature = "dev")]
   let title = format!("Zeus {} (dev build)", current_version);

   #[cfg(not(feature = "dev"))]
   let title = format!("Zeus {}", current_version);

   eframe::run_native(
      title.as_str(),
      options,
      Box::new(|cc| {
         egui_extras::install_image_loaders(&cc.egui_ctx);

         let app = ZeusApp::new(cc);

         Ok(Box::new(app))
      }),
   )
}

/// Request default/downlevel limits, then clamp anything the adapter cannot provide.
/// Incomplete virtual GPUs (eg. VirtualBox SVGA3D) advertise `max_compute_* = 0`; wgpu
/// rejects `Limits::default()` (65535) with "better than allowed 0" and never opens
/// a window. egui only needs raster, so clamping is safe.
fn wgpu_device_descriptor(adapter: &wgpu::Adapter) -> wgpu::DeviceDescriptor<'static> {
   let adapter_limits = adapter.limits();
   let base_limits = if adapter.get_info().backend == wgpu::Backend::Gl {
      wgpu::Limits::downlevel_webgl2_defaults()
   } else {
      wgpu::Limits::default()
   };

   let desired_limits = wgpu::Limits {
      max_texture_dimension_2d: 8192,
      ..base_limits
   };

   if !desired_limits.check_limits(&adapter_limits) {
      tracing::warn!(
         "wgpu adapter {:?} does not meet default limits; clamping to adapter.limits()",
         adapter.get_info()
      );
   }

   wgpu::DeviceDescriptor {
      label: Some("zeus wgpu device"),
      required_limits: desired_limits.or_worse_values_from(&adapter_limits),
      memory_hints: MemoryHints::MemoryUsage,
      trace: Trace::Off,
      ..Default::default()
   }
}

/// Daily rotated logs (`output.log.YYYY-MM-DD`, `trace.log.YYYY-MM-DD`)
/// older than this are deleted on startup.
const LOG_RETENTION_DAYS: u64 = 7;
const LOG_DIR: &str = "./logs";

pub fn setup_tracing() -> (WorkerGuard, WorkerGuard) {
   // Setup for file appenders
   let trace_appender = tracing_appender::rolling::daily(LOG_DIR, "trace.log");
   let output_appender = tracing_appender::rolling::daily(LOG_DIR, "output.log");

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

fn cleanup_old_logs() {
   let Ok(entries) = std::fs::read_dir(LOG_DIR) else {
      return;
   };

   let today = chrono::Local::now().date_naive();
   let Some(cutoff) = today.checked_sub_days(chrono::Days::new(LOG_RETENTION_DAYS)) else {
      return;
   };

   for entry in entries.flatten() {
      let path = entry.path();
      if !path.is_file() {
         continue;
      }

      let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
         continue;
      };

      let Some((prefix, date_str)) = name.rsplit_once('.') else {
         continue;
      };
      if prefix != "output.log" && prefix != "trace.log" {
         continue;
      }

      let Ok(date) = chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d") else {
         continue;
      };

      if date < cutoff {
         match std::fs::remove_file(&path) {
            Ok(()) => tracing::info!("Removed old log file: {name}"),
            Err(e) => tracing::warn!("Failed to remove old log file {name}: {e}"),
         }
      }
   }
}
