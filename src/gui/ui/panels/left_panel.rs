use crate::core::ZeusCtx;
use crate::gui::GUI;
use eframe::egui::{Align2, Button, Color32, Frame, Order, RichText, ScrollArea, Ui, Window, vec2};
use egui_theme::{Theme, utils};

pub fn show(ui: &mut Ui, gui: &mut GUI) {
   ui.vertical_centered(|ui| {
      ui.add_space(20.0);
      ui.spacing_mut().item_spacing.y = 30.0;
      ui.visuals_mut().widgets.hovered.expansion = 15.0;
      ui.visuals_mut().widgets.active.expansion = 15.0;

      utils::bg_color_on_idle(ui, Color32::TRANSPARENT);
      utils::no_border(ui);

      let text_size = gui.theme.text_sizes.very_large;

      let home = Button::new(RichText::new("Home").size(text_size));
      if ui.add(home).clicked() {
         gui.uniswap.close();
         gui.send_crypto.open = false;
         gui.settings.open = false;
         gui.wallet_ui.open = false;
         gui.tx_history.open = false;
         gui.across_bridge.open = false;
         gui.sync_pools_ui.open = false;
         gui.portofolio.open = true;
      }

      let send = Button::new(RichText::new("Send").size(text_size));
      if ui.add(send).clicked() {
         gui.send_crypto.open = true;
         gui.uniswap.close();
         gui.portofolio.open = false;
         gui.settings.open = false;
         gui.wallet_ui.open = false;
         gui.tx_history.open = false;
         gui.across_bridge.open = false;
         gui.sync_pools_ui.open = false;
         // This is shared, so reset it to avoid any issues
         gui.recipient_selection.reset();
      }

      let swap = Button::new(RichText::new("Swap").size(text_size));
      if ui.add(swap).clicked() {
         gui.uniswap.open();
         gui.portofolio.open = false;
         gui.send_crypto.open = false;
         gui.settings.open = false;
         gui.wallet_ui.open = false;
         gui.tx_history.open = false;
         gui.across_bridge.open = false;
         gui.sync_pools_ui.open = false;
      }

      let bridge = Button::new(RichText::new("Bridge").size(text_size));
      if ui.add(bridge).clicked() {
         gui.portofolio.open = false;
         gui.uniswap.close();
         gui.send_crypto.open = false;
         gui.settings.open = false;
         gui.wallet_ui.open = false;
         gui.tx_history.open = false;
         // This is shared, so reset it to avoid any issues
         gui.recipient_selection.reset();
         gui.sync_pools_ui.open = false;
         gui.across_bridge.open = true;
      }

      let wallets = Button::new(RichText::new("Wallets").size(text_size));
      if ui.add(wallets).clicked() {
         gui.portofolio.open = false;
         gui.uniswap.close();
         gui.send_crypto.open = false;
         gui.settings.open = false;
         gui.tx_history.open = false;
         gui.across_bridge.open = false;
         gui.sync_pools_ui.open = false;
         gui.wallet_ui.open = true;
      }

      let tx_history = Button::new(RichText::new("Transactions").size(text_size));
      if ui.add(tx_history).clicked() {
         gui.portofolio.open = false;
         gui.uniswap.close();
         gui.send_crypto.open = false;
         gui.settings.open = false;
         gui.wallet_ui.open = false;
         gui.across_bridge.open = false;
         gui.sync_pools_ui.open = false;
         gui.tx_history.open = true;
      }

      let settings = Button::new(RichText::new("Settings").size(text_size));
      if ui.add(settings).clicked() {
         gui.portofolio.open = false;
         gui.uniswap.close();
         gui.send_crypto.open = false;
         gui.wallet_ui.open = false;
         gui.tx_history.open = false;
         gui.across_bridge.open = false;
         gui.sync_pools_ui.open = false;
         gui.settings.open = true;
      }

      let connected_dapps = Button::new(RichText::new("Connected Dapps").size(text_size));
      if ui.add(connected_dapps).clicked() {
         gui.connected_dapps.open();
      }

      #[cfg(feature = "dev")]
      if ui
         .add(Button::new(
            RichText::new("Theme Editor").size(text_size),
         ))
         .clicked()
      {
         gui.editor.open = true;
      }

      #[cfg(feature = "dev")]
      if ui
         .add(Button::new(
            RichText::new("FPS Metrics").size(text_size),
         ))
         .clicked()
      {
         gui.fps_metrics.open = true;
      }

      #[cfg(feature = "dev")]
      {
         let sync_pools = ui.add(Button::new(
            RichText::new("Sync Pools").size(text_size),
         ));
         if sync_pools.clicked() {
            gui.portofolio.open = false;
            gui.uniswap.close();
            gui.send_crypto.open = false;
            gui.wallet_ui.open = false;
            gui.tx_history.open = false;
            gui.across_bridge.open = false;
            gui.sync_pools_ui.open = false;
            gui.settings.open = false;
            gui.sync_pools_ui.open = true;
         }
      }

      #[cfg(feature = "dev")]
      {
         let ui_testing = ui.add(Button::new(
            RichText::new("Ui Testing").size(text_size),
         ));
         if ui_testing.clicked() {
            gui.ui_testing.show = true;
            gui.portofolio.open = false;
            gui.uniswap.close();
            gui.send_crypto.open = false;
            gui.wallet_ui.open = false;
            gui.tx_history.open = false;
            gui.across_bridge.open = false;
            gui.sync_pools_ui.open = false;
            gui.settings.open = false;
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
