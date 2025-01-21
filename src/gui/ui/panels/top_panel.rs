use egui::{ vec2, Align, Align2, Color32, Frame, Grid, Layout, ScrollArea, Ui, Window };
use std::sync::Arc;
use std::str::FromStr;
use crate::core::user::Contact;
use crate::core::ZeusCtx;
use crate::assets::icons::Icons;
use crate::gui::{ GUI, SHARED_GUI, utils, ui::{ WalletUi, ChainSelect, rich_text, button, img_button } };
use egui_theme::{
    Theme,
    utils::{ bg_color_on_idle, bg_color_on_click, bg_color_on_hover, border_on_idle, border_on_hover, border_on_click },
};
use zeus_eth::alloy_primitives::Address;

pub fn show(gui: &mut GUI, ui: &mut Ui) {
    let frame = gui.theme.frame2;
    let ctx = gui.ctx.clone();
    let icons = gui.icons.clone();

    frame.show(ui, |ui| {
        ui.set_width(gui.profile_area.size.0);
        ui.set_height(gui.profile_area.size.1);

        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 20.0;

            // Chain selection
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                let clicked = gui.profile_area.chain_select.show(ui, &gui.theme, gui.icons.clone());
                if clicked {
                    // if we select a new chain update the necessary state
                    let chain = gui.profile_area.chain_select.chain.clone();
                    gui.swap_ui.default_currency_in(chain.id());
                    gui.swap_ui.default_currency_out(chain.id());

                    // update the chain
                    ctx.write(|ctx| {
                        ctx.chain = chain.clone();
                    });
                }
            });

            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                gui.profile_area.show(ctx, icons, &gui.theme, ui);
            });
        });
    });
}

// ! Rename to something else?
pub struct ProfileArea {
    pub open: bool,
    pub main_ui: bool,
    pub wallet_ui: WalletUi,
    pub contacts_ui: ContactsUi,
    pub chain_select: ChainSelect,
    pub size: (f32, f32),
}

impl ProfileArea {
    pub fn new() -> Self {
        Self {
            open: false,
            main_ui: true,
            wallet_ui: WalletUi::new(),
            contacts_ui: ContactsUi::new(),
            chain_select: ChainSelect::new("chain_select_1").width(100.0),
            size: (300.0, 140.0),
        }
    }

    pub fn show(&mut self, ctx: ZeusCtx, icons: Arc<Icons>, theme: &Theme, ui: &mut Ui) {
        if !self.open {
            return;
        }

        self.main_ui(ctx.clone(), icons.clone(), theme, ui);
        self.wallet_ui.show(ctx.clone(), icons.clone(), theme, ui);
        self.contacts_ui.show(ctx, icons, theme, ui);
    }

