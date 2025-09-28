use crate::gui::GUI;
use eframe::egui::{Frame, RichText, Ui, Window, vec2};

pub fn show(ui: &mut Ui, gui: &mut GUI) {
   let ctx = gui.ctx.clone();
   let vault_unlocked = ctx.vault_unlocked();
   let vault_exists = ctx.vault_exists();
   let theme = &gui.theme;
   let icons = gui.icons.clone();
   let token_selection = &mut gui.token_selection;
   let recipient_selection = &mut gui.recipient_selection;
   let contacts_ui = &mut gui.settings.contacts_ui;

   gui.tx_confirmation_window.show(ctx.clone(), theme, icons.clone(), ui);

   gui.tx_window.show(ctx.clone(), theme, icons.clone(), ui);

   gui.confirm_window.show(theme, ui);

   gui.msg_window.show(theme, ui);

   gui.loading_window.show(ui);

   gui.sign_msg_window.show(ctx.clone(), theme, icons.clone(), ui);

   if !vault_exists {
      gui.portofolio.close();
   }

   if vault_exists && !vault_unlocked {
      gui.portofolio.close();
   }

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
}

pub struct FPSMetrics {
   pub open: bool,
   pub max_fps: f64,
   pub time_ms: f64,
   pub time_ns: u128,
}

impl FPSMetrics {
   pub fn new() -> Self {
      Self {
         open: false,
         max_fps: 0.0,
         time_ms: 0.0,
         time_ns: 0,
      }
   }

   pub fn update(&mut self, time_ns: u128) {
      self.time_ns = time_ns;

      if self.time_ns > 0 {
         self.max_fps = 1_000_000_000.0 / self.time_ns as f64;

         self.time_ms = self.time_ns as f64 / 1_000_000.0;
      } else {
         self.max_fps = 0.0;
         self.time_ms = 0.0;
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
            ui.set_width(150.0);
            ui.set_height(100.0);

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing = vec2(0.0, 5.0);

               let max_fps = RichText::new(format!("Max FPS: {:.2}", self.max_fps)).size(14.0);
               ui.label(max_fps);

               let time_ms = RichText::new(format!("Time: {:.4} ms", self.time_ms)).size(14.0);
               ui.label(time_ms);

               let time_ns = RichText::new(format!("Time: {} ns", self.time_ns)).size(14.0);
               ui.label(time_ns);
            });
         });

      self.open = open;
   }
}
