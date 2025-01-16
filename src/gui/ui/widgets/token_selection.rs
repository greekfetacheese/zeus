use eframe::egui::Sense;
use eframe::egui::{
    emath::Vec2b,
    vec2,
    Align,
    Align2,
    Layout,
    ScrollArea,
    TextEdit,
    Ui,
    Window,
    Color32,
    Frame,
    FontId,
    Stroke
};

use std::str::FromStr;
use std::sync::Arc;

use crate::assets::fonts::roboto_regular;
use crate::core::utils::RT;
use crate::gui::ui::{ button, img_button, currency_balance, rich_text };
use crate::gui::{SHARED_GUI, ui::dapps::uniswap::swap::InOrOut};
use crate::assets::icons::Icons;
use crate::core::{ data::app_data::APP_DATA, utils::fetch };
use zeus_eth::alloy_primitives::Address;
use zeus_eth::defi::currency::Currency;

/// A simple window that allows the user to select a token
/// based on the a list of [Currency] we pass to it
///
/// We can also use the search bar to search for a specific token either by its name or symbol.
///
/// If a valid address is passed to the search bar, we can fetch the token from the blockchain if it exists
pub struct TokenSelectionWindow {
    pub open: bool,

    pub search_query: String,

    pub selected_currency: Option<Currency>,

    /// Currency direction, this only applies if we try to select a token from a SwapUi
    pub currency_direction: InOrOut,

}

impl TokenSelectionWindow {
    pub fn new() -> Self {
        Self {
            open: false,
            search_query: String::new(),
            selected_currency: None,
            currency_direction: InOrOut::In,
        }
    }

    pub fn set_currency_direction(&mut self, currency_direction: InOrOut) {
        self.currency_direction = currency_direction;
    }

    pub fn get_currency_direction(&self) -> &InOrOut {
        &self.currency_direction
    }

    pub fn get_currency(&self) -> Option<&Currency> {
        self.selected_currency.as_ref()
    }

    pub fn reset(&mut self) {
        self.selected_currency = None;
        self.search_query.clear();
    }

    /// Show This [TokenSelectionWindow]
    pub fn show(&mut self, ui: &mut Ui, icons: Arc<Icons>, currencies: &Vec<Currency>) {
        if !self.open {
            return;
        }

        let chain_id;
        let owner;
        {
            let app_data = APP_DATA.read().unwrap();
            chain_id = app_data.chain_id.id();
            owner = app_data.profile.wallet_address();
        }

        ui.scope(|ui| {

       
        Window::new(rich_text("Select Token").size(18.0))
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .resizable(false)
            .collapsible(false)
            .fade_in(true)
            .frame(Frame::window(ui.style()))
            .fade_out(true)
            .show(ui.ctx(), |ui| {
                ui.set_min_size(vec2(200.0, 130.0));

                ui.visuals_mut().extreme_bg_color = Color32::TRANSPARENT;
                ui.visuals_mut().window_fill = Color32::from_rgba_premultiplied(20, 17, 15, 255);
                ui.visuals_mut().widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
                ui.visuals_mut().widgets.hovered.weak_bg_fill = Color32::from_rgba_premultiplied(37, 35, 35, 140);
                ui.visuals_mut().button_frame = true;


                ui.vertical_centered(|ui| {
                    let font = FontId::new(18.0, roboto_regular());
                    ui.scope(|ui| {
                    ui.visuals_mut().widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgba_premultiplied(255, 255, 255, 71));
                    ui.add(
                        TextEdit::singleline(&mut self.search_query)
                            .hint_text(rich_text("Search tokens by symbol or address"))
                            .min_size((200.0, 30.0).into()).text_color(Color32::WHITE)
                            .font(font)
                            .frame(true)
                    );
                    ui.add_space(15.0);
                    });
                });

                ScrollArea::vertical()
                    .auto_shrink(Vec2b::new(false, false))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 10.0;
                        for (index, currency) in currencies.iter().enumerate() {
                            match currency {
                                Currency::Native(native) => {
                                    if native.symbol.to_lowercase().contains(&self.search_query) {
                                        ui.push_id(index, |ui| {
                                            let name = rich_text(native.name.clone());
                                            let symbol = format!("({})", native.symbol.clone());

                                            let icon = icons.currency_icon(chain_id);
                                            let button = img_button(icon, name).sense(
                                                Sense::click()
                                            );

                                            ui.horizontal(|ui| {
                                                ui.scope(|ui| {
                                                ui.visuals_mut().widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::WHITE);
                                            
                                                if ui.add(button).clicked() {
                                                    self.selected_currency = Some(currency.clone());
                                                    self.open = false;
                                                }

                                                ui.label(rich_text(symbol));
                                            });
                                                ui.with_layout(
                                                    Layout::right_to_left(Align::Min),
                                                    |ui| {
                                                        let balance = currency_balance(chain_id, owner, currency);
                                                        ui.label(rich_text(balance).size(15.0));
                                                    }
                                                );
                                            });

                                            ui.add_space(5.0);
                                        });
                                    }
                                }
                                Currency::ERC20(token) => {
                                    if token.symbol.to_lowercase().contains(&self.search_query) {
                                        ui.push_id(index, |ui| {
                                            // TODO: use something like numformat to deal with high numbers

                                            let name = rich_text(token.name.clone());
                                            let symbol = format!("({})", token.symbol.clone());

                                            let icon = icons.token_icon(token.address, chain_id);
                                            let button = img_button(icon, name).sense(
                                                Sense::click()
                                            );

                                            ui.horizontal(|ui| {
                                                ui.scope(|ui| {
                                                ui.visuals_mut().widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::WHITE);
                                            
                                                if ui.add(button).clicked() {
                                                    self.selected_currency = Some(currency.clone());
                                                    self.open = false;
                                                }

                                                    ui.label(rich_text(symbol));
                                            });
                                                ui.with_layout(
                                                    Layout::right_to_left(Align::Min),
                                                    |ui| {
                                                        let balance = currency_balance(chain_id, owner, currency);
                                                        ui.label(rich_text(balance).size(15.0));
                                                    }
                                                );
                                            });

                                            ui.add_space(5.0);
                                        });
                                    }
                                }
                            }
                        }

                        let add_token_text = rich_text("Add Token");
                        let add_token_button = button(add_token_text);

                        // if search string is a valid ethereum address
                        if let Ok(address) = Address::from_str(&self.search_query) {
                            ui.vertical_centered(|ui| {
                                if ui.add(add_token_button).clicked() {
                                    RT.spawn(async move {
                                        let token = match fetch::get_erc20_token(address, chain_id).await {
                                            Ok(token) => token,
                                            Err(e) => {
                                                let mut gui = SHARED_GUI.write().unwrap();
                                                gui.open_msg_window("Failed to fetch token", e.to_string());
                                                return;
                                            }
                                        };

                                        let currency = Currency::from_erc20(token);
                                        let mut gui = SHARED_GUI.write().unwrap();
                                        gui.token_selection.selected_currency = Some(currency);
                                    });

                                    // close the token selection window
                                    self.open = false;
                                }
                            });
                        }
                    });
            });
        });
    }
}
