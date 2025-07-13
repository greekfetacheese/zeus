use alloy_signer_local::{MnemonicBuilder, PrivateKeySigner, coins_bip39::English};
use anyhow::anyhow;
use argon2::{
   Algorithm, Argon2, Params, PasswordHasher, Version,
   password_hash::{Salt, SaltString},
};
use ncrypt_me::{Credentials, erase_output};
use secure_types::{SecureString, SecureVec, Zeroize};
use sha3::{Digest, Sha3_256};
use std::str::FromStr;
use zeus_eth::{alloy_primitives::{Address, hex}, utils::SecureSigner};

// Argon2 parameters used to derive the seed from the credentials

/// 1GB memory
pub const M_COST: u32 = 1024_000;
pub const T_COST: u32 = 8;
pub const P_COST: u32 = 1;
pub const HASH_LENGTH: usize = 64;

// For testing only
const TEST_M_COST: u32 = 16_000;

#[derive(Clone, Default, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WalletInfo {
   pub name: String,
   pub address: Address,
}

impl WalletInfo {
   pub fn address_string(&self) -> String {
      self.address.to_string()
   }

   pub fn address_truncated(&self) -> String {
      format!(
         "{}...{}",
         &self.address_string()[..6],
         &self.address_string()[36..]
      )
   }
}

/// User Wallet
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Wallet {
   pub info: WalletInfo,
   pub key: SecureSigner,
}

impl Wallet {
   pub fn is_key_erased(&self) -> bool {
      self.key.is_erased()
   }

   /// Return the wallet's key
   pub fn key_string(&self) -> SecureString {
      self.key.key_string()
   }

   /// Create a new wallet from a random private key
   pub fn new_rng(name: String) -> Self {
      let key = SecureSigner::random();
      let info = WalletInfo {
         name,
         address: key.address(),
      };

      Self { info, key }
   }

   /// Create a new wallet from a given private key
   pub fn new_from_key(name: String, key_str: SecureString) -> Result<Self, anyhow::Error> {
      let key = key_str.str_scope(|key_str| PrivateKeySigner::from_str(key_str))?;
      let key = SecureSigner::from(key);
      let info = WalletInfo {
         name,
         address: key.address(),
      };

      Ok(Self { info, key })
   }

   /// Create a new wallet from a mnemonic phrase
   pub fn new_from_mnemonic(name: String, phrase: SecureString) -> Result<Self, anyhow::Error> {
      // return a custom error to not expose the phrase in case it just has a typo
      let phrase_string = phrase.str_scope(|phrase| phrase.to_string());
      let wallet = MnemonicBuilder::<English>::default()
         .phrase(phrase_string)
         .index(0)?
         .build()
         .map_err(|_| anyhow!("It seems that the given phrase is invalid"))?;
      let key = SecureSigner::from(wallet);

      let info = WalletInfo {
         name,
         address: key.address(),
      };

      Ok(Self { info, key })
   }
}


fn erase_salt(salt: &mut Salt) {
   unsafe {
      let ptr: *mut Salt = salt;

      let size = core::mem::size_of::<Salt>();
      let bytes: &mut [u8] = core::slice::from_raw_parts_mut(ptr as *mut u8, size);
      bytes.zeroize();
   }
}

fn erase_salt_string(salt: &mut SaltString) {
   unsafe {
      let ptr: *mut SaltString = salt;

      let size = core::mem::size_of::<SaltString>();
      let bytes: &mut [u8] = core::slice::from_raw_parts_mut(ptr as *mut u8, size);
      bytes.zeroize();
   }
}

pub fn derive_seed(credentials: &Credentials) -> Result<SecureVec<u8>, anyhow::Error> {
   credentials.is_valid()?;

   let mut hasher = Sha3_256::new();

   credentials.username.str_scope(|username| {
      hasher.update(username.as_bytes());
   });

   let mut hash = hasher.finalize();
   let mut hash_string = hex::encode(hash);
   let mut salt_string = SaltString::from_b64(hash_string.as_str()).unwrap();

   hash.zeroize();
   hash_string.zeroize();

   let m_cost = if cfg!(test) { TEST_M_COST } else { M_COST };
   let params = Params::new(m_cost, T_COST, P_COST, Some(HASH_LENGTH)).map_err(|e| anyhow!(e))?;
   let argon2 = Argon2::new(Algorithm::default(), Version::default(), params);

   let mut password_hash = credentials.password.str_scope(|password| {
      argon2
         .hash_password(password.as_bytes(), &salt_string)
         .expect("Argon2 hash failed")
   });


   let mut seed = password_hash.hash.take().expect("Hash output is empty");
   let mut salt = password_hash.salt.take().expect("Salt is empty");
   let secure_vec = SecureVec::from_vec(seed.as_bytes().to_vec())?;

   erase_salt(&mut salt);
   erase_salt_string(&mut salt_string);
   erase_output(&mut seed);

   Ok(secure_vec)
}

pub struct SecureHDWallet {}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn test_erase_salt() {
      let string = "ishouldbeerased";

      let mut salt_string = SaltString::from_b64(string).unwrap();
      erase_salt_string(&mut salt_string);
      assert_eq!(salt_string.as_str().chars().count(), 0);

      let mut salt = Salt::from_b64(string).unwrap();
      erase_salt(&mut salt);
      assert_eq!(salt.as_str().chars().count(), 0);
   }

   #[test]
   fn test_derive_seed_simple() {
      let username = SecureString::from("username");
      let password = SecureString::from("password");
      let confirm_password = SecureString::from("password");

      let credentials = Credentials::new(username, password, confirm_password);

      let seed = derive_seed(&credentials).unwrap();
      assert_eq!(seed.len(), 64);
   }
}
