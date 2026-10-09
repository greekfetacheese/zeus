//! Window that allows the user to select a contact or a wallet as the recipient of a transaction

use crate::assets::icons::Icons;
use crate::core::{
   WalletInfo, WalletValue, ZeusContext, ZeusCtx,
   types::{Contact, Recipient},
};
use crate::gui::SHARED_GUI;
use crate::gui::ui::common::switch_chain;
use crate::gui::ui::{ContactsUi, WalletListByValue};
use crate::utils::{RT, TimeStamp};
use eframe::egui::{
   Align, FontId, Id, Layout, Margin, Order, RichText, ScrollArea, Sense, Spinner, TextWrapMode,
   Ui, vec2,
};
use egui_elements::{Button, Label, Modal, SecureTextEdit, Theme, utils::frame as frame_fn};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use zeus_eth::alloy_primitives::Address;
use zeus_eth::types::{ChainId, ETH};
use zeus_eth::utils::{ens, interoperable_name};
use zeus_railgun::RailgunAddress;

/// Validated address entered in the search bar that is not already a
/// wallet/contact — shown as an "Unknown Address" option.
#[derive(Clone, Debug)]
enum UnknownRecipient {
   Evm(Address),
   /// ENS name resolved to an address (mainnet, onchain only).
   Ens {
      name: String,
      address: Address,
      /// The name's registration expiry. `None` for a name with no onchain expiry (non-`.eth`).
      expiry: Option<ens::NameExpiry>,
   },
   /// An ERC-7828 `<address>@<chain>`. The chain is part of what the user typed, so it is
   /// carried through to the send path instead of being assumed from the active chain.
   Interoperable {
      /// `None` for a chain-specific *raw* address (`0x…@eip155:1`).
      name: Option<String>,
      address: Address,
      chain: u64,
      /// The address came from the name's ENSIP-19 *default EVM chain* record rather than a
      /// record set for this exact chain — a weaker claim, shown as such.
      from_default_evm_record: bool,
      /// The name's registration expiry. `None` for a chain-specific *raw address*, or a name
      /// with no onchain expiry (non-`.eth`).
      expiry: Option<ens::NameExpiry>,
   },
   /// An ERC-7828 name whose `#<checksum>` did not match the address and chain. Shown as a
   /// warning and never selectable: the checksum is the one thing the user asked Zeus to check.
   ChecksumMismatch {
      expected: String,
      found: String,
   },
   Zk(String),
}

/// How long the search query must sit still before an ENS lookup is issued.
///
/// An address / 0zk parse is local and runs on every keystroke; an ENS lookup is an
/// RPC round-trip, so it waits for the typing to stop.
const ENS_LOOKUP_DEBOUNCE_MILLIS: u64 = 400;

pub struct RecipientSelectionWindow {
   open: bool,
   loading: bool,
   contacts_tab_open: bool,
   wallets_tab_open: bool,
   pub recipient: Recipient,
   search_query: String,
   /// Result of async search-bar address parsing (unknown recipient suggestion).
   unknown_recipient: Option<UnknownRecipient>,
   /// `search_query` the current `unknown_recipient` / in-flight parse is for.
   unknown_recipient_query: String,
   /// Privacy mode used for the current parse / cache entry.
   unknown_recipient_privacy: bool,
   /// True while a parse task is running for `unknown_recipient_query`.
   parsing_unknown_recipient: bool,
   /// Millis timestamp at which an ENS lookup for `unknown_recipient_query` may run;
   /// `0` when no lookup is waiting out the debounce window.
   ens_lookup_due_at: u64,
   /// Chain id an ERC-7828 suggestion resolved for that differs from the active chain, awaiting
   /// an explicit "switch and use" confirmation. `None` when no switch is pending.
   pending_chain_switch: Option<u64>,
   wallets: Vec<WalletInfo>,
   /// Wallet value by address
   wallet_value: HashMap<Address, WalletValue>,
   /// Chains that the wallet has balance on
   wallet_chains: HashMap<Address, Vec<u64>>,
   /// Inline add-contact form inside this window (not a nested Window).
   adding_contact: bool,
   size: (f32, f32),
}

