use super::SafeSigner;
use alloy_network::EthereumWallet;
use zeroize::Zeroize;

/// Wrapper around [EthereumWallet] that zeroizes the key when dropped
///
/// Do not clone the inner value, as it will not be erased when the outer value is dropped
#[derive(Clone)]
pub struct SafeWallet(EthereumWallet);

impl SafeWallet {
   /// Do not clone the inner value, as it will not be erased when the outer value is dropped
   pub fn inner(&self) -> &EthereumWallet {
      &self.0
   }

   pub fn erase(&mut self) {
      unsafe {
         // Get a mutable pointer to the key
         let key_ptr: *mut EthereumWallet = &mut self.0;

         // Convert the key to a byte slice
         let key_bytes: &mut [u8] =
            std::slice::from_raw_parts_mut(key_ptr as *mut u8, std::mem::size_of::<EthereumWallet>());

         // Zeroize the byte slice
         key_bytes.zeroize();
      }
   }
}

impl Drop for SafeWallet {
   fn drop(&mut self) {
      self.erase();
   }
}

impl From<EthereumWallet> for SafeWallet {
   fn from(wallet: EthereumWallet) -> Self {
      Self(wallet)
   }
}

impl From<SafeSigner> for SafeWallet {
   fn from(signer: SafeSigner) -> Self {
      Self(EthereumWallet::from(signer.inner().clone()))
   }
}

mod tests {
   #[allow(unused_imports)]
   use super::{SafeSigner, SafeWallet};

   #[test]
   fn test_key_erase() {
      let mut safe_wallet = SafeWallet::from(SafeSigner::random());
      safe_wallet.erase();

      println!("Wallet erased: {:?}", safe_wallet.0);
   }
}
