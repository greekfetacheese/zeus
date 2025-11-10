use super::secure_key::SecureKey;
use alloy_primitives::Address;
use alloy_signer_local::{
   LocalSignerError, MnemonicBuilder, PrivateKeySigner, coins_bip39::English,
};
use anyhow::anyhow;
use argon2_rs::Argon2;
use rand::RngCore;
use secure_types::{SecureString, SecureVec, Zeroize};
use sha3::{Digest, Sha3_512};
use std::str::FromStr;
use zeus_bip32::{
   BIP32_HARDEN, DEFAULT_DERIVATION_PATH, DerivationPath, SecureXPriv, XKeyInfo, root_from_seed,
};

/// Derive the seed from the given username and password
pub fn derive_seed(
   username: &SecureString,
   password: &SecureString,
   m_cost: u32,
   t_cost: u32,
   p_cost: u32,
) -> Result<SecureVec<u8>, anyhow::Error> {

   let mut hasher = Sha3_512::new();

   username.unlock_str(|username| {
      hasher.update(username.as_bytes());
   });

   let mut result = hasher.finalize();
   let username_hash = result.to_vec();
   result.zeroize();

   let argon2 = Argon2::new(m_cost, t_cost, p_cost);

   let seed = password.unlock_str(|password| argon2.hash_password(password, username_hash))?;
   let secure_seed = SecureVec::from_vec(seed)?;

   // Should never happen but if seed is not 64 bytes, return an error
   // I prefer not to panic here
   if secure_seed.len() != 64 {
      return Err(anyhow!(
         "Seed is not 64 bytes long, this is a bug"
      ));
   }

   Ok(secure_seed)
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone)]
pub struct Wallet {
   pub name: String,
   pub key: SecureKey,
   pub xkey_info: Option<XKeyInfo>,
}

impl PartialEq for Wallet {
   fn eq(&self, other: &Wallet) -> bool {
      self.key.address() == other.key.address()
   }
}

impl Eq for Wallet {}

impl Wallet {
   pub fn new(name: String, key: SecureKey, xkey_info: Option<XKeyInfo>) -> Self {
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

   pub fn name_with_id_short(&self) -> String {
      let id = if self.is_master() {
         "(M)"
      } else if self.is_child() {
         "(C)"
      } else {
         "(I)"
      };

      format!("{} {}", self.name, id)
   }

   /// Create a new wallet from a random private key
   pub fn new_rng(name: String) -> Self {
      let key = SecureKey::random();
      Self {
         name,
         key,
         xkey_info: None,
      }
   }

   /// Create a new wallet from a given private key
   pub fn new_from_key_str(name: String, key_str: SecureString) -> Result<Self, LocalSignerError> {
      let signer = key_str.unlock_str(|key_str| PrivateKeySigner::from_str(key_str))?;
      let key = SecureKey::from(signer);

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
      let phrase_string = phrase.unlock_str(|phrase| phrase.to_string());
      let wallet = MnemonicBuilder::<English>::default()
         .phrase(phrase_string)
         .index(0)?
         .build()
         .map_err(|_| anyhow!("It seems that the given phrase is invalid"))?;

      let key = SecureKey::from(wallet);

      Ok(Self {
         name,
         key,
         xkey_info: None,
      })
   }

   /// Return the derivation path of the wallet as a string
   pub fn derivation_path_string(&self) -> String {
      if let Some(info) = self.xkey_info.as_ref() {
         let base_path = DerivationPath::from_str(DEFAULT_DERIVATION_PATH).unwrap();
         let path = base_path.extended(info.index);
         return path.derivation_string();
      } else {
         return DEFAULT_DERIVATION_PATH.to_string();
      }
   }

   /// Return the derivation path of the wallet
   pub fn derivation_path(&self) -> DerivationPath {
      if let Some(info) = self.xkey_info.as_ref() {
         let base_path = DerivationPath::from_str(DEFAULT_DERIVATION_PATH).unwrap();
         let path = base_path.extended(info.index);
         return path;
      } else {
         return DerivationPath::from_str(DEFAULT_DERIVATION_PATH).unwrap();
      }
   }

   /// Return the derivation index of the wallet
   pub fn index(&self) -> u32 {
      if let Some(info) = self.xkey_info.as_ref() {
         return info.index;
      } else {
         return 0;
      }
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
      self.xkey_info.as_ref().map(|info| info.parent.is_zero()).unwrap_or(false)
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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone)]
pub struct SecureHDWallet {
   /// The master wallet used to derive the children wallets
   pub master_wallet: Wallet,

   /// The children wallets derived from the master wallet
   pub children: Vec<Wallet>,

