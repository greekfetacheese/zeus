use super::secure_key::SecureKey;
use super::{Deriver, Error};
use alloy_primitives::Address;
use alloy_signer_local::{
   LocalSignerError, MnemonicBuilder, PrivateKeySigner,
   coins_bip39::{English, Mnemonic},
};
use rand::RngCore;
use secure_types::{SecureArray, SecureString, SecureVec, Zeroize};
use std::str::FromStr;
use zeus_bip32::{
   BIP32_HARDEN, DEFAULT_DERIVATION_PATH, DerivationPath, SecureXPriv, XKeyInfo, root_from_seed,
};

fn base_path() -> DerivationPath {
   DerivationPath::from_str(DEFAULT_DERIVATION_PATH)
      .expect("DEFAULT_DERIVATION_PATH is a valid path")
}

/// A single wallet: a private key plus what is known about where it came from.
///
/// A wallet is one of three kinds: the master of a [`SecureHDWallet`] (no parent,
/// so [`Wallet::is_master`]), a child of that master ([`Wallet::is_child`], carrying
/// an [`XKeyInfo`]), or an imported key with no derivation info at all
/// ([`Wallet::is_imported`]). Only the first two can be reproduced from credentials.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone)]
#[non_exhaustive]
pub struct Wallet {
   /// Human-readable label, e.g. `"Master Wallet"` or the name given to a child.
   pub name: String,
   /// The mnemonic this wallet was imported from, if it was imported from one.
   ///
   /// [`None`] for wallets derived from a seed, since their master can re-derive them.
   #[cfg_attr(feature = "serde", serde(default))]
   pub seed_phrase: Option<SecureString>,
   /// The private key and its address.
   pub key: SecureKey,
   /// The BIP32 derivation info: the child index and the chain code.
   ///
   /// [`None`] means the wallet is imported and was not derived from this crate's
   /// master, which is exactly what [`Wallet::is_imported`] reports.
   pub xkey_info: Option<XKeyInfo>,
}

impl PartialEq for Wallet {
   fn eq(&self, other: &Wallet) -> bool {
      self.key.address() == other.key.address()
   }
}

impl Eq for Wallet {}

impl Wallet {
   /// Build a wallet from its parts.
   ///
   /// `xkey_info` decides which kind it is: [`None`] makes it imported, [`Some`] makes
   /// it a master or a child depending on that info's parent.
   pub fn new(
      name: String,
      seed_phrase: Option<SecureString>,
      key: SecureKey,
      xkey_info: Option<XKeyInfo>,
   ) -> Self {
      Self {
         name,
         seed_phrase,
         key,
         xkey_info,
      }
   }

   /// Erase the wallet key and all associated data
   pub fn erase(&mut self) {
      self.key.erase();

      if let Some(ref mut xkey_info) = self.xkey_info {
         xkey_info.chain_code.data.erase();
      }

      if let Some(ref mut seed_phrase) = self.seed_phrase {
         seed_phrase.erase();
      }
   }

   /// Returns the private key + ChainCode as a 64-byte SecureArray
   pub fn full_key(&self) -> Result<SecureArray<u8, 64>, Error> {
      let Some(info) = self.xkey_info.as_ref() else {
         return Err(Error::XKeyInfoIsMissing);
      };

      let key = self.key.key();
      let chain_code = info.chain_code.data.clone();

      let mut bytes = [0u8; 64];

      key.unlock(|slice| bytes[..32].copy_from_slice(slice));
      chain_code.unlock(|slice| bytes[32..].copy_from_slice(slice));

      let full_key = SecureArray::from_slice_mut(&mut bytes)?;

      Ok(full_key)
   }

