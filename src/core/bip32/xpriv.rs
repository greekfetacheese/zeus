use zeus_eth::alloy_signer::k256::{
   self,
   ecdsa::{SigningKey, VerifyingKey},
};
use hmac::{Hmac, Mac};
use secure_types::Zeroize;
use sha2::{Sha256, Sha512};
use ripemd::{Ripemd160, Digest};
use zeus_eth::utils::SecureSigner;

use super::{
   Bip32Error,
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

   let chain_code = ChainCode::from(right);
   right.zeroize();

   Ok((left, chain_code))
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
/// A BIP32 eXtended Privkey
pub struct SecureXPriv {
   pub signer: SecureSigner,
   pub xkey_info: XKeyInfo,
}

impl PartialEq for SecureXPriv {
   fn eq(&self, other: &SecureXPriv) -> bool {
      self.xkey_info == other.xkey_info
   }
}

impl SecureXPriv {
   pub fn public_key(&self) -> XPub {
      XPub {
         key: self.signer.verifying_key(),
         xkey_info: self.xkey_info.clone(),
      }
   }

   /// The fingerprint is the first 4 bytes of the HASH160 of the public key
   pub fn fingerprint(&self) -> KeyFingerprint {
      self.public_key().fingerprint()
   }

   /// Instantiate a root node using a custom HMAC key.
   pub fn root_from_seed(
      data: &[u8],
      hint: Option<Hint>,
   ) -> Result<SecureXPriv, Bip32Error> {
      if data.len() < 16 {
         return Err(Bip32Error::SeedTooShort);
      }

      // let parent = KeyFingerprint([0u8; 4]);
      let (key, chain_code) = hmac_and_split(SEED, data)?;

      if bool::from(key.is_zero()) {
         // This can only be tested by mocking hmac_and_split
         return Err(Bip32Error::InvalidKey);
      }

      let signing_key = SigningKey::from(key);
      let signer = SecureSigner::from(signing_key);

      let key_info = XKeyInfo {
         depth: 0,
         index: 0,
         parent: KeyFingerprint([0u8; 4]),
         chain_code,
         hint: hint.unwrap_or(Hint::SegWit),
      };

      Ok(SecureXPriv {
         signer,
         xkey_info: key_info,
      })
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

      let key = self.signer.to_signing_key();

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
         .unlocked_scope(|seed| hmac_and_split(seed, &data).unwrap());

      data.zeroize();

      let parent_key = k256::NonZeroScalar::from_repr(key.to_bytes()).unwrap();
      let mut tweaked = tweak.add(&parent_key);

      let tweaked_key: k256::NonZeroScalar =
         Option::from(k256::NonZeroScalar::new(tweaked)).ok_or(Bip32Error::BadTweak)?;

      tweaked.zeroize();

      let new_key = SigningKey::from(tweaked_key);
      let signer = SecureSigner::from(new_key);

      let xkey_info = XKeyInfo {
         depth: self.xkey_info.depth + 1,
         index,
         parent: self.fingerprint(),
         chain_code,
         hint: self.xkey_info.hint,
      };

      Ok(Self { signer, xkey_info })
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