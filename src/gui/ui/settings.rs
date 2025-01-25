use egui::{ vec2, Align2, Align, ScrollArea, Label, Layout, Color32, Frame, Ui, Window };
use std::sync::Arc;
use std::str::FromStr;
use crate::core::{ ZeusCtx, user::Contact };
use crate::assets::icons::Icons;
use crate::gui::{ SHARED_GUI, utils, ui::{ CredentialsForm, rich_text, button, img_button } };
use egui_theme::{
    Theme,
    utils::{ bg_color_on_idle, bg_color_on_hover, border_on_idle, border_on_hover, border_on_click },
};
use zeus_eth::alloy_primitives::Address;

pub struct SettingsUi {
    pub open: bool,
    pub main_ui: bool,
    pub contacts_ui: ContactsUi,
    pub credentials: CredentialsForm,
    pub verified_credentials: bool,
    pub size: (f32, f32),
}

impl SettingsUi {
    pub fn new() -> Self {
        Self {
            open: false,
            main_ui: true,
            contacts_ui: ContactsUi::new(),
            credentials: CredentialsForm::new(),
            verified_credentials: false,
            size: (500.0, 400.0),
        }
    }

    pub fn show(&mut self, ctx: ZeusCtx, icons: Arc<Icons>, theme: &Theme, ui: &mut Ui) {
        if !self.open {
            return;
        }

        let mut main_ui = self.main_ui;
        self.main_ui(&mut main_ui, ui);
        self.change_credentials_ui(ctx.clone(), theme, ui);
        self.contacts_ui.show(ctx, icons, theme, &mut main_ui, ui);
        self.main_ui = main_ui;
    }

