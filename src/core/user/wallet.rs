use alloy_signer_local::{MnemonicBuilder, PrivateKeySigner, coins_bip39::English};
use anyhow::anyhow;
use ncrypt_me::{Argon2, Credentials};
use rand::RngCore;
use secure_types::{SecureString, SecureVec, Zeroize};
use std::str::FromStr;
use zeus_eth::{alloy_primitives::Address, utils::SecureSigner};

use crate::core::bip32::{path::*, primitives::XKeyInfo, xpriv::SecureXPriv};

// Argon2 parameters used to derive the seed from the credentials
// Hash lenght is always 64 bytes (512 bits)
pub const M_COST: u32 = 8192_000;
pub const T_COST: u32 = 128;
pub const P_COST: u32 = 64;

const DEV_M_COST: u32 = 16_000;
const DEV_T_COST: u32 = 3;
const DEV_P_COST: u32 = 8;


/// Helper struct to store info for a wallet (name, address, etc)
/// Useful to avoid unecessery cloning of the [SecureSigner]
/// which is expensive
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WalletInfo {
   pub address: Address,
   name: String,
   pub is_master: bool,
   pub is_child: bool,
   pub is_imported: bool,
}

impl WalletInfo {
   pub fn new(
      address: Address,
      name: String,
      is_master: bool,
      is_child: bool,
      is_imported: bool,
   ) -> Self {
      Self {
         address,
         name,
         is_master,
         is_child,
         is_imported,
      }
   }

   pub fn is_master(&self) -> bool {
      self.is_master
   }

   pub fn is_child(&self) -> bool {
      self.is_child
   }

   pub fn is_imported(&self) -> bool {
      self.is_imported
   }

   pub fn name(&self) -> String {
      let id = if self.is_master() {
         "(Master)"
      } else if self.is_child() {
         "(Child)"
      } else {
         "(Imported)"
      };

      format!("{} {}", self.name, id)
   }

   pub fn address_truncated(&self) -> String {
      format!(
         "{}...{}",
         &self.address.to_string()[..6],
         &self.address.to_string()[36..]
      )
   }
}


use sha3::{Digest, Sha3_512};

