//! Frequently-updated wallet app state, sealed separately from the Argon2 vault.
//!
//! The vault still holds HD/imported keys and the AEAD key used here. This file
//! (`wallet_state.data`) uses XChaCha20-Poly1305 so contacts, balances, portfolios,
//! approvals, and HD discovery can be saved without re-running Argon2.
//!
//! Transaction history lives in its own redb file (`tx_history.db`). Older
//! `wallet_state.data` blobs may still embed [`TxDBHandle`]; that is accepted
//! on load and then migrated out.

use crate::core::context::{
   ApprovalManagerHandle, BalanceManagerHandle, DiscoveredWallets, PortfolioDB, TxDBHandle,
};
use crate::core::persisted::{PersistedFile, file_path};
use crate::core::types::{Contact, DappAccounts};
use crate::utils::write_private_atomic;
use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

pub use crate::core::wallet_state_key::WalletStateKey;

/// Bound ciphertext to this logical slot (AAD).
const WALLET_STATE_AAD: &[u8] = b"zeus-wallet-state-v1";

fn tx_db_is_empty(db: &TxDBHandle) -> bool {
   db.txs_count() == 0
}

/// Payload held under [`WalletState`]'s lock.
#[derive(Clone, Serialize, Deserialize)]
pub struct WalletStateInner {
   #[serde(default)]
   pub contacts: Vec<Contact>,

   #[serde(default)]
   pub balance_manager: BalanceManagerHandle,

   #[serde(default)]
   pub portfolio_db: PortfolioDB,

   /// Legacy: accepted on load, omitted from new saves once migrated to redb.
   #[serde(default, skip_serializing_if = "tx_db_is_empty")]
   pub tx_db: TxDBHandle,

   #[serde(default)]
   pub approval_manager: ApprovalManagerHandle,

   #[serde(default)]
   pub discovered_wallets: DiscoveredWallets,

   #[serde(default)]
   pub dapp_accounts: DappAccounts,
}

impl Default for WalletStateInner {
   fn default() -> Self {
      Self {
         contacts: Vec::new(),
         balance_manager: BalanceManagerHandle::default(),
         portfolio_db: PortfolioDB::default(),
         tx_db: TxDBHandle::new(),
         approval_manager: ApprovalManagerHandle::new(),
         discovered_wallets: DiscoveredWallets::new(),
         dapp_accounts: DappAccounts::default(),
      }
   }
}

/// Shared handle for frequently updated wallet app state.
#[derive(Clone)]
pub struct WalletState(Arc<RwLock<WalletStateInner>>);

impl Default for WalletState {
   fn default() -> Self {
      Self::new(WalletStateInner::default())
   }
}

impl Serialize for WalletState {
   fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
   where
      S: serde::Serializer,
   {
      self.read(|inner| inner.serialize(serializer))
   }
}

impl<'de> Deserialize<'de> for WalletState {
   fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
   where
      D: serde::Deserializer<'de>,
   {
      let inner = WalletStateInner::deserialize(deserializer)?;
      Ok(Self::new(inner))
   }
}

impl WalletState {
   pub fn new(inner: WalletStateInner) -> Self {
      Self(Arc::new(RwLock::new(inner)))
   }

   pub fn read<R>(&self, reader: impl FnOnce(&WalletStateInner) -> R) -> R {
      reader(&self.0.read().unwrap())
   }

   pub fn write<R>(&self, writer: impl FnOnce(&mut WalletStateInner) -> R) -> R {
      writer(&mut self.0.write().unwrap())
   }

   /// Replace contents in-place (same `Arc`).
   pub fn set(&self, inner: WalletStateInner) {
      self.write(|ws| *ws = inner);
   }

   /// Deep-clone inner payload (e.g. offline snapshot / tests).
   pub fn clone_inner(&self) -> WalletStateInner {
      self.read(|ws| ws.clone())
   }

   pub fn dir() -> Result<PathBuf, anyhow::Error> {
      file_path(PersistedFile::WalletState)
   }

   pub fn exists() -> Result<bool, anyhow::Error> {
      Ok(Self::dir()?.exists())
   }

   /// Encrypt and write `wallet_state.data` (atomic replace).
   pub fn encrypt_and_save(&self, key: &WalletStateKey) -> Result<(), anyhow::Error> {
      let sealed = self.encrypt_to_bytes(key)?;
      let path = Self::dir()?;
      write_private_atomic(&path, &sealed)?;
      Ok(())
   }