    pub fn main_ui(&mut self, open: &mut bool, ui: &mut Ui) {
        if !*open {
            return;
        }

        // Transparent window
        Window::new("settings_main_ui")
            .title_bar(false)
            .resizable(false)
            .collapsible(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .frame(Frame::none())
            .show(ui.ctx(), |ui| {
                ui.set_width(self.size.0);
                ui.set_height(self.size.1);

                ui.vertical_centered(|ui| {
                    bg_color_on_idle(ui, Color32::TRANSPARENT);
                    border_on_idle(ui, 1.0, Color32::LIGHT_GRAY);
                    ui.spacing_mut().item_spacing.y = 20.0;

                    ui.label(rich_text("Settings").size(22.0));

                    let size = vec2(self.size.0, 50.0);
                    let credentials = button(rich_text("Change your Credentials").size(17.0)).rounding(5.0).min_size(size);
                    if ui.add(credentials).clicked() {
                        *open = false;
                        self.credentials.open = true;
                    }

                    let contacts = button(rich_text("Contacts").size(17.0)).rounding(5.0).min_size(size);
                    if ui.add(contacts).clicked() {
                        *open = false;
                        self.contacts_ui.open = true;
                    }
                });
            });
    }

    fn change_credentials_ui(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
        let title = if self.verified_credentials { "New Credentials" } else { "Verify Your Credentials" };

        let mut open = self.credentials.open;
        Window::new(rich_text(title).size(18.0))
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .frame(Frame::window(ui.style()))
            .show(ui.ctx(), |ui| {
                ui.set_width(self.size.0);
                ui.set_height(self.size.1);

                ui.vertical_centered(|ui| {
                    ui.add_space(20.0);

                    if !self.verified_credentials {
                        self.credentials.confrim_password = false;
                        self.credentials.show(theme, ui);
                        ui.add_space(15.0);

                        let verify = button(rich_text("Verify").size(16.0));
                        if ui.add(verify).clicked() {
                            let mut profile = ctx.profile();
                            profile.credentials = self.credentials.credentials.clone();

                            std::thread::spawn(move || {
                                let dir = utils::get_profile_dir();
                                utils::open_loading("Decrypting profile...".to_string());

                                match profile.decrypt_zero(&dir) {
                                    Ok(data) => {
                                        let mut gui = SHARED_GUI.write().unwrap();
                                        gui.settings.verified_credentials = true;
                                        gui.settings.credentials.erase();
                                        gui.loading_window.open = false;
                                        data
                                    }
                                    Err(e) => {
                                        let mut gui = SHARED_GUI.write().unwrap();
                                        gui.loading_window.open = false;
                                        gui.open_msg_window("Failed to decrypt profile", &format!("{}", e));
                                        return;
                                    }
                                };
                            });
                        }
                    }

                    if self.verified_credentials {
                        self.credentials.confrim_password = true;
                        self.credentials.show(theme, ui);
                        ui.add_space(15.0);

                        let save = button(rich_text("Save").size(16.0));

                        if ui.add(save).clicked() {
                            let mut profile = ctx.profile();
                            profile.credentials = self.credentials.credentials.clone();

                            std::thread::spawn(move || {
                                let dir = utils::get_profile_dir();
                                let params = utils::get_encrypted_info(&dir).argon2_params;
                                utils::open_loading("Encrypting profile...".to_string());

                                match profile.encrypt_and_save(&dir, params) {
                                    Ok(_) => {
                                        let mut gui = SHARED_GUI.write().unwrap();
                                        gui.settings.credentials.erase();
                                        gui.settings.verified_credentials = false;
                                        gui.settings.credentials.open = false;
                                        gui.settings.main_ui = true;
                                        gui.loading_window.open = false;
                                        gui.open_msg_window("Credentials have been updated", "");
                                    }
                                    Err(e) => {
                                        let mut gui = SHARED_GUI.write().unwrap();
                                        gui.loading_window.open = false;
                                        gui.open_msg_window("Failed to update credentials", &format!("{}", e));
                                        return;
                                    }
                                }
                                ctx.write(|ctx| {
                                    ctx.profile = profile;
                                });
                            });
                        }
                    }
                });
            });
        self.credentials.open = open;

        // reset credentials
        if !self.credentials.open {
            self.credentials.erase();
            self.verified_credentials = false;
            self.main_ui = true;
        }
    }
}

pub struct ContactsUi {
    pub open: bool,
    pub main_ui: bool,
    pub add_contact: bool,
    pub delete_contact: bool,
    pub edit_contact: bool,
    pub contact_to_add: Contact,
    pub contact_to_delete: Option<Contact>,
    pub contact_to_edit: Option<Contact>,
    pub old_contact: Option<Contact>,
    pub size: (f32, f32),
}

impl ContactsUi {
    pub fn new() -> Self {
        Self {
            open: false,
            main_ui: true,
            add_contact: false,
            delete_contact: false,
            edit_contact: false,
            contact_to_add: Contact::default(),
            contact_to_delete: None,
            contact_to_edit: None,
            old_contact: None,
            size: (500.0, 400.0),
        }
    }

    pub fn show(&mut self, ctx: ZeusCtx, icons: Arc<Icons>, theme: &Theme, parent_open: &mut bool, ui: &mut Ui) {
        if !self.open {
            // reopen the settings main ui
            *parent_open = true;
            return;
        }

        self.main_ui(ctx.clone(), icons.clone(), ui);
        self.add_contact_ui(ctx.clone(), icons.clone(), theme, ui);
        self.delete_contact_ui(ctx.clone(), ui);
        self.edit_contact_ui(ctx, icons, theme, ui);
    }

