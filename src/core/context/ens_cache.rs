//! ENS names resolved during this session, held in memory only.
//!
//! ENS names expire, and a *reverse* record is owned by the address rather than by the name it
//! points at — so it outlives the name. A persisted ENS label can therefore outlive the name and
//! keep labelling an address the name no longer belongs to, and every display path
//! (`gui::ui::tx::address` → the confirm window, tx history, notifications, approvals, ERC-7730
//! clear-signing) renders those labels as trusted names.
//!
//! Keeping them for the session only means a label cannot outlive the name by more than the session
//! it was resolved in: a name that expires, or is re-registered by someone else, is gone by the next
//! start. That is the whole of the expiry story — no stored timestamps, no revalidation sweep, and
//! nothing in `address_book.data` to migrate.
//!
//! Names that cannot expire — wallets, contacts, well-known contracts, and ERC-7730 / Sourcify
//! contract labels — stay in [`AddressBook`](super::address_book), which *is* persisted.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use zeus_eth::alloy_primitives::Address;

type NameMap = HashMap<(u64, Address), Arc<str>>;

/// A session-only map of `(chain, address) → ENS name`, never written to disk.
#[derive(Clone, Default)]
pub struct EnsCache(Arc<RwLock<NameMap>>);

impl EnsCache {
   pub fn new() -> Self {
      Self::default()
   }

   /// The ENS name resolved for `(chain, address)` this session.
   pub fn get(&self, chain: u64, address: Address) -> Option<Arc<str>> {
      if address.is_zero() {
         return None;
      }
      self.0.read().unwrap().get(&(chain, address)).cloned()
   }

   /// Remember a name resolved for `(chain, address)`.
   ///
   /// Refuses to overwrite, so a name the user entered themselves wins over a reverse lookup of the
   /// same address. Returns true if stored.
   pub fn insert(&self, chain: u64, address: Address, name: &str) -> bool {
      if address.is_zero() || name.trim().is_empty() {
         return false;
      }

      let mut map = self.0.write().unwrap();
      if map.contains_key(&(chain, address)) {
         return false;
      }

      map.insert((chain, address), Arc::from(name));
      true
   }

   /// Drop everything — session state must not survive a full data reset.
   pub fn clear(&self) {
      self.0.write().unwrap().clear();
   }
}

#[cfg(test)]
mod tests {
   use super::*;

   fn addr(byte: u8) -> Address {
      Address::repeat_byte(byte)
   }

   #[test]
   fn first_write_wins() {
      let cache = EnsCache::new();
      assert!(cache.insert(8453, addr(0x11), "jefflau.eth"));
      assert!(!cache.insert(8453, addr(0x11), "someone.else"));
      assert_eq!(
         cache.get(8453, addr(0x11)).as_deref(),
         Some("jefflau.eth")
      );
   }

   #[test]
   fn names_are_chain_specific() {
      let cache = EnsCache::new();
      cache.insert(8453, addr(0x11), "jefflau.eth");
      assert_eq!(
         cache.get(8453, addr(0x11)).as_deref(),
         Some("jefflau.eth")
      );
      assert_eq!(cache.get(1, addr(0x11)), None);
   }

   #[test]
   fn zero_address_is_never_named() {
      let cache = EnsCache::new();
      assert!(!cache.insert(1, Address::ZERO, "Hyperliquid Labs"));
      assert_eq!(cache.get(1, Address::ZERO), None);
   }

   #[test]
   fn blank_names_are_rejected() {
      let cache = EnsCache::new();
      assert!(!cache.insert(1, addr(0x22), ""));
      assert!(!cache.insert(1, addr(0x22), "   "));
      assert_eq!(cache.get(1, addr(0x22)), None);
   }

   #[test]
   fn clear_drops_everything() {
      let cache = EnsCache::new();
      cache.insert(8453, addr(0x11), "jefflau.eth");
      cache.clear();
      assert_eq!(cache.get(8453, addr(0x11)), None);
   }
}
