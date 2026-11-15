use alloy_network::EthereumWallet;
use alloy_primitives::{Address, hex};
use alloy_signer_local::PrivateKeySigner;
use k256::ecdsa::{SigningKey, VerifyingKey};
use secure_types::{SecureArray, SecureString, Zeroize};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use std::sync::{Arc, Mutex};

/// Wrapper around a [SecureArray<u8, 32>] that holds the private key
pub struct SecureKey {
   address: Address,
   data: Arc<Mutex<SecureArray<u8, 32>>>,
}

impl SecureKey {
   /// Generate a new random private key
   pub fn random() -> Self {
      let signer = PrivateKeySigner::random();
      Self::from(signer)
   }

   /// Return the private key by cloning it
   pub fn key(&self) -> SecureArray<u8, 32> {
      let data = self.data.lock().unwrap();
      data.clone()
   }

   /// Return the key in a SecureString
   ///
   /// # Panics
   /// Panics if the key is not a valid form of a private key
   /// or if it has been erased
   pub fn key_string(&self) -> SecureString {
      let signer = self.to_signer();
      let mut key = signer.to_bytes();
      let string = hex::encode(key);
      key.zeroize();
      SecureString::from(string)
   }

   /// Securely erase the key from memory
   pub fn erase(&mut self) {
      let mut data = self.data.lock().unwrap();
      data.erase();
   }

   /// Whether the underlying 32 key bytes are all zero, i.e. the key was erased.
   pub fn is_erased(&self) -> bool {
      let data = self.data.lock().unwrap();
      data.unlock(|slice| slice.iter().all(|byte| byte == &0))
   }

   /// The Ethereum address matching this private key.
   pub fn address(&self) -> Address {
      self.address
   }

   /// Converts the private key to a [PrivateKeySigner] by cloning the inner key
   ///
   /// # Panics
   /// Panics if the key is not a valid form of a private key
   /// or if it has been erased
   pub fn to_signer(&self) -> PrivateKeySigner {
      assert!(!self.is_erased(), "Key is erased");

      let data = self.data.lock().unwrap();
      data.unlock(|bytes| PrivateKeySigner::from_slice(bytes).expect("Key is invalid"))
   }

   /// Converts the private key to a [SigningKey] by cloning the inner key
   ///
   /// # Panics
   /// Panics if the key is not a valid form of a private key
   /// or if it has been erased
   pub fn to_signing_key(&self) -> SigningKey {
      let data = self.data.lock().unwrap();
      data.unlock(|bytes| SigningKey::from_slice(bytes).expect("Key is invalid"))
   }

   /// Converts the private key to an [EthereumWallet] by cloning the inner key
   ///
   /// # Panics
   /// Panics if the key is not a valid form of a private key
   /// or if it has been erased
   pub fn to_wallet(&self) -> EthereumWallet {
      EthereumWallet::from(self.to_signer())
   }

   /// Converts the private key to a [VerifyingKey] by cloning the inner key
   ///
   /// # Panics
   /// Panics if the key is not a valid form of a private key
   /// or if it has been erased
   pub fn verifying_key(&self) -> VerifyingKey {
      let key = self.to_signing_key();
      *key.verifying_key()
   }
}

impl Clone for SecureKey {
   fn clone(&self) -> Self {
      Self {
         address: self.address,
         data: Arc::clone(&self.data),
      }
   }
}

#[cfg(feature = "serde")]
impl Serialize for SecureKey {
   fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
   where
      S: serde::Serializer,
   {
      use serde::ser::SerializeStruct;

      let data = self.data.lock().unwrap();
      let mut state = serializer.serialize_struct("SecureKey", 2)?;
      state.serialize_field("address", &self.address)?;
      state.serialize_field("data", &*data)?;

      state.end()
   }
}

#[cfg(feature = "serde")]
impl<'de> Deserialize<'de> for SecureKey {
   fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
   where
      D: serde::Deserializer<'de>,
   {
      use serde::de::{Error, IgnoredAny, MapAccess, SeqAccess, Visitor};

      const FIELDS: &[&str] = &["address", "data"];

      struct SecureKeyVisitor;

      impl<'de> Visitor<'de> for SecureKeyVisitor {
         type Value = SecureKey;

         fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("struct SecureKey")
         }

         fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
         where
            A: SeqAccess<'de>,
         {
            let address = seq.next_element()?.ok_or_else(|| A::Error::invalid_length(0, &self))?;
            let data = seq.next_element()?.ok_or_else(|| A::Error::invalid_length(1, &self))?;

            Ok(SecureKey {
               address,
               data: Arc::new(Mutex::new(data)),
            })
         }

         fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
         where
            A: MapAccess<'de>,
         {
            let mut address = None;
            let mut data = None;

            while let Some(key) = map.next_key::<String>()? {
               match key.as_str() {
                  "address" => address = Some(map.next_value()?),
                  "data" => data = Some(map.next_value()?),
                  _ => {
                     map.next_value::<IgnoredAny>()?;
                  }
               }
            }

            let address = address.ok_or_else(|| A::Error::missing_field("address"))?;
            let data = data.ok_or_else(|| A::Error::missing_field("data"))?;

            Ok(SecureKey {
               address,
               data: Arc::new(Mutex::new(data)),
            })
         }
      }

