use egui::{
   Color32, Context, Frame, Key, LayerId, Order, Rect, RichText, Stroke, StrokeKind, ViewportBuilder, ViewportClass,
   ViewportCommand, ViewportId, pos2, vec2,
};

use enigo::Mouse;
use rqrr::PreparedImage;
use secure_types::{SecureString, Zeroize};
use std::sync::{
   Arc, Mutex,
   atomic::{AtomicBool, AtomicI32, Ordering},
};
use std::time::Duration;
use xcap::{Monitor, image::DynamicImage};

type Error = Box<dyn std::error::Error>;

const VIEWPORT_ID: &str = "qr_scanner";

/// A QR scanner that can be used to scan QR codes within the current monitor.
///
/// It is suitable for capturing QR codes that may contain sensitive information
/// all though in `Wayland` a temp file of the screenshot can be created for a brief moment.
///
/// # Usage:
///
/// ```
///  use egui::*;
///  let mut scanner = QRScanner::new();
///  scanner.open(Context::default());
///  let res = scanner.get_result();
///  if let Some(res) = res {
///     // reset the state
///     scanner.reset();
///     // use the decoded string
///  }
/// ```
#[derive(Clone)]
pub struct QRScanner {
   open: Arc<AtomicBool>,
   capture_size: Arc<AtomicI32>,
   capture_is_valid: Arc<AtomicBool>,
   result: Arc<Mutex<Option<SecureString>>>,
   last_error: Arc<Mutex<Option<String>>>,
   current_monitor: Option<xcap::Monitor>,
}

impl QRScanner {
   pub fn new() -> Self {
      Self {
         open: Arc::new(AtomicBool::new(false)),
         capture_size: Arc::new(AtomicI32::new(300)),
         capture_is_valid: Arc::new(AtomicBool::new(false)),
         result: Arc::new(Mutex::new(None)),
         last_error: Arc::new(Mutex::new(None)),
         current_monitor: None,
      }
   }

   pub fn open(&self, ctx: Context) {
      if !self.is_open() {
         let open = self.open.clone();
         std::thread::spawn(move || repaint_thread(ctx, open));
      }
      self.open.store(true, Ordering::Relaxed);
   }

   pub fn close(&self) {
      self.open.store(false, Ordering::Relaxed);
   }

   pub fn reset(&mut self) {
      *self = Self::new();
   }

   pub fn is_open(&self) -> bool {
      self.open.load(Ordering::Relaxed)
   }

