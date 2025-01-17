pub mod details;
pub use details::WalletDetails;

use eframe::egui::{ vec2, Align, Grid, Frame, Layout, Window, ScrollArea, Ui };
use egui::{ Align2, Color32 };
use egui_theme::{
    Theme,
    utils::{ border_on_idle, border_on_hover, bg_color_on_idle, bg_color_on_click, bg_color_on_hover, border_on_click },
};
use std::sync::Arc;
use crate::core::{Wallet, ZeusCtx};
use crate::assets::icons::Icons;
use crate::gui::{ self, ui::{ rich_text, button, img_button, text_edit_single }, SHARED_GUI };
use ncrypt::zeroize::Zeroize;

/// Ui to manage the wallets
pub struct WalletUi {
    pub open: bool,
    pub window: Window<'static>,
    pub main_ui: bool,
    pub add_wallet: bool,
    pub import_wallet: bool,
    pub generate_wallet: bool,
    pub imported_key: String,
    pub wallet_name: String,
    pub search_query: String,
    pub wallet_details: WalletDetails,
    pub size: (f32, f32),
}

impl WalletUi {
    pub fn new() -> Self {
        let size = (400.0, 400.0);
        let offset = vec2(size.0, 40.0);
        let window = Window::new("Wallet Ui")
            .resizable(false)
            .title_bar(false)
            .anchor(Align2::LEFT_TOP, offset)
            .collapsible(false);

        Self {
            open: false,
            window,
            main_ui: true,
            add_wallet: false,
            import_wallet: false,
            generate_wallet: false,
            imported_key: String::new(),
            wallet_name: String::new(),
            search_query: String::new(),
            wallet_details: WalletDetails::new(),
            size,
        }
    }

    fn window_id(&self) -> &str {
        if self.main_ui {
            "Wallets"
        } else if self.add_wallet {
            "Add Wallet"
        } else if self.import_wallet {
            "Import Wallet"
        } else if self.generate_wallet {
            "Generate Wallet"
        } else {
            "Wallet Details"
        }
    }

    fn window_size(&self) -> (f32, f32) {
        if self.main_ui {
            (self.size.0, self.size.1)
        } else if self.add_wallet {
            (self.size.0, 250.0)
        } else if self.import_wallet {
            (self.size.0, 250.0)
        } else if self.generate_wallet {
            (self.size.0, 250.0)
        } else {
            self.wallet_details.size
        }
    }

    pub fn show(&mut self, ctx: ZeusCtx, icons: Arc<Icons>, theme: &Theme, ui: &mut Ui) {
        if !self.open {
            return;
        }

        let offset = vec2(self.size.0, 40.0);
        Window::new(self.window_id())
            .resizable(false)
            .collapsible(false)
            .anchor(Align2::LEFT_TOP, offset)
            .frame(Frame::window(ui.style()))
            .show(ui.ctx(), |ui| {
                ui.set_width(self.window_size().0);
                ui.set_height(self.window_size().1);

                self.main_ui(ctx.clone(), icons.clone(), theme, ui);
                self.add_wallet_ui(icons.clone(), ui);
                self.import_wallet_ui(ctx.clone(), icons.clone(), theme, ui);
                self.generate_wallet_ui(ctx.clone(), icons.clone(), theme, ui);

                let mut main_ui = self.main_ui;
                self.wallet_details.show(ctx, icons, theme, &mut main_ui, ui);
                self.main_ui = main_ui;
            });
    }

