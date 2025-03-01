use alloy_signer_local::PrivateKeySigner;
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

/// Wrapper around [PrivateKeySigner] that zeroizes the key when dropped
///
/// Do not clone the inner value, as it will not be erased when the outer value is dropped
#[derive(Clone, PartialEq)]
pub struct SafeSigner(PrivateKeySigner);

impl SafeSigner {
   /// Create a new signer with a random key
   pub fn random() -> Self {
      Self(PrivateKeySigner::random())
   }

   /// Do not clone the inner value, as it will not be erased when the outer value is dropped
   pub fn inner(&self) -> &PrivateKeySigner {
      &self.0
   }

   /// Erase the signer's key from memory
   pub fn erase(&mut self) {
      unsafe {
         // Get a mutable pointer to the key
         let key_ptr: *mut PrivateKeySigner = &mut self.0;

         // Convert the key to a byte slice
         let key_bytes: &mut [u8] =
            std::slice::from_raw_parts_mut(key_ptr as *mut u8, std::mem::size_of::<PrivateKeySigner>());

         // Zeroize the byte slice
         key_bytes.zeroize();
      }
   }

   pub fn is_erased(&self) -> bool {
      let mut key_bytes = self.0.to_bytes();
      let erased = key_bytes.iter().all(|&b| b == 0);
      key_bytes.zeroize();
      erased
   }
}

impl Drop for SafeSigner {
   fn drop(&mut self) {
      self.erase();
   }
}

impl From<PrivateKeySigner> for SafeSigner {
   fn from(signer: PrivateKeySigner) -> Self {
      Self(signer)
   }
}

impl Serialize for SafeSigner {
   fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
   where
      S: serde::Serializer,
   {
      let key_bytes = self.0.to_bytes().to_vec();
      serializer.serialize_bytes(&key_bytes)
   }
}

impl<'de> Deserialize<'de> for SafeSigner {
   fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
   where
      D: serde::Deserializer<'de>,
   {
      let key_bytes: Vec<u8> = serde::Deserialize::deserialize(deserializer)?;
      let key = PrivateKeySigner::from_slice(&key_bytes).map_err(serde::de::Error::custom)?;
      Ok(Self(key))
   }
}

mod tests {
   #[allow(unused_imports)]
   use super::*;

   #[test]
   fn test_erase() {
      let mut safe_signer = SafeSigner::from(PrivateKeySigner::random());

      println!(
         "Signer key: {}",
         safe_signer
            .0
            .to_bytes()
            .to_vec()
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<String>()
      );
      assert!(!safe_signer.is_erased());

      safe_signer.erase();
      assert!(safe_signer.is_erased());

      println!(
         "Signer key erased, should be all zeros: {}",
         safe_signer
            .0
            .to_bytes()
            .to_vec()
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<String>()
      );
   }

   #[test]
   fn test_serde() {
      let safe_signer = SafeSigner::from(PrivateKeySigner::random());
      let serialized = serde_json::to_string(&safe_signer).unwrap();
      let deserialized: SafeSigner = serde_json::from_str(&serialized).unwrap();

      let key1 = safe_signer.0.to_bytes();
      let key2 = deserialized.0.to_bytes();
      assert_eq!(key1, key2);
   }
}