pub fn derive_seed(credentials: &Credentials) -> Result<SecureVec<u8>, anyhow::Error> {
   credentials.is_valid()?;

   let mut hasher = Sha3_512::new();

   credentials.username.str_scope(|username| {
      hasher.update(username.as_bytes());
   });

   let mut result = hasher.finalize();
   let username_hash = result.to_vec();
   result.zeroize();

   let m_cost = if cfg!(feature = "dev") {
      DEV_M_COST
   } else {
      M_COST
   };

   let t_cost = if cfg!(feature = "dev") {
      DEV_T_COST
   } else {
      T_COST
   };

   let p_cost = if cfg!(feature = "dev") {
      DEV_P_COST
   } else {
      P_COST
   };

   let argon2 = Argon2::new(m_cost, t_cost, p_cost);

   let seed = argon2.hash_password(&credentials.password, username_hash)?;

   Ok(seed)
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Wallet {
   pub name: String,
   pub key: SecureSigner,
   pub xkey_info: Option<XKeyInfo>,
}

impl Wallet {
   pub fn new(name: String, key: SecureSigner, xkey_info: Option<XKeyInfo>) -> Self {
      Self {
         name,
         key,
         xkey_info,
      }
   }

   pub fn name_with_id(&self) -> String {
      let id = if self.is_master() {
         "(Master)"
      } else if self.is_child() {
         "(Child)"
      } else {
         "(Imported)"
      };

      format!("{} {}", self.name, id)
   }

   pub fn to_wallet_info(&self) -> WalletInfo {
      WalletInfo::new(
         self.address(),
         self.name.clone(),
         self.is_master(),
         self.is_child(),
         self.is_imported(),
      )
   }

   /// Create a new wallet from a random private key
   pub fn new_rng(name: String) -> Self {
      let key = SecureSigner::random();

      Self {
         name,
         key,
         xkey_info: None,
      }
   }

   /// Create a new wallet from a given private key
   pub fn new_from_key(name: String, key_str: SecureString) -> Result<Self, anyhow::Error> {
      let key = key_str.str_scope(|key_str| PrivateKeySigner::from_str(key_str))?;
      let key = SecureSigner::from(key);

      Ok(Self {
         name,
         key,
         xkey_info: None,
      })
   }

   /// Create a new wallet from a mnemonic phrase
   pub fn new_from_mnemonic(name: String, phrase: SecureString) -> Result<Self, anyhow::Error> {
      // return a custom error to not expose the phrase in case it just has a typo
      // TODO: Erase the MnemonicBuilder
      let phrase_string = phrase.str_scope(|phrase| phrase.to_string());
      let wallet = MnemonicBuilder::<English>::default()
         .phrase(phrase_string)
         .index(0)?
         .build()
         .map_err(|_| anyhow!("It seems that the given phrase is invalid"))?;

      let key = SecureSigner::from(wallet);

      Ok(Self {
         name,
         key,
         xkey_info: None,
      })
   }

   pub fn address(&self) -> Address {
      self.key.address()
   }

   /// If [XKeyInfo] is None, that means the wallet is imported
   ///
   /// Otherwise, this wallet is a children of a parent wallet and has a [XKeyInfo]
   pub fn is_imported(&self) -> bool {
      self.xkey_info.is_none()
   }

   pub fn is_child(&self) -> bool {
      self.xkey_info.is_some() && !self.is_master()
   }

   pub fn is_master(&self) -> bool {
      self.xkey_info.as_ref().map(|info| info.index == 0).unwrap_or(false)
   }

   pub fn is_hardened(&self) -> bool {
      if self.is_master() {
         return true;
      } else {
         return self.xkey_info.as_ref().map(|info| info.index >= BIP32_HARDEN).unwrap_or(false);
      }
   }

   pub fn is_key_erased(&self) -> bool {
      self.key.is_erased()
   }

   pub fn key_string(&self) -> SecureString {
      self.key.key_string()
   }
}

/// Represesents a hierarchical deterministic wallet
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct SecureHDWallet {
   /// The master wallet used to derive the children wallets
   pub master_wallet: Wallet,

   /// The children wallets derived from the master wallet
   pub children: Vec<Wallet>,

   /// Keep track of the next child index to derive
   /// 
   /// Note: This is not the same as the [Self::START_INDEX] but just a counter
   /// used internally
   #[serde(default)]
   pub next_child_index: u32,
}

impl SecureHDWallet {

   pub const START_INDEX: u32 = BIP32_HARDEN;

   pub fn random() -> Self {
      let mut bytes = [0u8; 64];
      rand::rngs::OsRng.fill_bytes(&mut bytes);

      let xpriv = SecureXPriv::root_from_seed(&bytes, None).unwrap();

      let master_wallet = Wallet {
         name: "Master Wallet".to_string(),
         key: xpriv.signer,
         xkey_info: Some(xpriv.xkey_info),
      };

      Self {
         master_wallet,
         children: Vec::new(),
         next_child_index: 0,
      }
   }

   pub fn erase(&mut self) {
      self.master_wallet.key.erase();
      for child in self.children.iter_mut() {
         child.key.erase();
      }
   }

   /// Create a new `SecureHDWallet` from the given [Credentials]
   pub fn new_from_credentials(
      mut name: String,
      credentials: Credentials,
   ) -> Result<Self, anyhow::Error> {
      let seed = derive_seed(&credentials)?;
      let xpriv = seed.slice_scope(|slice| SecureXPriv::root_from_seed(slice, None))?;

      if name.is_empty() {
         name = "Master Wallet".to_string();
      }

      let master_wallet = Wallet {
         name,
         key: xpriv.signer,
         xkey_info: Some(xpriv.xkey_info),
      };

      Ok(Self {
         master_wallet,
         children: Vec::new(),
         next_child_index: 0,
      })
   }

   /// Create a new `SecureHDWallet` from a seed
   pub fn new_from_seed(name: String, seed: SecureVec<u8>) -> Self {
      let xpriv = seed.slice_scope(|slice| SecureXPriv::root_from_seed(slice, None).unwrap());

      let master_wallet = Wallet {
         name,
         key: xpriv.signer,
         xkey_info: Some(xpriv.xkey_info),
      };

      Self {
         master_wallet,
         children: Vec::new(),
         next_child_index: 0,
      }
   }

   pub fn master_to_xpriv(&self) -> SecureXPriv {
      SecureXPriv {
         signer: self.master_wallet.key.clone(),
         xkey_info: self.master_wallet.xkey_info.clone().unwrap(),
      }
   }

   /// Derive a new child wallet using the current path
   pub fn derive_child(&mut self, name:  String) -> Result<Address, anyhow::Error> {
      let xpriv = self.master_to_xpriv();

      let base_path = DerivationPath::from_str(DEFAULT_DERIVATION_PATH)?;
      let child_path = base_path.extended(self.next_child_index + BIP32_HARDEN);
      // eprintln!("Child Path: {}", child_path.derivation_string());

      let child = xpriv.derive_path(child_path)?;

      let address = child.signer.address();
      let child_wallet = Wallet {
         name,
         key: child.signer,
         xkey_info: Some(child.xkey_info),
      };

      self.children.push(child_wallet);
      self.next_child_index += 1;
      Ok(address)
   }
}

#[cfg(test)]
mod tests {

   use super::*;
   const TEST_M_COST: u32 = 16_000;
   const TEST_T_COST: u32 = 5;
   const TEST_P_COST: u32 = 4;

   #[test]
   fn test_hd_wallet_creation() {
      let username = SecureString::from("username");
      let password = SecureString::from("password");
      let confirm_password = SecureString::from("password");

      let credentials = Credentials::new(username, password, confirm_password);
      let seed = derive_seed_test(&credentials).unwrap();
      let mut hd_wallet = SecureHDWallet::new_from_seed("Master Wallet".to_string(), seed);
      eprintln!(
         "Master Wallet Address: {}",
         hd_wallet.master_wallet.address()
      );

      assert!(hd_wallet.master_wallet.is_master());

      for i in 0..10 {
         let name = format!("Child Wallet {}", i);
         hd_wallet.derive_child(name).unwrap();
      }

      for (i, children) in hd_wallet.children.iter().enumerate() {
         assert!(!children.is_master());
         assert!(!children.is_imported());
         assert!(children.is_hardened());
         assert!(children.is_child());
         let info = children.xkey_info.as_ref().unwrap();

         eprintln!(
            "Child {} Depth {} Index {} Address: {}",
            i,
            info.depth,
            info.index,
            children.address()
         );
      }
   }

   fn derive_seed_test(credentials: &Credentials) -> Result<SecureVec<u8>, anyhow::Error> {
      credentials.is_valid()?;

      let mut hasher = Sha3_512::new();

      credentials.username.str_scope(|username| {
         hasher.update(username.as_bytes());
      });

      let mut result = hasher.finalize();
      let username_hash = result.to_vec();
      result.zeroize();

      let argon2 = Argon2::new(TEST_M_COST, TEST_T_COST, TEST_P_COST);

      let seed = argon2.hash_password(&credentials.password, username_hash)?;

      Ok(seed)
   }
}
