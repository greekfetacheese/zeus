use eframe::egui::{ vec2, Align2, Id, Frame, Vec2, Ui, Window };
use egui_theme::{ Theme, utils::{ border_on_idle, border_on_hover } };
use crate::core::{ Wallet, ZeusCtx };
use crate::gui::{
    self,
    ui::{ rich_text, button, text_edit_multi, CredentialsForm },
    SHARED_GUI,
};
use ncrypt_me::zeroize::Zeroize;

pub struct ViewKeyUi {
    pub open: bool,
    pub credentials_form: CredentialsForm,
    pub verified_credentials: bool,
    pub key: String,
    pub size: (f32, f32),
    pub anchor: (Align2, Vec2),
}

impl ViewKeyUi {
    pub fn new() -> Self {
        Self {
            open: false,
            credentials_form: CredentialsForm::new(),
            verified_credentials: false,
            key: String::new(),
            size: (300.0, 400.0),
            anchor: (Align2::CENTER_CENTER, vec2(0.0, 0.0)),
        }
    }

    pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
        self.view_key(theme, ui);
        self.verify_credentials_ui(ctx, theme, ui);
    }

    pub fn verify_credentials_ui(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
        let mut open = self.credentials_form.open;
        let mut clicked = false;

        let id = Id::new("verify_credentials_view_key_ui");
        Window::new("Verify Credentials")
            .id(id)
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .frame(Frame::window(ui.style()))
            .show(ui.ctx(), |ui| {
                ui.set_width(self.size.0);
                ui.set_height(self.size.1);

                ui.vertical_centered(|ui| {
                    ui.spacing_mut().item_spacing.y = 20.0;

                    ui.label(rich_text("Verify your credentials to view the key"));

                    self.credentials_form.show(theme, ui);

                    let button = button(rich_text("Confrim"));
                    if ui.add(button).clicked() {
                        clicked = true;
                    }
                });
            });

        if clicked {
            let mut profile = ctx.profile();
            profile.credentials = self.credentials_form.credentials.clone();
            std::thread::spawn(move || {
                let ok = gui::utils::verify_credentials(&mut profile);

                if ok {
                    let mut gui = SHARED_GUI.write().unwrap();
                    // erase the credentials form
                    gui.profile_area.wallet_ui.wallet_details.view_key_ui.credentials_form.erase();
                    // set the key from the current wallet from the wallet details ui
                    gui.profile_area.wallet_ui.wallet_details.view_key_ui.key =
                        gui.profile_area.wallet_ui.wallet_details.wallet.key_string();

                    // show the key
                    gui.profile_area.wallet_ui.wallet_details.view_key_ui.open = true;
                    gui.profile_area.wallet_ui.wallet_details.view_key_ui.verified_credentials = true;

                    // close the verify credentials ui
                    gui.profile_area.wallet_ui.wallet_details.view_key_ui.credentials_form.open = false;
                } else {
                    let mut gui = SHARED_GUI.write().unwrap();
                    gui.open_msg_window("Failed to verify credentials", "Please try again".to_string());
                }
            });
        }

        self.credentials_form.open = open;
        if !self.credentials_form.open {
            self.credentials_form.erase();
        }
    }

    pub fn view_key(&mut self, theme: &Theme, ui: &mut Ui) {
        if !self.verified_credentials {
            return;
        }
        let mut open = self.open;
        let mut clicked = false;

        // TODO: Maybe add a timeout for the key to be shown
        let id = Id::new("view_key_view_key_ui");
        Window::new("View Key")
            .id(id)
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .anchor(self.anchor.0, self.anchor.1)
            .frame(Frame::window(ui.style()))
            .show(ui.ctx(), |ui| {
                ui.set_width(self.size.0);
                ui.set_height(self.size.1);

                ui.vertical_centered(|ui| {
                    ui.spacing_mut().item_spacing.y = 20.0;

                    ui.scope(|ui| {
                        border_on_idle(ui, 1.0, theme.colors.border_color_idle);
                        border_on_hover(ui, 1.0, theme.colors.border_color_hover);

                        // Wallet Key
                        ui.label(rich_text("Wallet Key"));
                        ui.add(text_edit_multi(&mut self.key));
                    });

                    // Copy Key Button
                    let button = button(rich_text("Copy Key"));
                    if ui.add(button).clicked() {
                        ui.ctx().copy_text(self.key.clone());
                        clicked = true;
                    }
                });
            });

        if clicked {
            open = false;
            self.verified_credentials = false;
            std::thread::spawn(move || {
                let mut gui = SHARED_GUI.write().unwrap();
                gui.open_msg_window(
                    "",
                    "The key has been copied to your clipboard, make sure to clear it after you done using it!!! (eg. copy some random text)".to_string()
                );
            });
        }

        self.open = open;
        if !self.open {
            self.key.zeroize();
        }
    }
}

