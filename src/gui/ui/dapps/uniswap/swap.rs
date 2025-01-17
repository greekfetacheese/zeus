use egui::{ vec2, Align, Color32, FontId, Grid, Layout, TextEdit, Ui };
use crate::assets::icons::Icons;
use crate::gui::ui::*;
use crate::core::ZeusCtx;
use zeus_eth::defi::currency::{ erc20::ERC20Token, native::NativeCurrency, Currency };
use egui_theme::Theme;
use std::sync::Arc;

/// Currency direction
#[derive(Clone)]
pub enum InOrOut {
    In,
    Out,
}

impl InOrOut {
    pub fn to_string(&self) -> String {
        (
            match self {
                Self::In => "In",
                Self::Out => "Out",
            }
        ).to_string()
    }
}

pub struct SwapUi {
    pub open: bool,

    pub currency_in: Currency,

    pub currency_out: Currency,

    pub amount_in: String,

    pub amount_out: String,
}

impl SwapUi {
    pub fn new() -> Self {
        let currency_in = NativeCurrency::from_chain_id(1);
        let currency_in = Currency::from_native(currency_in);
        let currency_out = Currency::from_erc20(ERC20Token::default());
        Self {
            open: false,
            currency_in,
            currency_out,
            amount_in: "".to_string(),
            amount_out: "".to_string(),
        }
    }

    fn amount_in(&mut self) -> &mut String {
        &mut self.amount_in
    }

    fn amount_out(&mut self) -> &mut String {
        &mut self.amount_out
    }

    /// Get the currency_in or currency_out based on the direction
    fn get_currency(&self, in_or_out: &InOrOut) -> &Currency {
        match in_or_out {
            InOrOut::In => &self.currency_in,
            InOrOut::Out => &self.currency_out,
        }
    }

    /// Replace the currency_in or currency_out based on the direction
    pub fn replace_currency(&mut self, in_or_out: &InOrOut, currency: Currency) {
        match in_or_out {
            InOrOut::In => {
                self.currency_in = currency;
            }
            InOrOut::Out => {
                self.currency_out = currency;
            }
        }
    }

    /// Give a default input currency based on the selected chain id
    pub fn default_currency_in(&mut self, id: u64) {
        let native = NativeCurrency::from_chain_id(id);
        self.currency_in = Currency::from_native(native);
    }

