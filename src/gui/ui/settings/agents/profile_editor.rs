//! The grant editor: the switches that decide what an agent may do.
//!
//! The catalog is the list of switches, not a copy of it: a capability that exists is drawn here,
//! with the tier it carries and the sentence it needs, and one that does not is absent from both.
//! Every switch is the grant, so the page writes the grant and nothing else.
//!
//! Two screens share this editor because they are the same kind of thing -- a list of switches --
//! and differ only in what they apply to: one wallet's row, or the app as a whole. The tiers that
//! only read, or change state without moving value, sit under the switch that owns the wallet. The
//! tiers that move value are hard to undo, so they sit behind their own disclosure, under the one
//! sentence that says what turning them on means.

use super::CONTROL;
use crate::core::WalletInfo;
use crate::core::agent::{
   AgentProfile, CAPABILITIES, Capability, CapabilityScope, CapabilityVerb, MAX_PRICE_IMPACT_BPS,
   MAX_TRANSFER_SHORTFALL_BPS, NftRef, NftScope, ProfileMode, RiskTier, TokenRef, TokenScope,
   capability,
};
use crate::gui::ui::common::{describe, q_mark};
use egui::{Align, CollapsingHeader, FontId, RichText, Ui, vec2};
use egui_elements::{Button, Checkbox, Label, SecureTextEdit, Theme};
use std::collections::HashMap;
use std::str::FromStr;
use zeus_eth::alloy_primitives::Address;
use zeus_eth::types::ChainId;

/// What the price-impact ceiling is for, in one sentence a reader can act on.
const PRICE_IMPACT_TIP: &str = "How much worse a trade may get than the price it was quoted at. \
A swap that would cost you more than this is refused.";

/// What the shortfall ceiling is for.
const SHORTFALL_TIP: &str = "How much of what the agent promised may fail to arrive. A swap, send or \
bridge that delivers less than this is refused.";

