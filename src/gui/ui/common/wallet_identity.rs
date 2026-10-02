//! `[icon] Wallet 2 · 0x1a2b…c3d4` — a wallet-identity line for dapp prompts.
//!
//! Apps are given their own account, so the account a prompt acts on can differ
//! from the one selected in the sidebar. Anywhere that happens the user needs to
//! see *which* account is in play, next to *which* app.

use crate::assets::icons::Icons;
use crate::core::ZeusContext;
use crate::utils::truncate_address;
use egui::RichText;
use egui_elements::{Label, Theme};
use std::sync::Arc;
use zeus_eth::alloy_primitives::Address;

/// One mixed-style [`Label`] naming `account`, with the address muted beside it.
///
/// The parts share a single galley, so the line sizes to its content and centers
/// as a unit under a centered heading. Falls back to the address alone when the
/// account has no name (or is unnamed by the address book).
pub fn wallet_identity(
   ctx: &ZeusContext,
   chain_id: u64,
   account: Address,
   theme: &Theme,
   icons: Arc<Icons>,
) -> Label {
   let normal = theme.typography.normal;
   let muted = theme.colors.text_muted;

   let name = ctx
      .get_address_name(chain_id, account)
      .map(|name| name.to_string())
      .filter(|name| !name.trim().is_empty());

   let mut parts = Vec::new();

   if let Some(name) = name {
      parts.push(RichText::new(name).size(normal));
      parts.push(RichText::new(" · ").size(normal).color(muted));
   }

   let address = truncate_address(account.to_string());
   parts.push(RichText::new(address).size(normal).color(muted));

   Label::sections(parts, Some(icons.wallet_main_x24()))
      .image_on_left()
      .interactive(false)
}
