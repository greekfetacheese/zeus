//! The grant editor: the switches that decide what an agent may do.
//!
//! The catalog is the list of switches, not a copy of it: a capability that exists is drawn here,
//! with the tier it carries and the sentence it needs, and one that does not is absent from both.
//! Every switch is the grant, so the page writes the grant and nothing else.

use crate::core::WalletInfo;
use crate::core::agent::{
   AgentProfile, CAPABILITIES, Capability, CapabilityScope, CapabilityVerb, NftRef, NftScope,
   ProfileMode, RiskTier, TokenRef, TokenScope, capability,
};
use egui::{RichText, Ui, vec2};
use egui_elements::{Button, Checkbox, Theme};
use std::collections::HashMap;
use std::str::FromStr;
use zeus_eth::alloy_primitives::Address;
use zeus_eth::types::ChainId;

/// A capability the user asked for once, waiting for the second press.
///
/// Tier 3 is the tier that moves value and is hard to undo, so it is the one that states its
/// consequence and takes a second action: the first press flips nothing and raises this.
struct Pending {
   name: String,
   address: Option<Address>,
}

/// What one pass of the editor did: whether the grant changed, and the sentence to announce.
#[derive(Default)]
pub struct Edited {
   pub changed: bool,
   pub notice: Option<String>,
}

/// The grant editor's own state: the pending grant, and the fields a new allowlist entry is typed
/// into, per wallet.
#[derive(Default)]
pub struct ProfileEditor {
   pending: Option<Pending>,
   token_chain: HashMap<Address, u64>,
   token_input: HashMap<Address, String>,
   nft_chain: HashMap<Address, u64>,
   nft_input: HashMap<Address, String>,
}

impl ProfileEditor {
   pub fn reset(&mut self) {
      self.pending = None;
      self.token_chain.clear();
      self.token_input.clear();
      self.nft_chain.clear();
      self.nft_input.clear();
   }

   /// Draw one profile's grants: whether they changed, and the sentence to announce.
   pub fn show(
      &mut self,
      profile: &mut AgentProfile,
      wallets: &[WalletInfo],
      theme: &Theme,
      ui: &mut Ui,
   ) -> Edited {
      let mut edited = Edited::default();

      let mut confirm_each = profile.mode == ProfileMode::ConfirmEach;
      let label = RichText::new("Ask before every action").size(theme.typography.normal);
      let row = ui.add(Checkbox::new(&mut confirm_each, label));
      ui.ctx().accesskit_node_builder(row.id, |node| {
         node.set_description(
            "When on, a granted action opens Zeus's own confirmation window instead of running on \
             its own.",
         );
      });

      if row.changed() {
         profile.mode = match confirm_each {
            true => ProfileMode::ConfirmEach,
            false => ProfileMode::Authorized,
         };
         edited.changed = true;
      }

      if let Some(sentence) = self.pending_block(profile, theme, ui) {
         edited.notice = Some(sentence);
         edited.changed = true;
      }

      ui.add_space(theme.spacing.sm);
      ui.label(RichText::new("Wallets").size(theme.typography.large));

      if wallets.is_empty() {
         ui.label(
            RichText::new("There is no wallet to grant anything on yet.")
               .size(theme.typography.normal),
         );
      }

      for wallet in wallets {
         if self.wallet_block(profile, wallet, theme, ui) {
            edited.changed = true;
         }
      }

      if self.screens(profile, theme, ui) {
         edited.changed = true;
      }

      edited
   }