pub struct DeleteWalletUi {
    pub open: bool,
    pub credentials_form: CredentialsForm,
    pub verified_credentials: bool,
    pub wallet_to_delete: Option<Wallet>,
    pub size: (f32, f32),
    pub anchor: (Align2, Vec2),
}

impl DeleteWalletUi {
    pub fn new() -> Self {
        Self {
            open: false,
            credentials_form: CredentialsForm::new(),
            verified_credentials: false,
            wallet_to_delete: None,
            size: (300.0, 400.0),
            anchor: (Align2::CENTER_CENTER, vec2(0.0, 0.0)),
        }
    }

    pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
        self.verify_credentials_ui(ctx.clone(), theme, ui);
        self.delete_wallet_ui(ctx, ui);
    }

    pub fn verify_credentials_ui(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
        let mut open = self.credentials_form.open;
        let mut clicked = false;

        let id = Id::new("verify_credentials_delete_wallet_ui");
        Window::new("Verify Credentials")
            .id(id)
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .frame(Frame::window(ui.style()))
            .show(ui.ctx(), |ui| {
                ui.set_width(self.size.0);
                ui.set_height(self.size.1);

                ui.vertical_centered(|ui| {
                    ui.spacing_mut().item_spacing.y = 20.0;

                    ui.label(rich_text("Verify your credentials to delete the wallet"));

                    self.credentials_form.show(theme, ui);

                    let button = button(rich_text("Confrim"));
                    if ui.add(button).clicked() {
                        clicked = true;
                    }
                });
            });

        if clicked {
            let mut profile = ctx.profile();
            profile.credentials = self.credentials_form.credentials.clone();
            std::thread::spawn(move || {
                let ok = gui::utils::verify_credentials(&mut profile);

                if ok {
                    let mut gui = SHARED_GUI.write().unwrap();
                    // credentials are verified
                    gui.profile_area.wallet_ui.wallet_details.delete_wallet_ui.verified_credentials = true;

                    // close the verify credentials ui
                    gui.profile_area.wallet_ui.wallet_details.delete_wallet_ui.credentials_form.open = false;

                    // open the delete wallet ui
                    gui.profile_area.wallet_ui.wallet_details.delete_wallet_ui.open = true;

                    // erase the credentials form
                    gui.profile_area.wallet_ui.wallet_details.delete_wallet_ui.credentials_form.erase();
                } else {
                    let mut gui = SHARED_GUI.write().unwrap();
                    gui.open_msg_window("Failed to verify credentials", "Please try again".to_string());
                }
            });
        }

        self.credentials_form.open = open;
        if !self.credentials_form.open {
            self.credentials_form.erase();
        }
    }

    pub fn delete_wallet_ui(&mut self, ctx: ZeusCtx, ui: &mut Ui) {
        if !self.verified_credentials {
            return;
        }
        let mut open = self.open;
        let mut clicked = false;

        let wallet = self.wallet_to_delete.clone();

        let id = Id::new("delete_wallet_ui_delete_wallet");
        Window::new("Delete Wallet")
            .id(id)
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .anchor(self.anchor.0, self.anchor.1)
            .frame(Frame::window(ui.style()))
            .show(ui.ctx(), |ui| {
                ui.set_width(self.size.0);
                ui.set_height(self.size.1);

                ui.vertical_centered(|ui| {
                    ui.spacing_mut().item_spacing.y = 20.0;

                    // should not happen
                    if wallet.is_none() {
                        ui.label(rich_text("No wallet to delete"));
                    } else {
                        let wallet = wallet.clone().unwrap();
                        ui.label(rich_text("Are you sure you want to delete this wallet?").heading());
                        ui.label(rich_text(wallet.name.clone()));

                        if ui.add(button(rich_text("Yes"))).clicked() {
                            clicked = true;
                        }
                    }
                });
            });

        if clicked {
            open = false;

            let mut profile = ctx.clone().profile();
            let wallet = wallet.unwrap();
            std::thread::spawn(move || {
                profile.remove_wallet(wallet);

                let dir = gui::utils::get_profile_dir();
                let params = gui::utils::get_encrypted_info(&dir);
                gui::utils::open_loading("Encrypting profile...".to_string());
                match profile.encrypt_and_save(&dir, params.argon2_params) {
                    Ok(_) => {
                        let mut gui = SHARED_GUI.write().unwrap();
                        gui.loading_window.open = false;
                        gui.profile_area.wallet_ui.wallet_details.delete_wallet_ui.wallet_to_delete = None;
                        gui.profile_area.wallet_ui.wallet_details.delete_wallet_ui.verified_credentials = false;
                        gui.open_msg_window("Wallet Deleted", "");
                    }
                    Err(e) => {
                        let mut gui = SHARED_GUI.write().unwrap();
                        gui.loading_window.open = false;
                        gui.open_msg_window("Failed to delete wallet", e.to_string());
                    }
                }

                ctx.write(|ctx| {
                    ctx.profile = profile;
                });
            });
        }
        self.open = open;
    }
}

