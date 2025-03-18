use crate::signer::SecureSigner;
use alloy_network::EthereumWallet;
use alloy_signer_local::PrivateKeySigner;
use std::borrow::Borrow;
use secure_types::Zeroize;
use memsec::{mlock, munlock};


/// Wrapper type around [EthereumWallet]
///
/// - Securely erases the key when it is dropped
/// - For `Windows` calls `VirtualLock` to protect the key from being swapped out to disk
/// - For `Unix` calls `mlock` to prevent the key from being swapped to disk and memory dumped
///
/// ### Note on `Windows` is not possible to prevent memory dumping
#[derive(Debug)]
pub struct SecureWallet {
   wallet: EthereumWallet,
}

impl SecureWallet {
   pub fn new(mut wallet: EthereumWallet) -> Self {
      unsafe {
         let ptr: *mut EthereumWallet = &mut wallet;
         let ptr = ptr as *mut u8;
         mlock(ptr, std::mem::size_of::<EthereumWallet>());
      }
      SecureWallet { wallet }
   }

   pub fn borrow(&self) -> &EthereumWallet {
      self.wallet.borrow()
   }

   pub fn erase(&mut self) {
      unsafe {
         let ptr: *mut EthereumWallet = &mut self.wallet;
         let bytes: &mut [u8] = std::slice::from_raw_parts_mut(ptr as *mut u8, std::mem::size_of::<EthereumWallet>());
         bytes.zeroize();
      }
   }

   // ! This actually doesnt work, it always return false
   #[warn(deprecated)]
   pub fn is_erased(&self) -> bool {
      unsafe {
         let mut clone = self.clone();
         let ptr: *mut EthereumWallet = &mut clone.wallet;
         let bytes: &mut [u8] = std::slice::from_raw_parts_mut(ptr as *mut u8, std::mem::size_of::<EthereumWallet>());
         let erased = bytes.iter().all(|b| *b == 0);
         bytes.zeroize();
         erased
      }
   }
}

impl Drop for SecureWallet {
   fn drop(&mut self) {
      self.erase();
      unsafe {
         let ptr: *mut EthereumWallet = &mut self.wallet;
         let ptr = ptr as *mut u8;
         munlock(ptr, std::mem::size_of::<EthereumWallet>());
      }
   }
}

impl Clone for SecureWallet {
   fn clone(&self) -> Self {
      Self::new(self.wallet.clone())
   }
}

impl Borrow<EthereumWallet> for SecureWallet {
   fn borrow(&self) -> &EthereumWallet {
      &self.wallet
   }
}

impl From<PrivateKeySigner> for SecureWallet {
   fn from(signer: PrivateKeySigner) -> Self {
      Self::new(signer.into())
   }
}

impl From<SecureSigner> for SecureWallet {
   fn from(signer: SecureSigner) -> Self {
      Self::new(signer.into_signer().into())
   }
}
 

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn test_wallet() {
      let wallet = EthereumWallet::from(PrivateKeySigner::random());
      println!("Wallet Address: {:?}", wallet.default_signer().address());
   }

   #[test]
   fn test_create_from_signer() {
      let wallet = SecureWallet::from(PrivateKeySigner::random());
      let address = wallet.wallet.default_signer().address();
      assert_eq!(address.len(), 20);
      println!("Wallet Address: {:?}", address);
   }

   #[test]
   fn test_create_from_secure_signer() {
      let secure_signer = SecureSigner::random();
      let wallet = SecureWallet::from(secure_signer);
      let address = wallet.wallet.default_signer().address();
      assert_eq!(address.len(), 20);
      println!("Wallet Address: {:?}", address);
   }
   

   #[test]
   #[should_panic]
   fn test_erase_wallet() {
      let secure_signer = SecureSigner::random();
      let mut secure_wallet = SecureWallet::from(secure_signer.into_signer());
      println!("Wallet Address before erase: {:?}", secure_wallet.wallet.default_signer().address());
      secure_wallet.erase();
      // this should panic
      let _address = secure_wallet.wallet.default_signer().address();
   }
   
}
