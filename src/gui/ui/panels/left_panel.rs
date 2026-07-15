use crate::core::ZeusCtx;
use crate::gui::{GUI, ui::dapps::railgun::RailgunMode};
use eframe::egui::{Align2, Order, RichText, ScrollArea, Ui, Window, vec2};
use egui::{FontId, Margin, Stroke};
use zeus_theme::{OverlayManager, Theme};
use zeus_widgets::{Button, SecureTextEdit};

pub fn show(ui: &mut Ui, gui: &mut GUI) {
   let ctx = gui.ctx.clone();
   let privacy_mode = ctx.read(|ctx| ctx.privacy_mode);
   let theme = &gui.theme;
   ui.set_width(140.0);

   let color = theme.colors.hover;
   let stroke = Stroke::new(1.0, color);
   let frame = theme.frame2.inner_margin(Margin::symmetric(0, 10)).stroke(stroke);

   frame.show(ui, |ui| {
      ui.vertical_centered(|ui| {
         ui.spacing_mut().button_padding = vec2(10.0, 8.0);

         let text_size = gui.theme.text_sizes.large;
         let button_size = vec2(80.0, 40.0);

         let is_open = gui.portofolio.is_open();
         let home = Button::selectable(is_open, RichText::new("Home").size(text_size))
            .min_size(button_size);

         if ui.add(home).clicked() {
            gui.portofolio.open();
            gui.uniswap.close();
            gui.send_crypto.close();
            gui.settings.close();
            gui.wallet_ui.close();
            gui.tx_history.close();
            gui.across_bridge.close();
            gui.dev.close();
            gui.shield_ui.close();
         }

         let is_open = gui.send_crypto.is_open();
         let send = Button::selectable(is_open, RichText::new("Send").size(text_size))
            .min_size(button_size);

         if ui.add(send).clicked() {
            gui.send_crypto.open();
            gui.uniswap.close();
            gui.portofolio.close();
            gui.settings.close();
            gui.wallet_ui.close();
            gui.tx_history.close();
            gui.across_bridge.close();
            gui.dev.close();
            // This is shared, so reset it to avoid any issues
            gui.recipient_selection.reset();
            gui.shield_ui.close();
         }

         let is_open = gui.shield_ui.is_open();
         let title = match privacy_mode {
            false => "Shield",
            true => "Unshield",
         };

         let mode = match privacy_mode {
            false => RailgunMode::Shield,
            true => RailgunMode::Unshield,
         };

         let shield =
            Button::selectable(is_open, RichText::new(title).size(text_size)).min_size(button_size);

         if cfg!(feature = "dev") {
            if ui.add(shield).clicked() {
               gui.shield_ui.open(mode);
               gui.portofolio.close();
               gui.uniswap.close();
               gui.send_crypto.close();
               gui.settings.close();
               gui.wallet_ui.close();
               gui.tx_history.close();
               gui.across_bridge.close();
               gui.dev.close();
               // This is shared, so reset it to avoid any issues
               gui.recipient_selection.reset();
            }
         }

         let is_open = gui.uniswap.is_open();
         let swap = Button::selectable(is_open, RichText::new("Swap").size(text_size))
            .min_size(button_size);

         if ui.add(swap).clicked() {
            gui.uniswap.open();
            gui.portofolio.close();
            gui.send_crypto.close();
            gui.settings.close();
            gui.wallet_ui.close();
            gui.tx_history.close();
            gui.across_bridge.close();
            gui.dev.close();
            gui.shield_ui.close();
         }

         let is_open = gui.across_bridge.is_open();
         let bridge = Button::selectable(is_open, RichText::new("Bridge").size(text_size))
            .min_size(button_size);

         if ui.add(bridge).clicked() {
            gui.across_bridge.open();
            gui.portofolio.close();
            gui.uniswap.close();
            gui.send_crypto.close();
            gui.settings.close();
            gui.wallet_ui.close();
            gui.tx_history.close();
            // This is shared, so reset it to avoid any issues
            gui.recipient_selection.reset();
            gui.dev.close();
            gui.shield_ui.close();
         }

         let is_open = gui.wallet_ui.is_open();
         let wallets = Button::selectable(is_open, RichText::new("Wallets").size(text_size))
            .min_size(button_size);

         if ui.add(wallets).clicked() {
            gui.wallet_ui.open(ctx.clone());
            gui.portofolio.close();
            gui.uniswap.close();
            gui.send_crypto.close();
            gui.settings.close();
            gui.tx_history.close();
            gui.across_bridge.close();
            gui.dev.close();
            gui.shield_ui.close();
         }

         let is_open = gui.tx_history.is_open();
         let tx_history = Button::selectable(
            is_open,
            RichText::new("Transactions").size(text_size),
         )
         .min_size(button_size);

         if ui.add(tx_history).clicked() {
            gui.tx_history.open(ctx);
            gui.portofolio.close();
            gui.uniswap.close();
            gui.send_crypto.close();
            gui.settings.close();
            gui.wallet_ui.close();
            gui.across_bridge.close();
            gui.dev.close();
            gui.shield_ui.close();
         }

         let is_open = gui.settings.is_open();
         let settings = Button::selectable(is_open, RichText::new("Settings").size(text_size))
            .min_size(button_size);

         if ui.add(settings).clicked() {
            gui.settings.open();
            gui.portofolio.close();
            gui.uniswap.close();
            gui.send_crypto.close();
            gui.wallet_ui.close();
            gui.tx_history.close();
            gui.across_bridge.close();
            gui.dev.close();
            gui.shield_ui.close();
         }

         let connected_dapps = Button::selectable(
            false,
            RichText::new("Connected Dapps").size(text_size),
         )
         .min_size(button_size);

         if ui.add(connected_dapps).clicked() {
            gui.connected_dapps.open();
         }

         #[cfg(feature = "dev")]
         {
            let text = RichText::new("Theme Editor").size(text_size);
            let theme_editor = Button::selectable(false, text).min_size(button_size);
            if ui.add(theme_editor).clicked() {
               gui.editor.open = true;
            }

            let text = RichText::new("FPS Metrics").size(text_size);
            let fps_metrics = Button::selectable(false, text).min_size(button_size);
            if ui.add(fps_metrics).clicked() {
               gui.fps_metrics.open = true;
            }

            {
               let text = RichText::new("Dev UI").size(text_size);
               let dev = Button::selectable(false, text).min_size(button_size);
               if ui.add(dev).clicked() {
                  gui.dev.open();
                  gui.portofolio.close();
                  gui.uniswap.close();
                  gui.send_crypto.close();
                  gui.wallet_ui.close();
                  gui.tx_history.close();
                  gui.across_bridge.close();
                  gui.settings.close();
                  gui.shield_ui.close();
               }
            }
         }
      });
   });
}

