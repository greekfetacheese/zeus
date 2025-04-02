pub mod auth;
pub mod dapps;
pub mod misc;
pub mod panels;
pub mod send_crypto;
pub mod settings;
pub mod token_selection;
pub mod recipient_selection;
pub mod wallet;

pub use auth::{CredentialsForm, LoginUi, RegisterUi};
pub use dapps::{across::AcrossBridge, uniswap::swap::SwapUi};
pub use misc::*;
pub use send_crypto::SendCryptoUi;
pub use token_selection::TokenSelectionWindow;
pub use recipient_selection::RecipientSelectionWindow;
pub use wallet::WalletUi;

use eframe::egui::{Button, FontId, RichText, Sense, TextEdit, vec2, widget_text::WidgetText, widgets::Image};

pub const GREEN_CIRCLE: &str = "🟢";
pub const RED_CIRCLE: &str = "🔴";
pub const LIGHTING_BOLT: &str = "⚡";
pub const BOOK: &str = "📖";
pub const GAS: &str = "⛽";
pub const GREEN_CHECK: &str = "✅";
pub const KEY: &str = "🔑";
pub const LOCK: &str = "🔒";
pub const UNLOCK: &str = "🔓";
pub const PENDING: &str = "⏳";

pub fn rich_text(text: impl Into<String>) -> RichText {
   RichText::new(text).size(15.0)
}

pub fn button(text: impl Into<WidgetText>) -> Button<'static> {
   Button::new(text)
      .sense(Sense::click())
      .min_size(vec2(70.0, 25.0))
}

pub fn img_button(image: Image<'static>, text: impl Into<WidgetText>) -> Button<'static> {
   Button::image_and_text(image, text).min_size(vec2(70.0, 25.0))
}

pub fn text_edit_single(text: &mut String) -> TextEdit {
   let font = FontId::proportional(15.0);
   TextEdit::singleline(text)
      .min_size(vec2(150.0, 25.0))
      .font(font)
}

pub fn text_edit_multi(text: &mut String) -> TextEdit {
   let font = FontId::proportional(15.0);
   TextEdit::multiline(text)
      .min_size(vec2(150.0, 25.0))
      .font(font)
}