   pub fn show(&mut self, ctx: &egui::Context) {
      if !self.is_open() {
         return;
      }

      // Get monitor under cursor if not set
      if self.current_monitor.is_none() {
         let enigo = enigo::Enigo::new(&enigo::Settings::default()).unwrap();
         let (mouse_x, mouse_y) = enigo.location().unwrap();
         if let Ok(monitor) = xcap::Monitor::from_point(mouse_x, mouse_y) {
            self.current_monitor = Some(monitor);
         } else {
            self.open.store(false, Ordering::Relaxed);
            return;
         }
      }

      let monitor = self.current_monitor.as_ref().unwrap();

      let mon_x_px = monitor.x().unwrap() as f32;
      let mon_y_px = monitor.y().unwrap() as f32;
      let mon_width_px = monitor.width().unwrap() as f32;
      let mon_height_px = monitor.height().unwrap() as f32;

      let ppp = ctx.pixels_per_point(); // Use main ctx ppp

      let open = self.open.clone();
      let capture_size = self.capture_size.clone();
      let capture_is_valid = self.capture_is_valid.clone();
      let last_error = self.last_error.clone();
      let result = self.result.clone();
      let monitor_clone = monitor.clone();

      ctx.show_viewport_deferred(
         ViewportId::from_hash_of(VIEWPORT_ID),
         ViewportBuilder::default()
            .with_title("QR Scan Overlay")
            .with_transparent(true)
            .with_decorations(false)
            .with_always_on_top()
            .with_mouse_passthrough(true)
            .with_position(pos2(mon_x_px / ppp, mon_y_px / ppp))
            .with_inner_size(vec2(mon_width_px / ppp, mon_height_px / ppp))
            .with_active(true),
         move |ctx, class| {
            if class == ViewportClass::Embedded {
               egui::CentralPanel::default().show(&ctx, |ui| {
                  ui.label("This viewport is embedded.");
               });
               return;
            }

            // Attempt to keep focus
            if !ctx.input(|i| i.viewport().focused.unwrap_or(false)) {
               ctx.send_viewport_cmd(ViewportCommand::Focus);
            }

            // Handle key inputs
            ctx.input_mut(|i| {
               if i.consume_key(egui::Modifiers::NONE, Key::Plus) || i.consume_key(egui::Modifiers::NONE, Key::Equals) {
                  capture_size.fetch_add(10, Ordering::Relaxed);
               }

               if i.consume_key(egui::Modifiers::NONE, Key::Minus) {
                  let new = capture_size.load(Ordering::Relaxed) - 10;
                  capture_size.store(new, Ordering::Relaxed);
               }

               if i.consume_key(egui::Modifiers::NONE, Key::Escape) {
                  open.store(false, Ordering::Relaxed);
               }
            });

            match capture_and_decode(capture_size.load(Ordering::Relaxed), &monitor_clone) {
               Ok(res) => {
                  capture_is_valid.store(true, Ordering::Relaxed);
                  *last_error.lock().unwrap() = None;
                  *result.lock().unwrap() = Some(res);
                  open.store(false, Ordering::Relaxed);
               }
               Err(e) => {
                  capture_is_valid.store(false, Ordering::Relaxed);
                  *last_error.lock().unwrap() = Some(e.to_string());
               }
            }

            // Get ppp for this viewport (should match main)
            let ppp = ctx.pixels_per_point();

            // Get current mouse pos in pixels
            let (mouse_x_px, mouse_y_px) = enigo::Enigo::new(&enigo::Settings::default())
               .unwrap()
               .location()
               .unwrap();

            // Calculate capture region in pixels (for xcap)
            let capture_size_px = capture_size.load(Ordering::Relaxed);
            let half_px = capture_size_px / 2;
            let mon_x_px = monitor_clone.x().unwrap();
            let mon_y_px = monitor_clone.y().unwrap();
            let mon_width_px = monitor_clone.width().unwrap() as i32;
            let mon_height_px = monitor_clone.height().unwrap() as i32;

            let cap_x_px = (mouse_x_px - half_px)
               .max(mon_x_px)
               .min(mon_x_px + mon_width_px - capture_size_px);

            let cap_y_px = (mouse_y_px - half_px)
               .max(mon_y_px)
               .min(mon_y_px + mon_height_px - capture_size_px);

            let cap_width_px = capture_size_px.min(mon_x_px + mon_width_px - cap_x_px);
            let cap_height_px = capture_size_px.min(mon_y_px + mon_height_px - cap_y_px);

            // Relative coords in pixels (from monitor origin)
            let rel_x_px = (cap_x_px - mon_x_px) as f32;
            let rel_y_px = (cap_y_px - mon_y_px) as f32;

            // Convert to egui points (local to viewport)
            let rel_x_pt = rel_x_px / ppp;
            let rel_y_pt = rel_y_px / ppp;
            let cap_width_pt = cap_width_px as f32 / ppp;
            let cap_height_pt = cap_height_px as f32 / ppp;

            let rect = Rect::from_min_size(pos2(rel_x_pt, rel_y_pt), vec2(cap_width_pt, cap_height_pt));

            // Draw border using layer painter
            let painter = ctx.layer_painter(LayerId::new(
               Order::Foreground,
               egui::Id::new("qr_border_layer"),
            ));

            let color = if capture_is_valid.load(Ordering::Relaxed) {
               Color32::GREEN
            } else {
               Color32::RED
            };

            let stroke = Stroke::new(3.0, color);
            let stroke_kind = StrokeKind::Outside;
            painter.rect_stroke(rect, 0.0, stroke, stroke_kind);

            // Show help frame above the capture area
            let help_pos = pos2(rel_x_pt, rel_y_pt - 100.0); // Offset above
            let last_error = last_error.lock().unwrap();
            egui::Area::new("qr_help".into())
               .fixed_pos(help_pos)
               .show(&ctx, |ui| {
                  let frame = Frame::window(ui.style());
                  frame.show(ui, |ui| {
                     let text = RichText::new("Use the mouse to target the QR code.").size(14.0);
                     ui.label(text);

                     let text =
                        RichText::new("Press + or - to increase or decrease the size of the captured area.").size(14.0);
                     ui.label(text);

                     let text = RichText::new("Press Esc to cancel.").size(14.0);
                     ui.label(text);

                     let mut text = String::new();

                     if let Some(e) = last_error.as_ref().map(|e| e.as_str()) {
                        text = e.to_string();
                     }

                     ui.label(RichText::new(text).size(14.0).color(Color32::RED));
                  });
               });

            if ctx.input(|i| i.viewport().close_requested()) {
               open.store(false, Ordering::Relaxed);
            }
         },
      );
   }

   pub fn get_result(&self) -> Option<SecureString> {
      self.result.lock().unwrap().clone()
   }
}

fn repaint_thread(ctx: Context, open: Arc<AtomicBool>) {
   loop {
      if !open.load(Ordering::Relaxed) {
         ctx.request_repaint();
         return;
      }

      ctx.request_repaint();
      std::thread::sleep(Duration::from_millis(16));
   }
}

pub fn capture_and_decode(capture_size: i32, monitor: &Monitor) -> Result<SecureString, Error> {
   // Get global mouse position
   let enigo = enigo::Enigo::new(&enigo::Settings::default())?;
   let (mouse_x, mouse_y) = enigo.location()?;

   // Define capture region (centered; clamp to monitor bounds)
   let half = capture_size / 2;
   let mon_x = monitor.x()?;
   let mon_y = monitor.y()?;
   let mon_width = monitor.width()? as i32;
   let mon_height = monitor.height()? as i32;
   let cap_x = (mouse_x - half)
      .max(mon_x)
      .min(mon_x + mon_width - capture_size);
   let cap_y = (mouse_y - half)
      .max(mon_y)
      .min(mon_y + mon_height - capture_size);
   let cap_width = capture_size.min(mon_x + mon_width - cap_x);
   let cap_height = capture_size.min(mon_y + mon_height - cap_y);

   let cap_width: u32 = cap_width.try_into()?;
   let cap_height: u32 = cap_height.try_into()?;

   // Capture (coords relative to monitor origin)
   let rel_x: u32 = (cap_x - mon_x).try_into()?;
   let rel_y: u32 = (cap_y - mon_y).try_into()?;
   let image = monitor.capture_region(rel_x, rel_y, cap_width, cap_height)?;

   // Decode QR
   let mut img = DynamicImage::ImageRgba8(image);
   let luma = img.to_luma8();

   if let Some(img) = img.as_mut_rgb8() {
      img.zeroize();
   }

   let mut prepared = PreparedImage::prepare(luma);
   let grids = prepared.detect_grids();
   // TODO: zeroize the buffer of the prepared image

   if grids.is_empty() {
      return Err(format!("No QR grids detected (try adjusting size/position)").into());
   }

   let (_, content) = grids[0].decode()?;
   let sec_string = SecureString::from(content);

   Ok(sec_string)
}
