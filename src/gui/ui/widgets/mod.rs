use eframe::egui::{ Ui, ComboBox, Align2, Window, Spinner, Frame, ScrollArea, Color32, Grid, vec2 };
use egui::Vec2;
use std::sync::Arc;
use crate::gui::ui::{ button, img_button, rich_text, currency_balance, currency_price, currency_value };
use crate::assets::icons::Icons;
use crate::core::{ Wallet, ZeusCtx };
use egui_theme::{ Theme, utils::{ window_fill, bg_color_on_idle } };

use zeus_eth::ChainId;

pub mod token_selection;
pub mod send_crypto;

pub use token_selection::TokenSelectionWindow;
pub use send_crypto::SendCryptoUi;

/// A ComboBox to select a chain
pub struct ChainSelect {
    pub id: &'static str,
    pub chain: ChainId,
    pub width: f32,
}

impl ChainSelect {
    pub fn new(id: &'static str) -> Self {
        Self {
            id,
            chain: ChainId::new(1).unwrap(),
            width: 200.0,
        }
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    /// Show the ComboBox
    ///
    /// Returns true if the chain was changed
    pub fn show(&mut self, ui: &mut Ui, theme: &Theme, icons: Arc<Icons>) -> bool {
        let mut selected_chain = self.chain.clone();
        let supported_chains = ChainId::supported_chains();
        bg_color_on_idle(ui, Color32::TRANSPARENT);
        window_fill(ui, theme.colors.bg_color);

        let icon = icons.chain_icon(&selected_chain.id());
        let mut clicked = false;

        ui.add(icon);
        ComboBox::from_id_salt(self.id)
            .width(self.width)
            .selected_text(rich_text(selected_chain.name()).size(16.0))
            .show_ui(ui, |ui| {
                for chain in supported_chains {
                    let value = ui.selectable_value(&mut selected_chain, chain.clone(), rich_text(chain.name()));

                    if value.clicked() {
                        self.chain = selected_chain.clone();
                        clicked = true;
                    }
                }
            });
        clicked
    }
}

/// A ComboBox to select a wallet
pub struct WalletSelect {
    pub id: &'static str,
    pub wallet: Wallet,
    pub width: f32,
}

impl WalletSelect {
    pub fn new(id: &'static str) -> Self {
        Self {
            id,
            wallet: Wallet::new_rng(String::new()),
            width: 200.0,
        }
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    pub fn show(&mut self, ctx: ZeusCtx, ui: &mut Ui) {
        let wallets = ctx.profile().wallets;
        if self.wallet.name.is_empty() {
            self.wallet = ctx.wallet();
        }

        ComboBox::from_id_salt(self.id)
            .selected_text(rich_text(self.wallet.name.clone()))
            .width(self.width)
            .show_ui(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 10.0;

                for wallet in wallets {
                    let value = ui.selectable_value(&mut self.wallet, wallet.clone(), rich_text(wallet.name.clone()));

                    // update the wallet
                    if value.clicked() {
                        self.wallet = wallet.clone();
                    }
                }
            });
    }
}

/// Window to indicate a loading state
pub struct LoadingWindow {
    pub open: bool,
    pub msg: String,
    pub size: (f32, f32),
    pub anchor: (Align2, Vec2),
}

impl LoadingWindow {
    pub fn new() -> Self {
        Self {
            open: false,
            msg: String::new(),
            size: (150.0, 100.0),
            anchor: (Align2::CENTER_CENTER, vec2(0.0, 0.0)),
        }
    }

    pub fn show(&mut self, ui: &mut Ui) {
        if !self.open {
            return;
        }

        Window::new("Loading")
            .title_bar(false)
            .resizable(false)
            .anchor(self.anchor.0, self.anchor.1)
            .collapsible(false)
            .frame(Frame::window(ui.style()))
            .show(ui.ctx(), |ui| {
                ui.set_width(self.size.0);
                ui.set_height(self.size.1);
                ui.vertical_centered(|ui| {
                ui.add(Spinner::new().size(50.0).color(Color32::WHITE));
                ui.label(rich_text(&self.msg));
                });
            });
    }
}

/// Simple window diplaying a message, for example an error
#[derive(Default)]
pub struct MsgWindow {
    pub open: bool,
    pub title: String,
    pub message: String,
}

impl MsgWindow {
    /// Open the window with this title and message
    pub fn open(&mut self, title: impl Into<String>, msg: impl Into<String>) {
        self.open = true;
        self.title = title.into();
        self.message = msg.into();
    }

    pub fn show(&mut self, ui: &mut Ui) {
        if !self.open {
            return;
        }

        let title = rich_text(self.title.clone()).size(20.0);
        let msg = rich_text(&self.message).size(16.0);
        let ok = button(rich_text("Ok"));

        Window::new(title)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .collapsible(false)
            .frame(Frame::window(ui.style()))
            .show(ui.ctx(), |ui| {
                ui.vertical_centered(|ui| {
                    ui.set_min_size(vec2(300.0, 100.0));
                    ui.scope(|ui| {
                        ui.spacing_mut().item_spacing.y = 20.0;

                        ui.label(msg);

                        if ui.add(ok).clicked() {
                            self.open = false;
                        }
                    });
                });
            });
    }
}

pub struct PortfolioUi {
    pub open: bool,
}

impl PortfolioUi {
    pub fn new() -> Self {
        Self {
            open: true,
        }
    }

    pub fn show(&mut self, ctx: ZeusCtx, icons: Arc<Icons>, ui: &mut Ui) {
        if !self.open {
            return;
        }

        let chain_id = ctx.chain().id();
        let owner = ctx.wallet().key.address();
        let portfolio = ctx.get_portfolio(owner);

        let currencies = portfolio.currencies();

        ui.vertical_centered_justified(|ui| {
            ui.set_width(600.0);
            ui.set_height(550.0);

            ScrollArea::vertical().show(ui, |ui| {
                Grid::new("currency_grid")
                    .min_col_width(50.0)
                    .spacing((150.0, 20.0))
                    .show(ui, |ui| {
                        // Header
                        ui.label(rich_text("Token").size(18.0));
                        ui.label(rich_text("Price").size(18.0));
                        ui.label(rich_text("Balance").size(18.0));
                        ui.label(rich_text("Value").size(18.0));

                        ui.end_row();

                        for currency in currencies {
                            let icon = if currency.is_native() {
                                icons.currency_icon(chain_id)
                            } else {
                                let token = currency.erc20().unwrap();
                                icons.token_icon(token.address, chain_id)
                            };

                            let token = img_button(icon, rich_text(currency.symbol()).size(15.0)).min_size(
                                vec2(30.0, 25.0)
                            );

                            let price = currency_price(ctx.clone(), currency);
                            let balance = currency_balance(ctx.clone(), owner, currency);
                            let value = currency_value(price.parse().unwrap_or(0.0), balance.parse().unwrap_or(0.0));

                            // Add each label into a grid cell
                            ui.add(token);
                            ui.label(rich_text(price));
                            ui.label(rich_text(balance));
                            ui.label(rich_text(value));

                            // move to the next row
                            ui.end_row();
                        }
                    });
            });
        });
    }
}