   /// Generate the seed needed to derive the RailgunKeys and the RailgunAddress
   pub fn seed(&self) -> Result<SecureArray<u8, 64>, Error> {
      if self.xkey_info.is_some() {
         let key = self.full_key()?;
         return Ok(key);
      }

      let Some(seed) = self.seed_phrase.as_ref() else {
         return Err(Error::WalletIsNotChildOrMaster);
      };

      let mnemonic = seed
         .unlock_str(Mnemonic::<English>::new_from_phrase)
         .map_err(|_| Error::Custom("invalid seed phrase".to_string()))?;
      let mut bytes = mnemonic
         .to_seed("".into())
         .map_err(|_| Error::Custom("failed to derive seed from phrase".to_string()))?;
      let sec_bytes = SecureArray::from_slice_mut(&mut bytes)?;
      Ok(sec_bytes)
   }

   /// The name followed by `(Master)`, `(Child)` or `(Imported)`.
   pub fn name_with_id(&self) -> String {
      format!("{} {}", self.name, self.kind_tag().0)
   }

   /// The name followed by the one-letter kind tag `(M)`, `(C)` or `(I)`.
   pub fn name_with_id_short(&self) -> String {
      format!("{} {}", self.name, self.kind_tag().1)
   }

   fn kind_tag(&self) -> (&'static str, &'static str) {
      if self.is_master() {
         ("(Master)", "(M)")
      } else if self.is_child() {
         ("(Child)", "(C)")
      } else {
         ("(Imported)", "(I)")
      }
   }

   /// Create a new wallet from a random private key
   pub fn new_rng(name: String) -> Self {
      let key = SecureKey::random();
      Self {
         name,
         seed_phrase: None,
         key,
         xkey_info: None,
      }
   }

   /// Create a new wallet from a given private key
   pub fn new_from_key_str(name: String, key_str: SecureString) -> Result<Self, Error> {
      let signer = key_str.unlock_str(PrivateKeySigner::from_str)?;
      let key = SecureKey::from(signer);

      Ok(Self {
         name,
         seed_phrase: None,
         key,
         xkey_info: None,
      })
   }

   /// Create a new wallet from a mnemonic phrase
   pub fn new_from_mnemonic(name: String, phrase: SecureString) -> Result<Self, Error> {
      let phrase_string = phrase.unlock_str(|phrase| phrase.to_string());
      let wallet = MnemonicBuilder::<English>::default()
         .phrase(phrase_string)
         .index(0)?
         .build()
         .map_err(|_| Error::Custom("It seems that the given phrase is invalid".to_string()))?;

      let key = SecureKey::from(wallet);

      Ok(Self {
         name,
         seed_phrase: Some(phrase),
         key,
         xkey_info: None,
      })
   }

   /// Return the derivation path of the wallet as a string
   pub fn derivation_path_string(&self) -> String {
      if let Some(info) = self.xkey_info.as_ref() {
         let path = base_path().extended(info.index);
         path.derivation_string()
      } else {
         DEFAULT_DERIVATION_PATH.to_string()
      }
   }

   /// Return the derivation path of the wallet
   pub fn derivation_path(&self) -> DerivationPath {
      if let Some(info) = self.xkey_info.as_ref() {
         base_path().extended(info.index)
      } else {
         base_path()
      }
   }

   /// Return the derivation index of the wallet
   pub fn index(&self) -> u32 {
      if let Some(info) = self.xkey_info.as_ref() {
         info.index
      } else {
         0
      }
   }

   /// The Ethereum address of this wallet's key.
   pub fn address(&self) -> Address {
      self.key.address()
   }

   /// If [XKeyInfo] is None, that means the wallet is imported
   ///
   /// Otherwise, this wallet is a children of a parent wallet and has a [XKeyInfo]
   pub fn is_imported(&self) -> bool {
      self.xkey_info.is_none()
   }

   /// Whether this wallet is a non-master child of an [`XKeyInfo`] parent.
   ///
   /// Equivalent to having an `xkey_info` whose `parent` is non-zero.
   pub fn is_child(&self) -> bool {
      self.xkey_info.is_some() && !self.is_master()
   }

   /// Whether this wallet sits at the root of the tree.
   ///
   /// True when its `xkey_info` records a `parent` of zero (`parent == 0`).
   pub fn is_master(&self) -> bool {
      self.xkey_info.as_ref().is_some_and(|info| info.parent.is_zero())
   }

