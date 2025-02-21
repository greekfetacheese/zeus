use eframe::egui::{
    Ui,
    Color32,
    Window,
    Id,
    RichText,
    ScrollArea,
    Vec2,
    Frame,
    TextEdit,
    Layout,
    Align,
    vec2,
    Align2,
    Response,
};
use zeus_eth::currency::erc20::ERC20Token;

use std::sync::Arc;
use std::str::FromStr;
use crate::core::utils::{RT, send_crypto};
use crate::core::ZeusCtx;
use crate::assets::icons::Icons;
use crate::gui::ui::img_button;
use crate::gui::ui::{
    TokenSelectionWindow,
    currency_balance,
    currency_value,
    rich_text,
    misc::{ ChainSelect, WalletSelect },
};
use crate::gui::SHARED_GUI;
use egui_theme::Theme;
use zeus_eth::{ types::ChainId, currency::{ Currency, native::NativeCurrency } };
use zeus_eth::alloy_primitives::{ Address, U256, utils::parse_units };


pub struct SendCryptoUi {
    pub open: bool,
    pub chain: ChainId,
    pub chain_select: ChainSelect,
    pub wallet_select: WalletSelect,
    pub priority_fee: String,
    pub token: Currency,
    pub amount: String,
    pub contact_search_open: bool,
    pub search_query: String,
    pub recipient: String,
}

impl SendCryptoUi {
    pub fn new() -> Self {
        Self {
            open: false,
            chain: ChainId::new(1).unwrap(),
            chain_select: ChainSelect::new("chain_select_2"),
            wallet_select: WalletSelect::new("wallet_select_2"),
            priority_fee: "1".to_string(),
            token: Currency::from_native(NativeCurrency::from_chain_id(1).unwrap()),
            amount: String::new(),
            contact_search_open: false,
            search_query: String::new(),
            recipient: String::new(),
        }
    }

