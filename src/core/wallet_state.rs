//! Frequently-updated wallet app state, sealed separately from the Argon2 vault.
//!
//! The vault still holds HD/imported keys and the AEAD key used here. This file
//! (`wallet_state.data`) uses XChaCha20-Poly1305 so contacts, balances, portfolios,
//! tx history, approvals, and HD discovery can be saved without re-running Argon2.

use crate::core::context::{
   ApprovalManagerHandle, BalanceManagerHandle, DiscoveredWallets, PortfolioDB, TxDBHandle,
   data_dir,
};
use crate::core::types::Contact;
use crate::utils::write_private_atomic;
use anyhow::anyhow;
use brotli::{BrotliCompress, BrotliDecompress, enc::BrotliEncoderParams};
use chacha20poly1305::{
   XChaCha20Poly1305, XNonce,
   aead::{Aead, AeadCore, KeyInit, OsRng, Payload},
};
use rand::RngCore;
use secure_types::{SecureArray, Zeroize};
use serde::{Deserialize, Serialize};
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

pub const WALLET_STATE_FILE: &str = "wallet_state.data";

/// Bound ciphertext to this logical slot (AAD).
const WALLET_STATE_AAD: &[u8] = b"zeus-wallet-state-v1";

/// Plaintext payload encoding (first byte after AEAD open).
///
/// - `0` raw JSON
/// - `1` brotli-compressed JSON
const PAYLOAD_RAW_JSON: u8 = 0;
const PAYLOAD_BROTLI: u8 = 1;

const NONCE_LEN: usize = 24;
const BROTLI_QUALITY: i32 = 5;

/// 32-byte AEAD key for [`WalletState`], persisted inside [super::Vault].
#[derive(Clone, Serialize, Deserialize)]
pub struct WalletStateKey(SecureArray<u8, 32>);

impl WalletStateKey {
   pub fn generate() -> Result<Self, anyhow::Error> {
      let mut bytes = [0u8; 32];
      rand::thread_rng().fill_bytes(&mut bytes);
      let key =
         SecureArray::from_slice_mut(&mut bytes).map_err(|e| anyhow!("wallet state key: {e}"))?;
      Ok(Self(key))
   }

   /// Seal plaintext: `nonce (24) || ciphertext+tag`.
   pub fn seal(&self, plaintext: &[u8], aad: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
      self.0.unlock(|key_bytes| {
         let mut key_arr: [u8; 32] =
            key_bytes.try_into().map_err(|_| anyhow!("wallet state key length"))?;
         let cipher = XChaCha20Poly1305::new_from_slice(&key_arr)
            .map_err(|e| anyhow!("wallet state cipher: {e}"))?;
         let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
         let ct = cipher
            .encrypt(
               &nonce,
               Payload {
                  msg: plaintext,
                  aad,
               },
            )
            .map_err(|e| anyhow!("wallet state encrypt: {e}"))?;

         key_arr.zeroize();

         let mut out = Vec::with_capacity(NONCE_LEN + ct.len());
         out.extend_from_slice(nonce.as_slice());
         out.extend_from_slice(&ct);
         Ok(out)
      })
   }

   /// Open a blob produced by [`Self::seal`].
   pub fn open(&self, sealed: &[u8], aad: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
      if sealed.len() <= NONCE_LEN {
         return Err(anyhow!("wallet state sealed blob too short"));
      }
      let (nonce_bytes, ct) = sealed.split_at(NONCE_LEN);
      let nonce = XNonce::from_slice(nonce_bytes);

      self.0.unlock(|key_bytes| {
         let mut key_arr: [u8; 32] =
            key_bytes.try_into().map_err(|_| anyhow!("wallet state key length"))?;
         let cipher = XChaCha20Poly1305::new_from_slice(&key_arr)
            .map_err(|e| anyhow!("wallet state cipher: {e}"))?;
         let data = cipher
            .decrypt(nonce, Payload { msg: ct, aad })
            .map_err(|e| anyhow!("wallet state decrypt: {e}"));

         key_arr.zeroize();
         data
      })
   }

   pub fn erase(&mut self) {
      self.0.erase();
   }
}

