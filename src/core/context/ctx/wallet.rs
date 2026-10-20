use super::super::{AddressBookHandle, EnsCache, ZeusContext, ZeusCtx};
use crate::core::{WalletInfo, types::*};
use anyhow::anyhow;
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::sync::Arc;
use zeus_eth::{alloy_primitives::Address, utils::ens::ENS_CHAIN};
use zeus_railgun::RailgunAddress;
use zeus_wallet::Wallet;

/// Whether the reverse-lookup worker should proceed for `(chain, address)`.
///
/// Read-only on purpose: the claim that bounds an address to one lookup per session belongs to
/// [`ZeusContext::address_name_requested`], the accessor the UI consults. A worker that re-made that
/// claim here would reject the very request the accessor spawned. A lapsed session label has
/// already been forgotten by the caller, so it reads as unnamed and is allowed through.
pub(super) fn should_lookup_address_name(
   book: &AddressBookHandle,
   cache: &EnsCache,
   chain: u64,
   address: Address,
) -> bool {
   book.get(chain, address).is_none() && cache.get(chain, address).is_none()
}

impl ZeusCtx {
   pub fn master_wallet_address(&self) -> Address {
      self.read_vault(|vault| vault.master_wallet_address())
   }

   /// Get the wallet with the given address
   pub fn get_wallet(&self, address: Address) -> Option<Wallet> {
      self.read_vault(|vault| {
         vault
            .all_wallets()
            .into_iter()
            .find(|wallet| wallet.address() == address)
            .cloned()
      })
   }

   /// Is this wallet selected as the current wallet
   pub fn is_current_wallet(&self, address: Address) -> bool {
      self.read(|ctx| ctx.current_wallet.address == address)
   }

   /// Clone the current wallet (including key material) from the vault.
   pub fn get_current_wallet(&self) -> Wallet {
      self.read(|ctx| ctx.get_current_wallet())
   }

   pub fn current_wallet_info(&self) -> WalletInfo {
      self.read(|ctx| {
         if !ctx.vault_unlocked {
            return WalletInfo::default();
         }
         ctx.current_wallet_info()
      })
   }

   pub fn wallet_exists(&self, address: Address) -> bool {
      self.read_vault(|vault| vault.wallet_address_exists(address))
   }

   /// Build the wallet info cache for all the wallets currently
   /// in the vault.
   ///
   /// This should be called at the startup and whenever a wallet is added or removed.
   pub fn build_wallet_info_cache(&self) {
      let previous: HashSet<Address> =
         self.read(|ctx| ctx.wallet_info_cache.map.keys().copied().collect());

      let mut new_map = HashMap::new();
      let mut new_ordered = Vec::new();

      let wallets = self.read_vault(|vault| vault.clone_all_wallets());
      for wallet in wallets {
         let info = WalletInfo::from_wallet(&wallet, true);
         new_map.insert(wallet.address(), info.clone());
         new_ordered.push(info);
      }

      let book = self.address_book();
      if book.is_persisted() {
         book.apply_wallet_diff(&previous, &new_ordered);
         self.save_address_book();
      }

      let new_wallet_info_cache = WalletInfoCache::new(new_map, new_ordered);

      self.write(|ctx| {
         ctx.wallet_info_cache = new_wallet_info_cache;
      });
   }

   pub fn wallet_with_zk_address_exists(&self, zk_address: &RailgunAddress) -> bool {
      self.read(|ctx| {
         for wallet in ctx.wallet_info_cache.ordered_slice() {
            if wallet.zk_address_ref() == zk_address.address.as_str() {
               return true;
            }
         }

         false
      })
   }

   /// Get all wallets info without cloning the private key
   pub fn get_all_wallets_info(&self) -> Vec<WalletInfo> {
      self.read(|ctx| ctx.wallet_info_cache.ordered_vec.clone())
   }

   pub fn contacts(&self) -> Vec<Contact> {
      self.read_wallet_state(|ws| ws.contacts.clone())
   }

   pub fn remove_contact(&self, evm_address: &str) {
      self.write_wallet_state(|ws| {
         ws.contacts.retain(|c| c.evm_address != evm_address);
      });
      if let Ok(address) = Address::from_str(evm_address) {
         self.address_book().remove_identity(address);
         self.save_address_book();
      }
   }

   pub fn add_contact(&self, contact: Contact) -> Result<(), anyhow::Error> {
      if contact.name.is_empty() {
         return Err(anyhow!("Contact name cannot be empty"));
      }

      let contacts = self.contacts();

      // make sure name and address are unique
      if contacts.iter().any(|c| c.name == contact.name) {
         return Err(anyhow!(
            "Contact with name {} already exists",
            contact.name
         ));
      } else if contacts.iter().any(|c| c.evm_address == contact.evm_address) {
         return Err(anyhow!(
            "Contact with address {} already exists",
            contact.evm_address
         ));
      }

      if let Ok(address) = Address::from_str(&contact.evm_address) {
         self.address_book().insert_identity(address, contact.name.as_str());
         self.save_address_book();
      }

      self.write_wallet_state(|ws| {
         ws.contacts.push(contact);
      });
      Ok(())
   }