    pub fn show(
        &mut self,
        ctx: ZeusCtx,
        icons: Arc<Icons>,
        theme: &Theme,
        token_selection: &mut TokenSelectionWindow,
        ui: &mut Ui
    ) {
        if !self.open {
            return;
        }

        Window::new("Send Crypto")
            .id(Id::new("send_crypto_ui"))
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .frame(Frame::window(ui.style()))
            .show(ui.ctx(), |ui| {
                ui.set_width(400.0);
                ui.spacing_mut().item_spacing.y = 16.0;
                ui.spacing_mut().button_padding = Vec2::new(12.0, 8.0);

                ui.separator();

                // Chain Selection
                ui.vertical(|ui| {
                    ui.label(rich_text("CHAIN").color(theme.colors.text_secondary).size(12.0));
                    ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                        self.chain_select.show(ui, theme, icons.clone());
                    });
                });

                // From Wallet
                ui.vertical(|ui| {
                    ui.label(rich_text("FROM").color(theme.colors.text_secondary).size(12.0));
                    self.wallet_select.show(ctx.clone(), ui);
                });

                // Recipient Input
                ui.vertical(|ui| {
                    ui.label(rich_text("TO").color(theme.colors.text_secondary).size(12.0));
                    ui.horizontal(|ui| {
                        // Recipient input with contact search
                        let response = ui.add(
                            TextEdit::singleline(&mut self.recipient)
                                .hint_text("Search contacts or enter address")
                                .desired_width(300.0)
                        );

                        if response.clicked() {
                            self.contact_search_open = true;
                        }
                    });

                    // Integrated contact search dropdown
                    if self.contact_search_open {
                        let contacts = ctx.contacts();
                        Frame::menu(ui.style()).show(ui, |ui| {
                            ScrollArea::vertical()
                                .max_height(200.0)
                                .show(ui, |ui| {
                                    ui.set_width(300.0);
                                    TextEdit::singleline(&mut self.search_query).hint_text("Search contacts").show(ui);

                                    ui.separator();

                                    for contact in contacts
                                        .iter()
                                        .filter(|c| c.name.to_lowercase().contains(&self.search_query.to_lowercase())) {
                                        ui.horizontal(|ui| {
                                            if ui.selectable_label(false, &contact.name).clicked() {
                                                self.recipient = contact.address.clone();
                                                self.contact_search_open = false;
                                            }
                                            ui.label(
                                                RichText::new(contact.address_short()).color(Color32::GRAY).size(12.0)
                                            );
                                        });
                                    }
                                });
                        });
                    }
                });

                // Token Selection
                ui.vertical(|ui| {
                    ui.label(rich_text("ASSET").color(theme.colors.text_secondary).size(12.0));
                    ui.horizontal(|ui| {
                        egui_theme::utils::bg_color_on_idle(ui, Color32::TRANSPARENT);

                        // Token button with icon and balance
                        let response = self.token_button(icons.clone(), ui);
                        if response.clicked() {
                            token_selection.open = true;
                        }

                        let chain = self.chain_select.chain.id();
                        let owner = self.wallet_select.wallet.key.address();
                        let currencies = ctx.get_currencies(chain);
                        token_selection.show(ctx.clone(), chain, owner, icons.clone(), &currencies, ui);

                        if let Some(currency) = token_selection.get_currency() {
                            self.token = currency.clone();
                            token_selection.reset();
                        }

                        // Balance display
                        let balance = currency_balance(ctx.clone(), owner, &self.token);
                        ui.label(
                            RichText::new(format!("Balance: {}", balance)).color(theme.colors.text_secondary).size(12.0)
                        );
                    });
                });

                // Amount Input
                ui.vertical(|ui| {
                    ui.label(rich_text("AMOUNT").color(theme.colors.text_secondary).size(12.0));
                    ui.add(
                        TextEdit::singleline(&mut self.amount)
                            .hint_text(rich_text("0.00").color(theme.colors.text_secondary))
                            .font(egui::FontId::proportional(20.0))
                            .desired_width(200.0)
                    );

                    // Priority Fee
                    ui.horizontal(|ui| {
                        ui.label(rich_text("Priority Fee").color(theme.colors.text_secondary).size(12.0));
                        ui.add(TextEdit::singleline(&mut self.priority_fee).desired_width(50.0));
                        ui.label(rich_text("Gwei").color(theme.colors.text_secondary).size(12.0));
                    });

                    // USD Value
                    let token = if self.token.is_native() {
                        ERC20Token::native_wrapped_token(self.chain_select.chain.id())
                    } else {
                        self.token.erc20().cloned().unwrap()
                    };
                    let price = ctx.get_token_price(&token).unwrap_or(0.0);
                    let amount: f64 = self.amount.parse().unwrap_or(0.0);
                    let value = currency_value(price, amount);
                    ui.label(RichText::new(format!("≈ ${}", value)).color(theme.colors.text_secondary).size(12.0));
                });

                // Send Button
                ui.horizontal(|ui| {
                    if ui.button("Send").clicked() {
                        self.send(ctx.clone());
                    }
                });
            });
    }

    fn token_button(&mut self, icons: Arc<Icons>, ui: &mut Ui) -> Response {
        let icon;
        let chain = self.chain_select.chain.id();
        if self.token.is_native() {
            icon = icons.currency_icon(chain);
        } else {
            let token = self.token.erc20().unwrap();
            icon = icons.token_icon(token.address, chain);
        }

        let button = img_button(icon, self.token.symbol());
        ui.add(button)
    }

    fn send(&self, ctx: ZeusCtx) {
        let from = self.wallet_select.wallet.clone();
        let to = Address::from_str(&self.recipient).unwrap_or(Address::ZERO);
        let amount = U256::from_str(&self.amount).unwrap_or_default();
        let currency = self.token.clone();
        let chain = self.chain_select.chain.id();
        let fee = self.priority_fee.clone();

        RT.spawn(async move {
           match send_crypto(ctx, from, to, currency, amount, fee, chain).await {
                Ok(_) => {
                     // TODO
                },
                Err(e) => {
                     let mut gui = SHARED_GUI.write().unwrap();
                     gui.msg_window.open("Transaction Error", &e.to_string());
                }
           }
        });
    }
}