    /// Give a default output currency based on the selected chain id
    pub fn default_currency_out(&mut self, id: u64) {
        let erc = ERC20Token::default();
        self.currency_out = Currency::from_erc20(erc);
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
        ui.label("Swap UI");

        let chain_id = ctx.chain().id();
        let owner = ctx.wallet().key.address();
        let currencies = ctx.get_currencies();

        let sell_text = rich_text("Sell").size(23.0);
        let buy_text = rich_text("Buy").size(23.0);

        let frame = theme.frame2;

        ui.vertical_centered_justified(|ui| {
            ui.set_width(500.0);
            ui.set_height(550.0);

            // Tx Settings
            ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
                ui.label("Tx Settings goes here");
            });

            Grid::new("swap_ui")
                .min_col_width(50.0)
                .spacing((0.0, 10.0))
                .show(ui, |ui| {
                    // Sell currency field
                    frame.clone().show(ui, |ui| {
                        ui.set_max_width(400.0);
                        ui.set_max_height(100.0);

                        ui.with_layout(Layout::left_to_right(Align::TOP), |ui| {
                            ui.label(sell_text);
                        });

                        ui.scope(|ui| {
                            egui_theme::utils::border_on_idle(ui, 1.0, Color32::WHITE);
                            egui_theme::utils::border_on_hover(ui, 1.0, theme.colors.border_color_hover);
                            egui_theme::utils::border_on_click(ui, 1.0, theme.colors.border_color_click);
                            self.amount_field(ui, InOrOut::In);
                        });

                        ui.scope(|ui| {
                            ui.set_max_width(30.0);
                            ui.set_max_height(20.0);
                            egui_theme::utils::bg_color_on_idle(ui, Color32::TRANSPARENT);

                            self.token_button(ui, chain_id, icons.clone(), InOrOut::In, token_selection);
                            ui.add_space(10.0);

                            let balance = currency_balance(ctx.clone(), owner, &self.currency_in);
                            let balance_text = rich_text(balance.clone());
                            ui.label(balance_text);
                            ui.add_space(5.0);

                            let max = rich_text("Max").size(17.0).color(Color32::RED);
                            // TODO: on hover change the cursor to a pointer
                            if ui.label(max).clicked() {
                                *self.amount_in() = balance;
                            }
                        });
                    });

                    ui.end_row();

                    frame.show(ui, |ui| {
                        ui.set_max_width(400.0);
                        ui.set_max_height(100.0);

                        ui.with_layout(Layout::left_to_right(Align::TOP), |ui| {
                            ui.label(buy_text);
                        });

                        ui.scope(|ui| {
                            egui_theme::utils::border_on_idle(ui, 1.0, Color32::WHITE);
                            egui_theme::utils::border_on_hover(ui, 1.0, theme.colors.border_color_hover);
                            egui_theme::utils::border_on_click(ui, 1.0, theme.colors.border_color_click);
                            self.amount_field(ui, InOrOut::Out);
                        });

                        ui.scope(|ui| {
                            ui.set_max_width(30.0);
                            ui.set_max_height(20.0);
                            egui_theme::utils::bg_color_on_idle(ui, Color32::TRANSPARENT);

                            self.token_button(ui, chain_id, icons.clone(), InOrOut::Out, token_selection);
                            ui.add_space(10.0);
                            let balance = currency_balance(ctx.clone(), owner, &self.currency_out);
                            let balance_text = rich_text(balance.clone());
                            ui.label(balance_text);

                            ui.add_space(5.0);

                            let max = rich_text("Max").size(17.0).color(Color32::RED);
                            if ui.label(max).clicked() {
                                *self.amount_out() = balance;
                            }
                        });
                    });

                    token_selection.show(ctx, icons, &currencies, ui);

                    let selected_currency = token_selection.get_currency();
                    let direction = token_selection.get_currency_direction();

                    if let Some(currency) = selected_currency {
                        self.replace_currency(&direction, currency.clone());
                        token_selection.reset();
                    }
                });
        });
    }

    /// Creates the amount field
    fn amount_field(&mut self, ui: &mut Ui, in_or_out: InOrOut) {
        let font = FontId::new(23.0, roboto_regular());
        let hint = rich_text("0").size(23.0);

        let amount = match in_or_out {
            InOrOut::In => self.amount_in(),
            InOrOut::Out => self.amount_out(),
        };

        let field = TextEdit::singleline(amount)
            .font(font)
            .min_size(vec2(100.0, 30.0))
            .text_color(Color32::WHITE)
            .hint_text(hint)
            .frame(true);

        ui.add(field);
    }

    /// Create the token button
    ///
    /// If clicked it will show the [TokenSelectionWindow]
    fn token_button(
        &mut self,
        ui: &mut Ui,
        chain_id: u64,
        icons: Arc<Icons>,
        in_or_out: InOrOut,
        token_selection: &mut TokenSelectionWindow
    ) {
        ui.push_id(in_or_out.to_string(), |ui| {
            let currency = self.get_currency(&in_or_out);
            let symbol_text = rich_text(currency.symbol()).size(17.0);

            let icon;
            if currency.is_native() {
                icon = icons.currency_icon(chain_id);
            } else {
                let token = currency.erc20().unwrap();
                icon = icons.token_icon(token.address, chain_id);
            }

            let button = img_button(icon, symbol_text).min_size(vec2(50.0, 25.0)).sense(Sense::click());

            if ui.add(button).clicked() {
                token_selection.currency_direction = in_or_out;
                token_selection.open = true;
            }
        });
    }
}