    pub fn main_ui(&mut self, ctx: ZeusCtx, icons: Arc<Icons>, ui: &mut Ui) {
        if !self.main_ui {
            return;
        }

        let mut open = self.open;
        Window::new("Contacts")
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .anchor(Align2::CENTER_CENTER, (0.0, 0.0))
            .frame(Frame::window(ui.style()))
            .show(ui.ctx(), |ui| {
                ui.set_width(self.size.0);
                ui.set_height(self.size.1);

                let contacts = ctx.profile().contacts;

                ui.vertical_centered(|ui| {
                    ui.spacing_mut().item_spacing.y = 10.0;

                    // Add contact button
                    if ui.add(button(rich_text("Add Contact"))).clicked() {
                        self.add_contact = true;
                        self.main_ui = false;
                    }

                    if contacts.is_empty() {
                        ui.label(rich_text("No contacts found"));
                    } else {
                        ScrollArea::vertical().show(ui, |ui| {
                            ui.set_width(self.size.0);
                            ui.vertical_centered(|ui| {
                                for contact in &contacts {
                                    Frame::group(ui.style())
                                        .inner_margin(8.0)
                                        .show(ui, |ui| {
                                            ui.set_width(250.0);
                                            self.contact(icons.clone(), contact, ui);
                                        });
                                }
                            });
                        });
                    }
                });
            });
        self.open = open;
    }

    /// Show a contact
    fn contact(&mut self, icons: Arc<Icons>, contact: &Contact, ui: &mut Ui) {
        ui.horizontal(|ui| {
            // Contact info column
            ui.vertical(|ui| {
                ui.set_width(ui.available_width() - 40.0); // Leave space for buttons

                // Name
                let name_label = Label::new(rich_text(&contact.name)).wrap();
                ui.add(name_label);

                // Address
                let address = contact.address_short();
                if ui.selectable_label(false, rich_text(&address).size(12.0)).clicked() {
                    ui.ctx().copy_text(contact.address.clone());
                }
            });

            // Buttons column
            ui.vertical(|ui| {
                ui.set_min_width(40.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    bg_color_on_idle(ui, Color32::TRANSPARENT);
                    bg_color_on_hover(ui, Color32::TRANSPARENT);

                    let delete_res = ui.add(img_button(icons.trash(), "").min_size((16.0, 16.0).into()));
                    let edit_res = ui.add(img_button(icons.edit(), "").min_size((16.0, 16.0).into()));

                    if delete_res.clicked() {
                        self.main_ui = false;
                        self.delete_contact = true;
                        self.contact_to_delete = Some(contact.clone());
                    }
                    if edit_res.clicked() {
                        self.main_ui = false;
                        self.edit_contact = true;
                        self.contact_to_edit = Some(contact.clone());
                        self.old_contact = Some(contact.clone());
                    }
                });
            });
        });
    }

    fn delete_contact_ui(&mut self, ctx: ZeusCtx, ui: &mut Ui) {
        if !self.delete_contact {
            return;
        }

        Window::new("Delete contact")
            .title_bar(false)
            .resizable(false)
            .collapsible(false)
            .anchor(Align2::CENTER_CENTER, (0.0, 0.0))
            .frame(Frame::window(ui.style()))
            .show(ui.ctx(), |ui| {
                ui.set_width(self.size.0);
                ui.set_height(self.size.1);

                ui.vertical_centered(|ui| {
                    ui.set_width(self.size.0);
                    ui.spacing_mut().item_spacing.y = 15.0;

                    // should not happen
                    if self.contact_to_delete.is_none() {
                        ui.label(rich_text("No contact to delete"));
                        if ui.add(button(rich_text("Close"))).clicked() {
                            self.delete_contact = false;
                            return;
                        }
                    }

                    let contact = self.contact_to_delete.clone().unwrap();
                    ui.label(rich_text("Are you sure you want to delete this contact?"));
                    ui.label(rich_text(&contact.name));
                    ui.label(rich_text(&contact.address_short()));

                    let res_delete = ui.add(button(rich_text("Delete")));
                    let res_cancel = ui.add(button(rich_text("Cancel")));

                    if res_cancel.clicked() {
                        self.delete_contact = false;
                        self.main_ui = true;
                        self.contact_to_delete = None;
                    }

                    if res_delete.clicked() {
                        let address = contact.address;
                        let mut profile = ctx.profile();
                        profile.remove_contact(address);
                        let dir = utils::get_profile_dir();
                        let info = utils::get_encrypted_info(&dir);
                        utils::open_loading("Encrypting profile...".to_string());

                        std::thread::spawn(move || {
                            match profile.encrypt_and_save(&dir, info.argon2_params) {
                                Ok(_) => {
                                    let mut gui = SHARED_GUI.write().unwrap();
                                    gui.open_msg_window("Contact Removed", "");
                                    gui.settings.contacts_ui.main_ui = true;
                                    gui.settings.contacts_ui.delete_contact = false;
                                    gui.settings.contacts_ui.contact_to_delete = None;
                                }
                                Err(e) => {
                                    let mut gui = SHARED_GUI.write().unwrap();
                                    gui.open_msg_window("Profile encryption failed", &format!("{}", e));
                                    gui.settings.contacts_ui.main_ui = true;
                                    gui.settings.contacts_ui.delete_contact = false;
                                    return;
                                }
                            }
                            ctx.write(|ctx| {
                                ctx.profile = profile;
                            });
                        });
                    }
                });
            });
    }