   /// Seal this wallet state (import/export verification, tests).
   pub fn encrypt_to_bytes(&self, key: &WalletStateKey) -> Result<Vec<u8>, anyhow::Error> {
      self.read(|ws| key.seal_json(ws, WALLET_STATE_AAD))
   }

   /// Open a sealed `wallet_state.data` blob with the vault-held key.
   pub fn decrypt_from_bytes(key: &WalletStateKey, sealed: &[u8]) -> Result<Self, anyhow::Error> {
      let inner: WalletStateInner = key
         .open_json(sealed, WALLET_STATE_AAD)
         .map_err(|e| anyhow!("Failed to decrypt wallet state: {e}"))?;
      Ok(Self::new(inner))
   }

   /// Load from `wallet_state.data` using the vault-held key.
   pub fn load(key: &WalletStateKey) -> Result<Self, anyhow::Error> {
      let path = Self::dir()?;
      let sealed = std::fs::read(&path).map_err(|e| anyhow!("read {}: {e}", path.display()))?;
      Self::decrypt_from_bytes(key, &sealed)
   }

   /// Load sealed file if present; otherwise use `legacy` (vault migration) or default.
   pub fn load_or_migrate(
      key: &WalletStateKey,
      legacy: Option<WalletStateInner>,
   ) -> Result<(Self, bool), anyhow::Error> {
      if Self::exists()? {
         Ok((Self::load(key)?, false))
      } else if let Some(inner) = legacy {
         let ws = Self::new(inner);
         ws.encrypt_and_save(key)?;
         Ok((ws, true))
      } else {
         let ws = Self::default();
         // Ensure file exists after first unlock / first vault create.
         ws.encrypt_and_save(key)?;
         Ok((ws, true))
      }
   }
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn seal_json_rejects_wrong_aad() {
      let key = WalletStateKey::generate().unwrap();
      let inner = WalletStateInner::default();
      let sealed = key.seal_json(&inner, WALLET_STATE_AAD).unwrap();
      let loaded: WalletStateInner = key.open_json(&sealed, WALLET_STATE_AAD).unwrap();
      assert!(loaded.contacts.is_empty());
      assert!(key.open_json::<WalletStateInner>(&sealed, b"wrong-aad").is_err());
   }

   #[test]
   fn dapp_accounts_survive_a_roundtrip() {
      let key = WalletStateKey::generate().unwrap();
      let origin = "https://app.uniswap.org";
      let address = zeus_eth::alloy_primitives::Address::from([0x11u8; 20]);

      let mut inner = WalletStateInner::default();
      inner.dapp_accounts.record(origin, address);

      let sealed = key.seal_json(&inner, WALLET_STATE_AAD).unwrap();
      let loaded: WalletStateInner = key.open_json(&sealed, WALLET_STATE_AAD).unwrap();
      assert_eq!(loaded.dapp_accounts.get(origin), Some(address));
      assert_eq!(loaded.dapp_accounts.seen(origin), [address]);
   }

   /// An app keeps every account it was given, so a later connection does not
   /// erase the earlier ones the app already knows about.
   #[test]
   fn dapp_accounts_keep_every_account_an_app_was_given() {
      let origin = "https://app.uniswap.org";
      let (first, second) = (
         zeus_eth::alloy_primitives::Address::from([0x11u8; 20]),
         zeus_eth::alloy_primitives::Address::from([0x22u8; 20]),
      );

      let mut accounts = DappAccounts::default();
      assert!(accounts.seen(origin).is_empty());

      accounts.record(origin, first);
      accounts.record(origin, second);
      assert_eq!(accounts.get(origin), Some(second));
      assert_eq!(accounts.seen(origin), [first, second]);

      // Switching back to the first: last used wins, history does not grow.
      accounts.record(origin, first);
      assert_eq!(accounts.get(origin), Some(first));
      assert_eq!(accounts.seen(origin), [first, second]);
   }

   #[test]
   fn wallet_state_json_roundtrip() {
      let mut inner = WalletStateInner::default();
      inner.contacts.push(Contact::new(
         "alice".into(),
         "0xabc".into(),
         String::new(),
      ));
      let ws = WalletState::new(inner.clone());
      let json = serde_json::to_vec(&ws).unwrap();
      let loaded: WalletState = serde_json::from_slice(&json).unwrap();
      assert_eq!(loaded.read(|s| s.contacts.len()), 1);
      assert_eq!(
         loaded.read(|s| s.contacts[0].name.clone()),
         "alice"
      );
   }
}