/// Ui to show the details of a wallet
pub struct WalletDetailsUi {
    pub open: bool,
    pub view_key_ui: ViewKeyUi,
    pub delete_wallet_ui: DeleteWalletUi,
    pub wallet: Wallet,
    pub size: (f32, f32),
    pub anchor: (Align2, Vec2),
}

impl WalletDetailsUi {
    pub fn new() -> Self {
        Self {
            open: false,
            view_key_ui: ViewKeyUi::new(),
            delete_wallet_ui: DeleteWalletUi::new(),
            wallet: Wallet::new_rng("I should not be here".to_string()),
            size: (300.0, 400.0),
            anchor: (Align2::CENTER_CENTER, vec2(0.0, 0.0)),
        }
    }

    pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
        self.main_ui(ui);
        self.view_key_ui.show(ctx.clone(), theme, ui);
        self.delete_wallet_ui.show(ctx.clone(), theme, ui);
    }

    fn main_ui(&mut self, ui: &mut Ui) {
        let mut open = self.open;
        let mut clicked1 = false;
        let mut clicked2 = false;

        let id = Id::new("wallet_details_main_ui");
        Window::new("Wallet Details")
            .id(id)
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .anchor(self.anchor.0, self.anchor.1)
            .frame(Frame::window(ui.style()))
            .show(ui.ctx(), |ui| {
                ui.set_width(self.size.0);
                ui.set_height(self.size.1);

                ui.vertical_centered(|ui| {
                    ui.spacing_mut().item_spacing.y = 20.0;

                    // Wallet Name
                    ui.label(rich_text(self.wallet.name.clone()).heading());

                    // Wallet Address
                    let address = rich_text(self.wallet.address()).size(13.0);
                    let res = ui.selectable_label(false, address);
                    if res.clicked() {
                        ui.ctx().copy_text(self.wallet.address());
                    }

                    // Wallet Value
                    // TODO: Calculate the value of the wallet
                    ui.label(rich_text("$100,000,00").size(12.0));

                        // View Key
                        let view_key = rich_text("View Key");
                        if ui.add(button(view_key)).clicked() {
                            clicked1 = true;
                        }

                        // Delete Wallet
                        let delete_wallet = rich_text("Delete Wallet");
                        if ui.add(button(delete_wallet)).clicked() {
                            clicked2 = true;
                        }
                    
                });
            });

        if clicked1 {
            self.view_key_ui.credentials_form.open = true;
            open = false;
        }

        if clicked2 {
            self.delete_wallet_ui.wallet_to_delete = Some(self.wallet.clone());
            self.delete_wallet_ui.credentials_form.open = true;
            open = false;
        }
        self.open = open;
    }
}