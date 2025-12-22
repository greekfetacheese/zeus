use crate::core::ZeusCtx;
use crate::gui::GUI;
use eframe::egui::{Align2, Button, Color32, Frame, Order, RichText, ScrollArea, Ui, Window, vec2};
use zeus_theme::{Theme, utils};

pub fn show(ui: &mut Ui, gui: &mut GUI) {
   let ctx = gui.ctx.clone();
   ui.set_width(140.0);

   ui.vertical_centered(|ui| {
      let selected_color = gui.theme.colors.bg4;
      utils::bg_color_on_idle(ui, Color32::TRANSPARENT);
      utils::no_border_on_idle(ui);

      let text_size = gui.theme.text_sizes.large;
      let button_size = vec2(100.0, 50.0);

      let home = if gui.portofolio.is_open() {
         Button::new(RichText::new("Home").size(text_size))
            .fill(selected_color)
            .min_size(button_size)
      } else {
         Button::new(RichText::new("Home").size(text_size)).min_size(button_size)
      };

      if ui.add(home).clicked() {
         gui.portofolio.open();
         gui.uniswap.close();
         gui.send_crypto.close();
         gui.settings.close();
         gui.wallet_ui.close();
         gui.tx_history.close();
         gui.across_bridge.close();
         gui.dev.close();
      }

      let send = if gui.send_crypto.is_open() {
         Button::new(RichText::new("Send").size(text_size))
            .fill(selected_color)
            .min_size(button_size)
      } else {
         Button::new(RichText::new("Send").size(text_size)).min_size(button_size)
      };

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
      }

      let swap = if gui.uniswap.is_open() {
         Button::new(RichText::new("Swap").size(text_size))
            .fill(selected_color)
            .min_size(button_size)
      } else {
         Button::new(RichText::new("Swap").size(text_size)).min_size(button_size)
      };

      if ui.add(swap).clicked() {
         gui.uniswap.open();
         gui.portofolio.close();
         gui.send_crypto.close();
         gui.settings.close();
         gui.wallet_ui.close();
         gui.tx_history.close();
         gui.across_bridge.close();
         gui.dev.close();
      }

      let bridge = if gui.across_bridge.is_open() {
         Button::new(RichText::new("Bridge").size(text_size))
            .fill(selected_color)
            .min_size(button_size)
      } else {
         Button::new(RichText::new("Bridge").size(text_size)).min_size(button_size)
      };

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
      }

      let wallets = if gui.wallet_ui.is_open() {
         Button::new(RichText::new("Wallets").size(text_size))
            .fill(selected_color)
            .min_size(button_size)
      } else {
         Button::new(RichText::new("Wallets").size(text_size)).min_size(button_size)
      };

      if ui.add(wallets).clicked() {
         gui.wallet_ui.open(ctx);
         gui.portofolio.close();
         gui.uniswap.close();
         gui.send_crypto.close();
         gui.settings.close();
         gui.tx_history.close();
         gui.across_bridge.close();
         gui.dev.close();
      }

      let tx_history = if gui.tx_history.is_open() {
         Button::new(RichText::new("Transactions").size(text_size))
            .fill(selected_color)
            .min_size(button_size)
      } else {
         Button::new(RichText::new("Transactions").size(text_size)).min_size(button_size)
      };

      if ui.add(tx_history).clicked() {
         gui.tx_history.open();
         gui.portofolio.close();
         gui.uniswap.close();
         gui.send_crypto.close();
         gui.settings.close();
         gui.wallet_ui.close();
         gui.across_bridge.close();
         gui.dev.close();
      }

      let settings = if gui.settings.is_open() {
         Button::new(RichText::new("Settings").size(text_size))
            .fill(selected_color)
            .min_size(button_size)
      } else {
         Button::new(RichText::new("Settings").size(text_size)).min_size(button_size)
      };

      if ui.add(settings).clicked() {
         gui.settings.open();
         gui.portofolio.close();
         gui.uniswap.close();
         gui.send_crypto.close();
         gui.wallet_ui.close();
         gui.tx_history.close();
         gui.across_bridge.close();
         gui.dev.close();
      }

      let connected_dapps =
         Button::new(RichText::new("Connected Dapps").size(text_size)).min_size(button_size);
      if ui.add(connected_dapps).clicked() {
         gui.connected_dapps.open();
      }

      #[cfg(feature = "dev")]
      if ui
         .add(Button::new(RichText::new("Theme Editor").size(text_size)).min_size(button_size))
         .clicked()
      {
         gui.editor.open = true;
      }

      #[cfg(feature = "dev")]
      if ui
         .add(Button::new(RichText::new("FPS Metrics").size(text_size)).min_size(button_size))
         .clicked()
      {
         gui.fps_metrics.open = true;
      }

      #[cfg(feature = "dev")]
      {
         let dev =
            ui.add(Button::new(RichText::new("Dev UI").size(text_size)).min_size(button_size));
         if dev.clicked() {
            gui.dev.open();
            gui.portofolio.close();
            gui.uniswap.close();
            gui.send_crypto.close();
            gui.wallet_ui.close();
            gui.tx_history.close();
            gui.across_bridge.close();
            gui.settings.close();
         }
      }
   });
}

pub struct ConnectedDappsUi {
   open: bool,
   pub size: (f32, f32),
}

impl ConnectedDappsUi {
   pub fn new() -> Self {
      Self {
         open: false,
         size: (300.0, 400.0),
      }
   }

   pub fn open(&mut self) {
      self.open = true;
   }
   pub fn close(&mut self) {
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

      let title = RichText::new("Connected Dapps").size(theme.text_sizes.heading);
      Window::new(title)
         .open(&mut open)
         .collapsible(false)
         .resizable(false)
         .order(Order::Foreground)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = 20.0;
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);
               ui.set_width(self.size.0);
               ui.set_height(self.size.1);

               let dapps = ctx.get_connected_dapps();

               if dapps.is_empty() {
                  ui.label(RichText::new("No connected dapps").size(theme.text_sizes.normal));
                  return;
               }

               let text = RichText::new("Disconnect all").size(theme.text_sizes.normal);
               if ui.add(Button::new(text)).clicked() {
                  ctx.disconnect_all_dapps();
               }

               ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
                  ui.set_width(ui.available_width());
                  ui.set_height(ui.available_height());
                  for dapp in dapps {
                     ui.horizontal(|ui| {
                        let text = RichText::new(&dapp).size(theme.text_sizes.normal);
                        ui.label(text);

                        let text = RichText::new("Disconnect").size(theme.text_sizes.normal);
                        if ui.add(Button::new(text)).clicked() {
                           ctx.disconnect_dapp(&dapp);
                        }
                     });
                  }
               });
            });
         });

      self.open = open;
   }
}
