use hmac::{Hmac, Mac};
use k256::ecdsa::{SigningKey, VerifyingKey};
use ripemd::{Digest, Ripemd160};
use secure_types::{SecureArray, Zeroize};
use sha2::{Sha256, Sha512};

use super::{
   error::Bip32Error,
   path::{BIP32_HARDEN, DerivationPath},
   primitives::*,
};

/// The BIP32-defined seed used for derivation of the root node.
pub const SEED: &[u8; 12] = b"Bitcoin seed";

fn hmac_and_split(
   seed: &[u8],
   data: &[u8],
) -> Result<(k256::NonZeroScalar, ChainCode), Bip32Error> {
   let mut mac = Hmac::<Sha512>::new_from_slice(seed).expect("key length is ok");
   mac.update(data);
   let mut result = mac.finalize().into_bytes();

   let left = k256::NonZeroScalar::try_from(&result[..32])?;

   let mut right = [0u8; 32];
   right.copy_from_slice(&result[32..]);

   result.zeroize();

   let chain_code =
      ChainCode::from_slice_mut(&mut right).map_err(|e| Bip32Error::Custom(e.to_string()))?;

   Ok((left, chain_code))
}

/// Instantiate a root node using a custom HMAC key.
///
/// # Returns
/// - `key` The private key
/// - `xkey_info` The extended key info
pub fn root_from_seed(
   data: &[u8],
   hint: Option<Hint>,
) -> Result<(SecureArray<u8, 32>, XKeyInfo), Bip32Error> {
   if data.len() < 16 {
      return Err(Bip32Error::SeedTooShort);
   }

   let (mut key, chain_code) = hmac_and_split(SEED, data)?;

   if bool::from(key.is_zero()) {
      return Err(Bip32Error::InvalidKey);
   }

   let mut bytes = key.to_bytes();
   key.zeroize();

   let sec_array =
      SecureArray::from_slice_mut(bytes.as_mut()).map_err(|e| Bip32Error::Custom(e.to_string()))?;

   let key_info = XKeyInfo {
      depth: 0,
      index: 0,
      parent: KeyFingerprint([0u8; 4]),
      chain_code,
      hint: hint.unwrap_or(Hint::SegWit),
   };

   Ok((sec_array, key_info))
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone)]
/// A BIP32 eXtended Privkey
pub struct SecureXPriv {
   pub key: SecureArray<u8, 32>,
   pub xkey_info: XKeyInfo,
}

impl PartialEq for SecureXPriv {
   fn eq(&self, other: &SecureXPriv) -> bool {
      self.xkey_info == other.xkey_info
   }
}

impl SecureXPriv {
   pub fn new(key: SecureArray<u8, 32>, xkey_info: XKeyInfo) -> Self {
      Self { key, xkey_info }
   }

   pub fn public_key(&self) -> XPub {
      XPub {
         key: self.verifying_key(),
         xkey_info: self.xkey_info.clone(),
      }
   }

   pub fn signing_key(&self) -> SigningKey {
      self.key.unlock(|key| SigningKey::from_slice(key).unwrap())
   }

   pub fn verifying_key(&self) -> VerifyingKey {
      let signing_key = self.signing_key();
      *signing_key.verifying_key()
   }

   /// The fingerprint is the first 4 bytes of the HASH160 of the public key
   pub fn fingerprint(&self) -> KeyFingerprint {
      self.public_key().fingerprint()
   }

   /// Derive a series of child indices. Allows traversing several levels of the tree at once.
   /// Accepts an iterator producing u32, or a string.
   pub fn derive_path<E, P>(&self, p: P) -> Result<Self, Bip32Error>
   where
      E: Into<Bip32Error>,
      P: TryInto<DerivationPath, Error = E>,
   {
      let path: DerivationPath = p.try_into().map_err(Into::into)?;

      if path.is_empty() {
         return Ok(self.clone());
      }

      let mut current = self.to_owned();
      for index in path.iter() {
         current = current.derive_child(*index)?;
      }
      Ok(current)
   }

   fn derive_child(&self, index: u32) -> Result<Self, Bip32Error> {
      let hardened = index >= BIP32_HARDEN;

      let key = self.signing_key();

      let mut data: Vec<u8> = vec![];
      if hardened {
         data.push(0);
         data.extend(key.to_bytes());
         data.extend(index.to_be_bytes());
      } else {
         data.extend(key.verifying_key().to_sec1_bytes().iter());
         data.extend(index.to_be_bytes());
      };

      let (tweak, chain_code) = self
         .xkey_info
         .chain_code
         .data
         .unlock(|seed| hmac_and_split(seed, &data).unwrap());

      data.zeroize();

      let parent_key = k256::NonZeroScalar::from_repr(key.to_bytes()).unwrap();
      let mut tweaked = tweak.add(&parent_key);

      let mut tweaked_key: k256::NonZeroScalar =
         Option::from(k256::NonZeroScalar::new(tweaked)).ok_or(Bip32Error::BadTweak)?;

      tweaked.zeroize();

      let mut bytes = tweaked_key.to_bytes();
      tweaked_key.zeroize();

      let sec_array = SecureArray::from_slice_mut(bytes.as_mut())
         .map_err(|e| Bip32Error::Custom(e.to_string()))?;

      let xkey_info = XKeyInfo {
         depth: self.xkey_info.depth + 1,
         index,
         parent: self.fingerprint(),
         chain_code,
         hint: self.xkey_info.hint,
      };

      Ok(Self {
         key: sec_array,
         xkey_info,
      })
   }
}

/// A BIP32 eXtended Public key
#[derive(Clone)]
pub struct XPub {
   pub key: VerifyingKey,
   pub xkey_info: XKeyInfo,
}

impl XPub {
   pub fn fingerprint(&self) -> KeyFingerprint {
      let compressed_pubkey = self.key.to_sec1_bytes();

      let sha256_hash = Sha256::digest(&compressed_pubkey);
      let ripemd160_hash = Ripemd160::digest(sha256_hash);

      let mut bytes = [0u8; 4];
      bytes.copy_from_slice(&ripemd160_hash[..4]);
      KeyFingerprint(bytes)
   }
}
