pub mod auth;
pub mod dapps;
pub mod header;
pub mod misc;
pub mod notification;
pub mod panels;
pub mod portfolio;
pub mod recipient_selection;
pub mod send_crypto;
pub mod settings;
pub mod sign_msg_window;
pub mod token_selection;
pub mod tx;
pub mod wallet;

pub use auth::{RecoverHDWallet, UnlockVault};
pub use dapps::{across::AcrossBridge, uniswap::swap::SwapUi};
pub use header::Header;
pub use misc::*;
pub use notification::{Notification, NotificationType};
pub use portfolio::PortfolioUi;
pub use recipient_selection::RecipientSelectionWindow;
pub use send_crypto::SendCryptoUi;
pub use settings::{ContactsUi, EncryptionSettings, NetworkSettings, SettingsUi};
pub use token_selection::TokenSelectionWindow;
pub use tx::{TxConfirmationWindow, TxWindow};
pub use wallet::WalletUi;

pub const GREEN_CHECK: &str = "✅";
pub const REFRESH: &str = "⟲";

use egui::FontFamily;

pub fn inter_bold() -> FontFamily {
   FontFamily::Name("inter_bold".into())
}
