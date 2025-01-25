use eframe::egui::{Ui, Color32};

use crate::gui::{GUI, ui::{rich_text, button}};
use egui_theme::utils;

pub fn show(ui: &mut Ui, gui: &mut GUI) {
    ui.vertical_centered(|ui| {

    ui.add_space(20.0);
    ui.spacing_mut().item_spacing.y = 30.0;
    ui.visuals_mut().widgets.hovered.expansion = 15.0;
    ui.visuals_mut().widgets.active.expansion = 15.0;

    utils::bg_color_on_idle(ui, Color32::TRANSPARENT);
    utils::bg_color_on_hover(ui, gui.theme.colors.widget_bg_color_idle);
    utils::bg_color_on_click(ui, gui.theme.colors.widget_bg_color_click);
    utils::no_border_on_click(ui);


    let home = button(rich_text("Home").size(21.0));
    if ui.add(home).clicked() {
        gui.swap_ui.open = false;
        gui.send_crypto.open = false;
        gui.settings.open = false;
        gui.portofolio.open = true;

    }

    let swap = button(rich_text("Swap").size(21.0));
    if ui.add(swap).clicked() {
        gui.portofolio.open = false;
        gui.send_crypto.open = false;
        gui.settings.open = false;
        gui.swap_ui.open = true;
    }
    
    let send = button(rich_text("Send").size(21.0));
    if ui.add(send).clicked() {
        gui.swap_ui.open = false;
        gui.portofolio.open = false;
        gui.settings.open = false;
        gui.send_crypto.open = true;
    }

    let settings = button(rich_text("Settings").size(21.0));
    if ui.add(settings).clicked() {
        gui.portofolio.open = false;
        gui.swap_ui.open = false;
        gui.send_crypto.open = false;
        gui.settings.open = true;
    }





    if ui.add(button(rich_text("Theme Editor").size(20.0))).clicked() {
        gui.editor.open = true;
    }
    
});
}