pub struct ConnectedDappsUi {
   open: bool,
   overlay: OverlayManager,
   pub size: (f32, f32),
}

impl ConnectedDappsUi {
   pub fn new(overlay: OverlayManager) -> Self {
      Self {
         open: false,
         overlay,
         size: (300.0, 400.0),
      }
   }

   pub fn open(&mut self) {
      if !self.open {
         self.overlay.window_opened();
      }
      self.open = true;
   }
   pub fn close(&mut self) {
      self.overlay.window_closed();
      self.open = false;
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      if !self.open {
         return;
      }

      let mut open = self.open;
      let button_visuals = theme.button_visuals();
      let text_edit_visuals = theme.text_edit_visuals();
      let window_frame = theme.frame1;

      let title = RichText::new("Connected Dapps").size(theme.text_sizes.heading);
      Window::new(title)
         .open(&mut open)
         .collapsible(false)
         .resizable(false)
         .order(Order::Foreground)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(window_frame)
         .show(ui.ctx(), |ui| {
            ui.spacing_mut().item_spacing.y = 20.0;
            ui.spacing_mut().button_padding = vec2(10.0, 8.0);
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);

            let mut dapps = ctx.get_connected_dapps();
            let dapps_are_empty = dapps.is_empty();

            ui.scope(|ui| {
               ui.vertical_centered(|ui| {
                  if dapps_are_empty {
                     ui.label(RichText::new("No connected dapps").size(theme.text_sizes.normal));
                     return;
                  }
               });
            });

            if !dapps_are_empty {
               let text = RichText::new("Disconnect all").size(theme.text_sizes.normal);
               let button = Button::new(text).visuals(button_visuals);
               if ui.add(button).clicked() {
                  ctx.disconnect_all_dapps();
               }
            }

            ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
               for dapp in dapps.iter_mut() {
                  ui.horizontal(|ui| {
                     let edit = SecureTextEdit::singleline(dapp)
                        .visuals(text_edit_visuals)
                        .min_size(vec2(ui.available_width() * 0.10, 25.0))
                        .margin(Margin::same(10))
                        .font(FontId::proportional(theme.text_sizes.normal));
                     ui.add(edit);

                     let text = RichText::new("Disconnect").size(theme.text_sizes.normal);
                     let button =
                        Button::new(text).visuals(button_visuals).min_size(vec2(50.0, 25.0));
                     if ui.add(button).clicked() {
                        ctx.disconnect_dapp(&dapp);
                     }
                  });
               }
            });
         });

      if !open {
         self.close();
      }
   }
}
