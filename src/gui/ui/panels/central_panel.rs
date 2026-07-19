use crate::core::ZeusContext;
use crate::gui::GUI;
use eframe::egui::{Frame, RichText, Ui, Window, vec2};
use zeus_theme::OverlayManager;

pub fn show(gui: &mut GUI, ctx: &mut ZeusContext, ui: &mut Ui) {
   let theme = &gui.theme;
   let icons = gui.icons.clone();
   let token_selection = &mut gui.token_selection;
   let recipient_selection = &mut gui.recipient_selection;
   let contacts_ui = &mut gui.settings.contacts_ui;

   gui.tx_confirmation_window.show(ctx, theme, icons.clone(), ui);

   gui.tx_window.show(ctx, theme, icons.clone(), ui);

   gui.confirm_window.show(theme, ui);

   gui.msg_window.show(theme, ui);

   gui.loading_window.show(theme, ui);

   gui.sign_msg_window.show(ctx, theme, icons.clone(), ui);

   gui.recover_wallet_ui.show(ctx, theme, ui);
   gui.unlock_vault_ui.show(ctx, theme, ui);

   gui.across_bridge.show(
      ctx,
      theme,
      icons.clone(),
      recipient_selection,
      contacts_ui,
      ui,
   );

   gui.send_crypto.show(
      ctx,
      icons.clone(),
      theme,
      token_selection,
      recipient_selection,
      contacts_ui,
      ui,
   );

   gui.portofolio.show(ctx, theme, icons.clone(), token_selection, ui);

   gui.uniswap.show(ctx, theme, icons.clone(), token_selection, ui);

   gui.shield_ui.show(
      ctx,
      theme,
      icons.clone(),
      token_selection,
      recipient_selection,
      contacts_ui,
      ui,
   );

   gui.settings.show(ctx, icons.clone(), theme, ui);
   gui.connected_dapps.show(ctx, theme, ui);

   gui.wallet_ui.show(ctx, theme, icons.clone(), ui);
   gui.tx_history.show(ctx, theme, ui);
   gui.update_window.show(theme, ui);

   // This allows to show the network settings independently from the settings ui
   gui.settings.network.show(ctx, theme, icons.clone(), ui);

   if ctx.vault_unlocked {
      let chain_id = ctx.chain.id();
      let owner = ctx.current_wallet_info().address;

      token_selection.show(ctx, theme, icons.clone(), chain_id, owner, ui);
   }

   #[cfg(feature = "dev")]
   gui.dev.show(ctx, theme, icons, ui);

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
