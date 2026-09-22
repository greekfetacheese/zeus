use super::{Error, SecureHDWallet, Wallet};
use argon2_rs::Argon2;
use secure_types::{SecureString, SecureVec, Zeroize};
use sha3::{Digest, Sha3_512};
use zeus_bip32::root_from_seed;

/// Argon2id parameters Zeus v1 derives the BIP32 seed with.
pub const V1_M_COST: u32 = 8192_000;
pub const V1_T_COST: u32 = 96;
pub const V1_P_COST: u32 = 1;

/// The first derivation stage: the Argon2 parameters that turn a username and
/// password into the 64-byte seed a wallet is derived from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Argon2Params(Argon2);

impl Argon2Params {
   pub fn new(m_cost: u32, t_cost: u32, p_cost: u32) -> Self {
      Self(Argon2::new(m_cost, t_cost, p_cost))
   }

   /// The Argon2 parameters of Zeus v1
   pub fn zeus_v1() -> Self {
      Self::new(V1_M_COST, V1_T_COST, V1_P_COST)
   }

   /// The underlying [`Argon2`]
   pub fn argon2(&self) -> &Argon2 {
      &self.0
   }
}

/// The second derivation stage: how the seed becomes keys and derivation paths
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DeriveMethod {
   /// The standard bip32 method from bitcoin
   BIP32,
}

/// The complete derivation scheme of a wallet
///
/// A version pins both derivation stages together, because the pair is what
/// reproduces a wallet. Changing the Argon2 parameters or the derivation
/// method means adding a **new** variant, so every change becomes an explicit
/// decision here instead of silently affecting wallets derived with an older
/// version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeriveVersion {
   /// Zeus v1: Argon2id with the [`V1_M_COST`] / [`V1_T_COST`] / [`V1_P_COST`]
   /// parameters, deriving keys with [`DeriveMethod::BIP32`].
   V1,

   /// A version with caller supplied Argon2 parameters, for `dev` builds and
   /// tests only.
   ///
   /// It is not a wallet generation: a wallet derived with arbitrary
   /// parameters is only recoverable with those exact parameters. Never use it
   /// for a wallet a user can lose access to, and never persist it.
   Custom(Argon2Params, DeriveMethod),
}

impl DeriveVersion {
   /// The Argon2 parameters this version derives the seed with
   pub fn kdf_params(&self) -> Argon2Params {
      match self {
         DeriveVersion::V1 => Argon2Params::zeus_v1(),
         DeriveVersion::Custom(params, _) => params.clone(),
      }
   }

   /// The method this version derives keys with
   pub fn method(&self) -> DeriveMethod {
      match self {
         DeriveVersion::V1 => DeriveMethod::BIP32,
         DeriveVersion::Custom(_, method) => *method,
      }
   }
}

/// Type that deterministically derives a hierarchical deterministic wallet
/// from a username and password
#[derive(Clone)]
pub struct Deriver {
   version: DeriveVersion,
   username: Option<SecureString>,
   password: Option<SecureString>,
}

impl Deriver {
   pub fn new(
      version: DeriveVersion,
      username: Option<SecureString>,
      password: Option<SecureString>,
   ) -> Self {
      Self {
         version,
         username,
         password,
      }
   }

   pub fn zeus_v1() -> Self {
      Deriver {
         version: DeriveVersion::V1,
         username: None,
         password: None,
      }
   }

   pub fn version(&self) -> &DeriveVersion {
      &self.version
   }

   pub fn set_username(&mut self, username: SecureString) {
      self.username = Some(username);
   }

   pub fn set_password(&mut self, password: SecureString) {
      self.password = Some(password);
   }

   /// Derive the 64-byte seed from a username and password
   pub fn derive_kdf_seed(&self) -> Result<SecureVec<u8>, Error> {
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

      let params = self.version.kdf_params();
      kdf_seed(username, password, params.argon2())
   }

   /// Create a new HD wallet
   ///
   /// # Arguments
   ///
   /// - `name` - The name of the wallet (optional)
   pub fn new_hd_wallet(&self, name: Option<String>) -> Result<SecureHDWallet, Error> {
      match self.version.method() {
         DeriveMethod::BIP32 => {
            let seed = self.derive_kdf_seed()?;
            derive_hd_wallet_bip32(name, seed)
         }
      }
   }
}

/// The key derivation stage of [`DeriveMethod::BIP32`]
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

/// The KDF stage: the SHA3-512 of the username is the salt for hashing the
/// password with Argon2, which produces the 64-byte seed
fn kdf_seed(
   username: &SecureString,
   password: &SecureString,
   params: &Argon2,
) -> Result<SecureVec<u8>, Error> {
   let mut hasher = Sha3_512::new();

   username.unlock_str(|username| {
      hasher.update(username.as_bytes());
   });

   let mut result = hasher.finalize();
   let username_hash = result.to_vec();
   result.zeroize();

   let seed = password.unlock_str(|password| params.hash_password(password, username_hash))?;
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
   fn test_zeus_v1_uses_the_v1_constants() {
      let params = DeriveVersion::V1.kdf_params();

      assert_eq!(params.argon2().m_cost, V1_M_COST);
      assert_eq!(params.argon2().t_cost, V1_T_COST);
      assert_eq!(params.argon2().p_cost, V1_P_COST);
      assert_eq!(params, Argon2Params::zeus_v1());
   }

   #[test]
   fn test_custom_version_with_v1_params_derives_like_zeus_v1() {
      let custom = DeriveVersion::Custom(Argon2Params::zeus_v1(), DeriveMethod::BIP32);

      assert_eq!(
         custom.kdf_params(),
         DeriveVersion::V1.kdf_params()
      );
      assert_eq!(custom.method(), DeriveVersion::V1.method());
   }
}