    pub fn main_ui(&mut self, ctx: ZeusCtx, icons: Arc<Icons>, theme: &Theme, ui: &mut Ui) {
        if !self.main_ui {
            return;
        }

        let wallet = ctx.wallet();
        ui.vertical(|ui| {
            // Show the current wallet, if clicked open the wallet_ui
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                Grid::new("profile_grid")
                    .min_row_height(30.0)
                    .show(ui, |ui| {
                        // Wallet button
                        let text = rich_text(wallet.name.clone()).size(16.0);
                        if ui.add(img_button(icons.right_arrow(), text)).clicked() {
                            self.wallet_ui.open = !self.wallet_ui.open;
                        }

                        // Contacts button
                        bg_color_on_idle(ui, Color32::TRANSPARENT);
                        bg_color_on_hover(ui, theme.colors.widget_bg_color_hover);
                        bg_color_on_click(ui, Color32::TRANSPARENT);
                        border_on_click(ui, 1.0, theme.colors.border_color_click);
                        if ui.add(img_button(icons.contact(), "").min_size((16.0, 16.0).into())).clicked() {
                            self.contacts_ui.open = !self.contacts_ui.open;
                        }
                    });
            });

            // Assets Value
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                ui.label(rich_text("Assets Value:"));
                // TODO: Calculate the total value of a wallet
                ui.label(rich_text("$100,000,00"));
            });
        });
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
            size: (300.0, 140.0),
        }
    }

    pub fn show(&mut self, ctx: ZeusCtx, icons: Arc<Icons>, theme: &Theme, ui: &mut Ui) {
        if !self.open {
            return;
        }

        let offset = vec2(self.size.0, 40.0);
        Window::new("Contacts")
            .resizable(false)
            .collapsible(false)
            .anchor(Align2::LEFT_TOP, offset)
            .frame(Frame::window(ui.style()))
            .show(ui.ctx(), |ui| {
                self.main_ui(ctx.clone(), icons.clone(), theme, ui);
                self.add_contact_ui(ctx.clone(), icons.clone(), theme, ui);
                self.delete_contact_ui(ctx.clone(), ui);
                self.edit_contact_ui(ctx, icons, theme, ui);
            });
    }

    pub fn main_ui(&mut self, ctx: ZeusCtx, icons: Arc<Icons>, theme: &Theme, ui: &mut Ui) {
        if !self.main_ui {
            return;
        }

        let contacts = ctx.profile().contacts;

        ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            bg_color_on_idle(ui, Color32::TRANSPARENT);
            if ui.add(img_button(icons.arrow_back(), "").min_size((16.0, 16.0).into())).clicked() {
                self.open = false;
            }
        });

        ui.vertical_centered(|ui| {
            ui.spacing_mut().item_spacing.y = 10.0;

            // Add contact button
            if ui.add(button(rich_text("Add Contact"))).clicked() {
                self.add_contact = true;
                self.main_ui = false;
            }

            if contacts.is_empty() {
                ui.label(rich_text("No contacts found"));
            }

            let frame = theme.frame2;

            // Contacts
            ScrollArea::vertical().show(ui, |ui| {
                for contact in &contacts {
                    frame.show(ui, |ui| {
                        ui.set_max_width(150.0);
                        ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                            ui.label(rich_text(&contact.name));
                            let res = ui.selectable_label(false, rich_text(&contact.address_short()).size(12.0));
                            if res.clicked() {
                                ui.ctx().copy_text(contact.address.to_string());
                            }

                            // Delete contact button
                            bg_color_on_idle(ui, Color32::TRANSPARENT);
                            bg_color_on_click(ui, Color32::TRANSPARENT);
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
                }
            });
        });
    }

    fn delete_contact_ui(&mut self, ctx: ZeusCtx, ui: &mut Ui) {
        if !self.delete_contact {
            return;
        }

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

                std::thread::spawn(move || {
                    match profile.encrypt_and_save(&dir, info.argon2_params) {
                        Ok(_) => {
                            let mut gui = SHARED_GUI.write().unwrap();
                            gui.open_msg_window("Contact Removed", "");
                            gui.profile_area.contacts_ui.main_ui = true;
                            gui.profile_area.contacts_ui.delete_contact = false;
                            gui.profile_area.contacts_ui.contact_to_delete = None;
                        }
                        Err(e) => {
                            let mut gui = SHARED_GUI.write().unwrap();
                            gui.open_msg_window("Profile encryption failed", &format!("{}", e));
                            gui.profile_area.contacts_ui.main_ui = true;
                            gui.profile_area.contacts_ui.delete_contact = false;
                            return;
                        }
                    }
                    ctx.write(|ctx| {
                        ctx.profile = profile;
                    });
                });
            }
        });
    }

    fn add_contact_ui(&mut self, ctx: ZeusCtx, icons: Arc<Icons>, theme: &Theme, ui: &mut Ui) {
        if !self.add_contact {
            return;
        }

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
                    match profile.encrypt_and_save(&dir, info.argon2_params) {
                        Ok(_) => {
                            let mut gui = SHARED_GUI.write().unwrap();
                            gui.open_msg_window("Contact has been added successfully", "");
                            gui.profile_area.contacts_ui.contact_to_add = Contact::default();
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
    }

    fn edit_contact_ui(&mut self, ctx: ZeusCtx, icons: Arc<Icons>, theme: &Theme, ui: &mut Ui) {
        if !self.edit_contact {
            return;
        }

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
                    {
                        let mut gui = SHARED_GUI.write().unwrap();
                        gui.loading_window.msg = "Encrypting profile...".to_string();
                        gui.loading_window.open = true;
                    }
                    match profile.encrypt_and_save(&dir, info.argon2_params) {
                        Ok(_) => {
                            let mut gui = SHARED_GUI.write().unwrap();
                            gui.profile_area.contacts_ui.contact_to_edit = None;
                            gui.profile_area.contacts_ui.edit_contact = false;
                            gui.profile_area.contacts_ui.main_ui = true;
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
    }
}
