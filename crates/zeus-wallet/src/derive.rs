use super::{Error, SecureHDWallet, Wallet};
use argon2_rs::Argon2;
use secure_types::{SecureString, SecureVec, Zeroize};
use sha3::{Digest, Sha3_512};
use zeus_bip32::root_from_seed;

pub const V1_M_COST: u32 = 8192_000;
pub const V1_T_COST: u32 = 96;
pub const V1_P_COST: u32 = 1;

/// Argon2 parameters based on the version
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Version {
   /// Version 1 of Argon2 parameters Zeus uses to derive the bip32 seed
   /// for the master wallet.
   ///
   /// - `m_cost`: 8192_000 KiB
   /// - `t_cost`: 96
   /// - `p_cost`: 1
   /// - `version`: 0x13
   /// - `Hash length`: 64 (512 bits)
   V1(Argon2),

   /// Custom Version useful for testing
   CUSTOM(Argon2),
}

impl Version {
   pub fn zeus_v1() -> Self {
      Version::V1(Argon2::new(V1_M_COST, V1_T_COST, V1_P_COST))
   }

   pub fn argon2(&self) -> &Argon2 {
      match self {
         Version::V1(argon2) => argon2,
         Version::CUSTOM(argon2) => argon2,
      }
   }

   /// Returns true if the custom version is the same as the Zeus v1 parameters
   pub fn matches_zeus_v1(&self) -> bool {
      matches!(self, Version::CUSTOM(argon2) if argon2 == &Argon2::new(V1_M_COST, V1_T_COST, V1_P_COST))
   }

   pub fn is_v1(&self) -> bool {
      matches!(self, Version::V1(_))
   }

   pub fn is_custom(&self) -> bool {
      matches!(self, Version::CUSTOM(_))
   }
}

#[derive(Copy, Clone)]
pub enum DeriveMethod {
   /// The standard bip32 method from bitcoin
   BIP32,
}

/// Type that deterministically derives a hierarchical deterministic wallet
/// from a username and password
#[derive(Clone)]
pub struct Deriver {
   version: Version,
   method: DeriveMethod,
   username: Option<SecureString>,
   password: Option<SecureString>,
}

impl Deriver {
   pub fn new(
      version: Version,
      method: DeriveMethod,
      username: Option<SecureString>,
      password: Option<SecureString>,
   ) -> Self {
      Self {
         version,
         method,
         username,
         password,
      }
   }

   pub fn zeus_v1() -> Self {
      Deriver {
         version: Version::zeus_v1(),
         method: DeriveMethod::BIP32,
         username: None,
         password: None,
      }
   }

   pub fn matches_zeus_v1(&self) -> bool {
      self.version.matches_zeus_v1()
   }

   pub fn set_username(&mut self, username: SecureString) {
      self.username = Some(username);
   }

   pub fn set_password(&mut self, password: SecureString) {
      self.password = Some(password);
   }

   /// Derive a Bip32 seed from a username and password
   pub fn derive_bip32_seed(&self) -> Result<SecureVec<u8>, Error> {
      let Some(username) = self.username.as_ref() else {
         return Err(Error::UsernameIsMissing);
      };

      let Some(password) = self.password.as_ref() else {
         return Err(Error::PasswordIsMissing);
      };

      if username.is_empty() {
         return Err(Error::UsernameIsEmpty);
      }

      if password.is_empty() {
         return Err(Error::PasswordIsEmpty);
      }

      match &self.version {
         Version::V1(argon2) => derive_bip32_seed(username, password, argon2),
         Version::CUSTOM(argon2) => derive_bip32_seed(username, password, argon2),
      }
   }

   /// Create a new HD wallet
   ///
   /// # Arguments
   ///
   /// - `name` - The name of the wallet (optional)
   pub fn new_hd_wallet(&self, name: Option<String>) -> Result<SecureHDWallet, Error> {
      match self.method {
         DeriveMethod::BIP32 => {
            let seed = self.derive_bip32_seed()?;
            derive_hd_wallet_bip32(name, seed)
         }
      }
   }
}

fn derive_hd_wallet_bip32(
   name_opt: Option<String>,
   seed: SecureVec<u8>,
) -> Result<SecureHDWallet, Error> {
   let (key, key_info) = seed.unlock_slice(|slice| root_from_seed(slice, None).unwrap());

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

   Ok(SecureHDWallet {
      master_wallet,
      children: Vec::new(),
      next_child_index: 0,
   })
}

fn derive_bip32_seed(
   username: &SecureString,
   password: &SecureString,
   argon2: &Argon2,
) -> Result<SecureVec<u8>, Error> {
   let mut hasher = Sha3_512::new();

   username.unlock_str(|username| {
      hasher.update(username.as_bytes());
   });

   let mut result = hasher.finalize();
   let username_hash = result.to_vec();
   result.zeroize();

   let seed = password.unlock_str(|password| argon2.hash_password(password, username_hash))?;
   let secure_seed = SecureVec::from_vec(seed)?;

   if secure_seed.len() != 64 {
      return Err(Error::SeedLengthTooShort);
   }

   Ok(secure_seed)
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn test_custom_version_matches_zeus_v1() {
      let argon2 = Argon2::new(V1_M_COST, V1_T_COST, V1_P_COST);
      let version = Version::CUSTOM(argon2);
      let method = DeriveMethod::BIP32;
      let deriver = Deriver::new(version, method, None, None);

      assert!(deriver.matches_zeus_v1());
   }
}
