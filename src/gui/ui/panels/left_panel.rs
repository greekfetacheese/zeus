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
      utils::bg_color_on_hover(ui, gui.theme.colors.widget_bg_color);
      utils::bg_color_on_click(ui, gui.theme.colors.widget_bg_color_click);
      utils::no_border_on_click(ui);

      let home = Button::new(RichText::new("Home").size(21.0));
      if ui.add(home).clicked() {
         gui.swap_ui.open = false;
         gui.send_crypto.open = false;
         gui.settings.open = false;
         gui.wallet_ui.open = false;
         gui.tx_history.open = false;
         gui.across_bridge.open = false;
         gui.portofolio.open = true;
      }

      let send = Button::new(RichText::new("Send").size(21.0));
      if ui.add(send).clicked() {
         gui.swap_ui.open = false;
         gui.portofolio.open = false;
         gui.settings.open = false;
         gui.wallet_ui.open = false;
         gui.tx_history.open = false;
         gui.across_bridge.open = false;
         gui.send_crypto.open = true;
         // This is shared, so reset it to avoid any issues
         gui.recipient_selection.reset();
      }

      let bridge = Button::new(RichText::new("Bridge").size(21.0));
      if ui.add(bridge).clicked() {
         gui.portofolio.open = false;
         gui.swap_ui.open = false;
         gui.send_crypto.open = false;
         gui.settings.open = false;
         gui.wallet_ui.open = false;
         gui.tx_history.open = false;
         // This is shared, so reset it to avoid any issues
         gui.recipient_selection.reset();
         gui.across_bridge.open = true;
      }

      let wallets = Button::new(RichText::new("Wallets").size(21.0));
      if ui.add(wallets).clicked() {
         gui.portofolio.open = false;
         gui.swap_ui.open = false;
         gui.send_crypto.open = false;
         gui.settings.open = false;
         gui.tx_history.open = false;
         gui.across_bridge.open = false;
         gui.wallet_ui.open = true;
      }

      let tx_history = Button::new(RichText::new("Transactions").size(21.0));
      if ui.add(tx_history).clicked() {
         gui.portofolio.open = false;
         gui.swap_ui.open = false;
         gui.send_crypto.open = false;
         gui.settings.open = false;
         gui.wallet_ui.open = false;
         gui.across_bridge.open = false;
         gui.tx_history.open = true;
      }

      
      #[cfg(feature = "dev")]
      {
         let swap = Button::new(RichText::new("Swap").size(21.0));
         if ui.add(swap).clicked() {
            gui.portofolio.open = false;
            gui.send_crypto.open = false;
            gui.settings.open = false;
            gui.wallet_ui.open = false;
            gui.tx_history.open = false;
            gui.across_bridge.open = false;
            gui.swap_ui.open = true;
         }
      }
      

      let settings = Button::new(RichText::new("Settings").size(21.0));
      if ui.add(settings).clicked() {
         gui.portofolio.open = false;
         gui.swap_ui.open = false;
         gui.send_crypto.open = false;
         gui.wallet_ui.open = false;
         gui.tx_history.open = false;
         gui.across_bridge.open = false;
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
         let ui_testing = ui.add(Button::new(
            RichText::new("Ui Testing").size(20.0),
         ));
         if ui_testing.clicked() {
            gui.ui_testing.show = true;
         }
      }
   });
}