    /// This is the first Ui we show to the user when this [WalletUi] is open.
    ///
    /// We can see, manage and add new wallets.
    pub fn main_ui(&mut self, ctx: ZeusCtx, icons: Arc<Icons>, theme: &Theme, ui: &mut Ui) {
        if !self.main_ui {
            return;
        }

        let profile = ctx.profile();
        let current_wallet = profile.current_wallet.clone();
        let wallets = profile.wallets.clone();
        drop(profile);

        ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            let close_button = img_button(icons.arrow_back(), "").min_size(vec2(30.0, 20.0));
            bg_color_on_idle(ui, Color32::TRANSPARENT);
            if ui.add(close_button).clicked() {
                self.open = false;
            }
        });

        ui.vertical_centered(|ui| {
            ui.spacing_mut().item_spacing.y = 20.0;
            let frame_width = (ui.available_width() * 9.0) / 10.0;

            // add wallet button
            let add_wallet_button = img_button(icons.add_wallet_icon(), "").min_size(vec2(30.0, 25.0));
            ui.scope(|ui| {
                bg_color_on_idle(ui, Color32::TRANSPARENT);
                if ui.add(add_wallet_button).clicked() {
                    self.add_wallet = true;
                    self.main_ui = false;
                }
            });

            // show the current wallet
            Frame::none().show(ui, |ui| {
                ui.set_width(frame_width);
                ui.set_height(40.0);

                ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                    ui.radio_value(&mut true, true, "");
                    self.wallet_name_and_address(icons.clone(), &current_wallet, ui);
                });
            });

            // show the search bar
            ui.scope(|ui| {
                border_on_idle(ui, 1.0, theme.colors.border_color_idle);
                border_on_hover(ui, 1.0, theme.colors.border_color_hover);
                ui.set_width(frame_width);
                ui.add(text_edit_single(&mut self.search_query).hint_text("Search"))
            });
            ui.add_space(20.0);

            // show the rest of the wallets
            ScrollArea::vertical().show(ui, |ui| {
                ui.set_width(self.size.0);
                // ui.set_height(self.size.1);

                for wallet in &wallets {
                    if wallet == &current_wallet {
                        continue;
                    }

                    let search_query = self.search_query.to_lowercase();
                    let valid_search = wallet.name.to_lowercase().contains(&search_query);
                    let mut checked = false;

                    if valid_search {
                        Frame::none().show(ui, |ui| {
                            ui.set_width(frame_width);
                            ui.set_height(40.0);
                             ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                              ui.radio_value(&mut checked, true, "");
                            self.wallet_name_and_address(icons.clone(), wallet, ui);
                             });
                        });
                    }

                    if checked {
                        // change wallet
                        ctx.write(|ctx| {
                            ctx.profile.current_wallet = wallet.clone();
                        });
                    }
                }
            });
        });
    }

    pub fn add_wallet_ui(&mut self, icons: Arc<Icons>, ui: &mut Ui) {
        if !self.add_wallet {
            return;
        }

        // Go back button
        ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.set_max_width(30.0);
            bg_color_on_idle(ui, Color32::TRANSPARENT);
            let back_button = img_button(icons.arrow_back(), "").min_size(vec2(30.0, 20.0));
            if ui.add(back_button).clicked() {
                self.add_wallet = false;
                self.main_ui = true;
            }
        });
        ui.add_space(30.0);

        ui.vertical_centered(|ui| {
            ui.spacing_mut().item_spacing.y = 20.0;

            let import_button = button(rich_text("Import from a private key").size(18.0));
            let generate_button = button(rich_text("Generate a new").size(18.0));

            if ui.add(import_button).clicked() {
                self.add_wallet = false;
                self.import_wallet = true;
            }

            if ui.add(generate_button).clicked() {
                self.add_wallet = false;
                self.generate_wallet = true;
            }
        });
    }

    pub fn import_wallet_ui(&mut self, ctx: ZeusCtx, icons: Arc<Icons>, theme: &Theme, ui: &mut Ui) {
        if !self.import_wallet {
            return;
        }

        // Go back button
        ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.set_max_width(30.0);
            bg_color_on_idle(ui, Color32::TRANSPARENT);
            let back_button = img_button(icons.arrow_back(), "").min_size(vec2(30.0, 20.0));
            if ui.add(back_button).clicked() {
                self.imported_key.zeroize();
                self.import_wallet = false;
                self.add_wallet = true;
            }
        });
        ui.add_space(30.0);

        ui.vertical_centered(|ui| {
            ui.spacing_mut().item_spacing.y = 20.0;

            ui.scope(|ui| {
                border_on_idle(ui, 1.0, theme.colors.border_color_idle);
                border_on_hover(ui, 1.0, theme.colors.border_color_hover);

                ui.label(rich_text("Private Key").size(18.0));
                ui.add(text_edit_single(&mut self.imported_key).password(true));

                ui.label(rich_text("Wallet Name (Optional)").size(18.0));
                ui.add(text_edit_single(&mut self.wallet_name));
            });

            let import_button = button(rich_text("Import").size(16.0));

            if ui.add(import_button).clicked() {
                let name = self.wallet_name.clone();
                let key = self.imported_key.clone();

                std::thread::spawn(move || {
                    let mut profile = ctx.profile();

                    gui::utils::import_wallet(&mut profile, name, key);
                    let dir = gui::utils::get_profile_dir();
                    let info = gui::utils::get_encrypted_info(&dir);

                    match profile.encrypt_and_save(&dir, info.argon2_params) {
                        Ok(_) => {
                            let mut gui = SHARED_GUI.write().unwrap();
                            gui.profile_area.wallet_ui.imported_key.zeroize();
                            gui.profile_area.wallet_ui.wallet_name.clear();
                            gui.open_msg_window("Wallet imported successfully", "");
                        }
                        Err(e) => {
                            let mut gui = SHARED_GUI.write().unwrap();
                            gui.open_msg_window("Failed to save profile", e.to_string());
                            return;
                        }
                    }
                });
            }
        });
    }

    pub fn generate_wallet_ui(&mut self, ctx: ZeusCtx, icons: Arc<Icons>, theme: &Theme, ui: &mut Ui) {
        if !self.generate_wallet {
            return;
        }

        ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.set_max_width(30.0);
            bg_color_on_idle(ui, Color32::TRANSPARENT);
            let back_button = img_button(icons.arrow_back(), "").min_size(vec2(30.0, 20.0));
            if ui.add(back_button).clicked() {
                self.generate_wallet = false;
                self.add_wallet = true;
            }
        });
        ui.add_space(30.0);

        ui.vertical_centered(|ui| {
            ui.spacing_mut().item_spacing.y = 20.0;
            let generate_button = button(rich_text("Generate").size(16.0));

            ui.label(rich_text("Wallet Name (Optional)").size(18.0));

            ui.scope(|ui| {
                border_on_idle(ui, 1.0, theme.colors.border_color_idle);
                border_on_hover(ui, 1.0, theme.colors.border_color_hover);
                ui.add(text_edit_single(&mut self.wallet_name));
            });

            if ui.add(generate_button).clicked() {
                let wallet_name = self.wallet_name.clone();

                std::thread::spawn(move || {
                    let mut profile = ctx.profile();

                    gui::utils::new_wallet(&mut profile, wallet_name);
                    let dir = gui::utils::get_profile_dir();
                    let info = gui::utils::get_encrypted_info(&dir);

                    match profile.encrypt_and_save(&dir, info.argon2_params) {
                        Ok(_) => {
                            ctx.write(|ctx| {
                                ctx.profile = profile;
                            });

                            let mut gui = SHARED_GUI.write().unwrap();
                            gui.open_msg_window("Wallet created successfully", "");
                        }
                        Err(e) => {
                            let mut gui = SHARED_GUI.write().unwrap();
                            gui.open_msg_window("Failed to encrypt and save profile", e.to_string());
                        }
                    }
                });
            }
        });
    }

    /// Show the wallet name and its address and the trash icon
    pub fn wallet_name_and_address(&mut self, icons: Arc<Icons>, wallet: &Wallet, ui: &mut Ui) {

         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
        Grid::new(format!("wallet_{}_grid", wallet.name))
            .show(ui, |ui| {
                let button = img_button(icons.right_arrow(), rich_text(wallet.name.clone())).min_size(vec2(150.0, 25.0));
                ui.horizontal_wrapped(|ui| {
                ui.set_max_width(150.0);
                if ui.add(button).clicked() {
                    self.main_ui = false;
                    self.wallet_details.wallet = wallet.clone();
                    self.wallet_details.open = true;
                }
            });
            });

        let address = wallet.address_truncated();
        if ui.selectable_label(false, rich_text(address).size(12.0)).clicked() {
            ui.ctx().copy_text(wallet.key.address().to_string());
        }

        let trash_button = img_button(icons.trash(), "").min_size(vec2(16.0, 16.0));
        ui.scope(|ui| {
            bg_color_on_idle(ui, Color32::TRANSPARENT);
            bg_color_on_hover(ui, Color32::TRANSPARENT);
            bg_color_on_click(ui, Color32::TRANSPARENT);
            border_on_click(ui, 1.0, Color32::RED);
            if ui.add(trash_button).clicked() {
                // TODO prompt the user to confirm the deletion
            }
        });
         });
    }
}