   /// The tier-3 warning, and the two buttons that answer it.
   fn pending_block(
      &mut self,
      profile: &mut AgentProfile,
      theme: &Theme,
      ui: &mut Ui,
   ) -> Option<String> {
      let (name, address) = {
         let pending = self.pending.as_ref()?;
         (pending.name.clone(), pending.address)
      };

      ui.add_space(theme.spacing.sm);

      let sentence = format!(
         "{} is a tier 3 grant: it moves value and is hard to undo. Press Grant it to allow it.",
         words(&name)
      );

      ui.label(RichText::new(sentence).size(theme.typography.normal));

      let mut notice = None;
      let size = vec2(140.0, 28.0);

      ui.horizontal(|ui| {
         let grant = Button::new(RichText::new("Grant it").size(theme.typography.normal))
            .visuals(theme.button_visuals())
            .min_size(size);

         if ui.add(grant).clicked() {
            apply(profile, &name, address, true);
            notice = Some(format!("Granted {}.", words(&name)));
         }

         let cancel = Button::new(RichText::new("Cancel").size(theme.typography.normal))
            .visuals(theme.button_visuals())
            .min_size(size);

         if ui.add(cancel).clicked() {
            notice = Some("Left ungranted.".to_owned());
         }
      });

      if notice.is_some() {
         self.pending = None;
      }

      notice
   }

   /// One wallet's slice: its kill switch, its capabilities, and what it may move.
   fn wallet_block(
      &mut self,
      profile: &mut AgentProfile,
      wallet: &WalletInfo,
      theme: &Theme,
      ui: &mut Ui,
   ) -> bool {
      let address = wallet.address;
      let mut changed = false;

      let enabled = profile.wallets.get(&address).is_some_and(|grant| grant.enabled);

      ui.add_space(theme.spacing.md);
      ui.label(
         RichText::new(format!("{} — {}", wallet.name(), short(address)))
            .size(theme.typography.normal),
      );

      let mut allow = enabled;
      let label = RichText::new("Allow this wallet").size(theme.typography.normal);
      let row = ui.add(Checkbox::new(&mut allow, label));

      if row.changed() {
         let grant = profile.wallets.entry(address).or_default();
         grant.enabled = allow;
         changed = true;
      }

      ui.add_enabled_ui(enabled, |ui| {
         let caps: Vec<&Capability> = CAPABILITIES
            .iter()
            .filter(|entry| entry.scope == CapabilityScope::Wallet)
            .collect();

         for chunk in caps.chunks(3) {
            ui.horizontal(|ui| {
               for cap in chunk {
                  let granted = profile
                     .wallets
                     .get(&address)
                     .is_some_and(|grant| grant.caps.contains(cap.name));

                  let mut on = granted;
                  let text = RichText::new(format!(
                     "{} · tier {}",
                     words(cap.name),
                     cap.tier.level()
                  ))
                  .size(theme.typography.normal);

                  let response = ui.add(Checkbox::new(&mut on, text));

                  if response.changed() && self.toggle(profile, cap.name, Some(address), on) {
                     changed = true;
                  }
               }
            });
         }

         if self.movable(profile, address, theme, ui) {
            changed = true;
         }
      });

      changed
   }

   /// What may leave the wallet: the token and NFT scopes, and their allowlists.
   fn movable(
      &mut self,
      profile: &mut AgentProfile,
      address: Address,
      theme: &Theme,
      ui: &mut Ui,
   ) -> bool {
      let mut changed = false;

      ui.add_space(theme.spacing.sm);

      let tokens = profile
         .wallets
         .get(&address)
         .map(|grant| grant.tokens.clone())
         .unwrap_or_default();

      let mut choice = token_choice(&tokens);
      let mut picked = choice;

      ui.horizontal(|ui| {
         ui.label(RichText::new("Tokens").size(theme.typography.normal));
         ui.radio_value(&mut picked, 0, "Nothing");
         ui.radio_value(&mut picked, 1, "Everything");
         ui.radio_value(&mut picked, 2, "Only these");
      });

      if picked != choice {
         let grant = profile.wallets.entry(address).or_default();
         grant.tokens = from_token_choice(picked, &grant.tokens);
         choice = picked;
         changed = true;
      }

      if choice == 2 && self.token_allowlist(profile, address, theme, ui) {
         changed = true;
      }

      if matches!(
         profile.wallets.get(&address).map(|grant| grant.tokens.clone()),
         Some(TokenScope::All)
      ) {
         let warning = "Every asset in this wallet may leave it.";
         ui.label(RichText::new(warning).size(theme.typography.normal));
      }

      let nfts = profile
         .wallets
         .get(&address)
         .map(|grant| grant.nfts.clone())
         .unwrap_or_default();

      let mut nft_choice = nft_choice(&nfts);
      let mut picked = nft_choice;

      ui.horizontal(|ui| {
         ui.label(RichText::new("NFTs").size(theme.typography.normal));
         ui.radio_value(&mut picked, 0, "Nothing");
         ui.radio_value(&mut picked, 1, "Everything");
         ui.radio_value(&mut picked, 2, "Only these");
      });

      if picked != nft_choice {
         let grant = profile.wallets.entry(address).or_default();
         grant.nfts = from_nft_choice(picked, &grant.nfts);
         nft_choice = picked;
         changed = true;
      }

      if nft_choice == 2 && self.nft_allowlist(profile, address, theme, ui) {
         changed = true;
      }

      changed
   }