   /// Returns true if we can derive a zkAddress from this wallet.
   pub fn can_derive_zk_address(&self) -> bool {
      self.xkey_info.is_some() || self.seed_phrase.is_some()
   }

   /// Whether this wallet's index is in the hardened range.
   ///
   /// True when `index >= BIP32_HARDEN`; a master wallet is always true, since it has
   /// no non-zero index to compare.
   pub fn is_hardened(&self) -> bool {
      self.is_master() || self.xkey_info.as_ref().is_some_and(|info| info.index >= BIP32_HARDEN)
   }

   /// Whether [`Wallet::erase`] (or the key's own erase) zeroed the private key.
   pub fn is_key_erased(&self) -> bool {
      self.key.is_erased()
   }

   /// The private key as a hex [`SecureString`].
   ///
   /// # Panics
   ///
   /// Panics if the key has been erased or is otherwise not a valid secp256k1 scalar.
   pub fn key_string(&self) -> SecureString {
      self.key.key_string()
   }
}

/// A hierarchical deterministic tree: one master wallet and the children derived from it.
///
/// The master comes from a [`Deriver`] (or [`SecureHDWallet::random`]); children come
/// from [`SecureHDWallet::derive_child`], which walks the hardened range, or from
/// [`SecureHDWallet::derive_child_at`] / `_at_mut` for an explicit index.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone)]
#[non_exhaustive]
pub struct SecureHDWallet {
   /// The master wallet used to derive the children wallets
   pub master_wallet: Wallet,

   /// The children wallets derived from the master wallet
   pub children: Vec<Wallet>,

   /// Keep track of the next child index to derive
   ///
   /// [`SecureHDWallet::derive_child`] derives at `next_child_index + BIP32_HARDEN` and then
   /// increments this counter; the `_at` methods leave it untouched.
   pub next_child_index: u32,
}

impl SecureHDWallet {
   /// Build a wallet tree from 64 fresh random bytes instead of user credentials.
   ///
   /// The bytes are zeroized after the master is derived; the result cannot be
   /// reproduced unless the caller saved something first.
   pub fn random() -> Self {
      let mut bytes = [0u8; 64];
      rand::rngs::OsRng.fill_bytes(&mut bytes);

      let (key, key_info) =
         root_from_seed(&bytes, None).expect("64 random bytes are a valid BIP32 seed");
      bytes.zeroize();
      let signer = SecureKey::from(key);

      let master_wallet = Wallet {
         name: "Master Wallet".to_string(),
         seed_phrase: None,
         key: signer,
         xkey_info: Some(key_info),
      };

      Self {
         master_wallet,
         children: Vec::new(),
         next_child_index: 0,
      }
   }

   /// Erase the master key and every child key, plus any stored seed phrases.
   pub fn erase(&mut self) {
      self.master_wallet.erase();
      for child in self.children.iter_mut() {
         child.erase();
      }
   }

   /// Whether any wallet in [`Self::children`] has this address.
   pub fn contains_child(&self, address: Address) -> bool {
      self.children.iter().any(|c| c.address() == address)
   }

   /// Create a new `SecureHDWallet` from the given deriver
   pub fn new_from_deriver(name: Option<String>, deriver: Deriver) -> Result<Self, Error> {
      let wallet = deriver.new_hd_wallet(name)?;
      Ok(wallet)
   }

   fn master_to_xpriv(&self) -> Result<SecureXPriv, Error> {
      let Some(xkey_info) = self.master_wallet.xkey_info.clone() else {
         return Err(Error::XKeyInfoIsMissing);
      };

      Ok(SecureXPriv {
         key: self.master_wallet.key.key(),
         xkey_info,
      })
   }

