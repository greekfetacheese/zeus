pub mod auth;
pub mod dapps;
pub mod misc;
pub mod panels;
pub mod recipient_selection;
pub mod send_crypto;
pub mod settings;
pub mod token_selection;
pub mod tx_window;
pub mod sign_msg_window;
pub mod wallet;

pub use auth::{CredentialsForm, LoginUi, RegisterUi};
pub use dapps::{across::AcrossBridge, uniswap::swap::SwapUi};
pub use misc::*;
pub use recipient_selection::RecipientSelectionWindow;
pub use send_crypto::SendCryptoUi;
pub use settings::{ContactsUi, EncryptionSettings, NetworkSettings, SettingsUi};
pub use token_selection::TokenSelectionWindow;
pub use tx_window::{TxConfirmationWindow, TxWindow};
pub use wallet::WalletUi;

pub const GREEN_CHECK: &str = "✅";