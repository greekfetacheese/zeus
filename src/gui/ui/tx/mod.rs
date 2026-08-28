//! This module contains the UI components for showing a transaction
//!
//! - The TxConfirmationWindow contains as much information as possible about the transaction before the user confirms it.
//! - The TxWindow is what we show to the user for a transaction that has been confirmed.

use egui::{Align, Layout, RichText, Ui};
use egui_elements::{Label, Theme};
use zeus_eth::alloy_primitives::TxHash;

use crate::assets::icons::Icons;
use crate::core::ZeusContext;
use crate::core::clear_signing::{ClearDisplay, FormattedValue};
use crate::utils::{truncate_address, truncate_hash};
use zeus_eth::{
   alloy_primitives::Address,
   currency::{Currency, NativeCurrency},
   types::ChainId,
   utils::NumericValue,
};

use std::sync::Arc;

pub mod confrim_window;
pub mod events;
pub mod spent_note_window;
pub mod tx_window;

pub use confrim_window::TxConfirmationWindow;
pub use spent_note_window::{SpentHistoryRow, SpentNoteWindow};
pub use tx_window::TxWindow;

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
         ui.label(RichText::new("Cost").size(theme.typography.large));
      });

      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         let cost = eth_cost.abbreviated();
         let text = format!(
            "{:.10} {} ~ ${}",
            cost,
            eth.symbol,
            eth_cost_usd.abbreviated()
         );
         ui.label(RichText::new(text).size(theme.typography.large));
      });
   });
}

/// Show the trasnsaction hash with a hyperlink to the block explorer
/// in a horizontal layout from left to right
pub fn tx_hash(chain: ChainId, tx_hash: &TxHash, theme: &Theme, ui: &mut Ui) {
   ui.horizontal(|ui| {
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         let text = "Transaction hash";
         ui.label(RichText::new(text).size(theme.typography.large));
      });

      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         let hash_str = truncate_hash(tx_hash.to_string());
         let explorer = chain.block_explorer();
         let link = format!("{}/tx/{}", explorer, tx_hash);
         ui.hyperlink_to(
            RichText::new(hash_str).size(theme.typography.large).color(theme.colors.info),
            link,
         );
      });
   });
}

/// Show the value of a transaction in a horizontal layout from left to right
pub fn value(
   ctx: &mut ZeusContext,
   chain: ChainId,
   value: NumericValue,
   theme: &Theme,
   ui: &mut Ui,
) {
   let eth = Currency::from(NativeCurrency::from(chain.id()));

   ui.horizontal(|ui| {
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         ui.label(RichText::new("Value").size(theme.typography.large));
      });

      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         let value_usd = ctx.get_currency_value_for_amount(value.f64(), &eth);
         let text = format!(
            "{} {} ~ ${:4}",
            value.abbreviated(),
            eth.symbol(),
            value_usd.abbreviated()
         );
         ui.label(RichText::new(text).size(theme.typography.large));
      });
   });
}

/// Show the contract interaction with a hyperlink to the block explorer
/// in a horizontal layout from left to right
pub fn contract_interact(
   ctx: &mut ZeusContext,
   chain: ChainId,
   interact_to: Address,
   theme: &Theme,
   ui: &mut Ui,
) {
   ui.horizontal(|ui| {
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         let text = RichText::new("Contract interaction").size(theme.typography.large);
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
               .size(theme.typography.large)
               .color(theme.colors.info),
            link,
         );
      });
   });
}

/// Show the address of the sender or recipient depending on the context
/// in a horizontal layout from left to right
pub fn address(
   ctx: &mut ZeusContext,
   chain: ChainId,
   label: &str,
   address: Address,
   theme: &Theme,
   ui: &mut Ui,
) {
   ui.horizontal(|ui| {
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         ui.label(RichText::new(label).size(theme.typography.large));
      });

      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         let address_name = ctx.get_address_name(chain.id(), address);
         let address_name = if let Some(address_name_str) = address_name {
            address_name_str
         } else {
            truncate_address(address.to_string())
         };

         let explorer = chain.block_explorer();
         let link = format!("{}/address/{}", explorer, address.to_string());
         ui.hyperlink_to(
            RichText::new(address_name)
               .size(theme.typography.large)
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
         ui.label(RichText::new("Chain").size(theme.typography.large));
      });

      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         let icon = icons.chain_icon(chain.id(), tint);
         let text = RichText::new(chain.name()).size(theme.typography.large);
         let label = Label::new(text, Some(icon)).image_on_left().interactive(false);
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
   let text = RichText::new(text).size(theme.typography.normal);
   ui.add(Label::new(text, Some(icon)).image_on_left().interactive(false));
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
   let text = RichText::new(text).size(theme.typography.large);
   ui.add(Label::new(text, None).image_on_left().interactive(false));
}

pub fn clear_display_ui(
   ctx: &mut ZeusContext,
   chain_id: ChainId,
   display: &ClearDisplay,
   theme: &Theme,
   icons: Arc<Icons>,
   ui: &mut Ui,
) {
   let tint = theme.image_tint_recommended;

   ui.spacing_mut().item_spacing.y = 10.0;

   if let Some(owner) = display.owner.as_ref() {
      let name = match &display.contract_name {
         Some(c) => format!("{owner} · {c}"),
         None => owner.clone(),
      };
      ui.label(RichText::new(name).size(theme.typography.normal));
   }

   if let Some(intent) = display.interpolated_intent.as_ref() {
      ui.label(RichText::new(intent).size(theme.typography.large));
   }

   for warning in &display.warnings {
      ui.label(RichText::new(warning).size(theme.typography.normal).color(theme.colors.warning));
   }

   for field in &display.fields {
      match &field.value {
         FormattedValue::Address(addr) => {
            address(ctx, chain_id, &field.label, *addr, theme, ui);
         }
         FormattedValue::TokenAmount {
            amount,
            token,
            unlimited,
         } => {
            ui.horizontal(|ui| {
               ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                  ui.label(RichText::new(&field.label).size(theme.typography.large));
               });
               ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                  let amount_txt = if *unlimited {
                     "Unlimited".to_string()
                  } else {
                     amount.abbreviated()
                  };
                  let text = format!("{} {}", amount_txt, token.symbol);
                  let icon = icons.token_icon_x24(token.address, token.chain_id, tint);
                  let text = RichText::new(text).size(theme.typography.large);
                  let label = Label::new(text, Some(icon))
                     .wrap()
                     .visuals(theme.label_visuals())
                     .interactive(false);
                  ui.add(label);
               });
            });
         }
         FormattedValue::Date(ts) => {
            ui.horizontal(|ui| {
               ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                  ui.label(RichText::new(&field.label).size(theme.typography.large));
               });
               ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                  ui.label(RichText::new(ts.to_relative()).size(theme.typography.large));
               });
            });
         }
         FormattedValue::Text(text) | FormattedValue::Bytes(text) => {
            ui.horizontal(|ui| {
               ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                  ui.label(RichText::new(&field.label).size(theme.typography.large));
               });
               ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                  ui.label(RichText::new(text).size(theme.typography.large));
               });
            });
         }
      }
   }
}