impl RecipientSelectionWindow {
   pub fn new() -> Self {
      Self {
         open: false,
         loading: false,
         contacts_tab_open: true,
         wallets_tab_open: false,
         recipient: Recipient::default(),
         search_query: String::new(),
         unknown_recipient: None,
         unknown_recipient_query: String::new(),
         unknown_recipient_privacy: false,
         parsing_unknown_recipient: false,
         ens_lookup_due_at: 0,
         pending_chain_switch: None,
         wallets: Vec::new(),
         wallet_value: HashMap::new(),
         wallet_chains: HashMap::new(),
         adding_contact: false,
         size: (560.0, 550.0),
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self) {
      self.open = true;
      self.calc_wallet_value();
   }

   pub fn calc_wallet_value(&mut self) {
      self.loading = true;

      RT.spawn_blocking(move || {
         let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
         let list = WalletListByValue::collect(&ctx);

         SHARED_GUI.write(|gui| {
            gui.recipient_selection.loading = false;
            gui.recipient_selection.wallets = list.wallets;
            gui.recipient_selection.wallet_value = list.values;
            gui.recipient_selection.wallet_chains = list.chains;
         });
      });
   }

   pub fn close(&mut self) {
      self.search_query.clear();
      self.open = false;
      self.adding_contact = false;
   }

   pub fn reset(&mut self) {
      self.recipient = Recipient::default();
      self.search_query.clear();
      self.clear_unknown_recipient_cache();
      self.adding_contact = false;
   }

   fn clear_unknown_recipient_cache(&mut self) {
      self.unknown_recipient = None;
      self.unknown_recipient_query.clear();
      self.parsing_unknown_recipient = false;
      self.ens_lookup_due_at = 0;
      self.pending_chain_switch = None;
   }

   /// Kick off (or skip) background parsing when the search query / privacy mode changes.
   ///
   /// Returns how long to wait before calling this again while an ENS lookup is
   /// sitting out its debounce window; `None` when nothing is pending.
   fn update_unknown_recipient_parse(&mut self, privacy_mode: bool) -> Option<Duration> {
      if self.search_query.is_empty() {
         self.clear_unknown_recipient_cache();
         return None;
      }

      let query_changed = self.unknown_recipient_query != self.search_query;
      let privacy_changed = self.unknown_recipient_privacy != privacy_mode;

      if query_changed || privacy_changed {
         self.unknown_recipient = None;
         self.unknown_recipient_query = self.search_query.clone();
         self.unknown_recipient_privacy = privacy_mode;
         self.parsing_unknown_recipient = false;
         self.ens_lookup_due_at = 0;
         self.pending_chain_switch = None;
      } else if self.ens_lookup_due_at == 0 {
         // Already parsed (or already parsing) this exact query.
         return None;
      }

      // A name costs an RPC round-trip, so the lookup waits for the typing to stop.
      // Address / 0zk parsing stays immediate — it is local.
      if needs_name_lookup(&self.search_query, privacy_mode) {
         if self.ens_lookup_due_at == 0 {
            self.ens_lookup_due_at = TimeStamp::now_as_millis().unwrap_or_default().timestamp()
               + ENS_LOOKUP_DEBOUNCE_MILLIS;
         }

         let now = TimeStamp::now_as_millis().unwrap_or_default().timestamp();
         if now < self.ens_lookup_due_at {
            return Some(Duration::from_millis(
               self.ens_lookup_due_at - now,
            ));
         }
      }

      self.ens_lookup_due_at = 0;
      self.parsing_unknown_recipient = true;

      let query = self.search_query.clone();
      RT.spawn_blocking(move || {
         let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
         let result = parse_unknown_recipient(ctx, &query, privacy_mode);
         SHARED_GUI.write(|gui| {
            let sel = &mut gui.recipient_selection;
            // Drop stale results if the user kept typing / flipped privacy mode.
            if sel.unknown_recipient_query == query && sel.unknown_recipient_privacy == privacy_mode
            {
               sel.unknown_recipient = result;
               sel.parsing_unknown_recipient = false;
               gui.request_repaint();
            }
         });
      });

      None
   }

   pub fn get_recipient(&self) -> Recipient {
      self.recipient.clone()
   }

   /// Accept a chain-specific recipient, remembering the name we resolved against the address it
   /// resolved to.
   ///
   /// The confirm window and tx history only ever see `(chain, address)`, and a chain-specific name
   /// cannot be re-derived from those — the address's primary name may be a different name, and
   /// most have none at all. Without this the recipient shows up as a truncated address.
   fn accept_interoperable(
      &mut self,
      ctx: &mut ZeusContext,
      name: Option<String>,
      address: Address,
      chain: u64,
      takeover_at: Option<u64>,
   ) {
      if let Some(name) = name.as_deref() {
         // The display paths (`tx::address` → the confirm window, history, notifications) only ever
         // see `(chain, address)`, and a chain-specific name cannot be re-derived from those: the
         // address's primary name may be a different name (`jefflau.eth@base` resolves to an address
         // whose primary name is `jeff.eth`), and most have none at all. Remembering it is what
         // keeps the recipient from rendering as a truncated address. Session-only and now
         // expiry-aware: the cache drops the label when the name's binding lapses. See [`EnsCache`].
         ctx.ens_cache.insert(chain, address, name, takeover_at.unwrap_or(0));
      }

      self.recipient = Recipient::from_ens_name(name, address, Some(chain), takeover_at);
   }

   /// `send_chain` is the chain this flow will actually send the recipient to: the active chain
   /// for send / unshield, and the destination chain for a bridge. A chain-specific name
   /// (`name@chain`) that disagrees with it is never sent silently.
   pub fn show(
      &mut self,
      ctx: &mut ZeusContext,
      theme: &Theme,
      _icons: Arc<Icons>,
      privacy_mode: bool,
      send_chain: u64,
      contacts_ui: &mut ContactsUi,
      ui: &mut Ui,
   ) {
      let mut open = self.open;

      if !open {
         return;
      }

      let mut close_window = false;

      let contact_added = contacts_ui.add_contact.contact_added();

      if contact_added {
         let contact = contacts_ui.add_contact.get_contact().clone();
         self.recipient = Recipient::from_contact(contact);

         contacts_ui.add_contact.reset();
         self.close();
      }

      let frame = theme.window_frame.fill(theme.colors.bg);
      let title = RichText::new("Recipient").size(theme.typography.heading);
      let id = Id::new("recipient_selection_window");

      Modal::new(id, &mut open)
         .backdrop_order(Order::Middle)
         .content_order(Order::Foreground)
         .heading(title)
         .header_separator(false)
         .center_header(true)
         .closable(true)
         .frame(frame)
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);
            ui.spacing_mut().button_padding = theme.button_padding;
            let size = vec2(ui.available_width() * 0.4, 45.0);
            let button_visuals = theme.button_visuals();
            let text_edit_visuals = theme.text_edit_visuals();

            if self.adding_contact {
               let text = RichText::new("Back").size(theme.typography.normal);
               let button = Button::new(text).min_size(vec2(50.0, 20.0));
               let res = ui.scope(|ui| {
                  ui.spacing_mut().button_padding = theme.button_padding;
                  ui.add(button)
               });
               if res.inner.clicked() {
                  self.adding_contact = false;
                  contacts_ui.add_contact.reset();
               }
               ui.add_space(8.0);
               ui.vertical_centered(|ui| {
                  ui.label(RichText::new("Add contact").size(theme.typography.heading));
                  ui.add_space(10.0);
                  contacts_ui.add_contact.body(theme, false, ui);
               });
               return;
            }

            ui.vertical_centered(|ui| {
               ui.add_space(20.0);

               if self.loading {
                  ui.add(Spinner::new().size(17.0).color(theme.colors.text));
                  return;
               }

               let text = RichText::new("Add a contact").size(theme.typography.normal);
               let add_contact = Button::new(text).visuals(button_visuals);

               if ui.add(add_contact).clicked() {
                  self.adding_contact = true;
               }

               ui.add_space(15.0);

               let hint_text = match privacy_mode {
                  false => "Search contacts, ENS or enter an address",
                  true => "Search contacts or enter a zk address",
               };

               // Search bar
               let hint = RichText::new(hint_text)
                  .size(theme.typography.normal)
                  .color(theme.colors.text_muted);

               ui.add(
                  SecureTextEdit::singleline(&mut self.search_query)
                     .visuals(text_edit_visuals)
                     .hint_text(hint)
                     .min_size(vec2(ui.available_width() * 0.80, 25.0))
                     .margin(Margin::same(10))
                     .font(FontId::proportional(theme.typography.normal)),
               );

               ui.add_space(15.0);

               ui.allocate_ui(size, |ui| {
                  ui.horizontal(|ui| {
                     let contacts_text = RichText::new("Contacts").size(theme.typography.large);
                     let wallet_text = RichText::new("Wallets").size(theme.typography.large);

                     let contact_button = Button::selectable(self.contacts_tab_open, contacts_text)
                        .visuals(button_visuals);

                     if ui.add(contact_button).clicked() {
                        self.contacts_tab_open = true;
                        self.wallets_tab_open = false;
                     }

                     ui.add_space(10.0);

                     let wallet_button = Button::selectable(self.wallets_tab_open, wallet_text)
                        .visuals(button_visuals);

                     if ui.add(wallet_button).clicked() {
                        self.wallets_tab_open = true;
                        self.contacts_tab_open = false;
                     }
                  });
               });

               ui.add_space(15.0);

               if self.contacts_tab_open {
                  self.contacts_tab(ctx, theme, privacy_mode, &mut close_window, ui);
               }

               if self.wallets_tab_open {
                  self.wallets_tab(ctx, theme, privacy_mode, &mut close_window, ui);
               }

               // Address / ENS parse
               if let Some(repaint_in) = self.update_unknown_recipient_parse(privacy_mode) {
                  // An ENS lookup is waiting for the typing to stop: keep repainting
                  // until it is due, otherwise the debounce would only end on input.
                  ui.ctx().request_repaint_after(repaint_in);
               }

               if self.parsing_unknown_recipient {
                  ui.add(Spinner::new().size(17.0).color(theme.colors.text));
               } else if let Some(unknown) = self.unknown_recipient.clone() {
                  let now = TimeStamp::now_as_secs().unwrap_or_default().timestamp();
                  let heading = match &unknown {
                     UnknownRecipient::Ens { .. } => "ENS",
                     UnknownRecipient::Interoperable { .. } => "Chain-specific address",
                     UnknownRecipient::ChecksumMismatch { .. } => "Checksum mismatch",
                     _ => "Unknown Address",
                  };
                  ui.label(RichText::new(heading).size(theme.typography.large));

                  match unknown {
                     UnknownRecipient::Evm(address) => {
                        let address_text =
                           RichText::new(address.to_string()).size(theme.typography.normal);
                        let button = Button::new(address_text).visuals(button_visuals);

                        if ui.add(button).clicked() {
                           self.recipient = Recipient::from_unknown_evm_address(address);
                           close_window = true;
                        }
                     }
                     UnknownRecipient::Ens {
                        name,
                        address,
                        expiry,
                     } => {
                        // Past its grace period the name can be registered by anyone, so it no
                        // longer identifies this address: shown, but never selectable.
                        let trusted = expiry.is_none_or(|expiry| expiry.is_trusted(now));

                        if trusted {
                           let name_text = RichText::new(&name).size(theme.typography.normal);
                           let button = Button::new(name_text).visuals(button_visuals);

                           if ui.add(button).clicked() {
                              // A plain name is chain-agnostic: `chain` stays `None`, so the send
                              // path keeps behaving exactly as it does today.
                              self.recipient = Recipient::from_ens_name(
                                 Some(name),
                                 address,
                                 None,
                                 expiry.map(|expiry| expiry.takeover_at),
                              );
                              close_window = true;
                           }

                           if let Some(expiry) = expiry
                              && now >= expiry.expires_at
                           {
                              ui.label(grace_note(expiry, theme));
                           }
                        } else {
                           ui.label(
                              RichText::new(&name)
                                 .size(theme.typography.normal)
                                 .color(theme.colors.error),
                           );
                           ui.label(lapsed_name_note(expiry, theme));
                        }

                        // Show what the name resolves to: the address is what is sent.
                        let block_explorer = ctx.chain.block_explorer();
                        let link = format!(
                           "{}/address/{}",
                           block_explorer,
                           address.to_string()
                        );

                        let text = RichText::new(address.to_string())
                           .size(theme.typography.normal)
                           .color(theme.colors.info);

                        ui.hyperlink_to(text, link);
                     }
                     UnknownRecipient::Interoperable {
                        name,
                        address,
                        chain,
                        from_default_evm_record,
                        expiry,
                     } => {
                        // `None` when the chain is not one Zeus can send to.
                        let chain_id = ChainId::new(chain).ok();

                        // Past its grace period the name can be registered by anyone, so it no
                        // longer identifies this address: shown, but never selectable.
                        let trusted = expiry.is_none_or(|expiry| expiry.is_trusted(now));
                        let takeover_at = expiry.map(|expiry| expiry.takeover_at);

                        let label = match (&name, chain_id) {
                           (Some(name), Some(chain_id)) => {
                              format!("{} @ {}", name, chain_id.name())
                           }
                           (None, Some(chain_id)) => {
                              format!("{} on {}", address, chain_id.name())
                           }
                           (Some(name), None) => format!("{} @ chain {}", name, chain),
                           (None, None) => address.to_string(),
                        };

                        let label_color = match trusted {
                           true => theme.colors.text,
                           false => theme.colors.error,
                        };

                        let button = Button::new(
                           RichText::new(label).size(theme.typography.normal).color(label_color),
                        )
                        .visuals(button_visuals);

                        // Where this flow sends. Switching the *active* chain only fixes a
                        // mismatch when the flow sends on the active chain — a bridge sends on
                        // its own destination chain, which this window cannot change.
                        let can_switch_active_chain = send_chain == ctx.chain.id();

                        if ui.add_enabled(chain_id.is_some() && trusted, button).clicked() {
                           if chain == send_chain {
                              self.accept_interoperable(
                                 ctx,
                                 name.clone(),
                                 address,
                                 chain,
                                 takeover_at,
                              );
                              close_window = true;
                           } else if can_switch_active_chain {
                              // The chain in the name is authoritative for resolution, but
                              // changing the active chain is the user's call: a recipient on
                              // another chain is never sent silently.
                              self.pending_chain_switch = Some(chain);
                           } else {
                              // Select it with its chain and let the flow offer the fix; the
                              // flow's own guard keeps it from being sent on the wrong chain.
                              self.accept_interoperable(
                                 ctx,
                                 name.clone(),
                                 address,
                                 chain,
                                 takeover_at,
                              );
                              close_window = true;
                           }
                        }

                        if !trusted {
                           ui.label(lapsed_name_note(expiry, theme));
                        }

                        match chain_id {
                           Some(chain_id) => {
                              let link = format!(
                                 "{}/address/{}",
                                 chain_id.block_explorer(),
                                 address
                              );

                              let text = RichText::new(address.to_string())
                                 .size(theme.typography.normal)
                                 .color(theme.colors.info);

                              ui.hyperlink_to(text, link);

                              if from_default_evm_record {
                                 let note = RichText::new(format!(
                                    "{} has no {} address — using its default EVM address",
                                    name.as_deref().unwrap_or("this name"),
                                    chain_id.name()
                                 ))
                                 .size(theme.typography.normal)
                                 .color(theme.colors.text_muted);

                                 ui.label(note);
                              }

                              if let Some(expiry) = expiry
                                 && expiry.is_trusted(now)
                                 && now >= expiry.expires_at
                              {
                                 ui.label(grace_note(expiry, theme));
                              }

                              if self.pending_chain_switch == Some(chain) {
                                 let text = RichText::new(format!(
                                    "Switch to {} and use this recipient",
                                    chain_id.name()
                                 ))
                                 .size(theme.typography.normal);

                                 let button = Button::new(text).visuals(button_visuals);

                                 if ui.add(button).clicked() {
                                    switch_chain(ctx, chain_id);
                                    self.accept_interoperable(
                                       ctx,
                                       name.clone(),
                                       address,
                                       chain,
                                       takeover_at,
                                    );
                                    close_window = true;
                                 }
                              }
                           }
                           None => {
                              let note = RichText::new(format!(
                                 "Chain {} is not supported by Zeus",
                                 chain
                              ))
                              .size(theme.typography.normal)
                              .color(theme.colors.warning);

                              ui.label(note);
                           }
                        }
                     }
                     UnknownRecipient::ChecksumMismatch { expected, found } => {
                        let text = RichText::new(format!(
                           "The name claims #{} but the address and chain hash to #{}",
                           found, expected
                        ))
                        .size(theme.typography.normal)
                        .color(theme.colors.error);

                        ui.label(text);
                     }
                     UnknownRecipient::Zk(zk_address) => {
                        let address_text = RichText::new(&zk_address).size(theme.typography.normal);
                        let button = Button::new(address_text).visuals(button_visuals);

                        if ui.add(button).clicked() {
                           self.recipient = Recipient::from_unknown_zk_address(zk_address);
                           close_window = true;
                        }
                     }
                  }
               }
            });
         });

      if close_window || !open {
         if self.adding_contact {
            contacts_ui.add_contact.reset();
         }
         self.close();
      }
   }

   fn contacts_tab(
      &mut self,
      ctx: &mut ZeusContext,
      theme: &Theme,
      privacy_mode: bool,
      close_window: &mut bool,
      ui: &mut Ui,
   ) {
      let contacts = ctx.read_wallet_state(|ws| ws.contacts.clone());
      let are_valid_contacts = contacts
         .iter()
         .any(|c| valid_contact_search(c, privacy_mode, &self.search_query));

      ScrollArea::vertical()
         .id_salt("contact_tabs_scroll")
         .max_height(self.size.1)
         .max_width(ui.available_width())
         .content_margin(10)
         .show(ui, |ui| {
            if are_valid_contacts {
               self.show_contacts(ctx, theme, privacy_mode, close_window, ui);
            }
         });
   }

   fn show_contacts(
      &mut self,
      ctx: &mut ZeusContext,
      theme: &Theme,
      privacy_mode: bool,
      close_window: &mut bool,
      ui: &mut Ui,
   ) {
      let contacts = ctx.read_wallet_state(|ws| ws.contacts.clone());

      ui.spacing_mut().item_spacing = vec2(0.0, theme.spacing.md);
      ui.spacing_mut().button_padding = theme.button_padding;

      let mut frame = theme.frame1;
      let visuals = theme.visuals.frame1_visuals;

      for contact in &contacts {
         let valid_search = valid_contact_search(contact, privacy_mode, &self.search_query);

         let address = match privacy_mode {
            false => contact.evm_address.clone(),
            true => contact.zk_address_truncated(),
         };

         let address_full = match privacy_mode {
            false => contact.evm_address.clone(),
            true => contact.zk_address.clone(),
         };

         if valid_search {
            let res = frame_fn(&mut frame, visuals, ui, |ui| {
               ui.set_width(ui.available_width());
               let text = RichText::new(contact.name.clone())
                  .size(theme.typography.large)
                  .color(theme.colors.text);
               ui.horizontal(|ui| {
                  let label = Label::new(text, None).interactive(false);
                  ui.add(label);
               });

               ui.add_space(6.0);

               let address_text = RichText::new(&address)
                  .size(theme.typography.normal)
                  .color(theme.colors.text_muted);
               let button = Button::selectable(false, address_text);

               ui.horizontal(|ui| {
                  if ui.add(button).clicked() {
                     ui.ctx().copy_text(address_full.clone());
                  }
               });
            });

            if res.interact(Sense::click()).clicked() {
               self.recipient = Recipient::from_contact(contact.clone());
               *close_window = true;
            }
         }
      }
   }

   fn wallets_tab(
      &mut self,
      ctx: &mut ZeusContext,
      theme: &Theme,
      privacy_mode: bool,
      close_window: &mut bool,
      ui: &mut Ui,
   ) {
      let wallets = &self.wallets;
      let are_valid_wallets = !wallets.is_empty()
         && wallets.iter().any(|w| valid_wallet_search(w, privacy_mode, &self.search_query));

      ScrollArea::vertical()
         .id_salt("wallets_tabs_scroll")
         .max_height(self.size.1)
         .max_width(ui.available_width())
         .content_margin(10)
         .show(ui, |ui| {
            if are_valid_wallets {
               self.show_wallets(ctx, theme, privacy_mode, close_window, ui);
            }
         });
   }

   fn show_wallets(
      &mut self,
      _ctx: &mut ZeusContext,
      theme: &Theme,
      privacy_mode: bool,
      close_window: &mut bool,
      ui: &mut Ui,
   ) {
      ui.spacing_mut().item_spacing = vec2(0.0, theme.spacing.md);
      ui.spacing_mut().button_padding = theme.button_padding;

      let mut frame = theme.frame1;
      let visuals = theme.visuals.frame1_visuals;

      let wallets = &self.wallets;

      for wallet in wallets {
         let valid_search = valid_wallet_search(wallet, privacy_mode, &self.search_query);

         // Wallet value across all chains
         let value = self.wallet_value.get(&wallet.address).cloned().unwrap_or_default();

         let address = match privacy_mode {
            false => wallet.address.to_string(),
            true => wallet.zk_address_truncated(),
         };

         let address_full = match privacy_mode {
            false => wallet.address.to_string(),
            true => wallet.zk_address(),
         };

         let large = theme.typography.large;
         let normal = theme.typography.normal;

         if valid_search {
            let res = frame_fn(&mut frame, visuals, ui, |ui| {
               ui.set_width(ui.available_width());
               // Values take their intrinsic width on the right so they never wrap;
               // the name truncates in whatever is left.
               ui.horizontal(|ui| {
                  ui.spacing_mut().item_spacing.x = theme.spacing.sm;
                  ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                     let text = RichText::new(format!(
                        "Private ${:.10}",
                        value.private.abbreviated()
                     ))
                     .size(normal);
                     let label =
                        Label::new(text, None).wrap_mode(TextWrapMode::Extend).interactive(false);
                     ui.add(label);

                     ui.separator();

                     let text = RichText::new(format!(
                        "Public ${:.10}",
                        value.public.abbreviated()
                     ))
                     .size(normal);
                     let label =
                        Label::new(text, None).wrap_mode(TextWrapMode::Extend).interactive(false);
                     ui.add(label);

                     ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        let text = RichText::new(wallet.name_with_source())
                           .size(large)
                           .color(theme.colors.text);
                        let label = Label::new(text, None)
                           .wrap_mode(TextWrapMode::Truncate)
                           .fill_width(true)
                           .interactive(false);
                        ui.add(label);
                     });
                  });
               });

               ui.add_space(6.0);

               let address_text = RichText::new(&address)
                  .size(theme.typography.normal)
                  .color(theme.colors.text_muted);
               let button = Button::selectable(false, address_text);

               ui.horizontal(|ui| {
                  if ui.add(button).clicked() {
                     ui.ctx().copy_text(address_full.clone());
                  }
               });
            });

            if res.interact(Sense::click()).clicked() {
               self.recipient = Recipient::from_wallet_info(wallet.clone());
               *close_window = true;
            }
         }
      }
   }
}

