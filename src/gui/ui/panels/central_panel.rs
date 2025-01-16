use eframe::egui::Ui;
use crate::core::data::app_data::APP_DATA;
use crate::gui::GUI;

pub fn show(ui: &mut Ui, gui: &mut GUI) {
    gui.msg_window.show(ui);

    let logged_in;
    let profile_exists;
    {
        let app_data = APP_DATA.read().unwrap();
        logged_in = app_data.logged_in;
        profile_exists = app_data.profile_exists;
    }

    let theme = &gui.theme;
    let icons = gui.icons.clone();
    let token_selection = &mut gui.token_selection;

    if !profile_exists {
        gui.register.show(theme, ui);
        gui.portofolio.open = false;
        // ! We could early return but for some reason the whole window becomes transparent
    }

    if profile_exists && !logged_in {
        gui.login.show(theme, ui);
        gui.portofolio.open = false;
    }

    gui.portofolio.show(ui, gui.icons.clone());
    gui.swap_ui.show(ui, icons.clone(), theme, token_selection);
    gui.send_crypto.show(ui, &gui.theme, icons);

    let theme = gui.editor.show(&mut gui.theme, ui);
    if let Some(theme) = theme {
        gui.theme = theme;
    }
}
