//! AEAD helpers for sensitive Railgun DB values (account notes, POI state).

use chacha20poly1305::{
   XChaCha20Poly1305, XNonce,
   aead::{Aead, AeadCore, KeyInit, OsRng, Payload},
};
use rand::RngCore;
use secure_types::{SecureArray, Zeroize};
use serde::{Deserialize, Serialize};

use super::DatabaseError;

/// Envelope payload version: bincode plaintext sealed with [`RailgunDbKey`].
pub const ENCRYPTED_ENVELOPE_VERSION: u32 = 4;

const NONCE_LEN: usize = 24;

/// 32-byte session key for AEAD of sensitive Railgun DB blobs.
#[derive(Clone, Serialize, Deserialize)]
pub struct RailgunDbKey(SecureArray<u8, 32>);

impl RailgunDbKey {
   /// Generate a fresh random key.
   pub fn generate() -> Result<Self, DatabaseError> {
      let mut bytes = [0u8; 32];
      rand::rng().fill_bytes(&mut bytes);
      let key = SecureArray::from_slice_mut(&mut bytes)
         .map_err(|e| DatabaseError::StorageError(format!("railgun db key: {e}")))?;
      Ok(Self(key))
   }

   /// Seal plaintext: `nonce (24) || ciphertext+tag`.
   ///
   /// `aad` should be the storage key bytes so ciphertext is bound to the slot.
   pub fn seal(&self, plaintext: &[u8], aad: &[u8]) -> Result<Vec<u8>, DatabaseError> {
      self.0.unlock(|key_bytes| {
         let mut key_arr: [u8; 32] = key_bytes
            .try_into()
            .map_err(|_| DatabaseError::StorageError("railgun db key length".into()))?;
         let cipher = XChaCha20Poly1305::new_from_slice(&key_arr)
            .map_err(|e| DatabaseError::StorageError(e.to_string()))?;
         let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
         let ct = cipher
            .encrypt(
               &nonce,
               Payload {
                  msg: plaintext,
                  aad,
               },
            )
            .map_err(|e| DatabaseError::EncryptionError(e.to_string()))?;

         key_arr.zeroize();

         let mut out = Vec::with_capacity(NONCE_LEN + ct.len());
         out.extend_from_slice(nonce.as_slice());
         out.extend_from_slice(&ct);
         Ok(out)
      })
   }

   /// Open a blob produced by [`Self::seal`].
   pub fn open(&self, sealed: &[u8], aad: &[u8]) -> Result<Vec<u8>, DatabaseError> {
      if sealed.len() <= NONCE_LEN {
         return Err(DatabaseError::DecryptionError(
            "sealed blob too short".into(),
         ));
      }
      let (nonce_bytes, ct) = sealed.split_at(NONCE_LEN);
      let nonce = XNonce::from_slice(nonce_bytes);

      self.0.unlock(|key_bytes| {
         let mut key_arr: [u8; 32] = key_bytes
            .try_into()
            .map_err(|_| DatabaseError::StorageError("railgun db key length".into()))?;
         let cipher = XChaCha20Poly1305::new_from_slice(&key_arr)
            .map_err(|e| DatabaseError::StorageError(e.to_string()))?;
         let data = cipher
            .decrypt(nonce, Payload { msg: ct, aad })
            .map_err(|e| DatabaseError::DecryptionError(e.to_string()));

         key_arr.zeroize();

         data
      })
   }

   /// Zeroize key material (e.g. vault lock / shutdown).
   pub fn erase(&mut self) {
      self.0.erase();
   }
}

impl std::fmt::Debug for RailgunDbKey {
   fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
      f.write_str("RailgunDbKey([redacted])")
   }
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn seal_open_roundtrip() {
      let key = RailgunDbKey::generate().unwrap();
      let aad = b"account:test";
      let pt = b"hello-notes-payload";
      let sealed = key.seal(pt, aad).unwrap();
      assert_ne!(&sealed[NONCE_LEN..], pt);
      let opened = key.open(&sealed, aad).unwrap();
      assert_eq!(opened, pt);
   }

   #[test]
   fn wrong_aad_fails() {
      let key = RailgunDbKey::generate().unwrap();
      let sealed = key.seal(b"data", b"aad-a").unwrap();
      assert!(key.open(&sealed, b"aad-b").is_err());
   }

   #[test]
   fn wrong_key_fails() {
      let key_a = RailgunDbKey::generate().unwrap();
      let key_b = RailgunDbKey::generate().unwrap();
      let sealed = key_a.seal(b"data", b"aad").unwrap();
      assert!(key_b.open(&sealed, b"aad").is_err());
   }
}
