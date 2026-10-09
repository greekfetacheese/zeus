//! ENS names resolved during this session, held in memory only.
//!
//! ENS `.eth` names expire — and, crucially, **an expired name keeps resolving to its last
//! records**: the registry owner, the resolver and the resolver's `addr` record all survive
//! expiry, so nothing onchain resets when a registration lapses. A name can therefore be
//! re-registered by someone else *while Zeus is still running*, and every label pointing at its
//! previous owner's address becomes a lie.
//!
//! A *reverse* record makes it worse: it is owned by the address rather than by the name it points
//! at, so it outlives the name outright. Every display path (`gui::ui::tx::address` → the confirm
//! window, tx history, notifications, approvals, ERC-7730 clear-signing) renders cached labels as
//! trusted names.
//!
//! So this cache is expiry-aware. Each entry carries the name's **takeover** time — the first
//! moment a third party can register it (`BaseRegistrar.nameExpires + grace`; during the 90-day
//! grace period nobody can register the name and its owner cannot even transfer it). An entry is
//! served only while `now < takeover_at`, so a label can never outlive the binding that made it
//! true by more than the registration itself. That is stronger than the previous session-only
//! guarantee, which only bounded the lie by the length of the session.
//!
//! Names that cannot expire — wallets, contacts, well-known contracts, and ERC-7730 / Sourcify
//! contract labels — stay in [`AddressBook`](super::address_book), which *is* persisted.

use crate::utils::TimeStamp;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use zeus_eth::alloy_primitives::Address;

/// A name resolved for an address, with the moment its binding stops being trustworthy.
#[derive(Clone)]
struct EnsEntry {
   name: Arc<str>,
   /// Unix seconds at which a third party can first take the name over (`expires_at + grace`).
   /// `0` means no onchain expiry is known (e.g. a non-`.eth` name) — such an entry never lapses,
   /// so behaviour is exactly the session-only one Zeus had before expiry awareness.
   takeover_at: u64,
}

type NameMap = HashMap<(u64, Address), EnsEntry>;

/// Now, in unix seconds. A clock that cannot be read reports `0`, which makes every entry look
/// unexpired: it is never expiry-aware code's job to fail closed on a broken wall clock.
fn now_secs() -> u64 {
   TimeStamp::now_as_secs().unwrap_or_default().timestamp()
}

/// Has a binding with this `takeover_at` lapsed by `now`?
fn is_lapsed(takeover_at: u64, now: u64) -> bool {
   takeover_at != 0 && now >= takeover_at
}

/// A session-only map of `(chain, address) → ENS name`, never written to disk.
#[derive(Clone, Default)]
pub struct EnsCache(Arc<RwLock<NameMap>>);

impl EnsCache {
   pub fn new() -> Self {
      Self::default()
   }

   /// The ENS name resolved for `(chain, address)`, while its binding is still trustworthy.
   ///
   /// A name whose registration lapsed past its grace period is treated as absent: after that
   /// point anyone can register it, so it no longer identifies the address.
   pub fn get(&self, chain: u64, address: Address) -> Option<Arc<str>> {
      if address.is_zero() {
         return None;
      }

      let entry = self.0.read().unwrap().get(&(chain, address))?.clone();

      if is_lapsed(entry.takeover_at, now_secs()) {
         return None;
      }

      Some(entry.name)
   }

   /// Remember a name resolved for `(chain, address)`.
   ///
   /// `takeover_at` is the name's takeover time (`0` = no onchain expiry known). A name that has
   /// *already* lapsed is refused — storing it would present a binding that no longer holds.
   ///
   /// Refuses to overwrite a live entry, so a name the user entered themselves wins over a reverse
   /// lookup of the same address. A *lapsed* entry is replaced: that is the whole point of expiry
   /// awareness, and the lapsed name it held is gone. Returns true if stored.
   pub fn insert(&self, chain: u64, address: Address, name: &str, takeover_at: u64) -> bool {
      if address.is_zero() || name.trim().is_empty() {
         return false;
      }

      let now = now_secs();

      if is_lapsed(takeover_at, now) {
         return false;
      }

      let mut map = self.0.write().unwrap();

      if let Some(existing) = map.get(&(chain, address))
         && !is_lapsed(existing.takeover_at, now)
      {
         return false;
      }

      map.insert(
         (chain, address),
         EnsEntry {
            name: Arc::from(name),
            takeover_at,
         },
      );

      true
   }

