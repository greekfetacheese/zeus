use eframe::egui::{
    Ui,
    ComboBox,
    RichText,
    Layout,
    Align,
    Align2,
    Window,
    Spinner,
    Frame,
    ScrollArea,
    Color32,
    Grid,
    vec2,
    Vec2,
};
use zeus_eth::currency::ERC20Token;
use std::sync::Arc;

use crate::gui::ui::{ TokenSelectionWindow, button, rich_text, currency_balance, wallet_value, currency_price, currency_value };
use crate::assets::icons::Icons;
use crate::core::{ Wallet, ZeusCtx, user::Portfolio };
use crate::core::utils::{ RT, fetch };
use crate::gui::SHARED_GUI;

use egui_theme::{ Theme, utils::{ window_fill, bg_color_on_idle } };

use zeus_eth::{ currency::Currency, types::ChainId };



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

    /// Show the ComboBox
    ///
    /// Returns true if the wallet was changed
    pub fn show(&mut self, ctx: ZeusCtx, ui: &mut Ui) -> bool {
        let wallets = ctx.profile().wallets;
        if self.wallet.name.is_empty() {
            self.wallet = ctx.wallet();
        }

        let mut clicked = false;
        ComboBox::from_id_salt(self.id)
            .selected_text(rich_text(self.wallet.name.clone()))
            .width(self.width)
            .show_ui(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 10.0;

                for wallet in wallets {
                    let value = ui.selectable_value(&mut self.wallet, wallet.clone(), rich_text(wallet.name.clone()));

                    if value.clicked() {
                        clicked = true;
                        self.wallet = wallet.clone();
                    }
                }
            });
        clicked
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
    pub bg_color: Option<Color32>,
}

impl MsgWindow {
    pub fn new(color: Option<Color32>) -> Self {
        Self {
            open: false,
            title: String::new(),
            message: String::new(),
            bg_color: color,
        }
    }

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

        let frame = if let Some(color) = self.bg_color {
            Frame::window(ui.style()).fill(color)
        } else {
            Frame::window(ui.style())
        };

        Window::new(title)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .collapsible(false)
            .frame(frame)
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
    pub show_spinner: bool,
}

impl PortfolioUi {
    pub fn new() -> Self {
        Self {
            open: true,
            show_spinner: false,
        }
    }

    pub fn show(&mut self, ctx: ZeusCtx, icons: Arc<Icons>, token_selection: &mut TokenSelectionWindow, ui: &mut Ui) {
        if !self.open {
            return;
        }

        let chain_id = ctx.chain().id();
        let owner = ctx.wallet().key.inner().address();
        let portfolio = ctx.get_portfolio(chain_id, owner).unwrap_or_default();

        let currencies = portfolio.currencies();
        let portfolio_value = wallet_value(ctx.clone(), chain_id, owner);

        ui.vertical_centered_justified(|ui| {
            ui.set_width(ui.available_width() * 0.8);

            ui.spacing_mut().item_spacing = Vec2::new(16.0, 20.0);

            ui.horizontal(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("+ Add Token").clicked() {
                        token_selection.open = true;
                    }

                    if ui.button("Update Prices").clicked() {
                        self.update_prices(ctx.clone());
                    }

                    if self.show_spinner {
                        ui.add(Spinner::new().size(15.0).color(Color32::WHITE));
                    }
                });
            });

            // Total Value
            ui.vertical(|ui| {
                Frame::group(ui.style())
                    .inner_margin(16.0)
                    .fill(ui.style().visuals.extreme_bg_color)
                    .show(ui, |ui| {
                        ui.vertical_centered(|ui| {
                            ui.label(RichText::new("Total Portfolio Value").color(Color32::GRAY).size(14.0));
                            ui.add_space(8.0);
                            ui.label(RichText::new(format!("${:.2}", portfolio_value)).heading().size(32.0));
                        });
                    });
            });