   fn derive_wallet_at(&self, name: &str, index: u32) -> Result<(Wallet, DerivationPath), Error> {
      let xpriv = self.master_to_xpriv()?;

      let child_path = base_path().extended(index);

      let child = xpriv.derive_path(child_path.clone())?;
      // `from_slice` reports `k256::ecdsa::Error`; wrapping it in
      // `LocalSignerError` keeps `Error` free of k256's error type while
      // still propagating instead of panicking.
      let pkey = child
         .key
         .unlock(|slice| PrivateKeySigner::from_slice(slice).map_err(LocalSignerError::from))?;

      let wallet = Wallet {
         name: name.to_string(),
         seed_phrase: None,
         key: pkey.into(),
         xkey_info: Some(child.xkey_info),
      };

      Ok((wallet, child_path))
   }

   /// Derive a new child wallet using the current path
   pub fn derive_child(&mut self, name: String) -> Result<Address, Error> {
      loop {
         let index = self.next_child_index + BIP32_HARDEN;
         let (wallet, _) = self.derive_wallet_at(&name, index)?;
         self.next_child_index += 1;

         if self.children.contains(&wallet) {
            continue;
         }

         let address = wallet.address();
         self.children.push(wallet);
         return Ok(address);
      }
   }

   /// Derive a new child wallet using the given name and index
   ///
   /// Does not increments the `next_child_index` nor it does save the wallet
   pub fn derive_child_at(&self, name: String, index: u32) -> Result<Wallet, Error> {
      self.derive_wallet_at(&name, index).map(|(wallet, _)| wallet)
   }

   /// Derive a new child wallet using the given name and index
   ///
   /// Does not increments the `next_child_index` but adds the wallet to [Self::children]
   pub fn derive_child_at_mut(&mut self, name: String, index: u32) -> Result<Wallet, Error> {
      let (wallet, path) = self.derive_wallet_at(&name, index)?;

      if self.children.contains(&wallet) {
         Err(Error::ChildAlreadyExists {
            path,
            address: wallet.address(),
         })
      } else {
         self.children.push(wallet.clone());
         Ok(wallet)
      }
   }

   /// Create a new `SecureHDWallet` from a seed
   #[deprecated(since = "0.1.22", note = "Use new_from_deriver instead")]
   pub fn new_from_seed(name_opt: Option<String>, seed: SecureVec<u8>) -> Self {
      let (key, key_info) = seed.unlock_slice(|slice| {
         root_from_seed(slice, None).expect("BIP32 needs a seed of at least 16 bytes")
      });

      let name = match name_opt {
         Some(name) => name,
         None => "Master Wallet".to_string(),
      };

      let master_wallet = Wallet {
         name,
         seed_phrase: None,
         key: key.into(),
         xkey_info: Some(key_info),
      };

      Self {
         master_wallet,
         children: Vec::new(),
         next_child_index: 0,
      }
   }
}

#[cfg(test)]
mod tests {

   use super::*;
   use crate::derive::{Argon2Params, DeriveMethod, DeriveVersion};
   use alloy_primitives::address;
   const TEST_M_COST: u32 = 16_000;
   const TEST_T_COST: u32 = 8;
   const TEST_P_COST: u32 = 1;

