use crate::gui::GUI;
use eframe::egui::{Align, RichText, Layout, Ui};

pub fn show(ui: &mut Ui, gui: &mut GUI) {
   let theme = &gui.theme;
   let icons = gui.icons.clone();
   let ctx = gui.ctx.clone();

   ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
      let server_running = ctx.server_running();

      let icon = match server_running {
         true => icons.server_green(theme.image_tint_recommended),
         false => icons.server_red(theme.image_tint_recommended),
      };

      let hover_text = if server_running {
         RichText::new("RPC Server is running").size(theme.text_sizes.normal)
      } else {
         RichText::new("RPC Server is not running").size(theme.text_sizes.normal)
      };

      ui.add_space(10.0);
      ui.add(icon).on_hover_text(hover_text);
   });
}
