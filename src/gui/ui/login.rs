use eframe::egui::Ui;
use egui_theme::{ Theme, utils::{ border_on_idle, border_on_click, border_on_hover } };
use crate::core::ZeusCtx;
use crate::gui::{SHARED_GUI, utils::get_profile_dir, ui::{ button, rich_text, text_edit_single }};
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

    pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {

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
                let mut profile = ctx.profile();
                profile.credentials = self.credentials.clone();

                std::thread::spawn(move || {
                    let dir = get_profile_dir();
                    match profile.decrypt_and_load(&dir) {
                        Ok(_) => {
                            let mut gui = SHARED_GUI.write().unwrap();
                            gui.login.credentials.erase();
                            gui.portofolio.open = true;
                            gui.profile_area.open = true;
                            gui.wallet_select.wallet = profile.current_wallet.clone();

                            ctx.write(|ctx| {
                                ctx.profile = profile;
                                ctx.logged_in = true;
                            });
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

    pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
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
                    let mut profile = ctx.profile();
                    profile.credentials = self.credentials.clone();

                    std::thread::spawn(move || {
                        let dir = get_profile_dir();
                        match profile.encrypt_and_save(&dir, Argon2Params::very_fast()) {
                            Ok(_) => {
                                let mut gui = SHARED_GUI.write().unwrap();
                                gui.wallet_select.wallet = profile.current_wallet.clone();
                                gui.register.credentials.erase();
                                gui.portofolio.open = true;
                                gui.profile_area.open = true;

                                ctx.write(|ctx| {
                                ctx.profile_exists = true;
                                ctx.logged_in = true;
                                ctx.profile = profile;
                                });
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