   #[test]
   fn test_hd_wallet_creation() {
      let username = SecureString::from("zeus");
      let password = SecureString::from("zeus");

      let expected_master = address!("0x7c6B09491773800D80deb32d12616411c6238878");

      let params = Argon2Params::new(TEST_M_COST, TEST_T_COST, TEST_P_COST);
      let method = DeriveMethod::BIP32;
      let version = DeriveVersion::Custom(params, method);
      let deriver = Deriver::new(version, Some(username), Some(password));

      let mut hd_wallet = SecureHDWallet::new_from_deriver(None, deriver).unwrap();
      eprintln!(
         "Master Wallet Address: {}",
         hd_wallet.master_wallet.address()
      );

      assert!(hd_wallet.master_wallet.is_master());
      assert_eq!(hd_wallet.master_wallet.address(), expected_master);

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

   #[test]
   fn test_seed_error_does_not_echo_phrase() {
      let phrase = "not a real mnemonic phrase zoo";
      let wallet = Wallet {
         name: "imported".to_string(),
         seed_phrase: Some(SecureString::from(phrase)),
         key: SecureKey::random(),
         xkey_info: None,
      };

      let err = match wallet.seed() {
         Ok(_) => panic!("expected invalid phrase to fail"),
         Err(e) => e,
      };
      let msg = err.to_string();
      assert!(
         !msg.contains(phrase),
         "error echoed the phrase: {msg}"
      );
   }

   #[test]
   fn test_child_derivation_is_pinned() {
      let params = Argon2Params::new(TEST_M_COST, TEST_T_COST, TEST_P_COST);
      let version = DeriveVersion::Custom(params, DeriveMethod::BIP32);
      let deriver = Deriver::new(
         version,
         Some(SecureString::from("zeus")),
         Some(SecureString::from("zeus")),
      );
      let mut hd_wallet = SecureHDWallet::new_from_deriver(None, deriver).unwrap();

      assert_eq!(
         hd_wallet.master_wallet.address(),
         address!("0x7c6B09491773800D80deb32d12616411c6238878")
      );
      assert_eq!(
         hd_wallet.master_wallet.derivation_path_string(),
         "m/44'/60'/0'/0/0/0"
      );

      // `derive_child` follows `next_child_index + BIP32_HARDEN` and appends
      let expected = [
         (
            "0x8FebA2067706431bD5942e7861A789Ea6432f9CE",
            "m/44'/60'/0'/0/0/0'",
            2_147_483_648u32,
         ),
         (
            "0x7c8420215C62761aa695aF0BF293eBe97C46e6EB",
            "m/44'/60'/0'/0/0/1'",
            2_147_483_649,
         ),
         (
            "0x23137Be5D34899cc855E43d72781A37F0b72424C",
            "m/44'/60'/0'/0/0/2'",
            2_147_483_650,
         ),
      ];
      for (i, (expected_address, expected_path, expected_index)) in expected.iter().enumerate() {
         let address = hd_wallet.derive_child(format!("child {i}")).unwrap();
         let child = hd_wallet.children.last().unwrap();
         assert_eq!(address.to_string(), *expected_address);
         assert_eq!(child.address().to_string(), *expected_address);
         assert_eq!(child.derivation_path_string(), *expected_path);
         assert_eq!(child.index(), *expected_index);
         assert!(child.is_hardened());
         assert!(!child.is_imported());
         assert_eq!(hd_wallet.next_child_index, i as u32 + 1);
      }

      // `derive_child_at` takes a raw index: no counter bump, no append, not hardened
      let at_five = hd_wallet.derive_child_at("at five".to_string(), 5).unwrap();
      assert_eq!(
         at_five.address().to_string(),
         "0x8FcE2Ab3719937a115EEe5d8366814D9eB7F1D73"
      );
      assert_eq!(
         at_five.derivation_path_string(),
         "m/44'/60'/0'/0/0/5"
      );
      assert_eq!(at_five.index(), 5);
      assert!(!at_five.is_hardened());
      assert_eq!(hd_wallet.children.len(), 3);
      assert_eq!(hd_wallet.next_child_index, 3);

      // `derive_child_at_mut` appends, and refuses a second wallet at the same path
      let at_nine = hd_wallet.derive_child_at_mut("at nine".to_string(), 9).unwrap();
      assert_eq!(
         at_nine.address().to_string(),
         "0xD5aE2042392b91D8d4C3bB48A294B4d61F6CA97c"
      );
      assert_eq!(
         at_nine.derivation_path_string(),
         "m/44'/60'/0'/0/0/9"
      );
      assert_eq!(hd_wallet.children.len(), 4);
      assert_eq!(hd_wallet.next_child_index, 3);

      // `Wallet` implements no `Debug`, so no `.unwrap_err()` — match instead
      let duplicate = match hd_wallet.derive_child_at_mut("at nine".to_string(), 9) {
         Ok(_) => panic!("a second wallet at the same path must be refused"),
         Err(e) => e.to_string(),
      };
      assert_eq!(
         duplicate,
         "Wallet At m/44'/60'/0'/0/0/9 with Address 0xD5aE2042392b91D8d4C3bB48A294B4d61F6CA97c already exists"
      );
   }
}