   /// Get a contact by it's address
   pub fn get_contact(&self, evm_address: &str) -> Option<Contact> {
      self.read(|ctx| ctx.get_contact_by_address(evm_address))
   }

   pub fn get_contact_by_zk_address(&self, zk_address: &str) -> Option<Contact> {
      let zk_address = zk_address.to_lowercase();
      self.read_wallet_state(|ws| {
         ws.contacts.iter().find(|c| c.zk_address.to_lowercase() == zk_address).cloned()
      })
   }

   /// Return the name of this address if its known
   pub fn get_address_name(&self, chain: u64, address: Address) -> Option<Arc<str>> {
      self.read(|ctx| ctx.get_address_name(chain, address))
   }

   /// Off-frame ERC-7730 / Sourcify / ENS fill. No-op if already named.
   ///
   /// A token or an NFT collection Zeus already has cached is named by [`Self::get_address_name`],
   /// which the UI reads first, so this never runs for one — it is the names that need a registry,
   /// Sourcify or ENS to learn.
   ///
   /// The once-per-session claim belongs to `address_name_requested` on the frame path — it is what
   /// spawned this call. Re-checking that claim here used to reject every request it spawned, so
   /// this only asks the read-only guard whether anything named the address in the meantime.
   ///
   /// Returns true if a new name was stored.
   pub async fn lookup_address_name(&self, chain: u64, address: Address) -> bool {
      if address.is_zero() {
         return false;
      }

      if self.get_address_name(chain, address).is_some() {
         return false;
      }

      // A reverse label whose binding has lapsed is forgotten and re-resolved once — after its
      // grace period the name may belong to someone else. Kept for direct callers; on the frame
      // path `address_name_requested` has already consumed the lapse.
      let cache = self.ens_cache();
      cache.remove_lapsed(chain, address);

      let book = self.address_book();

      if !should_lookup_address_name(&book, &cache, chain, address) {
         return false;
      }

      if self.read(|ctx| ctx.currency_db.get_erc20_token(chain, address).is_some()) {
         return false;
      }

      // Read the opt-in once: it gates the registry's remote fallback as well as Sourcify, while the
      // registry's embedded descriptors answer offline either way.
      let fetch_names = self.read(|ctx| ctx.misc_config.fetch_contract_names());

      let contract_name = if let Some(name) =
         crate::core::clear_signing::registry::resolve_contract_label(chain, address, fetch_names)
            .await
      {
         Some(name)
      } else if fetch_names {
         crate::core::clear_signing::sourcify::contract_name(chain, address).await
      } else {
         None
      };

      // A contract label is durable, so it belongs in the address book. ENS is only asked for when
      // there is no registry label, so a contract keeps the name it already has.
      if let Some(name) = contract_name {
         if name.trim().is_empty() {
            return false;
         }

         if !book.insert_contract(chain, address, name.as_str()) {
            return false;
         }

         self.save_address_book();
         return true;
      }

      // ENS reverse, asked for on mainnet (ENS only exists there). The name goes to the session
      // cache rather than the book: ENS names expire and a reverse record outlives the name it
      // points at, so a persisted label would outlive the name. The cache also refuses a name
      // whose binding has already lapsed. See [`EnsCache`].
      let Some((name, takeover_at)) = self.lookup_ens_name(address).await else {
         return false;
      };

      self.ens_cache().insert(chain, address, &name, takeover_at)
   }

   /// ENS reverse lookup for [`Self::lookup_address_name`], with the name's takeover time.
   ///
   /// Onchain only (`zeus_eth::utils::ens` refuses offchain redirects) and mainnet
   /// only. `None` covers every ordinary outcome: mainnet disabled, no RPC
   /// reachable, or the address simply has no primary name — none of them errors.
   ///
   /// The returned `u64` is the name's takeover time (`0` = no onchain expiry known, e.g. a
   /// non-`.eth` name). It bounds the label's life in [`EnsCache`]: a *reverse* record is owned by
   /// the address rather than the name, so it survives the name's expiry and would otherwise keep
   /// labelling the address forever.
   ///
   /// Fails closed: if the expiry cannot be read the name is **not** returned, so the address is
   /// left unlabelled rather than cached with an unbounded (`0`) life. A transient read failure
   /// costs the label, never the guarantee.
   async fn lookup_ens_name(&self, address: Address) -> Option<(String, u64)> {
      if self.is_chain_disabled(ENS_CHAIN) {
         return None;
      }

      let client = self.get_client(ENS_CHAIN).await.ok()?;

      let name = match zeus_eth::utils::ens::lookup_name(&client, &address).await {
         Ok(name) => name?,
         Err(e) => {
            tracing::error!("ENS reverse lookup failed {}", e);
            return None;
         }
      };

      let takeover_at = match zeus_eth::utils::ens::name_expiry(&client, &name).await {
         Ok(Some(expiry)) => expiry.takeover_at,
         // No onchain expiry to read (a non-`.eth` name): session-only, as before.
         Ok(None) => 0,
         // The expiry could not be read, so the label's life cannot be bounded. Fail closed:
         // do not label the address at all rather than cache a name that can never be
         // invalidated — an unbounded label is exactly what expiry awareness exists to prevent.
         Err(_e) => {
            #[cfg(feature = "dev")]
            tracing::error!(
               "ENS expiry lookup failed for {}, not labelling: {}",
               name,
               _e
            );
            return None;
         }
      };

      Some((name, takeover_at))
   }
}

