//! Fast address-name lookup, sealed with the vault's [`WalletStateKey`].
//!
//! Token names stay in [`super::CurrencyDB`] — this map is wallets, contacts,
//! well-known contracts, and names learned from ERC-7730 / Sourcify.

use crate::core::persisted::{PersistedFile, file_path};
use crate::core::types::Contact;
use crate::core::{WalletInfo, WalletStateKey, serde_hashmap};
use crate::utils::write_private_atomic;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::sync::{Arc, RwLock};
use zeus_eth::{alloy_primitives::Address, types::SUPPORTED_CHAINS, utils::address_book};

const ADDRESS_BOOK_AAD: &[u8] = b"zeus-address-book-v1";

type NameMap = HashMap<(u64, Address), Arc<str>>;

#[derive(Clone, Serialize, Deserialize)]
pub struct AddressBook {
   #[serde(with = "serde_hashmap")]
   names: NameMap,
   /// In-flight / already-attempted remote lookups (this session only).
   #[serde(skip)]
   pending: HashSet<(u64, Address)>,
   /// True after a successful load or first save.
   #[serde(skip)]
   persisted: bool,
}

impl Default for AddressBook {
   fn default() -> Self {
      Self {
         names: HashMap::new(),
         pending: HashSet::new(),
         persisted: false,
      }
   }
}

#[derive(Clone)]
pub struct AddressBookHandle(Arc<RwLock<AddressBook>>);

impl Default for AddressBookHandle {
   fn default() -> Self {
      Self::new(AddressBook::default())
   }
}

impl AddressBookHandle {
   pub fn new(inner: AddressBook) -> Self {
      Self(Arc::new(RwLock::new(inner)))
   }

   pub fn read<R>(&self, reader: impl FnOnce(&AddressBook) -> R) -> R {
      reader(&self.0.read().unwrap())
   }

   pub fn write<R>(&self, writer: impl FnOnce(&mut AddressBook) -> R) -> R {
      writer(&mut self.0.write().unwrap())
   }

   pub fn dir() -> Result<std::path::PathBuf, anyhow::Error> {
      file_path(PersistedFile::AddressBook)
   }

   pub fn exists() -> Result<bool, anyhow::Error> {
      Ok(Self::dir()?.exists())
   }

   pub fn is_persisted(&self) -> bool {
      self.read(|book| book.persisted)
   }

   pub fn get(&self, chain: u64, address: Address) -> Option<Arc<str>> {
      self.read(|book| book.names.get(&(chain, address)).cloned())
   }

   /// Insert a chain-agnostic identity (wallet or contact) on every supported chain.
   pub fn insert_identity(&self, address: Address, name: impl Into<Arc<str>>) {
      let name = name.into();
      self.write(|book| {
         for chain in SUPPORTED_CHAINS {
            book.names.insert((chain, address), name.clone());
         }
      });
   }

   pub fn remove_identity(&self, address: Address) {
      self.write(|book| {
         for chain in SUPPORTED_CHAINS {
            book.names.remove(&(chain, address));
         }
      });
   }

   /// Contract / registry name for a single chain. Does not overwrite an existing name.
   pub fn insert_contract(&self, chain: u64, address: Address, name: impl Into<Arc<str>>) -> bool {
      let name = name.into();
      self.write(|book| {
         book.pending.remove(&(chain, address));
         if book.names.contains_key(&(chain, address)) {
            return false;
         }
         book.names.insert((chain, address), name);
         true
      })
   }

   /// Returns true if this is the first request for `(chain, address)` this session.
   pub fn mark_pending(&self, chain: u64, address: Address) -> bool {
      self.write(|book| {
         if book.names.contains_key(&(chain, address)) {
            return false;
         }
         book.pending.insert((chain, address))
      })
   }

   pub fn load_from_file(key: &WalletStateKey) -> Result<Self, anyhow::Error> {
      let path = Self::dir()?;
      let sealed = std::fs::read(&path)?;
      let mut book: AddressBook = key.open_json(&sealed, ADDRESS_BOOK_AAD)?;
      book.persisted = true;
      Ok(Self::new(book))
   }

   pub fn save(&self, key: &WalletStateKey) -> Result<(), anyhow::Error> {
      let sealed = self.read(|book| key.seal_json(book, ADDRESS_BOOK_AAD))?;
      write_private_atomic(&Self::dir()?, &sealed)?;
      self.write(|book| book.persisted = true);
      Ok(())
   }

   /// Replace inner map (same `Arc`) after a file load.
   pub fn replace_from(&self, other: &AddressBookHandle) {
      other.read(|src| {
         self.write(|dst| {
            dst.names.clone_from(&src.names);
            dst.persisted = src.persisted;
         });
      });
   }

   pub fn seed_well_known(&self) {
      self.write(|book| {
         for ((chain, address), name) in well_known_entries() {
            book.names.entry((chain, address)).or_insert(name);
         }
      });
   }

