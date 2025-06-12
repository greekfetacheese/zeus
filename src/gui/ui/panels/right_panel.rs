use crate::gui::GUI;
use eframe::egui::Ui;



pub fn show(ui: &mut Ui, gui: &mut GUI) {
    let theme = &gui.theme;

    let swap_ui_open = gui.uniswap.swap_ui.open;
    gui.uniswap.settings.show(swap_ui_open, theme, ui);
}