   /// The token allowlist: what is on it, and the row that adds to it.
   fn token_allowlist(
      &mut self,
      profile: &mut AgentProfile,
      address: Address,
      theme: &Theme,
      ui: &mut Ui,
   ) -> bool {
      let mut changed = false;

      let tokens = match profile.wallets.get(&address).map(|grant| grant.tokens.clone()) {
         Some(TokenScope::Allowlist { tokens }) => tokens,
         _ => Vec::new(),
      };

      if tokens.is_empty() {
         ui.label(
            RichText::new("No asset is allowed yet, so nothing may leave this wallet.")
               .size(theme.typography.normal),
         );
      }

      for token in &tokens {
         let mut remove = false;

         ui.horizontal(|ui| {
            ui.label(
               RichText::new(format!(
                  "{} on {}",
                  short(token.address),
                  chain_name(token.chain)
               ))
               .size(theme.typography.normal),
            );

            let button = Button::new(RichText::new("Remove").size(theme.typography.normal))
               .visuals(theme.button_visuals())
               .min_size(vec2(100.0, 26.0));

            remove = ui.add(button).clicked();
         });

         if remove
            && let Some(grant) = profile.wallets.get_mut(&address)
            && let TokenScope::Allowlist { tokens } = &mut grant.tokens
         {
            tokens.retain(|entry| entry != token);
            changed = true;
         }
      }

      let added = self.add_token_row(profile, address, theme, ui);

      changed || added
   }

   fn add_token_row(
      &mut self,
      profile: &mut AgentProfile,
      address: Address,
      theme: &Theme,
      ui: &mut Ui,
   ) -> bool {
      let mut chain = *self.token_chain.entry(address).or_insert_with(|| ChainId::Ethereum.id());
      let mut add = false;

      ui.horizontal(|ui| {
         ui.label(RichText::new("Chain").size(theme.typography.normal));
         chain_picker(ui, ("token_chain", address), &mut chain);
      });

      self.token_chain.insert(address, chain);

      let typed = self.token_input.entry(address).or_default();

      ui.horizontal(|ui| {
         let field = ui.add(
            egui::TextEdit::singleline(typed)
               .hint_text("Token address")
               .desired_width(320.0),
         );

         ui.ctx()
            .accesskit_node_builder(field.id, |node| node.set_label("Token address"));

         let button = Button::new(RichText::new("Add token").size(theme.typography.normal))
            .visuals(theme.button_visuals())
            .min_size(vec2(120.0, 26.0));

         add = ui.add(button).clicked();
      });

      if !add {
         return false;
      }

      let text = self.token_input.get(&address).cloned().unwrap_or_default();

      let Ok(token) = Address::from_str(text.trim()) else {
         return false;
      };

      let Some(grant) = profile.wallets.get_mut(&address) else {
         return false;
      };

      let TokenScope::Allowlist { tokens } = &mut grant.tokens else {
         return false;
      };

      tokens.push(TokenRef {
         chain,
         address: token,
      });

      self.token_input.insert(address, String::new());

      true
   }