   /// Keep track of the next child index to derive
   ///
   /// We do `next_child_index + BIP32_HARDEN`
   pub next_child_index: u32,
}

impl SecureHDWallet {
   pub fn random() -> Self {
      let mut bytes = [0u8; 64];
      rand::rngs::OsRng.fill_bytes(&mut bytes);

      let (key, key_info) = root_from_seed(&bytes, None).unwrap();
      let signer = SecureKey::from(key);

      let master_wallet = Wallet {
         name: "Master Wallet".to_string(),
         key: signer,
         xkey_info: Some(key_info),
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

   pub fn contains_child(&self, address: Address) -> bool {
      self.children.iter().find(|c| c.address() == address).is_some()
   }

   /// Create a new `SecureHDWallet` from a seed
   pub fn new_from_seed(name_opt: Option<String>, seed: SecureVec<u8>) -> Self {
      let (key, key_info) = seed.unlock_slice(|slice| root_from_seed(slice, None).unwrap());

      let name = match name_opt {
         Some(name) => name,
         None => "Master Wallet".to_string(),
      };

      let master_wallet = Wallet {
         name,
         key: key.into(),
         xkey_info: Some(key_info),
      };

      Self {
         master_wallet,
         children: Vec::new(),
         next_child_index: 0,
      }
   }

   fn master_to_xpriv(&self) -> SecureXPriv {
      SecureXPriv {
         key: self.master_wallet.key.key(),
         xkey_info: self.master_wallet.xkey_info.clone().unwrap(),
      }
   }

   /// Derive a new child wallet using the current path
   pub fn derive_child(&mut self, name: String) -> Result<Address, anyhow::Error> {
      let xpriv = self.master_to_xpriv();

      let base_path = DerivationPath::from_str(DEFAULT_DERIVATION_PATH)?;
      let child_path = base_path.extended(self.next_child_index + BIP32_HARDEN);

      let child = xpriv.derive_path(child_path.clone())?;
      let pkey = child.key.unlock(|slice| PrivateKeySigner::from_slice(slice))?;

      let address = pkey.address();
      let child_wallet = Wallet {
         name: name.clone(),
         key: pkey.into(),
         xkey_info: Some(child.xkey_info),
      };

      if self.children.contains(&child_wallet) {
         self.next_child_index += 1;
         self.derive_child(name)
      } else {
         self.children.push(child_wallet);
         self.next_child_index += 1;
         Ok(address)
      }
   }

   /// Derive a new child wallet using the given name and index
   ///
   /// Does not increments the `next_child_index` nor it does save the wallet
   pub fn derive_child_at(&self, name: String, index: u32) -> Result<Wallet, anyhow::Error> {
      let xpriv = self.master_to_xpriv();

      let base_path = DerivationPath::from_str(DEFAULT_DERIVATION_PATH)?;
      let child_path = base_path.extended(index);

      let child = xpriv.derive_path(child_path.clone())?;
      let pkey = child.key.unlock(|slice| PrivateKeySigner::from_slice(slice).unwrap());

      let wallet = Wallet {
         name: name.clone(),
         key: pkey.into(),
         xkey_info: Some(child.xkey_info),
      };

      Ok(wallet)
   }

   /// Derive a new child wallet using the given name and index
   ///
   /// Does not increments the `next_child_index` but adds the wallet to [Self::children]
   pub fn derive_child_at_mut(
      &mut self,
      name: String,
      index: u32,
   ) -> Result<Wallet, anyhow::Error> {
      let xpriv = self.master_to_xpriv();

      let base_path = DerivationPath::from_str(DEFAULT_DERIVATION_PATH)?;
      let child_path = base_path.extended(index);

      let child = xpriv.derive_path(child_path.clone())?;
      let pkey = child.key.unlock(|slice| PrivateKeySigner::from_slice(slice).unwrap());

      let wallet = Wallet {
         name: name.clone(),
         key: pkey.into(),
         xkey_info: Some(child.xkey_info),
      };

      if self.children.contains(&wallet) {
         return Err(anyhow!(
            "Wallet At {} with Address {} already exists",
            child_path.derivation_string(),
            wallet.address()
         ));
      } else {
         self.children.push(wallet.clone());
         Ok(wallet)
      }
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

      let seed = derive_seed(
         &username,
         &password,
         TEST_M_COST,
         TEST_T_COST,
         TEST_P_COST,
      )
      .unwrap();

      let mut hd_wallet = SecureHDWallet::new_from_seed(None, seed);
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

         let path = children.derivation_path_string();
         eprintln!(
            "Child: {} Path: {} Address: {}",
            i,
            path,
            children.address()
         );
      }
   }
}
