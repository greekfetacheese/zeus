pub mod dapps;
pub mod login;
pub mod wallet;
//pub mod theme;
pub mod panels;
pub mod widgets;

pub use dapps::uniswap::swap::SwapUi;
pub use login::{ LoginUi, RegisterUi };
pub use wallet::WalletUi;
pub use widgets::*;

use crate::core::data::db::ZEUS_DB;
use crate::assets::fonts::roboto_regular;
use eframe::egui::{
    FontId,
    Button,
    Color32,
    RichText,
    TextEdit,
    widgets::Image,
    widget_text::WidgetText,
    Sense,
    vec2,
};
use zeus_eth::alloy_primitives::{ Address, utils::format_units };
use zeus_eth::defi::currency::Currency;
use zeus_eth::defi::utils::common_addr::native_wrapped_token;
use tracing::error;

// ** HELPER FUNCTIONS **

/// Return a [String] that displays the formatted balance of the selected currency
// TODO: Use something like numformat to deal with very large numbers
pub fn currency_balance(chain_id: u64, owner: Address, currency: &Currency) -> String {
    let balance_text;

    if currency.is_native() {
        let db = ZEUS_DB.read().unwrap();
        let balance = db.get_eth_balance(chain_id, owner);
        balance_text = format_units(balance, currency.decimals().clone()).unwrap_or(
            "0.0".to_string()
        );
    } else {
        let db = ZEUS_DB.read().unwrap();
        let currency = currency.erc20().unwrap();
        let balance = db.get_token_balance(chain_id, owner, currency.address);
        balance_text = format_units(balance, currency.decimals).unwrap_or("0.0".to_string());
    }

    format!("{:.4}", balance_text)
}

/// Return the USD price of a token in String format
pub fn currency_price(chain_id: u64, currency: &Currency) -> String {
    let price;

    if currency.is_native() {
        let address = if let Ok(address) = native_wrapped_token(chain_id) {
            address
        } else {
            error!("Failed to get native wrapped token address for {}", chain_id);
            return "0.0".to_string();
        };
        let db = ZEUS_DB.read().unwrap();
        price = db.get_price(chain_id, address);
    } else {
        let db = ZEUS_DB.read().unwrap();
        let currency = currency.erc20().unwrap();
        price = db.get_price(chain_id, currency.address);
    }

    format!("{:.2}", price)
}

/// Return the USD Value of a token in String format
pub fn currency_value(chain_id: u64, owner: Address, currency: &Currency) -> String {
    let price = currency_price(chain_id, currency).parse::<f64>().unwrap_or(0.0);
    let balance = currency_balance(chain_id, owner, currency).parse::<f64>().unwrap_or(0.0);

    format!("{:.2}", price * balance)
}

pub fn rich_text(text: impl Into<String>) -> RichText {
    RichText::new(text).size(15.0).family(roboto_regular()).color(Color32::WHITE)
}

pub fn button(text: impl Into<WidgetText>) -> Button<'static> {
    Button::new(text).sense(Sense::click()).min_size(vec2(70.0, 25.0))
}

pub fn img_button(image: Image<'static>, text: impl Into<WidgetText>) -> Button<'static> {
    Button::image_and_text(image, text).min_size(vec2(70.0, 25.0))
}

pub fn text_edit_single(text: &mut String) -> TextEdit {
    let font = FontId::new(15.0, roboto_regular());
    TextEdit::singleline(text).min_size(vec2(150.0, 25.0)).font(font)
}

pub fn text_edit_multi(text: &mut String) -> TextEdit {
    let font = FontId::new(15.0, roboto_regular());
    TextEdit::multiline(text).min_size(vec2(150.0, 25.0)).font(font)
}