use crate::gui::GUI;
use eframe::egui::Ui;

pub fn show(ui: &mut Ui, gui: &mut GUI) {
   let theme = &gui.theme;
   let ctx = gui.ctx.clone();

   let swap_ui_open = gui.uniswap.swap_ui.open;
   let view_positions_open = gui.uniswap.view_positions_ui.open;
   gui.uniswap.settings.show(ctx, swap_ui_open, view_positions_open, theme, ui);
}
