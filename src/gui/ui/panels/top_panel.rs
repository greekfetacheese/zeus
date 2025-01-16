use eframe::egui::{ vec2, Align, Grid, Align2, Window, Frame, Margin, Layout, Ui };

use crate::core::data::APP_DATA;
use crate::gui::{ GUI, ui::{ WalletUi, ChainSelect, rich_text, button, img_button } };
use egui_theme::Theme;

pub fn show(ui: &mut Ui, gui: &mut GUI) {
    ui.horizontal(|ui| {
        ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            gui.profile_area.show(ui, &gui.theme);
        });
    });

    // if a new chain is selected, update the necessary state
    gui.swap_ui.default_currency_in(gui.profile_area.chain_select.chain.id());
    gui.swap_ui.default_currency_out(gui.profile_area.chain_select.chain.id());
}

pub struct ProfileArea {
    pub open: bool,
    pub main_ui: bool,
    pub wallet_ui: WalletUi,
    pub chain_select: ChainSelect,
    pub size: (f32, f32),
}

impl ProfileArea {
    pub fn new() -> Self {
        Self {
            open: false,
            main_ui: true,
            wallet_ui: WalletUi::new(),
            chain_select: ChainSelect::new("chain_select_1").width(100.0),
            size: (300.0, 140.0),
        }
    }

    pub fn show(&mut self, ui: &mut Ui, theme: &Theme) {
        if !self.open {
            return;
        }

        self.main_ui(ui, theme);
        self.wallet_ui.show(ui, theme);
    }

    pub fn main_ui(&mut self, ui: &mut Ui, theme: &Theme) {
        if !self.main_ui {
            return;
        }

        let wallet;
        let icons;
        {
            let data = APP_DATA.read().unwrap();
            wallet = data.get_wallet().clone();
            icons = data.icons.clone().unwrap();
        }

        // let frame = Frame::none().outer_margin(Margin::same(10.0)).inner_margin(Margin::same(10.0));
        let frame = theme.frame2;

        frame.show(ui, |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);

            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 15.0;

                // show the chain select
                ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                    self.chain_select.show(ui, theme, icons.clone());
                });

                // Show the current wallet, if clicked open the wallet_ui
                ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                    ui.vertical_centered(|ui| {
                        Grid::new("profile_grid")
                            .min_row_height(30.0)
                            .show(ui, |ui| {
                                let text = rich_text(wallet.name).size(16.0);
                                if ui.add(img_button(icons.right_arrow(), text)).clicked() {
                                    self.wallet_ui.open = !self.wallet_ui.open;
                                }
                            });
                    });
                });

                ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                    ui.label(rich_text("Assets Value:"));
                    // TODO: Calculate the total value of a wallet
                    ui.label(rich_text("$100,000,00"));
                });

                // update the chain_id
                {
                    let mut data = APP_DATA.write().unwrap();
                    data.chain_id = self.chain_select.chain.clone();
                }
            });
        });
    }
}

pub struct ContactsUi {
    pub open: bool,
    pub main_ui: bool,
    pub add_contact: bool,
    pub size: (f32, f32),
}

impl ContactsUi {
    pub fn new() -> Self {
        Self {
            open: false,
            main_ui: true,
            add_contact: false,
            size: (300.0, 140.0),
        }
    }

    pub fn main_ui(&mut self, ui: &mut Ui) {
        if !self.main_ui {
            return;
        }

        ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            if ui.add(button(rich_text("Back"))).clicked() {
                self.main_ui = false;
            }
        });

        let contacts;
        {
            let data = APP_DATA.read().unwrap();
            contacts = data.profile.contacts.clone();
        }

        ui.vertical_centered(|ui| {
            ui.spacing_mut().item_spacing.y = 20.0;

            ui.label(rich_text("Contacts"));
        });
    }
}