/// Parse the search bar and return an "unknown recipient" suggestion when what was
/// entered is valid but not already a wallet or contact.
///
/// Runs on a blocking worker thread — `RailgunAddress::from_zk_address`, the ENS
/// lookup and `wallet_with_zk_address_exists` are all too expensive for the GUI frame.
fn parse_unknown_recipient(
   ctx: ZeusCtx,
   query: &str,
   privacy_mode: bool,
) -> Option<UnknownRecipient> {
   if query.is_empty() {
      return None;
   }

   if !privacy_mode {
      if let Ok(address) = Address::from_str(query) {
         if ctx.wallet_exists(address) || ctx.get_contact(&address.to_string()).is_some() {
            return None;
         }

         return Some(UnknownRecipient::Evm(address));
      }

      // `<address>@<chain>` (ERC-7828) is checked before the plain-name path: the `@` means a
      // name lookup would reject it anyway, and a chain-specific *raw* address would otherwise
      // never be checksum-verified.
      if interoperable_name::looks_like_interoperable_name(query) {
         return resolve_interoperable_recipient(&ctx, query);
      }

      // Not an address: it may be an ENS name.
      return resolve_ens_recipient(&ctx, query);
   }

   let zk_address = RailgunAddress::from_zk_address(query).ok()?;
   if ctx.wallet_with_zk_address_exists(&zk_address)
      || ctx.get_contact_by_zk_address(&zk_address.address).is_some()
   {
      return None;
   }
   Some(UnknownRecipient::Zk(zk_address.address))
}