   /// The NFT allowlist, which is the whole protection an NFT has: no price bounds it.
   fn nft_allowlist(
      &mut self,
      profile: &mut AgentProfile,
      address: Address,
      theme: &Theme,
      ui: &mut Ui,
   ) -> bool {
      let mut changed = false;

      let nfts = match profile.wallets.get(&address).map(|grant| grant.nfts.clone()) {
         Some(NftScope::Allowlist { nfts }) => nfts,
         _ => Vec::new(),
      };

      if nfts.is_empty() {
         ui.label(
            RichText::new("No collection is allowed yet, so no NFT may leave this wallet.")
               .size(theme.typography.normal),
         );
      }

      for nft in &nfts {
         let mut remove = false;

         ui.horizontal(|ui| {
            ui.label(
               RichText::new(format!(
                  "{} on {}",
                  short(nft.collection),
                  chain_name(nft.chain)
               ))
               .size(theme.typography.normal),
            );

            let button = Button::new(RichText::new("Remove").size(theme.typography.normal))
               .visuals(theme.button_visuals())
               .min_size(vec2(100.0, 26.0));

            remove = ui.add(button).clicked();
         });

         if remove
            && let Some(grant) = profile.wallets.get_mut(&address)
            && let NftScope::Allowlist { nfts } = &mut grant.nfts
         {
            nfts.retain(|entry| entry != nft);
            changed = true;
         }
      }

      let added = self.add_nft_row(profile, address, theme, ui);

      changed || added
   }

   fn add_nft_row(
      &mut self,
      profile: &mut AgentProfile,
      address: Address,
      theme: &Theme,
      ui: &mut Ui,
   ) -> bool {
      let mut chain = *self.nft_chain.entry(address).or_insert_with(|| ChainId::Ethereum.id());

      ui.horizontal(|ui| {
         chain_picker(ui, ("nft_chain", address), &mut chain);
      });

      self.nft_chain.insert(address, chain);

      let typed = self.nft_input.entry(address).or_default();
      let mut add = false;

      ui.horizontal(|ui| {
         let field = ui.add(
            egui::TextEdit::singleline(typed)
               .hint_text("Collection address")
               .desired_width(320.0),
         );

         ui.ctx().accesskit_node_builder(field.id, |node| {
            node.set_label("Collection address")
         });

         let button = Button::new(RichText::new("Add collection").size(theme.typography.normal))
            .visuals(theme.button_visuals())
            .min_size(vec2(140.0, 26.0));

         add = ui.add(button).clicked();
      });

      if !add {
         return false;
      }

      let text = self.nft_input.get(&address).cloned().unwrap_or_default();

      match Address::from_str(text.trim()) {
         Ok(collection) => {
            let Some(grant) = profile.wallets.get_mut(&address) else {
               return false;
            };

            let NftScope::Allowlist { nfts } = &mut grant.nfts else {
               return false;
            };

            nfts.push(NftRef {
               chain,
               collection,
               token_ids: None,
            });

            self.nft_input.insert(address, String::new());
            true
         }
         Err(_) => false,
      }
   }

   /// The global screens: one switch per capability, with the tier read out and the badge stated.
   fn screens(&mut self, profile: &mut AgentProfile, theme: &Theme, ui: &mut Ui) -> bool {
      let mut changed = false;
      let mut family = "";

      ui.add_space(theme.spacing.md);
      ui.label(RichText::new("Settings screens").size(theme.typography.large));

      for cap in CAPABILITIES.iter().filter(|entry| entry.scope == CapabilityScope::Global) {
         let next = cap.name.split('.').nth(1).unwrap_or(cap.name);

         if next != family {
            family = next;
            ui.add_space(theme.spacing.xs);
            ui.label(RichText::new(words(family)).size(theme.typography.normal));
         }

         let granted =
            profile.screens.get(cap.name).is_some_and(|grant| grant.is_granted(cap.verb()));

         let mut on = granted;
         let verb = match cap.verb() {
            CapabilityVerb::Read => "read",
            CapabilityVerb::Write => "write",
         };

         let text = format!(
            "{} · {} · tier {}",
            words(family),
            verb,
            cap.tier.level()
         );
         let response = ui.add(Checkbox::new(
            &mut on,
            RichText::new(text).size(theme.typography.normal),
         ));

         if let Some(sentence) = badge_sentence(cap.name) {
            ui.ctx()
               .accesskit_node_builder(response.id, |node| node.set_description(sentence));
            ui.label(RichText::new(sentence).size(theme.typography.normal));
         }

         if response.changed() && self.toggle(profile, cap.name, None, on) {
            changed = true;
         }
      }

      changed
   }

