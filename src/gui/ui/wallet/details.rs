use eframe::egui::{ vec2, Align, Align2, Frame, Layout, Ui, Window };
use egui::Color32;
use std::sync::Arc;
use egui_theme::{ Theme, utils::{ border_on_idle, border_on_hover, bg_color_on_idle } };
use crate::core::{ Wallet, ZeusCtx };
use crate::assets::icons::Icons;
use crate::gui::{ self, ui::{ rich_text, button, img_button, text_edit_single, text_edit_multi }, SHARED_GUI };
use ncrypt::{ prelude::Credentials, zeroize::Zeroize };

/// Ui to show the details of a wallet
pub struct WalletDetails {
    pub open: bool,
    pub main_ui: bool,
    pub view_key: bool,
    pub verify_credentials_ui: bool,
    pub verified_credentials: bool,
    pub delete_wallet: bool,
    pub wallet: Wallet,
    pub credentials: Credentials,
    pub key: String,
    pub size: (f32, f32),
}

impl WalletDetails {
    pub fn new() -> Self {
        Self {
            open: false,
            main_ui: true,
            wallet: Wallet::new_rng("I should not be here".to_string()),
            view_key: false,
            verify_credentials_ui: false,
            verified_credentials: false,
            delete_wallet: false,
            credentials: Credentials::default(),
            key: String::new(),
            size: (300.0, 400.0),
        }
    }

    pub fn show(&mut self, ctx: ZeusCtx, icons: Arc<Icons>, theme: &Theme, parent_open: &mut bool, ui: &mut Ui) {
        if !self.open {
            return;
        }

        ui.set_width(self.size.0);
        ui.set_height(self.size.1);

        self.main_ui(ctx.clone(), icons.clone(), theme, parent_open, ui);
        self.view_key(ui);
        self.verify_credentials(ctx, icons, theme, ui);
    }

    fn main_ui(&mut self, ctx: ZeusCtx, icons: Arc<Icons>, theme: &Theme, parent_open: &mut bool, ui: &mut Ui) {
        if !self.main_ui {
            return;
        }

        // Go back button
        ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.set_max_width(30.0);
            bg_color_on_idle(ui, Color32::TRANSPARENT);
            let back_button = img_button(icons.arrow_back(), "").min_size(vec2(30.0, 20.0));
            if ui.add(back_button).clicked() {
                self.open = false;
                *parent_open = true;
            }
        });

        let width = (ui.available_width() * 8.0) / 10.0;
        let frame = theme.frame2;

        ui.vertical_centered(|ui| {
            ui.spacing_mut().item_spacing.y = 10.0;

            ui.label(rich_text("Wallet Details"));

            ui.label(rich_text(self.wallet.name.clone()));

            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                ui.label(rich_text("Address"));
            });

            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                frame.show(ui, |ui| {
                    ui.set_width(width);
                    ui.horizontal_wrapped(|ui| {
                        if ui.selectable_label(false, rich_text(self.wallet.address())).clicked() {
                            ui.ctx().copy_text(self.wallet.address());
                        }
                    });
                });
            });

            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                ui.label(rich_text("Notes"));
            });

            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                frame.show(ui, |ui| {
                    ui.set_width(width);
                    border_on_idle(ui, 1.0, Color32::WHITE);
                    border_on_hover(ui, 1.0, theme.colors.border_color_hover);
                    if ui.add(text_edit_multi(&mut self.wallet.notes).desired_rows(2)).changed() {
                        // println!("Notes: {:?}", self.wallet.notes);
                    }
                });
            });

            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                ui.label(rich_text("Assets Value"));
            });

            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                // TODO: calculate assets value
                frame.show(ui, |ui| {
                    ui.set_width(width);
                    ui.label(rich_text("$100,000,00"));
                });
            });

            let view_key = button(rich_text("View Key"));
            if ui.add(view_key).clicked() {
                self.main_ui = false;
                *parent_open = false;
                self.verify_credentials_ui = true;
            }

            // let delete_button = button(rich_text("Delete Wallet"));
        });
    }

    fn view_key(&mut self, ui: &mut Ui) {
        if !self.view_key {
            return;
        }

        Window::new("view_key")
            .title_bar(false)
            .anchor(Align2::LEFT_TOP, vec2(200.0, 65.0))
            .resizable(false)
            .frame(Frame::window(ui.style()))
            .show(ui.ctx(), |ui| {
                ui.spacing_mut().item_spacing.y = 20.0;
                ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                    let close = button(rich_text("Close"));
                    if ui.add(close).clicked() {
                        self.key.zeroize();
                        self.main_ui = true;
                        self.view_key = false;
                    }
                });

                ui.vertical_centered(|ui| {
                    ui.add(text_edit_multi(&mut self.key));
                    if ui.add(button(rich_text("Copy Key"))).clicked() {
                        ui.ctx().copy_text(self.key.clone());
                    }
                });
            });
    }

    fn verify_credentials(&mut self, ctx: ZeusCtx, icons: Arc<Icons>, theme: &Theme, ui: &mut Ui) {
        if !self.verify_credentials_ui {
            return;
        }

        // Go back button
        ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.set_max_width(30.0);
            bg_color_on_idle(ui, Color32::TRANSPARENT);
            let back_button = img_button(icons.arrow_back(), "").min_size(vec2(30.0, 20.0));
            if ui.add(back_button).clicked() {
                self.verify_credentials_ui = false;
                self.main_ui = true;
                self.key.zeroize();
                self.credentials.erase();
            }
        });

        ui.vertical_centered(|ui| {
            ui.spacing_mut().item_spacing.y = 20.0;
            ui.label(rich_text("Verify your credentials").size(18.0));

            ui.scope(|ui| {
                border_on_idle(ui, 1.0, theme.colors.border_color_idle);
                border_on_hover(ui, 1.0, theme.colors.border_color_hover);

                ui.label(rich_text("Username"));
                ui.add(text_edit_single(self.credentials.user_mut()));

                ui.label(rich_text("Password"));
                ui.add(text_edit_single(self.credentials.passwd_mut()).password(true));
            });

            // skip the passwd confrimation
            self.credentials.copy_passwd_to_confirm();

            let confirm_button = button(rich_text("Confirm"));
            if ui.add(confirm_button).clicked() {
                let mut profile = ctx.profile();
                profile.credentials = self.credentials.clone();

                std::thread::spawn(move || {
                    let dir = gui::utils::get_profile_dir();

                    let mut data = match profile.decrypt(&dir) {
                        Ok(data) => {
                            let mut gui = SHARED_GUI.write().unwrap();
                            gui.profile_area.wallet_ui.wallet_details.credentials.erase();
                            gui.profile_area.wallet_ui.wallet_details.key =
                                gui.profile_area.wallet_ui.wallet_details.wallet.key_string();
                            gui.profile_area.wallet_ui.wallet_details.view_key = true;
                            gui.profile_area.wallet_ui.wallet_details.verify_credentials_ui = false;
                            gui.profile_area.wallet_ui.wallet_details.main_ui = true;
                            data
                        }
                        Err(e) => {
                            let mut gui = SHARED_GUI.write().unwrap();
                            gui.open_msg_window("Invalid Credentials", e.to_string());
                            return;
                        }
                    };
                    // erase data we dont need it
                    data.zeroize();
                });
            }
        });
    }
}