/// Does `query` need a name round-trip?
///
/// Covers both the plain ENS path and ERC-7828 chain-specific names. Names are public-mode only:
/// in privacy mode the recipient is a `0zk` address and ENS has nothing to say about it.
fn needs_name_lookup(query: &str, privacy_mode: bool) -> bool {
   !privacy_mode
      && (ens::looks_like_name(query) || interoperable_name::looks_like_interoperable_name(query))
}

/// Forward ENS resolution for the search bar.
///
/// Mainnet only and onchain only: [`ens::NoOffchainGateway`] refuses ERC-3668
/// redirects, so a name that can only be answered offchain is reported as "not
/// found" instead of reaching a third party. Every failure ends as `None`, exactly
/// like "no such name" — the user is still typing, and a name that does not exist
/// yet is not worth a dialog.
fn resolve_ens_recipient(ctx: &ZeusCtx, query: &str) -> Option<UnknownRecipient> {
   if !ens::looks_like_name(query) || ctx.is_chain_disabled(ETH) {
      return None;
   }

   let name = ens::normalize_name(query).ok()?;

   let resolved = RT.block_on(async {
      let client = ctx.get_client(ETH).await?;
      ens::resolve_name_with_expiry(&client, &name)
         .await
         .map_err(|e| anyhow::anyhow!("{:?}", e))
   });

   match resolved {
      Ok(Some((address, expiry))) => Some(UnknownRecipient::Ens {
         name,
         address,
         expiry,
      }),
      Ok(None) => None,
      Err(e) => {
         tracing::error!("Could not resolve ENS {}", e);
         None
      }
   }
}