   /// Switch one capability, honouring the second press a tier 3 grant needs.
   fn toggle(
      &mut self,
      profile: &mut AgentProfile,
      name: &str,
      address: Option<Address>,
      on: bool,
   ) -> bool {
      let critical = capability(name).is_some_and(|entry| entry.tier == RiskTier::Critical);

      if on && critical && !self.pending_matches(name, address) {
         self.pending = Some(Pending {
            name: name.to_owned(),
            address,
         });

         return false;
      }

      apply(profile, name, address, on);
      true
   }

   fn pending_matches(&self, name: &str, address: Option<Address>) -> bool {
      self
         .pending
         .as_ref()
         .is_some_and(|pending| pending.name == name && pending.address == address)
   }
}

/// Write one switch into the grant the profile holds.
fn apply(profile: &mut AgentProfile, name: &str, address: Option<Address>, on: bool) {
   match address {
      Some(address) => {
         let grant = profile.wallets.entry(address).or_default();

         if on {
            grant.enabled = true;
            grant.caps.insert(name.to_owned());
         } else {
            grant.caps.remove(name);
         }
      }
      None => {
         let Some(entry) = capability(name) else {
            return;
         };

         let mut grant = profile.screens.get(name).copied().unwrap_or_default();

         match entry.verb() {
            CapabilityVerb::Read => grant.read = on,
            CapabilityVerb::Write => grant.write = on,
         }

         profile.screens.insert(name.to_owned(), grant);
      }
   }
}

fn token_choice(scope: &TokenScope) -> u8 {
   match scope {
      TokenScope::None => 0,
      TokenScope::All => 1,
      TokenScope::Allowlist { .. } => 2,
   }
}

fn from_token_choice(choice: u8, stored: &TokenScope) -> TokenScope {
   match choice {
      1 => TokenScope::All,
      2 => match stored {
         TokenScope::Allowlist { tokens } => TokenScope::Allowlist {
            tokens: tokens.clone(),
         },
         _ => TokenScope::Allowlist { tokens: Vec::new() },
      },
      _ => TokenScope::None,
   }
}

fn nft_choice(scope: &NftScope) -> u8 {
   match scope {
      NftScope::None => 0,
      NftScope::All => 1,
      NftScope::Allowlist { .. } => 2,
   }
}

fn from_nft_choice(choice: u8, stored: &NftScope) -> NftScope {
   match choice {
      1 => NftScope::All,
      2 => match stored {
         NftScope::Allowlist { nfts } => NftScope::Allowlist { nfts: nfts.clone() },
         _ => NftScope::Allowlist { nfts: Vec::new() },
      },
      _ => NftScope::None,
   }
}

/// A chain picker: the seven chains Zeus supports, by name.
fn chain_picker(ui: &mut Ui, salt: (&'static str, Address), chain: &mut u64) {
   egui::ComboBox::from_id_salt(salt)
      .selected_text(chain_name(*chain))
      .show_ui(ui, |ui| {
         for supported in ChainId::supported_chains() {
            ui.selectable_value(chain, supported.id(), supported.name());
         }
      });
}

fn chain_name(chain: u64) -> String {
   match ChainId::new(chain) {
      Ok(chain) => chain.name().to_owned(),
      Err(_) => format!("chain {chain}"),
   }
}

/// The badge sentence a capability states, when it carries one (D22).
fn badge_sentence(name: &str) -> Option<&'static str> {
   match name {
      "wallet.read.private" => {
         Some("Your agent's provider will see your private balances and note counts.")
      }
      "settings.unshield.write" => Some(
         "Switching to self-broadcast links your private funds to your public wallet and your IP.",
      ),
      "settings.networks.write" => {
         Some("The endpoints you add see every request this wallet makes.")
      }
      _ => None,
   }
}