impl ZeusContext {
   /// Return the name of this address if its known.
   ///
   /// Checks the address book, then the session ENS cache, then token metadata. No list scans.
   /// The zero address is never named (ERC-7730 placeholders must not label burns).
   ///
   /// The book comes first on purpose: a wallet, contact or contract label the user set (or that
   /// ERC-7730 gave us) outranks an ENS name resolved for the same address.
   pub fn get_address_name(&self, chain: u64, address: Address) -> Option<Arc<str>> {
      if address.is_zero() {
         return None;
      }

      if let Some(name) = self.address_book.get(chain, address) {
         return Some(name);
      }

      if let Some(name) = self.ens_cache.get(chain, address) {
         return Some(name);
      }

      if let Some(name) = self.currency_db.get_token_name(chain, address) {
         return Some(name);
      }

      // An NFT collection names itself: on-chain metadata, cached by the time a token of it was
      // discovered or added, so this needs no registry or Sourcify round trip. Last, because it is
      // self-reported and a book or ENS name outranks it. The confirmation window's "Contract
      // interaction" row reads this for the collection being called.
      let name = self.nft_db.get_collection(chain, address)?.name?;
      let name = name.trim();

      if name.is_empty() {
         return None;
      }

      Some(Arc::from(name))
   }

   /// Get the wallet info for the given zk address
   pub fn get_wallet_info_by_zk_address(&self, address: &str) -> Option<WalletInfo> {
      for wallet in self.wallet_info_cache.ordered_slice() {
         if wallet.zk_address_ref() == address {
            return Some(wallet.clone());
         }
      }
      None
   }

   /// Get the wallet name for the given address
   pub fn get_wallet_name(&self, address: Address) -> Option<String> {
      let wallet = self.wallet_info_cache.get(&address);
      wallet.map(|wallet| wallet.name().to_string())
   }

   pub fn current_wallet_info(&self) -> WalletInfo {
      let w = self.wallet_info_cache.get(&self.current_wallet.address).cloned();
      w.expect("Current Wallet Info should be in cache")
   }

   /// Get the wallet with the given address
   pub fn get_wallet(&self, address: Address) -> Option<Wallet> {
      self.read_vault(|vault| {
         vault
            .all_wallets()
            .into_iter()
            .find(|wallet| wallet.address() == address)
            .cloned()
      })
   }

   /// Clone the current wallet (including key material) from the vault.
   pub fn get_current_wallet(&self) -> Wallet {
      let w = self.get_wallet(self.current_wallet.address);
      w.expect("Current Wallet should be in cache")
   }

   /// Get all wallets info by this order:
   /// - Master wallet
   /// - Children wallets
   /// - Imported wallets
   pub fn all_wallets_info_ordered(&self) -> &[WalletInfo] {
      self.wallet_info_cache.ordered_slice()
   }

   /// Is this wallet selected as the current wallet
   pub fn is_current_wallet(&self, address: Address) -> bool {
      self.current_wallet.address == address
   }

   /// Get a contact by it's zk address
   pub fn get_contact_by_zk_address(&self, zk_address: &str) -> Option<Contact> {
      let zk_address = zk_address.to_lowercase();
      self
         .wallet_state
         .read(|ws| ws.contacts.iter().find(|c| c.zk_address.to_lowercase() == zk_address).cloned())
   }

   /// Get a contact by it's address
   pub fn get_contact_by_address(&self, evm_address: &str) -> Option<Contact> {
      let evm_address = evm_address.to_lowercase();
      self.wallet_state.read(|ws| {
         ws.contacts
            .iter()
            .find(|c| c.evm_address.to_lowercase() == evm_address)
            .cloned()
      })
   }

   /// Is a name lookup due for this address, or has one already been requested?
   ///
   /// `false` means a request is due, and this call has just claimed it. `true` means a name is
   /// already known, or the request was already made this session.
   ///
   /// This is the **single consumption point** for the once-per-session rule: the UI consults it
   /// every frame, so the claim has to be made here and not in the worker, which runs once per
   /// request. It is also where a lapsed session ENS label is forgotten, which is what makes the
   /// address requestable exactly once more.
   pub fn address_name_requested(&self, chain: u64, address: Address) -> bool {
      if address.is_zero() {
         return true;
      }

      // A known name needs no request: a wallet, contact or contract label, a live session ENS
      // label, or token / collection metadata.
      if self.get_address_name(chain, address).is_some() {
         return true;
      }

      // A reverse label whose binding has lapsed is forgotten here, exactly once, so the claim
      // below starts a fresh request for that address.
      let relapsed = self.ens_cache.remove_lapsed(chain, address);

      let first = self.address_book.mark_pending(chain, address);

      // Due on the frame that claims the address, and on the frame that consumes a lapsed binding.
      !(first || relapsed)
   }
}
