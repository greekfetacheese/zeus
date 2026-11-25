//! The sealed store for every agent profile on this machine: `data/agent_profiles.data`.
//!
//! Shaped like `crate::context::AddressBookHandle`: a `RwLockHandle` with the lock kept private, its own
//! AAD, `write_private_atomic` on the way out, and a load failure that blocks the next save rather than
//! let an in-memory default replace bytes that did not read.
//!
//! The store is the only writer of a grant document, so the sanitising and clamping [`AgentProfile`] owns
//! happen here once instead of at every caller — a file can only enter memory through [`Self::decode`] and
//! can only leave it through [`Self::save`].

use super::profile::{AgentProfile, AgentProfileId};
use crate::WalletStateKey;
use crate::context::RwLockHandle;
use crate::persisted::{PersistedFile, ensure_data_dir, file_path};
use crate::platform::fs::write_private_atomic;
use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use zeus_eth::alloy_primitives::Address;

/// Its own AAD: a shared one is a silent mismatched read across stores.
const AGENT_PROFILES_AAD: &[u8] = b"zeus-agent-profiles-v1";

/// What the file holds.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct AgentProfiles {
   /// Keyed by id, so the on-disk order is stable without a sort.
   #[serde(default)]
   profiles: BTreeMap<AgentProfileId, AgentProfile>,
   /// Why a save would replace bytes that did not read. Never persisted: it describes this process's
   /// read of the file, and the next successful load clears it.
   #[serde(skip)]
   unreadable: Option<String>,
}

/// Every agent profile and its grant, held behind a lock and sealed on disk.
#[derive(Clone)]
pub struct AgentProfileStore(RwLockHandle<AgentProfiles>);

impl Default for AgentProfileStore {
   fn default() -> Self {
      Self::new(AgentProfiles::default())
   }
}

impl AgentProfileStore {
   fn new(profiles: AgentProfiles) -> Self {
      Self(RwLockHandle::new(profiles))
   }

   fn read<R>(&self, reader: impl FnOnce(&AgentProfiles) -> R) -> R {
      self.0.read(reader)
   }

   fn write<R>(&self, writer: impl FnOnce(&mut AgentProfiles) -> R) -> R {
      self.0.write(writer)
   }

   pub fn dir() -> Result<std::path::PathBuf, anyhow::Error> {
      file_path(PersistedFile::AgentProfiles)
   }

   pub fn exists() -> Result<bool, anyhow::Error> {
      Ok(Self::dir()?.exists())
   }

   /// Seal the store's contents without writing them. Pair with [`Self::decode`].
   pub fn encode(&self, key: &WalletStateKey) -> Result<Vec<u8>, anyhow::Error> {
      self.read(|profiles| key.seal_json(profiles, AGENT_PROFILES_AAD))
   }

   /// Open sealed bytes, dropping grant keys this build does not know.
   pub fn decode(key: &WalletStateKey, sealed: &[u8]) -> Result<Self, anyhow::Error> {
      let mut profiles: AgentProfiles = key.open_json(sealed, AGENT_PROFILES_AAD)?;

      for profile in profiles.profiles.values_mut() {
         let dropped = profile.sanitize();
         if !dropped.is_empty() {
            tracing::warn!(
               "Dropping unknown grant keys from agent profile {}: {dropped:?}",
               profile.id
            );
         }
      }

      Ok(Self::new(profiles))
   }

   pub fn load_from_file(key: &WalletStateKey) -> Result<Self, anyhow::Error> {
      let sealed = std::fs::read(Self::dir()?)?;
      Self::decode(key, &sealed)
   }

   /// Seal and write the store.
   ///
   /// Refuses while [`Self::unreadable`] is set: the profiles in memory would then be a default, and
   /// sealing them over a file that did not read would destroy the grants for good.
   pub fn save(&self, key: &WalletStateKey) -> Result<(), anyhow::Error> {
      if let Some(reason) = self.unreadable() {
         let path = Self::dir()?;
         return Err(anyhow!(
            "Refusing to overwrite {}: it could not be read ({reason})",
            path.display()
         ));
      }

      ensure_data_dir()?;
      self.write(|profiles| {
         for profile in profiles.profiles.values_mut() {
            profile.sanitize();
         }
      });

      let sealed = self.encode(key)?;
      write_private_atomic(&Self::dir()?, &sealed)
   }

