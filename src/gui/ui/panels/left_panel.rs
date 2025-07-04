use crate::gui::GUI;
use eframe::egui::{Button, Color32, RichText, Ui};
use egui_theme::utils;

pub fn show(ui: &mut Ui, gui: &mut GUI) {
   ui.vertical_centered(|ui| {
      ui.add_space(20.0);
      ui.spacing_mut().item_spacing.y = 30.0;
      ui.visuals_mut().widgets.hovered.expansion = 15.0;
      ui.visuals_mut().widgets.active.expansion = 15.0;

      utils::bg_color_on_idle(ui, Color32::TRANSPARENT);
      utils::no_border(ui);

      let home = Button::new(RichText::new("Home").size(21.0));
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

      let swap = Button::new(RichText::new("Swap").size(21.0));
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

      let send = Button::new(RichText::new("Send").size(21.0));
      if ui.add(send).clicked() {
         gui.uniswap.close();
         gui.portofolio.open = false;
         gui.settings.open = false;
         gui.wallet_ui.open = false;
         gui.tx_history.open = false;
         gui.across_bridge.open = false;
         gui.sync_pools_ui.open = false;
         gui.send_crypto.open = true;
         // This is shared, so reset it to avoid any issues
         gui.recipient_selection.reset();
      }

      let bridge = Button::new(RichText::new("Bridge").size(21.0));
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

      let wallets = Button::new(RichText::new("Wallets").size(21.0));
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

      let tx_history = Button::new(RichText::new("Transactions").size(21.0));
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

      let settings = Button::new(RichText::new("Settings").size(21.0));
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

      #[cfg(feature = "dev")]
      if ui
         .add(Button::new(
            RichText::new("Theme Editor").size(20.0),
         ))
         .clicked()
      {
         gui.editor.open = true;
      }

      #[cfg(feature = "dev")]
      if ui.add(Button::new(RichText::new("FPS Metrics").size(20.0))).clicked() {
         gui.fps_metrics.open = true;
      }

      #[cfg(feature = "dev")]
      {
         let test_window = ui.add(Button::new(
            RichText::new("Test Window").size(20.0),
         ));
         if test_window.clicked() {
            gui.testing_window.open = true;
         }
      }

      #[cfg(feature = "dev")]
      {
         let sync_pools = ui.add(Button::new(
            RichText::new("Sync Pools").size(20.0),
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
            RichText::new("Ui Testing").size(20.0),
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
