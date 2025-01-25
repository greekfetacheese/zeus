use eframe::egui::Ui;
use egui_theme::{ Theme, utils::{ border_on_idle, border_on_click, border_on_hover } };
use crate::core::ZeusCtx;
use crate::gui::{ SHARED_GUI, utils::get_profile_dir, ui::{ button, rich_text, text_edit_single } };
use ncrypt::prelude::{ Argon2Params, Credentials };

pub struct CredentialsForm {
    pub open: bool,
    pub confrim_password: bool,
    pub credentials: Credentials,
}

impl CredentialsForm {
    pub fn new() -> Self {
        Self {
            open: false,
            confrim_password: false,
            credentials: Credentials::default(),
        }
    }

    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    pub fn confirm_password(mut self, confirm_password: bool) -> Self {
        self.confrim_password = confirm_password;
        self
    }

    pub fn erase(&mut self) {
        self.credentials.erase();
    }

    pub fn show(&mut self, theme: &Theme, ui: &mut Ui) {
        if !self.open {
            return;
        }

        ui.vertical_centered(|ui| {
            border_on_idle(ui, 1.0, theme.colors.border_color_idle);
            border_on_hover(ui, 1.0, theme.colors.border_color_hover);
            border_on_click(ui, 1.0, theme.colors.border_color_click);
            ui.spacing_mut().item_spacing.y = 15.0;

            let username = self.credentials.user_mut();
            let text_edit = text_edit_single(username);

            ui.label(rich_text("Username"));
            ui.add(text_edit);

            let password = self.credentials.passwd_mut();
            let text_edit = text_edit_single(password).password(true);

            ui.label(rich_text("Password"));
            ui.add(text_edit);

            if self.confrim_password {
                let confirm_password = self.credentials.confirm_passwd_mut();
                let text_edit = text_edit_single(confirm_password).password(true);

                ui.label(rich_text("Confirm Password"));
                ui.add(text_edit);
            } else {
                // copy password to confirm password
                self.credentials.copy_passwd_to_confirm();
            }
        });
    }
}

pub struct LoginUi {
    pub credentials_form: CredentialsForm,
}

impl LoginUi {
    pub fn new() -> Self {
        Self {
            credentials_form: CredentialsForm::new().open(true),
        }
    }

    pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
        ui.label(rich_text("Unlock your profile").size(16.0));
        ui.add_space(15.0);

        self.credentials_form.show(theme, ui);
        ui.add_space(15.0);

        let button = button(rich_text("Unlock").size(16.0));

        if ui.add(button).clicked() {
            let mut profile = ctx.profile();
            profile.credentials = self.credentials_form.credentials.clone();

            std::thread::spawn(move || {
                {
                    let mut gui = SHARED_GUI.write().unwrap();
                    gui.loading_window.msg = "Unlocking profile...".to_string();
                    gui.loading_window.open = true;
                }
                let dir = get_profile_dir();
                match profile.decrypt_and_load(&dir) {
                    Ok(_) => {
                        let mut gui = SHARED_GUI.write().unwrap();
                        gui.login.credentials_form.erase();
                        gui.portofolio.open = true;
                        gui.profile_area.open = true;
                        gui.wallet_select.wallet = profile.current_wallet.clone();
                        gui.loading_window.open = false;

                        ctx.write(|ctx| {
                            ctx.profile = profile;
                            ctx.logged_in = true;
                        });
                    }
                    Err(e) => {
                        let mut gui = SHARED_GUI.write().unwrap();
                        gui.open_msg_window("Failed to unlock profile", e.to_string());
                        gui.loading_window.open = false;
                    }
                };
            });
        }
    }
}

pub struct RegisterUi {
    pub credentials_form: CredentialsForm,
}

impl RegisterUi {
    pub fn new() -> Self {
        Self {
            credentials_form: CredentialsForm::new().open(true).confirm_password(true),
        }
    }

    pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
        ui.label(rich_text("Create a new profile").size(16.0));
        ui.add_space(15.0);

        self.credentials_form.show(theme, ui);
        ui.add_space(15.0);

        {
            let button = button(rich_text("Create").size(16.0));

            if ui.add(button).clicked() {
                let mut profile = ctx.profile();
                profile.credentials = self.credentials_form.credentials.clone();

                std::thread::spawn(move || {
                    {
                        let mut gui = SHARED_GUI.write().unwrap();
                        gui.loading_window.msg = "Creating profile...".to_string();
                        gui.loading_window.open = true;
                    }
                    let dir = get_profile_dir();
                    match profile.encrypt_and_save(&dir, Argon2Params::very_fast()) {
                        Ok(_) => {
                            let mut gui = SHARED_GUI.write().unwrap();
                            gui.wallet_select.wallet = profile.current_wallet.clone();
                            gui.register.credentials_form.erase();
                            gui.portofolio.open = true;
                            gui.profile_area.open = true;
                            gui.loading_window.open = false;

                            ctx.write(|ctx| {
                                ctx.profile_exists = true;
                                ctx.logged_in = true;
                                ctx.profile = profile;
                            });
                        }
                        Err(e) => {
                            let mut gui = SHARED_GUI.write().unwrap();
                            gui.open_msg_window("Failed to create profile", e.to_string());
                            gui.loading_window.open = false;
                        }
                    };
                });
            }
        }
    }
}