/// Forward resolution for an ERC-7828 chain-specific name or address (`name.eth@base`).
///
/// Mainnet only and onchain only, exactly like [`resolve_ens_recipient`]: the `on.eth` chain
/// registry and the name's per-chain records are both read through the mainnet client, and
/// [`interoperable_name::resolve`] never follows an offchain gateway.
///
/// Every failure ends as `None` — the user is still typing — except a checksum mismatch, which is
/// surfaced: the checksum is the one thing the user explicitly asked Zeus to verify.
fn resolve_interoperable_recipient(ctx: &ZeusCtx, query: &str) -> Option<UnknownRecipient> {
   if !interoperable_name::looks_like_interoperable_name(query) || ctx.is_chain_disabled(ETH) {
      return None;
   }

   let resolved = RT.block_on(async {
      let client = ctx.get_client(ETH).await?;
      interoperable_name::resolve(&client, query).await.map_err(anyhow::Error::new)
   });

   match resolved {
      Ok(Some(resolved)) => Some(UnknownRecipient::Interoperable {
         name: resolved.name,
         address: resolved.address,
         chain: resolved.chain_id,
         from_default_evm_record: resolved.from_default_evm_record,
         expiry: resolved.expiry,
      }),
      Ok(None) => None,
      Err(e) => {
         if let Some(interoperable_name::InteroperableNameError::ChecksumMismatch {
            expected,
            found,
         }) = e.downcast_ref::<interoperable_name::InteroperableNameError>()
         {
            return Some(UnknownRecipient::ChecksumMismatch {
               expected: interoperable_name::format_checksum(expected),
               found: interoperable_name::format_checksum(found),
            });
         }

         tracing::error!(
            "Could not resolve Interoperable Name {}: {}",
            query,
            e
         );
         None
      }
   }
}