/// The capability's own words: `wallet.read.private` is `Private`, `settings.general.read` is
/// `General read`.
fn words(name: &str) -> String {
   let trimmed = name
      .strip_prefix("wallet.read.")
      .or_else(|| name.strip_prefix("wallet.write."))
      .or_else(|| name.strip_prefix("settings."))
      .unwrap_or(name);

   let mut words = String::with_capacity(trimmed.len());

   for (index, ch) in trimmed.chars().enumerate() {
      match ch {
         '.' | '_' => words.push(' '),
         _ if index == 0 => words.extend(ch.to_uppercase()),
         _ => words.push(ch),
      }
   }

   words
}

fn short(address: Address) -> String {
   let text = format!("{address:#x}");
   format!("{}…{}", &text[..8], &text[text.len() - 6..])
}

#[cfg(test)]
mod tests {
   use super::*;
   use crate::core::agent::PrivacyBadge;

   fn profile() -> AgentProfile {
      AgentProfile::new("Test", [7u8; 32], 0)
   }

   #[test]
   fn a_tier_three_grant_needs_two_presses() {
      let address = Address::repeat_byte(1);
      let mut profile = profile();
      let mut editor = ProfileEditor::default();

      assert!(!editor.toggle(
         &mut profile,
         "wallet.write.delegate",
         Some(address),
         true
      ));
      assert!(!profile.allows(address, "wallet.write.delegate"));

      assert!(editor.pending_matches("wallet.write.delegate", Some(address)));

      apply(
         &mut profile,
         "wallet.write.delegate",
         Some(address),
         true,
      );
      assert!(profile.allows(address, "wallet.write.delegate"));
   }

   #[test]
   fn a_lower_tier_grant_is_applied_at_once() {
      let address = Address::repeat_byte(2);
      let mut profile = profile();
      let mut editor = ProfileEditor::default();

      assert!(editor.toggle(
         &mut profile,
         "wallet.write.send",
         Some(address),
         true
      ));
      assert!(profile.allows(address, "wallet.write.send"));
      assert!(editor.pending.is_none());
   }

   #[test]
   fn a_screen_grant_lands_on_the_switch_its_verb_names() {
      let mut profile = profile();
      let mut editor = ProfileEditor::default();

      assert!(editor.toggle(&mut profile, "settings.networks.read", None, true));
      assert!(profile.allows_global("settings.networks.read"));
      assert!(!profile.allows_global("settings.networks.write"));

      // The write switch is tier 3, so it is the one that waits.
      assert!(!editor.toggle(
         &mut profile,
         "settings.networks.write",
         None,
         true
      ));
      assert!(!profile.allows_global("settings.networks.write"));
   }

   #[test]
   fn every_badged_capability_states_its_badge() {
      for cap in CAPABILITIES {
         if cap.badge.is_some() {
            assert!(
               badge_sentence(cap.name).is_some(),
               "{} has no sentence",
               cap.name
            );
         }
      }

      assert_eq!(
         capability("settings.networks.write").map(|cap| cap.badge),
         Some(Some(PrivacyBadge::Reveals))
      );
   }

   #[test]
   fn a_capability_name_reads_as_words() {
      assert_eq!(words("wallet.write.nft_send"), "Nft send");
      assert_eq!(words("settings.general.read"), "General read");
   }

   #[test]
   fn a_shortened_address_keeps_its_ends() {
      let address = Address::repeat_byte(0xab);
      let text = short(address);
      assert!(text.starts_with("0xababab"));
      assert!(text.ends_with("ababab"));
   }

   #[test]
   fn a_scope_choice_round_trips() {
      let allowlist = TokenScope::Allowlist {
         tokens: vec![TokenRef {
            chain: 1,
            address: Address::ZERO,
         }],
      };

      assert_eq!(token_choice(&from_token_choice(1, &allowlist)), 1);
      assert_eq!(token_choice(&from_token_choice(0, &allowlist)), 0);
      assert_eq!(
         token_choice(&from_token_choice(2, &TokenScope::All)),
         2
      );
      assert_eq!(from_token_choice(2, &allowlist), allowlist);
   }
}
