use alloy_network::EthereumWallet;
use alloy_primitives::Address;
use alloy_signer_local::PrivateKeySigner;
use secure_types::{SecureString, SecureVec, Zeroize};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct SecureSigner {
   address: Address,
   vec: SecureVec<u8>,
}

impl SecureSigner {
   pub fn random() -> Self {
      let signer = PrivateKeySigner::random();
      Self::from(signer)
   }

   /// Return the signer's key in a SecureString
   pub fn key_string(&self) -> SecureString {
      let signer = self.to_signer();
      let mut key = signer.to_bytes();
      let string = key.iter().map(|b| format!("{b:02x}")).collect::<String>();
      key.zeroize();
      SecureString::from(string)
   }

   /// Securely erase the signer's key from memory
   pub fn erase(&mut self) {
      self.vec.erase();
   }

   pub fn is_erased(&self) -> bool {
      self.vec.len() == 0
   }

   pub fn address(&self) -> Address {
      self.address
   }

   pub fn to_signer(&self) -> PrivateKeySigner {
      let signer = self
         .vec
         .slice_scope(|bytes| PrivateKeySigner::from_slice(bytes).expect("Failed to create signer"));
      signer
   }

   pub fn to_wallet(&self) -> EthereumWallet {
      EthereumWallet::from(self.to_signer())
   }
}

impl From<PrivateKeySigner> for SecureSigner {
   fn from(value: PrivateKeySigner) -> Self {
      let bytes = value.to_bytes().to_vec();
      let secure_vec = SecureVec::from_vec(bytes).unwrap();
      let address = value.address();
      SecureSigner {
         address,
         vec: secure_vec,
      }
   }
}

pub fn erase_signer(mut signer: PrivateKeySigner) {
   unsafe {
      let ptr: *mut PrivateKeySigner = &mut signer;
      let bytes: &mut [u8] = core::slice::from_raw_parts_mut(ptr as *mut u8, core::mem::size_of::<PrivateKeySigner>());
      bytes.zeroize();
   }
}

pub fn erase_wallet(mut wallet: EthereumWallet) {
   unsafe {
      let ptr: *mut EthereumWallet = &mut wallet;
      let bytes: &mut [u8] = std::slice::from_raw_parts_mut(ptr as *mut u8, std::mem::size_of::<EthereumWallet>());
      bytes.zeroize();
   }
}

#[cfg(test)]
mod tests {
   use std::str::FromStr;

   use super::*;

   #[test]
   fn test_create() {
      let signer = PrivateKeySigner::random();
      let secure_signer = SecureSigner::from(signer.clone());
      let signer2 = secure_signer.to_signer();
      assert_eq!(signer.address(), signer2.address());
   }

   #[test]
   fn test_key_string() {
      let signer = PrivateKeySigner::random();
      let secure_signer = SecureSigner::from(signer.clone());
      let key_secure_string = secure_signer.key_string();

      key_secure_string.str_scope(|key_string| {
         let new_signer = PrivateKeySigner::from_str(key_string).unwrap();
         assert_eq!(signer.address(), new_signer.address());
      });
   }

   #[test]
   #[should_panic]
   fn test_erase() {
      let signer = PrivateKeySigner::random();
      let mut secure_signer = SecureSigner::from(signer.clone());
      secure_signer.erase();
      let _address = secure_signer.to_signer().address();
   }

   #[test]
   fn test_serde() {
      let signer = PrivateKeySigner::random();
      let secure_signer = SecureSigner::from(signer.clone());
      let json = serde_json::to_string(&secure_signer).unwrap();
      let deserialized: SecureSigner = serde_json::from_str(&json).unwrap();

      let signer2 = deserialized.to_signer();
      assert_eq!(signer.address(), signer2.address());
   }
}