            // Token List
            ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());

                    let column_widths = [
                        ui.available_width() * 0.2, // Asset
                        ui.available_width() * 0.2, // Price
                        ui.available_width() * 0.2, // Balance
                        ui.available_width() * 0.2, // Value
                        // ui.available_width() * 0.1, // 24h price change
                        ui.available_width() * 0.1, // Remove button
                    ];

                    // Center the grid within the available space
                    ui.horizontal(|ui| {
                        ui.add_space((ui.available_width() - column_widths.iter().sum::<f32>()) / 2.0);

                        Grid::new("currency_grid")
                            .num_columns(5)
                            .spacing([20.0, 30.0])
                            .show(ui, |ui| {
                                // Header
                                ui.label(RichText::new("Asset").strong().size(15.0));

                                ui.label(RichText::new("Price").strong().size(15.0));

                                ui.label(RichText::new("Balance").strong().size(15.0));

                                ui.label(RichText::new("Value").strong().size(15.0));

                                // ui.label(RichText::new("24h").strong().size(15.0));

                                ui.end_row();

                                // Token Rows
                                for currency in currencies {
                                    self.token(ctx.clone(), icons.clone(), currency, ui, column_widths[0]);
                                    self.price(ctx.clone(), currency, ui, column_widths[1]);
                                    self.balance(ctx.clone(), currency, ui, column_widths[2]);
                                    self.value(ctx.clone(), currency, ui, column_widths[3]);
                                    // self.change_24h(ui, column_widths[4]);
                                    self.remove_currency(ctx.clone(), currency, ui, column_widths[4]);
                                    ui.end_row();
                                }
                            });
                    });

                    // Token selection
                    let all_currencies = ctx.get_currencies(chain_id);
                    token_selection.show(ctx.clone(), chain_id, owner, icons.clone(), &all_currencies, ui);
                    let currency = token_selection.get_currency().cloned();

                    if let Some(currency) = currency {
                        token_selection.reset();
                        self.add_currency(ctx.clone(), currency);
                    }
                });
        });
    }

    fn token(&self, ctx: ZeusCtx, icons: Arc<Icons>, currency: &Currency, ui: &mut Ui, width: f32) {
        let icon = if currency.is_native() {
            icons.currency_icon(ctx.chain().id())
        } else {
            let token = currency.erc20().unwrap();
            icons.token_icon(token.address, ctx.chain().id())
        };
        ui.horizontal(|ui| {
            ui.set_width(width);
            ui.add(icon);
            ui.label(RichText::new(currency.symbol()).strong()).on_hover_text(currency.name());
        });
    }

    fn price(&self, ctx: ZeusCtx, currency: &Currency, ui: &mut Ui, width: f32) {
        let price = currency_price(ctx.clone(), currency);
        //println!("Price for {}: {}", currency.symbol(), price);
        ui.horizontal(|ui| {
            ui.set_width(width);
            ui.label(format!("${}", price));
        });
    }

    fn balance(&self, ctx: ZeusCtx, currency: &Currency, ui: &mut Ui, width: f32) {
        let balance = currency_balance(ctx.clone(), ctx.wallet().key.inner().address(), currency);
        ui.horizontal(|ui| {
            ui.set_width(width);
            ui.label(balance);
        });
    }

    fn value(&self, ctx: ZeusCtx, currency: &Currency, ui: &mut Ui, width: f32) {
        let price = currency_price(ctx.clone(), currency);
        let balance = currency_balance(ctx.clone(), ctx.wallet().key.inner().address(), currency);
        let value = currency_value(price.parse().unwrap_or(0.0), balance.parse().unwrap_or(0.0));
        ui.horizontal(|ui| {
            ui.set_width(width);
            ui.label(RichText::new(format!("${:.2}", value)).color(Color32::GRAY).size(12.0));
        });
    }

    #[allow(dead_code)]
    fn change_24h(&self, ui: &mut Ui, width: f32) {
        ui.horizontal(|ui| {
            ui.set_width(width);
            ui.label(RichText::new("12.4%").color(Color32::from_rgb(0, 200, 0)).size(12.0)); // Replace with real data
        });
    }

    fn update_prices(&mut self, ctx: ZeusCtx) {
        self.show_spinner = true;
        RT.spawn(async move {
            let pool_manager = ctx.pool_manager();
            let chain = ctx.chain().id();
            let owner = ctx.profile().wallet_address();
            let client = ctx.get_client_with_id(chain).unwrap();

            let portfolio = ctx.get_portfolio(chain, owner).unwrap_or_default();
            let tokens = portfolio.erc20_tokens();

            match pool_manager.update_minimal(client, chain, tokens).await {
                Ok(_) => tracing::info!("Updated prices for chain: {}", chain),
                Err(e) => tracing::error!("Error updating prices: {:?}", e),
            }
            let mut gui = SHARED_GUI.write().unwrap();
            gui.portofolio.show_spinner = false;
            let _ = ctx.save_pool_data();
        });
    }

    fn add_currency(&self, ctx: ZeusCtx, currency: Currency) {
        let chain_id = ctx.chain().id();
        let owner = ctx.wallet().key.inner().address();
        ctx.write(|ctx| {
            let portfolio = ctx.db.get_portfolio_mut(chain_id, owner);
            if portfolio.is_none() {
                let portfolio = Portfolio::new(vec![currency.clone()], owner);
                ctx.db.insert_portfolio(chain_id, owner, portfolio);
            } else {
                let portfolio = portfolio.unwrap();
                portfolio.add_currency(currency.clone());
            }
        });
        let _ = ctx.save_db();

        let token = if currency.is_native() {
            ERC20Token::native_wrapped_token(currency.chain_id())
        } else {
            currency.erc20().cloned().unwrap()
        };

        let v2_pools = ctx.get_v2_pools(token.clone());
        let v3_pools = ctx.get_v3_pools(token.clone());

        if v2_pools.is_empty() {
            let token = token.clone();
            let ctx = ctx.clone();
            RT.spawn(async move {
                let _ = fetch::get_v2_pools_for_token(ctx.clone(), token.clone()).await;
            });
        }

        if v3_pools.is_empty() {
            let token = token.clone();
            let ctx = ctx.clone();
            RT.spawn(async move {
                let _ = fetch::get_v3_pools_for_token(ctx.clone(), token.clone()).await;
            });
        }
        let _ = ctx.save_pool_data();
    }

    fn remove_currency(&self, ctx: ZeusCtx, currency: &Currency, ui: &mut Ui, width: f32) {
        ui.horizontal(|ui| {
            ui.set_width(width);
            if ui.button("X").clicked() {
                ctx.write(|ctx| {
                    let owner = ctx.wallet().key.inner().address();
                    let chain = ctx.chain.id();
                    let portfolio = ctx.db.get_portfolio_mut(chain, owner);
                    if let Some(portfolio) = portfolio {
                        portfolio.remove_currency(currency);
                        let _ = ctx.db.save_to_file();
                    }
                })
            }
        });
    }
}
