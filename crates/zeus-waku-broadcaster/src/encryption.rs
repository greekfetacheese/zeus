//! Encryption helpers for Railgun broadcaster transact messages.
//!
//! Matches the TS flow from broadcaster-transaction.ts + crypto.ts:
//! - Client generates random 16-byte responseKey (included in the encrypted payload).
//! - Client derives sharedKey = ECDH(random_priv, broadcaster_viewing_pubkey)
//! - Encrypts the transact params JSON with AES-GCM using the sharedKey.
//! - Sends { pubkey: randomPubKey, encryptedData }
//! - Broadcaster responds encrypted with the responseKey (symmetric).

use zeus_railgun_shared::crypto::babyjub_shared_secret;
use aes_gcm::{
   Aes256Gcm, Nonce,
   aead::{Aead, KeyInit, OsRng},
};
use anyhow::{Result, anyhow};
use rand::RngCore;

use crate::models::transact::BroadcasterRawParamsTransact;

/// Generates a random 16-byte response key (the broadcaster will use this to encrypt its reply).
pub fn generate_response_key() -> [u8; 16] {
   let mut key = [0u8; 16];
   OsRng.fill_bytes(&mut key);
   key
}

/// Real BabyJubJub ECDH using primitives from zeus-railgun.
pub fn derive_shared_key(broadcaster_viewing_pubkey: &[u8; 32]) -> Result<([u8; 32], [u8; 32])> {
   // Generate fresh random private key for this transact (32 bytes)
   let mut random_priv = [0u8; 32];
   OsRng.fill_bytes(&mut random_priv);

   babyjub_shared_secret(&random_priv, broadcaster_viewing_pubkey)
}

/// Encrypts arbitrary JSON with AES-256-GCM.
/// Returns a structure compatible with what the broadcaster expects.
pub fn aes_gcm_encrypt(
   data: &serde_json::Value,
   shared_key: &[u8; 32],
) -> Result<serde_json::Value> {
   let key = aes_gcm::Key::<Aes256Gcm>::from_slice(shared_key);
   let cipher = Aes256Gcm::new(key);

   let plaintext = serde_json::to_vec(data)?;
   let mut nonce_bytes = [0u8; 12];
   OsRng.fill_bytes(&mut nonce_bytes);
   let nonce = Nonce::from_slice(&nonce_bytes);

   let ciphertext = cipher
      .encrypt(nonce, plaintext.as_ref())
      .map_err(|e| anyhow!("aes-gcm encrypt failed: {}", e))?;

   Ok(serde_json::json!({
       "iv": hex::encode(nonce_bytes),
       "ciphertext": hex::encode(&ciphertext),
   }))
}

/// Decrypts a response that was encrypted with the responseKey (or shared key).
pub fn aes_gcm_decrypt(encrypted: &serde_json::Value, key: &[u8]) -> Result<serde_json::Value> {
   // Support both 16-byte responseKey and 32-byte shared keys
   let key32: [u8; 32] = if key.len() == 32 {
      key.try_into().unwrap()
   } else if key.len() == 16 {
      let mut k = [0u8; 32];
      k[0..16].copy_from_slice(key);
      k[16..32].copy_from_slice(key); // simple expansion for responseKey
      k
   } else {
      return Err(anyhow!("key must be 16 or 32 bytes"));
   };

   let key = aes_gcm::Key::<Aes256Gcm>::from_slice(&key32);
   let cipher = Aes256Gcm::new(key);

   let iv_hex = encrypted["iv"].as_str().ok_or_else(|| anyhow!("missing iv"))?;
   let ct_hex = encrypted["ciphertext"].as_str().ok_or_else(|| anyhow!("missing ciphertext"))?;

   let nonce_bytes = hex::decode(iv_hex)?;
   let ciphertext = hex::decode(ct_hex)?;
   let nonce = Nonce::from_slice(&nonce_bytes);

   let plaintext = cipher
      .decrypt(nonce, ciphertext.as_ref())
      .map_err(|e| anyhow!("aes-gcm decrypt failed: {}", e))?;

   let json: serde_json::Value = serde_json::from_slice(&plaintext)?;
   Ok(json)
}

/// High-level helper: takes the raw transact params + broadcaster's viewing pubkey (32 bytes),
/// injects responseKey, encrypts with derived shared key, returns (random_pubkey_hex, encrypted_data).
pub fn encrypt_transact_payload(
   transact_data: &mut BroadcasterRawParamsTransact,
   broadcaster_viewing_pubkey: &[u8; 32],
) -> Result<(String, serde_json::Value)> {
   let response_key = generate_response_key();
   transact_data.response_key = Some(hex::encode(response_key));

   let data_json = serde_json::to_value(&*transact_data)?;

   let (random_pub, shared) = derive_shared_key(broadcaster_viewing_pubkey)?;

   let encrypted = aes_gcm_encrypt(&data_json, &shared)?;

   Ok((hex::encode(random_pub), encrypted))
}