   /// Forget a name whose binding has lapsed, returning true if one was removed.
   ///
   /// This is what lets the reverse path self-heal: the moment a label's binding lapses the
   /// address must be re-resolved once, and until the lapsed entry is gone [`Self::insert`] would
   /// also refuse to replace it. Consuming the lapse here means a re-lookup happens exactly once,
   /// not on every frame.
   pub fn remove_lapsed(&self, chain: u64, address: Address) -> bool {
      let now = now_secs();

      let mut map = self.0.write().unwrap();

      let lapsed = map
         .get(&(chain, address))
         .is_some_and(|entry| is_lapsed(entry.takeover_at, now));

      if lapsed {
         map.remove(&(chain, address));
      }

      lapsed
   }

   /// Drop everything — session state must not survive a full data reset.
   pub fn clear(&self) {
      self.0.write().unwrap().clear();
   }
}

#[cfg(test)]
mod tests {
   use super::*;
   use std::time::Duration;

   fn addr(byte: u8) -> Address {
      Address::repeat_byte(byte)
   }

   #[test]
   fn first_write_wins_among_live_names() {
      let cache = EnsCache::new();
      assert!(cache.insert(8453, addr(0x11), "jefflau.eth", 0));
      assert!(!cache.insert(8453, addr(0x11), "someone.else", 0));
      assert_eq!(
         cache.get(8453, addr(0x11)).as_deref(),
         Some("jefflau.eth")
      );
   }

   #[test]
   fn names_are_chain_specific() {
      let cache = EnsCache::new();
      cache.insert(8453, addr(0x11), "jefflau.eth", 0);
      assert_eq!(
         cache.get(8453, addr(0x11)).as_deref(),
         Some("jefflau.eth")
      );
      assert_eq!(cache.get(1, addr(0x11)), None);
   }

   #[test]
   fn zero_address_is_never_named() {
      let cache = EnsCache::new();
      assert!(!cache.insert(1, Address::ZERO, "Hyperliquid Labs", 0));
      assert_eq!(cache.get(1, Address::ZERO), None);
   }

   #[test]
   fn blank_names_are_rejected() {
      let cache = EnsCache::new();
      assert!(!cache.insert(1, addr(0x22), "", 0));
      assert!(!cache.insert(1, addr(0x22), "   ", 0));
      assert_eq!(cache.get(1, addr(0x22)), None);
   }

   /// A name whose binding has already lapsed must never be stored.
   #[test]
   fn an_already_lapsed_name_is_refused() {
      let cache = EnsCache::new();
      let past = now_secs().saturating_sub(60);

      assert!(!cache.insert(1, addr(0x33), "gone.eth", past));
      assert_eq!(cache.get(1, addr(0x33)), None);
      assert!(!cache.remove_lapsed(1, addr(0x33)));
   }

   /// A name with no onchain expiry never lapses — the session-only behaviour is unchanged.
   #[test]
   fn unknown_expiry_never_lapses() {
      let cache = EnsCache::new();
      assert!(cache.insert(1, addr(0x44), "alice.xyz", 0));
      assert_eq!(
         cache.get(1, addr(0x44)).as_deref(),
         Some("alice.xyz")
      );
      assert!(!cache.remove_lapsed(1, addr(0x44)));
   }

   /// The security property: the moment a binding lapses the label disappears, the lapse is
   /// consumable exactly once (so the reverse path re-resolves it once), and the address is then
   /// free to take a different name.
   #[test]
   fn a_lapsed_name_disappears_and_can_be_replaced() {
      let cache = EnsCache::new();
      let takeover_at = now_secs() + 1;

      assert!(cache.insert(1, addr(0x55), "alice.eth", takeover_at));
      assert_eq!(
         cache.get(1, addr(0x55)).as_deref(),
         Some("alice.eth")
      );

      std::thread::sleep(Duration::from_millis(1_500));

      assert_eq!(cache.get(1, addr(0x55)), None);
      assert!(cache.remove_lapsed(1, addr(0x55)));
      // Consumed once: a re-lookup happens on the lapse, not on every frame.
      assert!(!cache.remove_lapsed(1, addr(0x55)));

      assert!(cache.insert(1, addr(0x55), "alice2.eth", 0));
      assert_eq!(
         cache.get(1, addr(0x55)).as_deref(),
         Some("alice2.eth")
      );
   }

   #[test]
   fn clear_drops_everything() {
      let cache = EnsCache::new();
      cache.insert(8453, addr(0x11), "jefflau.eth", 0);
      cache.clear();
      assert_eq!(cache.get(8453, addr(0x11)), None);
   }
}
