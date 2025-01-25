use eframe::egui::Ui;
use crate::gui::GUI;

pub fn show(ui: &mut Ui, gui: &mut GUI) {
    should_show_overlay(gui);
    gui.msg_window.show(ui);
    gui.loading_window.show(ui);

    let ctx = gui.ctx.clone();

    let logged_in = ctx.logged_in();
    let profile_exists = ctx.profile_exists();

    let theme = &gui.theme;
    let icons = gui.icons.clone();
    let token_selection = &mut gui.token_selection;

    if !profile_exists {
        gui.register.show(ctx.clone(), theme, ui);
        gui.portofolio.open = false;
        // ! We could early return but for some reason the whole window becomes transparent
    }

    if profile_exists && !logged_in {
        gui.login.show(ctx.clone(), theme, ui);
        gui.portofolio.open = false;
    }

    gui.portofolio.show(ctx.clone(), icons.clone(), ui);
    gui.swap_ui.show(ctx.clone(), icons.clone(), theme, token_selection, ui);
    gui.settings.show(ctx.clone(), icons.clone(), theme, ui);
    gui.send_crypto.show(ctx.clone(), icons.clone(), theme, ui);

    gui.profile_area.wallet_ui.add_wallet_ui.show(ctx.clone(), theme, ui);
    gui.profile_area.wallet_ui.wallet_details.show(ctx.clone(), theme, ui);

    let theme = gui.editor.show(&mut gui.theme, ui);
    if let Some(theme) = theme {
        gui.theme = theme;
    }
}

fn should_show_overlay(gui: &mut GUI) {
    if gui.settings.credentials.open {
        gui.show_overlay = true;
    } else if gui.msg_window.open {
        gui.show_overlay = true;
    } else if gui.loading_window.open {
        gui.show_overlay = true;
    } else if gui.settings.contacts_ui.open {
        gui.show_overlay = true;
    } else {
        gui.show_overlay = false;
    }
}
