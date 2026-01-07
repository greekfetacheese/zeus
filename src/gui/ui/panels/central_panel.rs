use crate::gui::GUI;
use eframe::egui::{Frame, RichText, Ui, Window, vec2};
use zeus_theme::OverlayManager;

pub fn show(ui: &mut Ui, gui: &mut GUI) {
   let ctx = gui.ctx.clone();
   let theme = &gui.theme;
   let icons = gui.icons.clone();
   let token_selection = &mut gui.token_selection;
   let recipient_selection = &mut gui.recipient_selection;
   let contacts_ui = &mut gui.settings.contacts_ui;

   gui.tx_confirmation_window.show(ctx.clone(), theme, icons.clone(), ui);

   gui.tx_window.show(ctx.clone(), theme, icons.clone(), ui);

   gui.confirm_window.show(theme, ui);

   gui.msg_window.show(theme, ui);

   gui.loading_window.show(theme, ui);

   gui.sign_msg_window.show(ctx.clone(), theme, icons.clone(), ui);

   gui.recover_wallet_ui.show(ctx.clone(), theme, icons.clone(), ui);
   gui.unlock_vault_ui.show(ctx.clone(), theme, icons.clone(), ui);

   gui.across_bridge.show(
      ctx.clone(),
      theme,
      icons.clone(),
      recipient_selection,
      contacts_ui,
      ui,
   );

   gui.send_crypto.show(
      ctx.clone(),
      icons.clone(),
      theme,
      token_selection,
      recipient_selection,
      contacts_ui,
      ui,
   );

   gui.portofolio.show(
      ctx.clone(),
      theme,
      icons.clone(),
      token_selection,
      ui,
   );

   gui.uniswap.show(
      ctx.clone(),
      theme,
      icons.clone(),
      token_selection,
      ui,
   );

   gui.settings.show(ctx.clone(), icons.clone(), theme, ui);
   gui.connected_dapps.show(ctx.clone(), theme, ui);

   gui.wallet_ui.show(ctx.clone(), theme, icons.clone(), ui);
   gui.tx_history.show(ctx.clone(), theme, ui);
   gui.update_window.show(theme, ui);

   #[cfg(feature = "dev")]
   gui.dev.show(ctx.clone(), theme, icons, ui);

   #[cfg(feature = "dev")]
   gui.fps_metrics.show(ui);

   #[cfg(feature = "dev")]
   {
      let theme = gui.editor.show(&mut gui.theme, ui);
      if let Some(theme) = theme {
         gui.theme = theme;
      }
   }

   #[cfg(feature = "dev")]
   {
      gui.qr_scanner.show(ui.ctx());
      let res = gui.qr_scanner.get_result();
      if let Some(result) = res {
         gui.qr_scanner.reset();
         result.unlock_str(|str| {
            tracing::info!("QR code found: {}", str);
         });
      }
   }
}

pub struct FPSMetrics {
   pub open: bool,
   overlay: OverlayManager,
   pub max_fps: f64,
   pub time_ms: f64,
   pub time_micros: f64,
}

impl FPSMetrics {
   pub fn new(overlay: OverlayManager) -> Self {
      Self {
         open: false,
         overlay,
         max_fps: 0.0,
         time_ms: 0.0,
         time_micros: 0.0,
      }
   }

   pub fn update(&mut self, time_ms: f64) {
      if time_ms > 0.0 {
         self.max_fps = 1_000.0 / time_ms;
         self.time_ms = time_ms;
         self.time_micros = time_ms * 1_000.0;
      } else {
         self.max_fps = 0.0;
         self.time_ms = 0.0;
         self.time_micros = 0.0;
      }
   }

   pub fn show(&mut self, ui: &mut Ui) {
      let mut open = self.open;

      let title = RichText::new("FPS Metrics").size(18.0);
      Window::new(title)
         .open(&mut open)
         .resizable(true)
         .collapsible(true)
         .movable(true)
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_width(170.0);
            ui.set_height(130.0);

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing = vec2(0.0, 5.0);

               let counter = self.overlay.counter();
               let text = format!("Overlay Counter: {}", counter);
               let text = RichText::new(text).size(14.0);
               ui.label(text);

               let order = self.overlay.order();
               let text = format!("Order: {}", order.short_debug_format());
               let text = RichText::new(text).size(14.0);
               ui.label(text);

               let order = self.overlay.recommended_order();
               let text = format!(
                  "Recommended Order: {}",
                  order.short_debug_format()
               );
               let text = RichText::new(text).size(14.0);
               ui.label(text);

               let alpha = self.overlay.calculate_alpha();
               let text = format!("Overlay Alpha: {}", alpha);
               let text = RichText::new(text).size(14.0);
               ui.label(text);

               let max_fps = RichText::new(format!("Max FPS: {:.2}", self.max_fps)).size(14.0);
               ui.label(max_fps);

               let time_ms = RichText::new(format!("Time: {:.4} ms", self.time_ms)).size(14.0);
               ui.label(time_ms);

               let time_micros =
                  RichText::new(format!("Time: {:.2} μs", self.time_micros)).size(14.0);
               ui.label(time_micros);
            });
         });

      self.open = open;
   }
}