   pub fn seed_wallets<'a>(&self, wallets: impl IntoIterator<Item = &'a WalletInfo>) {
      for wallet in wallets {
         self.insert_identity(wallet.address, wallet.name());
      }
   }

   pub fn seed_contacts<'a>(&self, contacts: impl IntoIterator<Item = &'a Contact>) {
      for contact in contacts {
         if let Ok(address) = Address::from_str(&contact.evm_address) {
            if !contact.name.is_empty() {
               self.insert_identity(address, contact.name.as_str());
            }
         }
      }
   }

   /// Drop deleted wallets, then upsert current names.
   pub fn apply_wallet_diff(&self, previous: &HashSet<Address>, current: &[WalletInfo]) {
      let current_addrs: HashSet<Address> = current.iter().map(|w| w.address).collect();
      for addr in previous.difference(&current_addrs) {
         self.remove_identity(*addr);
      }
      self.seed_wallets(current);
   }
}

pub fn well_known_entries() -> Vec<((u64, Address), Arc<str>)> {
   let mut entries = Vec::new();

   for chain in SUPPORTED_CHAINS {
      if let Ok(address) = address_book::railgun_smart_wallet(chain) {
         entries.push((
            (chain, address),
            Arc::from("Railgun Smart Wallet"),
         ));
      }
      if let Ok(address) = address_book::entry_point(chain) {
         entries.push(((chain, address), Arc::from("Entry Point")));
      }
      if let Ok(address) = address_book::permit2_contract(chain) {
         entries.push(((chain, address), Arc::from("Permit2")));
      }
      if let Ok(address) = address_book::uniswap_v4_pool_manager(chain) {
         entries.push((
            (chain, address),
            Arc::from("Uniswap V4 Pool Manager"),
         ));
      }
      if let Ok(address) = address_book::universal_router_v2(chain) {
         entries.push(((chain, address), Arc::from("Universal Router V2")));
      }
      if let Ok(address) = address_book::uniswap_v3_nft_position_manager(chain) {
         entries.push((
            (chain, address),
            Arc::from("Uniswap V3 NFT Position Manager"),
         ));
      }
      if let Ok(address) = address_book::uniswap_v4_nft_position_manager(chain) {
         entries.push((
            (chain, address),
            Arc::from("Uniswap V4 NFT Position Manager"),
         ));
      }
      if let Ok(address) = address_book::uniswap_v2_factory(chain) {
         entries.push(((chain, address), Arc::from("Uniswap V2 Factory")));
      }
      if let Ok(address) = address_book::uniswap_v2_router(chain) {
         entries.push(((chain, address), Arc::from("Uniswap V2 Router")));
      }
      if let Ok(address) = address_book::uniswap_v3_factory(chain) {
         entries.push(((chain, address), Arc::from("Uniswap V3 Factory")));
      }
      if let Ok(address) = address_book::pancakeswap_v2_factory(chain) {
         entries.push((
            (chain, address),
            Arc::from("PancakeSwap V2 Factory"),
         ));
      }
      if let Ok(address) = address_book::pancakeswap_v2_router(chain) {
         entries.push((
            (chain, address),
            Arc::from("PancakeSwap V2 Router"),
         ));
      }
      if let Ok(address) = address_book::pancakeswap_v3_factory(chain) {
         entries.push((
            (chain, address),
            Arc::from("PancakeSwap V3 Factory"),
         ));
      }
      if let Ok(address) = address_book::pancakeswap_v3_router(chain) {
         entries.push((
            (chain, address),
            Arc::from("PancakeSwap V3 Router"),
         ));
      }
      if let Ok(address) = address_book::across_spoke_pool_v2(chain) {
         entries.push((
            (chain, address),
            Arc::from("Across Spoke Pool V2"),
         ));
      }

      #[cfg(feature = "dev")]
      {
         entries.push((
            (chain, address_book::vitalik()),
            Arc::from("Vitalik"),
         ));
      }
   }

   entries
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn identity_is_visible_on_all_chains() {
      let book = AddressBookHandle::default();
      let addr = Address::repeat_byte(0x11);
      book.insert_identity(addr, "Alice");
      for chain in SUPPORTED_CHAINS {
         assert_eq!(book.get(chain, addr).as_deref(), Some("Alice"));
      }
   }

   #[test]
   fn contract_does_not_overwrite_identity() {
      let book = AddressBookHandle::default();
      let addr = Address::repeat_byte(0x22);
      book.insert_identity(addr, "Alice");
      assert!(!book.insert_contract(1, addr, "SomePool"));
      assert_eq!(book.get(1, addr).as_deref(), Some("Alice"));
   }

   #[test]
   fn pending_is_once_per_session() {
      let book = AddressBookHandle::default();
      let addr = Address::repeat_byte(0x33);
      assert!(book.mark_pending(1, addr));
      assert!(!book.mark_pending(1, addr));
   }
}