      deserializer.deserialize_struct("SecureKey", FIELDS, SecureKeyVisitor)
   }
}

impl From<SecureArray<u8, 32>> for SecureKey {
   /// Converts a [SecureArray<u8, 32>] to a [SecureKey]
   ///
   /// # Panics
   /// Panics if the key is not a valid form of a private key
   fn from(value: SecureArray<u8, 32>) -> Self {
      let signer =
         value.unlock(|slice| PrivateKeySigner::from_slice(slice).expect("Key is invalid"));
      let address = signer.address();

      SecureKey {
         address,
         data: Arc::new(Mutex::new(value)),
      }
   }
}

impl From<PrivateKeySigner> for SecureKey {
   fn from(value: PrivateKeySigner) -> Self {
      let address = value.address();
      let mut key_bytes = value.to_bytes();
      let data = SecureArray::from_slice_mut(key_bytes.as_mut()).expect("Key is invalid");

      SecureKey {
         address,
         data: Arc::new(Mutex::new(data)),
      }
   }
}

impl From<SigningKey> for SecureKey {
   fn from(value: SigningKey) -> Self {
      let mut bytes = value.to_bytes();
      let signer = PrivateKeySigner::from_slice(&bytes).expect("Key is invalid");
      let address = signer.address();
      let data = SecureArray::from_slice_mut(bytes.as_mut()).expect("Key is invalid");

      SecureKey {
         address,
         data: Arc::new(Mutex::new(data)),
      }
   }
}

#[cfg(test)]
mod tests {
   use std::str::FromStr;

   use super::*;

   #[test]
   fn test_create() {
      let signer = PrivateKeySigner::random();
      let secure_signer = SecureKey::from(signer.clone());
      let signer2 = secure_signer.to_signer();
      assert_eq!(signer.address(), signer2.address());
   }

   #[test]
   fn sanity_check() {
      let signer = PrivateKeySigner::random();
      let secure_signer = SecureKey::from(signer.clone());
      let signing_key = secure_signer.to_signing_key();
      let secure_signer2 = SecureKey::from(signing_key);
      assert_eq!(secure_signer.address(), secure_signer2.address());
   }

   #[test]
   fn test_key_string() {
      let signer = PrivateKeySigner::random();
      let secure_signer = SecureKey::from(signer.clone());
      let key_secure_string = secure_signer.key_string();

      key_secure_string.unlock_str(|key_string| {
         let new_signer = PrivateKeySigner::from_str(key_string).unwrap();
         assert_eq!(signer.address(), new_signer.address());
      });
   }

   #[test]
   #[should_panic]
   fn test_erase() {
      let signer = PrivateKeySigner::random();
      let mut secure_signer = SecureKey::from(signer.clone());
      secure_signer.erase();
      let _address = secure_signer.to_signer().address();
   }

   #[test]
   fn test_is_erased() {
      let signer = PrivateKeySigner::random();
      let mut secure_signer = SecureKey::from(signer.clone());
      assert!(!secure_signer.is_erased());
      secure_signer.erase();
      assert!(secure_signer.is_erased());
   }

   #[cfg(feature = "serde")]
   #[test]
   fn test_serde() {
      let signer = PrivateKeySigner::random();
      let secure_signer = SecureKey::from(signer.clone());

      let json_string = serde_json::to_string(&secure_signer).unwrap();
      let json_bytes = serde_json::to_vec(&secure_signer).unwrap();

      // The vault payload goes through the same `serialize_struct` call that
      // produces this JSON, so pin the struct and field names here: they must
      // stay exactly what `derive` produced, or stored vaults stop decoding.
      let expected_prefix = "{\"address\":\"0x";
      assert!(
         json_string.starts_with(expected_prefix),
         "unexpected key JSON: {json_string}"
      );
      assert!(json_string.contains(",\"data\":"));

      let deserialized_string: SecureKey = serde_json::from_str(&json_string).unwrap();
      let deserialized_bytes: SecureKey = serde_json::from_slice(&json_bytes).unwrap();

      let signer2 = deserialized_string.to_signer();
      let signer3 = deserialized_bytes.to_signer();

      assert_eq!(signer.address(), signer2.address());
      assert_eq!(signer.address(), signer3.address());
   }
}
