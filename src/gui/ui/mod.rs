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
    let font = FontId::proportional(15.0);
    TextEdit::singleline(text).min_size(vec2(150.0, 25.0)).font(font)
}

pub fn text_edit_multi(text: &mut String) -> TextEdit {
    let font = FontId::proportional(15.0);
    TextEdit::multiline(text).min_size(vec2(150.0, 25.0)).font(font)
}