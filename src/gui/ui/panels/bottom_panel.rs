use crate::core::ZeusContext;
use crate::gui::GUI;
use eframe::egui::{Align, Layout, RichText, Ui};

pub fn show(gui: &mut GUI, ctx: &mut ZeusContext, ui: &mut Ui) {
   let theme = &gui.theme;
   let icons = gui.icons.clone();

   ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
      let icon = match ctx.server_running {
         true => icons.server_green(theme.image_tint_recommended),
         false => icons.server_red(theme.image_tint_recommended),
      };

      let hover_text = if ctx.server_running {
         RichText::new("RPC Server is running").size(theme.text_sizes.normal)
      } else {
         RichText::new("RPC Server is not running").size(theme.text_sizes.normal)
      };

      ui.add_space(10.0);
      ui.add(icon).on_hover_text(hover_text);
   });
}
