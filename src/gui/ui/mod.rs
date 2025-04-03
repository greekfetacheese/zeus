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
pub const REFRESH: &str = "⟲";