impl std::fmt::Debug for WalletStateKey {
   fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
      f.write_str("WalletStateKey([redacted])")
   }
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

   #[serde(default)]
   pub tx_db: TxDBHandle,

   #[serde(default)]
   pub approval_manager: ApprovalManagerHandle,

   #[serde(default)]
   pub discovered_wallets: DiscoveredWallets,
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
      Ok(data_dir()?.join(WALLET_STATE_FILE))
   }

   pub fn exists() -> Result<bool, anyhow::Error> {
      Ok(Self::dir()?.exists())
   }

   /// Encrypt and write `wallet_state.data` (atomic replace).
   pub fn encrypt_and_save(&self, key: &WalletStateKey) -> Result<(), anyhow::Error> {
      let mut json = self.read(|ws| serde_json::to_vec(ws))?;
      let mut payload = match encode_payload(&json) {
         Ok(p) => p,
         Err(e) => {
            json.zeroize();
            return Err(e);
         }
      };
      json.zeroize();

      let sealed = match key.seal(&payload, WALLET_STATE_AAD) {
         Ok(s) => s,
         Err(e) => {
            payload.zeroize();
            return Err(e);
         }
      };
      payload.zeroize();

      let path = Self::dir()?;
      write_private_atomic(&path, &sealed)?;
      Ok(())
   }

   /// Load from `wallet_state.data` using the vault-held key.
   pub fn load(key: &WalletStateKey) -> Result<Self, anyhow::Error> {
      let path = Self::dir()?;
      let sealed = std::fs::read(&path).map_err(|e| anyhow!("read {}: {e}", path.display()))?;
      let mut plain = key.open(&sealed, WALLET_STATE_AAD)?;
      let mut json = match decode_payload(&plain) {
         Ok(j) => j,
         Err(e) => {
            plain.zeroize();
            return Err(e);
         }
      };
      plain.zeroize();

      let inner: WalletStateInner = match serde_json::from_slice(&json) {
         Ok(inner) => inner,
         Err(e) => {
            json.zeroize();
            return Err(anyhow!("parse wallet state: {e}"));
         }
      };
      json.zeroize();
      Ok(Self::new(inner))
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

fn brotli_compress(input: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
   let mut params = BrotliEncoderParams::default();
   params.quality = BROTLI_QUALITY;
   let mut out = Vec::new();
   BrotliCompress(&mut Cursor::new(input), &mut out, &params)
      .map_err(|e| anyhow!("brotli compress wallet state: {e}"))?;
   Ok(out)
}

fn brotli_decompress(input: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
   let mut out = Vec::new();
   BrotliDecompress(&mut &input[..], &mut out)
      .map_err(|e| anyhow!("brotli decompress wallet state: {e}"))?;
   Ok(out)
}

fn encode_payload(json: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
   let compressed = brotli_compress(json)?;
   let mut out = Vec::with_capacity(1 + compressed.len());
   out.push(PAYLOAD_BROTLI);
   out.extend_from_slice(&compressed);
   Ok(out)
}

fn decode_payload(data: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
   if data.is_empty() {
      return Err(anyhow!("wallet state payload is empty"));
   }

   // Accept raw JSON without version byte (defensive).
   if data[0] == b'{' {
      return Ok(data.to_vec());
   }

   let version = data[0];
   let payload = &data[1..];
   match version {
      PAYLOAD_RAW_JSON => Ok(payload.to_vec()),
      PAYLOAD_BROTLI => brotli_decompress(payload),
      other => Err(anyhow!(
         "unknown wallet state payload version: {other}"
      )),
   }
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn key_seal_open_roundtrip() {
      let key = WalletStateKey::generate().unwrap();
      let pt = b"wallet-state-bytes";
      let sealed = key.seal(pt, WALLET_STATE_AAD).unwrap();
      assert_ne!(&sealed[NONCE_LEN..], pt);
      assert_eq!(key.open(&sealed, WALLET_STATE_AAD).unwrap(), pt);
   }

   #[test]
   fn payload_brotli_roundtrip() {
      let inner = WalletStateInner::default();
      let json = serde_json::to_vec(&inner).unwrap();
      let encoded = encode_payload(&json).unwrap();
      assert_eq!(encoded[0], PAYLOAD_BROTLI);
      let decoded = decode_payload(&encoded).unwrap();
      let loaded: WalletStateInner = serde_json::from_slice(&decoded).unwrap();
      assert!(loaded.contacts.is_empty());
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
