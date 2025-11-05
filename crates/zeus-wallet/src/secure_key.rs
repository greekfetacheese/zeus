use alloy_network::EthereumWallet;
use alloy_primitives::{Address, hex};
use alloy_signer_local::PrivateKeySigner;
use k256::ecdsa::{SigningKey, VerifyingKey};
use secure_types::{SecureArray, SecureString, Zeroize};

/// Wrapper around a [SecureArray<u8, 32>] that holds the private key
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone)]
pub struct SecureKey {
   address: Address,
   data: SecureArray<u8, 32>,
}

impl SecureKey {
   /// Generate a new random private key
   pub fn random() -> Self {
      let signer = PrivateKeySigner::random();
      Self::from(signer)
   }

   /// Return the private key by cloning it
   pub fn key(&self) -> SecureArray<u8, 32> {
      self.data.clone()
   }

   /// Return the key in a SecureString
   pub fn key_string(&self) -> SecureString {
      let signer = self.to_signer();
      let mut key = signer.to_bytes();
      let string = hex::encode(&key);
      key.zeroize();
      SecureString::from(string)
   }

   /// Securely erase the key from memory
   pub fn erase(&mut self) {
      self.data.erase();
   }

   pub fn is_erased(&self) -> bool {
      self.data.unlock(|slice| slice.iter().all(|byte| byte == &0))
   }

   pub fn address(&self) -> Address {
      self.address
   }

   /// Converts the private key to a [PrivateKeySigner] by cloning the inner key
   ///
   /// # Panics
   /// Panics if the key is not a valid form of a private key
   /// or if it has been erased
   pub fn to_signer(&self) -> PrivateKeySigner {
      self.data.unlock(|bytes| PrivateKeySigner::from_slice(bytes).unwrap())
   }

   /// Converts the private key to a [SigningKey] by cloning the inner key
   ///
   /// # Panics
   /// Panics if the key is not a valid form of a private key
   /// or if it has been erased
   pub fn to_signing_key(&self) -> SigningKey {
      self.data.unlock(|bytes| SigningKey::from_slice(bytes).unwrap())
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

impl From<SecureArray<u8, 32>> for SecureKey {
   /// Converts a [SecureArray<u8, 32>] to a [SecureKey]
   ///
   /// # Panics
   /// Panics if the key is not a valid form of a private key
   fn from(value: SecureArray<u8, 32>) -> Self {
      let signer = value.unlock(|slice| PrivateKeySigner::from_slice(slice).unwrap());
      let address = signer.address();
      SecureKey {
         address,
         data: value,
      }
   }
}

impl From<PrivateKeySigner> for SecureKey {
   fn from(value: PrivateKeySigner) -> Self {
      let address = value.address();
      let mut key_bytes = value.to_bytes();
      let data = SecureArray::from_slice_mut(key_bytes.as_mut()).unwrap();

      SecureKey { address, data }
   }
}

impl From<SigningKey> for SecureKey {
   fn from(value: SigningKey) -> Self {
      let mut bytes = value.to_bytes();
      let signer = PrivateKeySigner::from_slice(&bytes).unwrap();
      let address = signer.address();
      let data = SecureArray::from_slice_mut(bytes.as_mut()).unwrap();

      SecureKey { address, data }
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

      let deserialized_string: SecureKey = serde_json::from_str(&json_string).unwrap();
      let deserialized_bytes: SecureKey = serde_json::from_slice(&json_bytes).unwrap();

      let signer2 = deserialized_string.to_signer();
      let signer3 = deserialized_bytes.to_signer();

      assert_eq!(signer.address(), signer2.address());
      assert_eq!(signer.address(), signer3.address());
   }
}