/// What the two moving sections hold, in words rather than tier numbers.
const MOVE_TIP: &str = "Sending and trading move money out of this wallet. Delegating lets the agent \
hand control of this wallet to someone else, and cannot be undone by Zeus.";

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

   /// The order the agent acts in, which belongs to no one wallet and so sits above the table.
   ///
   /// Ask-before-every-action and the two ceilings are properties of the agent, not of a wallet, so
   /// they are drawn on the Wallets screen; the per-wallet switches are one wallet's dialog.
   pub fn mode_and_limits(
      &mut self,
      profile: &mut AgentProfile,
      theme: &Theme,
      ui: &mut Ui,
   ) -> Edited {
      let mut edited = Edited::default();

      if self.mode(profile, theme, ui) {
         edited.changed = true;
      }

      if self.guards(profile, theme, ui) {
         edited.changed = true;
      }

      edited
   }

   /// One wallet's whole grant, which is what its dialog holds.
   pub fn wallet(
      &mut self,
      profile: &mut AgentProfile,
      wallet: &WalletInfo,
      theme: &Theme,
      ui: &mut Ui,
   ) -> Edited {
      let mut edited = Edited::default();

      if let Some(sentence) = self.pending_block(profile, theme, ui) {
         edited.notice = Some(sentence);
         edited.changed = true;
      }

      if self.wallet_block(profile, wallet, theme, ui) {
         edited.changed = true;
      }

      edited
   }

   /// The Settings screen: one switch per screen and verb, grouped by screen, with the reason a
   /// consequential one reads the way it does printed under it.
   pub fn screens(&mut self, profile: &mut AgentProfile, theme: &Theme, ui: &mut Ui) -> Edited {
      let mut edited = Edited::default();

      let normal = theme.typography.normal;
      let small = theme.typography.small;

      let intro = "What this agent may read and change in the app itself. These are not tied to a wallet, \
          so a write here is a write for every wallet.";
      let text = RichText::new(intro).size(normal);
      let label = Label::text(text).wrap();
      ui.add(label);

      if let Some(sentence) = self.pending_block(profile, theme, ui) {
         edited.notice = Some(sentence);
         edited.changed = true;
      }

      let mut family = "";

      for cap in CAPABILITIES.iter().filter(|entry| entry.scope == CapabilityScope::Global) {
         let next = cap.name.split('.').nth(1).unwrap_or(cap.name);

         if next != family {
            family = next;

            ui.add_space(theme.spacing.md);
            ui.label(RichText::new(words(family)).size(theme.typography.large));
         }

         let granted =
            profile.screens.get(cap.name).is_some_and(|grant| grant.is_granted(cap.verb()));

         let mut on = granted;

         let text = format!("{} · {}", words(family), verb(cap.verb()));

         let response = ui.add(Checkbox::new(
            &mut on,
            RichText::new(&text).size(theme.typography.normal),
         ));

         // The verb says what a switch does; the tier says how far it reaches, which is the part a
         // reader needs and a sighted user gets from the sentence under a consequential one.
         describe(
            ui,
            &response,
            format!("{text} · tier {}", cap.tier.level()),
         );

         if let Some(sentence) = badge_sentence(cap.name) {
            ui.ctx()
               .accesskit_node_builder(response.id, |node| node.set_description(sentence));

            let text = RichText::new(sentence).size(small);
            let label = Label::text(text).wrap();
            ui.add(label);
         }

         if response.changed() && self.toggle(profile, cap.name, None, on) {
            edited.changed = true;
         }
      }

      edited
   }

   /// How the agent acts: on its grant, or asking Zeus's own confirmation window every time.
   fn mode(&mut self, profile: &mut AgentProfile, theme: &Theme, ui: &mut Ui) -> bool {
      let mut confirm_each = profile.mode == ProfileMode::ConfirmEach;

      let sentence = "When on, a granted action opens Zeus's own confirmation window instead of running on its own.";

      let row = ui.add(Checkbox::new(
         &mut confirm_each,
         RichText::new("Ask before every action").size(theme.typography.normal),
      ));

      ui.ctx().accesskit_node_builder(row.id, |node| {
         node.set_description(sentence);
      });

      let text = RichText::new(sentence).size(theme.typography.small);
      let label = Label::text(text).wrap();
      ui.add(label);

      if !row.changed() {
         return false;
      }

      profile.mode = match confirm_each {
         true => ProfileMode::ConfirmEach,
         false => ProfileMode::Authorized,
      };

      true
   }

   /// The two ceilings an autonomous action is held to, in percent.
   ///
   /// A swap's price impact is measured against the quote the agent was shown; everything else is
   /// measured as how much of what it promised actually arrives. Both only bound an agent that acts on
   /// its grant -- one that asks before every action is reviewed by the person answering it -- so they
   /// sit behind their own disclosure rather than at the top of the screen.
   fn guards(&mut self, profile: &mut AgentProfile, theme: &Theme, ui: &mut Ui) -> bool {
      use super::percent_slider;

      let mut changed = false;

      ui.add_space(theme.spacing.sm);

      CollapsingHeader::new(
         RichText::new("How far it may go on its own").size(theme.typography.large),
      )
      .id_salt("agent_limits")
      .show(ui, |ui| {
         let text = "These apply when the agent acts without asking you first. Turn on Ask before every \
            action above to see each one before it happens instead.";
         let label = Label::text(RichText::new(text).size(theme.typography.normal)).wrap();
         ui.add(label);

         ui.add_space(theme.spacing.sm);

         changed |= percent_slider(
            ui,
            theme,
            "Max price impact",
            PRICE_IMPACT_TIP,
            &mut profile.max_price_impact_bps,
            MAX_PRICE_IMPACT_BPS,
         );

         ui.add_space(theme.spacing.xs);

         changed |= percent_slider(
            ui,
            theme,
            "Max shortfall",
            SHORTFALL_TIP,
            &mut profile.max_transfer_shortfall_bps,
            MAX_TRANSFER_SHORTFALL_BPS,
         );
      });

      if changed {
         profile.clamp();
      }

      changed
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

      let text = RichText::new(sentence).size(theme.typography.normal);
      let label = Label::text(text).wrap();
      ui.add(label);

      let mut notice = None;

      ui.horizontal(|ui| {
         let grant = Button::new(RichText::new("Grant it").size(theme.typography.normal))
            .min_size(vec2(140.0, CONTROL))
            .label_align(Align::Center);

         if ui.add(grant).clicked() {
            apply(profile, &name, address, true);
            notice = Some(format!("Granted {}.", words(&name)));
         }

         let cancel = Button::new(RichText::new("Cancel").size(theme.typography.normal))
            .min_size(vec2(140.0, CONTROL))
            .label_align(Align::Center);

         if ui.add(cancel).clicked() {
            notice = Some("Left ungranted.".to_owned());
         }
      });

      if notice.is_some() {
         self.pending = None;
      }

      notice
   }

   /// One wallet's slice: its kill switch, what it may read, what it may do, and what may leave it.
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

      {
         ui.spacing_mut().item_spacing = vec2(theme.spacing.sm, theme.spacing.xs);

         let mut allow = enabled;
         let row = ui.add(Checkbox::new(
            &mut allow,
            RichText::new("Allow this wallet").size(theme.typography.normal),
         ));

         let sentence = "Off means every switch below is ignored for this wallet.";
         ui.ctx().accesskit_node_builder(row.id, |node| {
            node.set_description(sentence);
         });

         let text = RichText::new(sentence).size(theme.typography.small);
         let label = Label::text(text).wrap();
         ui.add(label);

         if row.changed() {
            let grant = profile.wallets.entry(address).or_default();
            grant.enabled = allow;
            changed = true;
         }

         ui.add_enabled_ui(enabled, |ui| {
            if self.capabilities(
               profile,
               address,
               RiskTier::Read,
               "What it may read",
               theme,
               ui,
            ) {
               changed = true;
            }

            if self.capabilities(
               profile,
               address,
               RiskTier::Low,
               "What it may do",
               theme,
               ui,
            ) {
               changed = true;
            }

            // The tiers that move value are behind their own disclosure, under the one sentence
            // that says what turning one on means.
            let mut risky = false;

            CollapsingHeader::new(RichText::new("Moving money").size(theme.typography.large))
               .id_salt(("agent_wallet_risky", address))
               .show(ui, |ui| {
                  // The chip sits on the section's own line, right after the sentence it explains:
                  // what the two groups mean is the part a user cannot guess from their names.
                  ui.horizontal(|ui| {
                     ui.spacing_mut().item_spacing.x = theme.spacing.xs;

                     let heading = RichText::new("These move funds out of this wallet.")
                        .size(theme.typography.normal);
                     let sentence = ui.add(Label::text(heading));

                     ui.ctx().accesskit_node_builder(sentence.id, |node| {
                        node.set_description(MOVE_TIP);
                     });

                     q_mark(ui, theme, MOVE_TIP);
                  });

                  let hint =
                     "Each one runs as soon as the agent asks, unless Ask before every action is on.";
                  let text = RichText::new(hint).size(theme.typography.small);
                  let label = Label::text(text).wrap();
                  ui.add(label);

                  if self.capabilities(
                     profile,
                     address,
                     RiskTier::Money,
                     "Out of this wallet",
                     theme,
                     ui,
                  ) {
                     risky = true;
                  }

                  if self.capabilities(
                     profile,
                     address,
                     RiskTier::Critical,
                     "Hands over control",
                     theme,
                     ui,
                  ) {
                     risky = true;
                  }

                  if self.movable(profile, address, theme, ui) {
                     risky = true;
                  }
               });

            changed |= risky;
         });
      }

      changed
   }

   /// Every switch of one tier on one wallet, as a vertical list.
   ///
   /// A checkbox per row rather than three to a line: the name and the badge sentence are each too
   /// long to share a line, and a grid of them is what made the old page unreadable.
   fn capabilities(
      &mut self,
      profile: &mut AgentProfile,
      address: Address,
      tier: RiskTier,
      heading: &'static str,
      theme: &Theme,
      ui: &mut Ui,
   ) -> bool {
      let caps: Vec<&Capability> = CAPABILITIES
         .iter()
         .filter(|entry| entry.scope == CapabilityScope::Wallet && entry.tier == tier)
         .collect();

      if caps.is_empty() {
         return false;
      }

      let mut changed = false;

      ui.add_space(theme.spacing.sm);
      ui.label(RichText::new(heading).size(theme.typography.large));

      for cap in caps {
         let granted =
            profile.wallets.get(&address).is_some_and(|grant| grant.caps.contains(cap.name));

         let mut on = granted;

         let text = words(cap.name);

         // The sentence a capability carries is what it exposes, and a line under every switch is how
         // the page became a wall of prose: it rides on the question mark beside the switch. The two
         // are one row, because every direct child of a vertical `Ui` gets a row of its own — the
         // chip alone on a line under the switch was that rule, not a spacing problem.
         let response = ui
            .horizontal(|ui| {
               ui.spacing_mut().item_spacing.x = theme.spacing.xs;

               let response = ui.add(Checkbox::new(
                  &mut on,
                  RichText::new(&text).size(theme.typography.normal),
               ));

               match badge_sentence(cap.name) {
                  Some(sentence) => {
                     ui.ctx()
                        .accesskit_node_builder(response.id, |node| node.set_description(sentence));

                     q_mark(ui, theme, sentence);
                  }
                  None => describe(
                     ui,
                     &response,
                     format!("{text} · tier {}", cap.tier.level()),
                  ),
               }

               response
            })
            .inner;

         if response.changed() && self.toggle(profile, cap.name, Some(address), on) {
            changed = true;
         }
      }

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

      ui.add_space(theme.spacing.md);

      let tokens = profile
         .wallets
         .get(&address)
         .map(|grant| grant.tokens.clone())
         .unwrap_or_default();

      let choice = token_choice(&tokens);
      let mut picked = choice;

      ui.horizontal(|ui| {
         ui.add(Label::text(RichText::new("Tokens").size(theme.typography.normal)).wrap());

         // Every scope says what it belongs to, because three radios named Nothing, Everything and
         // Only these are the same words for tokens and for NFTs.
         ui.radio_value(&mut picked, 0, "No token");
         ui.radio_value(&mut picked, 1, "Every token");
         ui.radio_value(&mut picked, 2, "Only these tokens");
      });

      if picked != choice {
         let grant = profile.wallets.entry(address).or_default();
         grant.tokens = from_token_choice(picked, &grant.tokens);
         changed = true;
      }

      if picked == 2 && self.token_allowlist(profile, address, theme, ui) {
         changed = true;
      }

      if matches!(
         profile.wallets.get(&address).map(|grant| grant.tokens.clone()),
         Some(TokenScope::All)
      ) {
         let text =
            RichText::new("Every token in this wallet may leave it.").size(theme.typography.small);
         let label = Label::text(text).wrap();
         ui.add(label);
      }

      let nfts = profile
         .wallets
         .get(&address)
         .map(|grant| grant.nfts.clone())
         .unwrap_or_default();

      let choice = nft_choice(&nfts);
      let mut picked = choice;

      ui.add_space(theme.spacing.sm);

      ui.horizontal(|ui| {
         ui.add(Label::text(RichText::new("NFTs").size(theme.typography.normal)).wrap());

         ui.radio_value(&mut picked, 0, "No NFT");
         ui.radio_value(&mut picked, 1, "Every NFT");
         ui.radio_value(&mut picked, 2, "Only these collections");
      });

      if picked != choice {
         let grant = profile.wallets.entry(address).or_default();
         grant.nfts = from_nft_choice(picked, &grant.nfts);
         changed = true;
      }

      if picked == 2 && self.nft_allowlist(profile, address, theme, ui) {
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
            RichText::new("No token is allowed yet, so nothing may leave this wallet.")
               .size(theme.typography.small),
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
               .min_size(vec2(100.0, CONTROL))
               .label_align(Align::Center);

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
         ui.add(
            SecureTextEdit::singleline(typed)
               .accessible_name("Token address")
               .font(FontId::proportional(theme.typography.normal))
               .desired_width(320.0),
         );

         let button = Button::new(RichText::new("Add token").size(theme.typography.normal))
            .min_size(vec2(120.0, CONTROL))
            .label_align(Align::Center);

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
               .size(theme.typography.small),
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
               .min_size(vec2(100.0, CONTROL))
               .label_align(Align::Center);

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
         ui.label(RichText::new("Chain").size(theme.typography.normal));
         chain_picker(ui, ("nft_chain", address), &mut chain);
      });

      self.nft_chain.insert(address, chain);

      let typed = self.nft_input.entry(address).or_default();
      let mut add = false;

      ui.horizontal(|ui| {
         ui.add(
            SecureTextEdit::singleline(typed)
               .accessible_name("Collection address")
               .font(FontId::proportional(theme.typography.normal))
               .desired_width(320.0),
         );

         let button = Button::new(RichText::new("Add collection").size(theme.typography.normal))
            .min_size(vec2(140.0, CONTROL))
            .label_align(Align::Center);

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

fn verb(verb: CapabilityVerb) -> &'static str {
   match verb {
      CapabilityVerb::Read => "read",
      CapabilityVerb::Write => "write",
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

/// A chain picker: the chains Zeus supports, by name.
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

/// The badge sentence a capability states, when it carries one.
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
