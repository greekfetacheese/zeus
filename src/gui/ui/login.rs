use eframe::egui::Ui;
use egui_theme::{ Theme, utils::{ border_on_idle, border_on_click, border_on_hover } };
use crate::core::{ user::profile::Profile, data::app_data::APP_DATA };
use crate::gui::SHARED_GUI;
use crate::gui::ui::{ button, rich_text, text_edit_single };
use ncrypt::prelude::{ Argon2Params, Credentials };


pub struct LoginUi {
    pub credentials: Credentials,
}

impl LoginUi {
    pub fn new() -> Self {
        Self {
            credentials: Credentials::default(),
        }
    }

    pub fn show(&mut self, theme: &Theme, ui: &mut Ui) {

        ui.vertical_centered(|ui| {
            border_on_idle(ui, 1.0, theme.colors.border_color_idle);
            border_on_hover(ui, 1.0, theme.colors.border_color_hover);
            border_on_click(ui, 1.0, theme.colors.border_color_click);
            ui.spacing_mut().item_spacing.y = 20.0;
            ui.label(rich_text("Unlock Profile").size(20.0));

            {

                let username = self.credentials.user_mut();
                let text_edit = text_edit_single(username);

                ui.label(rich_text("Username").size(16.0));
                ui.add(text_edit);
            }

            {
                let password = self.credentials.passwd_mut();
                let text_edit = text_edit_single(password).password(true);

                ui.label(rich_text("Password").size(16.0));
                ui.add(text_edit);
            }

            {
                // copy password to confirm password
                self.credentials.copy_passwd_to_confirm();
            }

            let button = button(rich_text("Unlock").size(16.0));

            if ui.add(button).clicked() {
                let mut profile;
                {
                    let app_data = APP_DATA.read().unwrap();
                    profile = app_data.profile.clone();
                }
                profile.credentials = self.credentials.clone();

                std::thread::spawn(move || {
                    let dir = match Profile::profile_dir() {
                        Ok(dir) => dir,
                        Err(e) => {
                            let mut gui = SHARED_GUI.write().unwrap();
                            gui.open_msg_window("Failed to unlock profile", e.to_string());
                            return;
                        }
                    };
                    match profile.decrypt_and_load(&dir) {
                        Ok(_) => {
                            let mut gui = SHARED_GUI.write().unwrap();
                            gui.login.credentials.erase();
                            gui.portofolio.open = true;
                            gui.profile_area.open = true;
                            gui.wallet_select.wallet = profile.current_wallet.clone();

                            let mut app_data = APP_DATA.write().unwrap();
                            app_data.logged_in = true;
                            app_data.profile = profile;
                        }
                        Err(e) => {
                            let mut gui = SHARED_GUI.write().unwrap();
                            gui.open_msg_window("Failed to unlock profile", e.to_string());
                        }
                    };
                });
            }
        });
    }
}

pub struct RegisterUi {
    pub main_ui_open: bool,
    pub wallet_ui_open: bool,
    pub credentials: Credentials,
}

impl RegisterUi {
    pub fn new() -> Self {
        Self {
            main_ui_open: true,
            wallet_ui_open: false,
            credentials: Credentials::default(),
        }
    }

    pub fn show(&mut self, theme: &Theme, ui: &mut Ui) {
        self.main_ui(theme, ui);
    }

    pub fn main_ui(&mut self, theme: &Theme, ui: &mut Ui) {
        if !self.main_ui_open {
            return;
        }

        ui.vertical_centered(|ui| {
            ui.spacing_mut().item_spacing.y = 20.0;
            border_on_idle(ui, 1.0, theme.colors.border_color_idle);
            border_on_hover(ui, 1.0, theme.colors.border_color_hover);
            border_on_click(ui, 1.0, theme.colors.border_color_click);

            ui.label(rich_text("Create Profile").size(20.0));

            {
                let username = self.credentials.user_mut();
                let text_edit = text_edit_single(username);

                ui.label(rich_text("Username").size(16.0));
                ui.add(text_edit);
            }

            {
                let password = self.credentials.passwd_mut();
                let text_edit = text_edit_single(password).password(true);

                ui.label(rich_text("Password").size(16.0));
                ui.add(text_edit);
            }

            {
                let confirm_password = self.credentials.confirm_passwd_mut();
                let text_edit = text_edit_single(confirm_password).password(true);

                ui.label(rich_text("Confirm Password").size(16.0));
                ui.add(text_edit);
            }

            {
                let button = button(rich_text("Create").size(16.0));

                if ui.add(button).clicked() {
                    let mut profile;
                    {
                        let app_data = APP_DATA.read().unwrap();
                        profile = app_data.profile.clone();
                    }

                    profile.credentials = self.credentials.clone();

                    std::thread::spawn(move || {
                        let dir = match Profile::profile_dir() {
                            Ok(dir) => dir,
                            Err(e) => {
                                let mut gui = SHARED_GUI.write().unwrap();
                                gui.open_msg_window("Failed to create profile", e.to_string());
                                return;
                            }
                        };
                        match profile.encrypt_and_save(&dir, Argon2Params::very_fast()) {
                            Ok(_) => {
                                let mut gui = SHARED_GUI.write().unwrap();
                                gui.wallet_select.wallet = profile.current_wallet.clone();
                                gui.register.credentials.erase();
                                gui.portofolio.open = true;
                                gui.profile_area.open = true;

                                let mut app_data = APP_DATA.write().unwrap();
                                app_data.profile_exists = true;
                                app_data.logged_in = true;
                                app_data.profile = profile;
                            }
                            Err(e) => {
                                let mut gui = SHARED_GUI.write().unwrap();
                                gui.open_msg_window("Failed to create profile", e.to_string());
                            }
                        };
                    });
                }
            }
        });
    }
}
