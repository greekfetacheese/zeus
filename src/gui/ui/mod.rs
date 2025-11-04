pub mod auth;
pub mod dapps;
pub mod header;
pub mod misc;
pub mod notification;
pub mod panels;
pub mod recipient_selection;
pub mod send_crypto;
pub mod settings;
pub mod sign_msg_window;
pub mod token_selection;
pub mod tx_window;
pub mod wallet;

pub use auth::{CredentialsForm, RecoverHDWallet, UnlockVault};
pub use dapps::{across::AcrossBridge, uniswap::swap::SwapUi};
pub use header::Header;
pub use misc::*;
pub use notification::{Notification, NotificationType};
pub use recipient_selection::RecipientSelectionWindow;
pub use send_crypto::SendCryptoUi;
pub use settings::{ContactsUi, EncryptionSettings, NetworkSettings, SettingsUi};
pub use token_selection::TokenSelectionWindow;
pub use tx_window::{TxConfirmationWindow, TxWindow};
pub use wallet::WalletUi;

pub const GREEN_CHECK: &str = "✅";
pub const REFRESH: &str = "⟲";
pub const EXTERNAL_LINK: &str = "↗";

use crate::assets::icons::Icons;
use crate::core::ZeusCtx;
use crate::utils::{truncate_address, truncate_hash};
use egui::{Align, FontFamily, Layout, RichText, Ui};
use egui_widgets::Label;
use zeus_eth::{
   alloy_primitives::{Address, TxHash},
   currency::{Currency, NativeCurrency},
   types::ChainId,
   utils::NumericValue,
};
use zeus_theme::Theme;

use std::sync::Arc;

pub fn inter_bold() -> FontFamily {
   FontFamily::Name("inter_bold".into())
}

/// Show the transaction cost in a horizontal layout from left to right
pub fn tx_cost(
   chain: ChainId,
   eth_cost: &NumericValue,
   eth_cost_usd: &NumericValue,
   theme: &Theme,
   ui: &mut Ui,
) {
   let eth = NativeCurrency::from(chain.id());

   ui.horizontal(|ui| {
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         ui.label(RichText::new("Cost").size(theme.text_sizes.large));
      });

      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         let cost = eth_cost.abbreviated();
         let text = format!(
            "{:.10} {} ~ ${}",
            cost,
            eth.symbol,
            eth_cost_usd.abbreviated()
         );
         ui.label(RichText::new(text).size(theme.text_sizes.large));
      });
   });
}

/// Show the trasnsaction hash with a hyperlink to the block explorer
/// in a horizontal layout from left to right
pub fn tx_hash(chain: ChainId, tx_hash: &TxHash, theme: &Theme, ui: &mut Ui) {
   ui.horizontal(|ui| {
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         let text = "Transaction hash";
         ui.label(RichText::new(text).size(theme.text_sizes.large));
      });

      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         let hash_str = truncate_hash(tx_hash.to_string());
         let explorer = chain.block_explorer();
         let link = format!("{}/tx/{}", explorer, tx_hash);
         ui.hyperlink_to(
            RichText::new(hash_str).size(theme.text_sizes.large).color(theme.colors.info),
            link,
         );
      });
   });
}

/// Show the value of a transaction in a horizontal layout from left to right
pub fn value(ctx: ZeusCtx, chain: ChainId, value: NumericValue, theme: &Theme, ui: &mut Ui) {
   let eth = Currency::from(NativeCurrency::from(chain.id()));

   ui.horizontal(|ui| {
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         ui.label(RichText::new("Value").size(theme.text_sizes.large));
      });

      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         let value_usd = ctx.get_currency_value_for_amount(value.f64(), &eth);
         let text = format!(
            "{} {} ~ ${:4}",
            value.abbreviated(),
            eth.symbol(),
            value_usd.abbreviated()
         );
         ui.label(RichText::new(text).size(theme.text_sizes.large));
      });
   });
}

/// Show the contract interaction with a hyperlink to the block explorer
/// in a horizontal layout from left to right
pub fn contract_interact(
   ctx: ZeusCtx,
   chain: ChainId,
   interact_to: Address,
   theme: &Theme,
   ui: &mut Ui,
) {
   ui.horizontal(|ui| {
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         let text = RichText::new("Contract interaction").size(theme.text_sizes.large);
         ui.label(text);
      });

      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         let interact_to_name = ctx.get_address_name(chain.id(), interact_to);

         let interact_to_name = if let Some(interact_to_name_str) = interact_to_name {
            interact_to_name_str
         } else {
            truncate_address(interact_to.to_string())
         };

         let explorer = chain.block_explorer();
         let link = format!("{}/address/{}", explorer, interact_to);

         ui.hyperlink_to(
            RichText::new(interact_to_name)
               .size(theme.text_sizes.large)
               .color(theme.colors.info),
            link,
         );
      });
   });
}

/// Show the address of the sender or recipient depending on the context
/// in a horizontal layout from left to right
pub fn address(
   ctx: ZeusCtx,
   chain: ChainId,
   label: &str,
   address: Address,
   theme: &Theme,
   ui: &mut Ui,
) {
   ui.horizontal(|ui| {
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         ui.label(RichText::new(label).size(theme.text_sizes.large));
      });

      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         let address_short = truncate_address(address.to_string());
         let address_name = ctx.get_address_name(chain.id(), address);
         let address_name = if let Some(address_name_str) = address_name {
            address_name_str
         } else {
            address_short
         };

         let explorer = chain.block_explorer();
         let link = format!("{}/address/{}", explorer, address.to_string());
         ui.hyperlink_to(
            RichText::new(address_name)
               .size(theme.text_sizes.large)
               .color(theme.colors.info),
            link,
         );
      });
   });
}

/// Show the chain name with an icon in a horizontal layout from left to right
pub fn chain(chain: ChainId, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
   let tint = theme.image_tint_recommended;
   ui.horizontal(|ui| {
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         ui.label(RichText::new("Chain").size(theme.text_sizes.large));
      });

      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         let icon = icons.chain_icon(chain.id(), tint);
         let label = Label::new(
            RichText::new(chain.name()).size(theme.text_sizes.large),
            Some(icon),
         )
         .image_on_left();
         ui.add(label);
      });
   });
}

/// Show the ETH spent in a horizontal layout from left to right
pub fn eth_spent(
   chain: u64,
   eth_spent: NumericValue,
   eth_spent_usd: NumericValue,
   theme: &Theme,
   icons: Arc<Icons>,
   _text: &str,
   ui: &mut Ui,
) {
   let tint = theme.image_tint_recommended;
   let native = NativeCurrency::from(chain);
   let icon = icons.native_currency_icon_x24(chain, tint);
   let text = format!(
      "{} {} ≈ {}",
      eth_spent.abbreviated(),
      native.symbol,
      eth_spent_usd.abbreviated()
   );
   let text = RichText::new(text).size(theme.text_sizes.normal);
   ui.add(Label::new(text, Some(icon)).image_on_left());
}

/// Show the ETH received in a horizontal layout from left to right
pub fn eth_received(
   chain: u64,
   eth_received: NumericValue,
   eth_received_usd: NumericValue,
   theme: &Theme,
   _icons: Arc<Icons>,
   text: &str,
   ui: &mut Ui,
) {
   let native = NativeCurrency::from(chain);
   // let icon = icons.native_currency_icon_x24(chain);
   let text = format!(
      "{text} {} {} ≈ ${}",
      eth_received.abbreviated(),
      native.symbol,
      eth_received_usd.abbreviated()
   );
   let text = RichText::new(text).size(theme.text_sizes.large);
   ui.add(Label::new(text, None).image_on_left());
}
