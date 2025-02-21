pub mod dapps;
pub mod auth;
pub mod wallet;
pub mod panels;
pub mod misc;
pub mod settings;
pub mod send_crypto;
pub mod token_selection;

pub use dapps::uniswap::swap::SwapUi;
pub use auth::{ CredentialsForm, LoginUi, RegisterUi };
pub use wallet::WalletUi;
pub use misc::*;
pub use settings::SettingsUi;
pub use send_crypto::SendCryptoUi;
pub use token_selection::TokenSelectionWindow;


use eframe::egui::{
    FontId,
    Button,
    RichText,
    TextEdit,
    widgets::Image,
    widget_text::WidgetText,
    Sense,
    vec2,
};
use crate::assets::fonts::roboto_regular;
use crate::core::ZeusCtx;
use zeus_eth::alloy_primitives::{ Address, utils::format_units };
use zeus_eth::currency::{Currency, erc20::ERC20Token};


// ** HELPER FUNCTIONS **

/// Calculate the total value of a wallet in USD
pub fn wallet_value(ctx: ZeusCtx, chain: u64, owner: Address) -> f64 {
    let portfolio = ctx.get_portfolio(chain, owner).unwrap_or_default();
    if portfolio.currencies.is_empty() {
        return 0.0;
    }

    let currencies = portfolio.currencies();
    let mut value = 0.0;

    for currency in currencies {
        let usd_price: f64 =  currency_price(ctx.clone(), currency).parse().unwrap_or(0.0);
        let balance: f64 = currency_balance(ctx.clone(), owner, currency).parse().unwrap_or(0.0);
        value += currency_value_f64(usd_price, balance);
    }

    value
}

/// Return a [String] that displays the formatted balance of the selected currency
// TODO: Use something like numformat to deal with very large numbers
pub fn currency_balance(ctx: ZeusCtx, owner: Address, currency: &Currency) -> String {
    let balance_text;

    if currency.is_native() {
        let balance = ctx.get_eth_balance(owner);
        balance_text = format_units(balance, currency.decimals().clone()).unwrap_or(
            "0.0".to_string()
        );
    } else {
        let currency = currency.erc20().unwrap();
        let balance = ctx.get_token_balance(owner, currency.address);
        balance_text = format_units(balance, currency.decimals).unwrap_or("0.0".to_string());
    }

    format!("{:.4}", balance_text)
}

/// Return the USD price of a token in String format
pub fn currency_price(ctx: ZeusCtx, currency: &Currency) -> String {
    let price;
    let chain = ctx.chain().id();

    if currency.is_native() {
        let wrapped_token = ERC20Token::native_wrapped_token(chain);
        price = ctx.get_token_price(&wrapped_token).unwrap_or(0.0);
    } else {
        let currency = currency.erc20().unwrap();
        price = ctx.get_token_price(&currency).unwrap_or(0.0);
    }

    format!("{:.2}", price)
}

/// Return the USD Value of a token in String format
pub fn currency_value(price: f64, balance: f64) -> String {
    if price == 0.0 || balance == 0.0 {
        return "0.00".to_string();
    }
    format!("{:.2}", price * balance)
}

pub fn currency_value_f64(price: f64, balance: f64) -> f64 {
    if price == 0.0 || balance == 0.0 {
        return 0.0;
    }
    price * balance
}

pub fn rich_text(text: impl Into<String>) -> RichText {
    RichText::new(text).size(15.0)
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