/// Why a name whose grace period has run out cannot be used as a recipient.
fn lapsed_name_note(expiry: Option<ens::NameExpiry>, theme: &Theme) -> RichText {
   let text = match expiry {
      Some(expiry) => format!(
         "This name expired on {} and left its grace period on {} — it may now belong to someone \
          else, so it is not offered as a recipient. Enter its address instead.",
         TimeStamp::Seconds(expiry.expires_at).to_date_string(),
         TimeStamp::Seconds(expiry.takeover_at).to_date_string(),
      ),
      None => {
         "This name can no longer be trusted as a recipient. Enter its address instead.".to_string()
      }
   };

   RichText::new(text).size(theme.typography.normal).color(theme.colors.error)
}

/// An informational note for a name that has expired but is still inside its grace period: the
/// owner is frozen, so the binding still holds, but the reader is owed the date.
fn grace_note(expiry: ens::NameExpiry, theme: &Theme) -> RichText {
   RichText::new(format!(
      "Expired {} — the name can be released to the market from {}.",
      TimeStamp::Seconds(expiry.expires_at).to_date_string(),
      TimeStamp::Seconds(expiry.takeover_at).to_date_string(),
   ))
   .size(theme.typography.normal)
   .color(theme.colors.text_muted)
}

fn valid_contact_search(contact: &Contact, privacy_mode: bool, query: &str) -> bool {
   let query = query.to_lowercase();

   if query.is_empty() {
      return true;
   }

   if !privacy_mode {
      return contact.name.to_lowercase().contains(&query)
         || contact.evm_address.to_lowercase().contains(&query);
   } else {
      return contact.name.to_lowercase().contains(&query)
         || contact.zk_address.to_lowercase().contains(&query);
   }
}

fn valid_wallet_search(wallet: &WalletInfo, privacy_mode: bool, query: &str) -> bool {
   let query = query.to_lowercase();

   if query.is_empty() {
      return true;
   }

   if !privacy_mode {
      return wallet.name_with_source().to_lowercase().contains(&query)
         || wallet.address.to_string().to_lowercase().contains(&query);
   } else {
      return wallet.name_with_source().to_lowercase().contains(&query)
         || wallet.zk_address().to_string().to_lowercase().contains(&query);
   }
}