    fn add_contact_ui(&mut self, ctx: ZeusCtx, icons: Arc<Icons>, theme: &Theme, ui: &mut Ui) {
        if !self.add_contact {
            return;
        }

        Window::new("Add new contact")
            .resizable(false)
            .collapsible(false)
            .anchor(Align2::CENTER_CENTER, (0.0, 0.0))
            .frame(Frame::window(ui.style()))
            .show(ui.ctx(), |ui| {
                ui.set_width(self.size.0);
                ui.set_height(self.size.1);

                ui.vertical_centered(|ui| {
                    ui.spacing_mut().item_spacing.y = 20.0;

                    // Go back button
                    ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                        let button = img_button(icons.arrow_back(), "").min_size(vec2(30.0, 20.0));
                        bg_color_on_idle(ui, Color32::TRANSPARENT);
                        if ui.add(button).clicked() {
                            self.add_contact = false;
                            self.main_ui = true;
                        }
                    });

                    ui.scope(|ui| {
                        border_on_idle(ui, 1.0, theme.colors.border_color_idle);
                        border_on_hover(ui, 1.0, theme.colors.border_color_hover);
                        border_on_click(ui, 1.0, theme.colors.border_color_click);

                        ui.label(rich_text("Name:"));
                        ui.text_edit_singleline(&mut self.contact_to_add.name);

                        ui.label(rich_text("Address:"));
                        ui.text_edit_singleline(&mut self.contact_to_add.address);
                    });

                    if ui.add(button(rich_text("Add"))).clicked() {
                        let contact = self.contact_to_add.clone();
                        let mut profile = ctx.profile();
                        std::thread::spawn(move || {
                            // make sure the address is valid
                            let _ = match Address::from_str(&contact.address) {
                                Ok(address) => address,
                                Err(e) => {
                                    let mut gui = SHARED_GUI.write().unwrap();
                                    gui.open_msg_window("Address is not an Ethereum address", &format!("{}", e));
                                    return;
                                }
                            };

                            match profile.add_contact(contact) {
                                Ok(_) => {}
                                Err(e) => {
                                    let mut gui = SHARED_GUI.write().unwrap();
                                    gui.open_msg_window("Failed to add contact", &format!("{}", e));
                                    return;
                                }
                            }

                            let dir = utils::get_profile_dir();
                            let info = utils::get_encrypted_info(&dir);
                            utils::open_loading("Encrypting profile...".to_string());

                            match profile.encrypt_and_save(&dir, info.argon2_params) {
                                Ok(_) => {
                                    let mut gui = SHARED_GUI.write().unwrap();
                                    gui.open_msg_window("Contact has been added successfully", "");
                                    gui.settings.contacts_ui.contact_to_add = Contact::default();
                                }
                                Err(e) => {
                                    let mut gui = SHARED_GUI.write().unwrap();
                                    gui.open_msg_window("Profile encryption failed", &format!("{}", e));
                                    return;
                                }
                            }
                            ctx.write(|ctx| {
                                ctx.profile = profile;
                            });
                        });
                    }
                });
            });
    }

    fn edit_contact_ui(&mut self, ctx: ZeusCtx, icons: Arc<Icons>, theme: &Theme, ui: &mut Ui) {
        if !self.edit_contact {
            return;
        }

        Window::new("Edit contact")
            .resizable(false)
            .collapsible(false)
            .anchor(Align2::CENTER_CENTER, (0.0, 0.0))
            .frame(Frame::window(ui.style()))
            .show(ui.ctx(), |ui| {
                ui.set_width(self.size.0);
                ui.set_height(self.size.1);

                ui.vertical_centered(|ui| {
                    ui.spacing_mut().item_spacing.y = 20.0;

                    // Go back button
                    ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                        let button = img_button(icons.arrow_back(), "").min_size(vec2(30.0, 20.0));
                        bg_color_on_idle(ui, Color32::TRANSPARENT);
                        if ui.add(button).clicked() {
                            self.edit_contact = false;
                            self.main_ui = true;
                        }
                    });

                    // should not happen
                    if self.contact_to_edit.is_none() {
                        ui.label(rich_text("No contact to edit"));
                        if ui.add(button(rich_text("Close"))).clicked() {
                            self.edit_contact = false;
                            return;
                        }
                    }

                    let mut contact = self.contact_to_edit.clone().unwrap();
                    ui.scope(|ui| {
                        border_on_idle(ui, 1.0, theme.colors.border_color_idle);
                        border_on_hover(ui, 1.0, theme.colors.border_color_hover);
                        border_on_click(ui, 1.0, theme.colors.border_color_click);

                        ui.label(rich_text("Name:"));
                        ui.text_edit_singleline(&mut contact.name);

                        ui.label(rich_text("Address:"));
                        ui.text_edit_singleline(&mut contact.address);
                    });

                    self.contact_to_edit = Some(contact.clone());

                    if ui.add(button(rich_text("Save"))).clicked() {
                        let old_contact = self.old_contact.clone().unwrap();
                        let new_contact = self.contact_to_edit.clone().unwrap();

                        std::thread::spawn(move || {
                            // make sure the address is valid
                            let _ = match Address::from_str(&contact.address) {
                                Ok(address) => address,
                                Err(e) => {
                                    let mut gui = SHARED_GUI.write().unwrap();
                                    gui.open_msg_window("Address is not an Ethereum address", &format!("{}", e));
                                    return;
                                }
                            };

                            let mut profile = ctx.profile();
                            profile.remove_contact(old_contact.address.clone());
                            match profile.add_contact(new_contact) {
                                Ok(_) => {}
                                Err(e) => {
                                    let mut gui = SHARED_GUI.write().unwrap();
                                    gui.open_msg_window("Failed to edit contact", &format!("{}", e));
                                    return;
                                }
                            }

                            let dir = utils::get_profile_dir();
                            let info = utils::get_encrypted_info(&dir);
                            utils::open_loading("Encrypting profile...".to_string());

                            match profile.encrypt_and_save(&dir, info.argon2_params) {
                                Ok(_) => {
                                    let mut gui = SHARED_GUI.write().unwrap();
                                    gui.settings.contacts_ui.contact_to_edit = None;
                                    gui.settings.contacts_ui.edit_contact = false;
                                    gui.settings.contacts_ui.main_ui = true;
                                    gui.loading_window.open = false;
                                }
                                Err(e) => {
                                    let mut gui = SHARED_GUI.write().unwrap();
                                    gui.open_msg_window("Profile encryption failed", &format!("{}", e));
                                    gui.loading_window.open = false;
                                    return;
                                }
                            }
                            ctx.write(|ctx| {
                                ctx.profile = profile;
                            });
                        });
                    }
                });
            });
    }
}