   /// Record that the file on disk did not read, so no save may replace it.
   ///
   /// The caller logs the original error; this keeps the reason next to [`Self::save`]'s refusal, which
   /// names the path. A successful load clears it through [`Self::replace_from`].
   pub fn block_saves(&self, reason: impl Into<String>) {
      self.write(|profiles| profiles.unreadable = Some(reason.into()));
   }

   /// Why saves are blocked, if they are.
   pub fn unreadable(&self) -> Option<String> {
      self.read(|profiles| profiles.unreadable.clone())
   }

   /// Replace the contents in place after a load, so clones of this handle see the new ones.
   pub fn replace_from(&self, other: &AgentProfileStore) {
      other.read(|source| {
         self.write(|target| *target = source.clone());
      });
   }

   /// Every profile, in id order.
   pub fn profiles(&self) -> Vec<AgentProfile> {
      self.read(|profiles| profiles.profiles.values().cloned().collect())
   }

   /// One profile by id.
   pub fn profile(&self, id: &AgentProfileId) -> Option<AgentProfile> {
      self.read(|profiles| profiles.profiles.get(id).cloned())
   }

   /// Every profile whose own switch is on.
   ///
   /// Expiry is deliberately not consulted: it needs a clock, and a caller that has one decides.
   pub fn enabled_profiles(&self) -> Vec<AgentProfile> {
      self.read(|profiles| {
         profiles.profiles.values().filter(|profile| profile.enabled).cloned().collect()
      })
   }

   /// Add or replace a profile, sanitised, returning the one it replaced.
   pub fn insert(&self, mut profile: AgentProfile) -> Option<AgentProfile> {
      profile.sanitize();
      let id = profile.id.clone();
      self.write(|profiles| profiles.profiles.insert(id, profile))
   }

   /// Remove a profile, returning it. Its audit history stays: an ids log outlives the id.
   pub fn remove(&self, id: &AgentProfileId) -> Option<AgentProfile> {
      self.write(|profiles| profiles.profiles.remove(id))
   }

   /// Turn one profile's kill switch on or off; false when there is no such profile.
   pub fn set_enabled(&self, id: &AgentProfileId, enabled: bool) -> bool {
      self.write(|profiles| match profiles.profiles.get_mut(id) {
         Some(profile) => {
            profile.enabled = enabled;
            true
         }
         None => false,
      })
   }

   /// Replace a profile's stored token hash; false when there is no such profile.
   pub fn rotate_token(&self, id: &AgentProfileId, token_hash: [u8; 32]) -> bool {
      self.write(|profiles| match profiles.profiles.get_mut(id) {
         Some(profile) => {
            profile.token_hash = token_hash;
            true
         }
         None => false,
      })
   }

   /// Record that a profile was used, for the profile list's "last used" column.
   pub fn mark_used(&self, id: &AgentProfileId, at: u64) {
      self.write(|profiles| {
         if let Some(profile) = profiles.profiles.get_mut(id) {
            profile.last_used_at = Some(at);
         }
      });
   }

   /// Drop grants for wallets that no longer exist, returning how many were dropped.
   ///
   /// A grant is bound to an address, and a deleted wallet's grant must not be inherited by a wallet
   /// derived at the same address later: deriving a wallet is adding one, not restoring a permission.
   /// Call this with the **authoritative** wallet set — after the wallet cache is built, never with a
   /// partial one, or it prunes grants that are still real.
   pub fn retain_wallets(&self, wallets: &HashSet<Address>) -> usize {
      self.write(|profiles| {
         let mut dropped = 0;
         for profile in profiles.profiles.values_mut() {
            let before = profile.wallets.len();
            profile.wallets.retain(|address, _| wallets.contains(address));
            dropped += before - profile.wallets.len();
         }
         dropped
      })
   }
}
