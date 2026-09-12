//! AEAD key for [`super::WalletState`] and other fast sealed stores.
//!
//! Lives in its own module so stores such as tx history can use the key
//! without a `wallet_state` ↔ `context::tx` import cycle.

use anyhow::anyhow;
use brotli::{BrotliCompress, BrotliDecompress, enc::BrotliEncoderParams};
use chacha20poly1305::{
   XChaCha20Poly1305, XNonce,
   aead::{Aead, AeadCore, KeyInit, OsRng, Payload},
};
use rand::RngCore;
use secure_types::{SecureArray, Zeroize};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::io::Cursor;

/// Plaintext payload encoding (first byte after AEAD open).
///
/// - `0` raw JSON
/// - `1` brotli-compressed JSON
const PAYLOAD_RAW_JSON: u8 = 0;
const PAYLOAD_BROTLI: u8 = 1;

const NONCE_LEN: usize = 24;
const BROTLI_QUALITY: i32 = 5;

/// 32-byte AEAD key for [`super::WalletState`], persisted inside [super::Vault].
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

         let cipher = match XChaCha20Poly1305::new_from_slice(&key_arr) {
            Ok(cipher) => cipher,
            Err(e) => {
               key_arr.zeroize();
               return Err(anyhow!("wallet state cipher: {e}"));
            }
         };

         let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);

         let ct = match cipher.encrypt(
            &nonce,
            Payload {
               msg: plaintext,
               aad,
            },
         ) {
            Ok(ct) => ct,
            Err(e) => {
               key_arr.zeroize();
               return Err(anyhow!("wallet state encrypt: {e}"));
            }
         };

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

   /// Serialize `value` as brotli-compressed JSON and seal it.
   pub fn seal_json<T: Serialize>(&self, value: &T, aad: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
      let mut json =
         serde_json::to_vec(value).map_err(|e| anyhow!("serialize sealed json: {e}"))?;
      let mut payload = match encode_payload(&json) {
         Ok(p) => p,
         Err(e) => {
            json.zeroize();
            return Err(e);
         }
      };
      json.zeroize();

      let sealed = match self.seal(&payload, aad) {
         Ok(s) => s,
         Err(e) => {
            payload.zeroize();
            return Err(e);
         }
      };
      payload.zeroize();
      Ok(sealed)
   }

   /// Open a blob produced by [`Self::seal_json`].
   pub fn open_json<T: DeserializeOwned>(
      &self,
      sealed: &[u8],
      aad: &[u8],
   ) -> Result<T, anyhow::Error> {
      let mut plain = self.open(sealed, aad)?;
      let mut json = match decode_payload(&plain) {
         Ok(j) => j,
         Err(e) => {
            plain.zeroize();
            return Err(e);
         }
      };
      plain.zeroize();

      let value = match serde_json::from_slice(&json) {
         Ok(v) => v,
         Err(e) => {
            json.zeroize();
            return Err(anyhow!("parse sealed json: {e}"));
         }
      };
      json.zeroize();
      Ok(value)
   }
}

impl std::fmt::Debug for WalletStateKey {
   fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
      f.write_str("WalletStateKey([redacted])")
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

   const AAD: &[u8] = b"zeus-wallet-state-v1";

   #[test]
   fn key_seal_open_roundtrip() {
      let key = WalletStateKey::generate().unwrap();
      let pt = b"wallet-state-bytes";
      let sealed = key.seal(pt, AAD).unwrap();
      assert_ne!(&sealed[NONCE_LEN..], pt);
      assert_eq!(key.open(&sealed, AAD).unwrap(), pt);
   }

   #[test]
   fn payload_brotli_roundtrip() {
      let json = br#"{"contacts":[]}"#;
      let encoded = encode_payload(json).unwrap();
      assert_eq!(encoded[0], PAYLOAD_BROTLI);
      let decoded = decode_payload(&encoded).unwrap();
      assert_eq!(decoded, json);
   }
}
