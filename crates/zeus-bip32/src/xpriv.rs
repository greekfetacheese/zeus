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

   let left = match k256::NonZeroScalar::try_from(&result[..32]) {
      Ok(left) => left,
      Err(_) => {
         result.zeroize();
         return Err(Bip32Error::InvalidKey);
      }
   };

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
      let keys_eq = other.key.unlock(|other_key| self.key.unlock(|self_key| self_key == other_key));
      keys_eq && self.xkey_info == other.xkey_info
   }
}

impl SecureXPriv {
   pub fn new(key: SecureArray<u8, 32>, xkey_info: XKeyInfo) -> Self {
      Self { key, xkey_info }
   }

   pub fn public_key(&self) -> Result<XPub, Bip32Error> {
      Ok(XPub {
         key: self.verifying_key()?,
         xkey_info: self.xkey_info.clone(),
      })
   }

   pub(crate) fn signing_key(&self) -> Result<SigningKey, Bip32Error> {
      self.key.unlock(|key| SigningKey::from_slice(key).map_err(Into::into))
   }

   pub fn verifying_key(&self) -> Result<VerifyingKey, Bip32Error> {
      let signing_key = self.signing_key()?;
      Ok(*signing_key.verifying_key())
   }

   /// The fingerprint is the first 4 bytes of the HASH160 of the public key
   pub fn fingerprint(&self) -> Result<KeyFingerprint, Bip32Error> {
      Ok(self.public_key()?.fingerprint())
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
      let depth = self.xkey_info.depth.checked_add(1).ok_or(Bip32Error::DepthOverflow)?;

      let hardened = index >= BIP32_HARDEN;

      let mut data: Vec<u8> = Vec::with_capacity(37);
      if hardened {
         data.push(0);
         self.key.unlock(|key_bytes| data.extend_from_slice(key_bytes));
         data.extend(index.to_be_bytes());
      } else {
         data.extend(self.verifying_key()?.to_sec1_bytes().iter());
         data.extend(index.to_be_bytes());
      }

      let hmac_res = self.xkey_info.chain_code.data.unlock(|seed| hmac_and_split(seed, &data));

      data.zeroize();

      let (mut tweak, chain_code) = match hmac_res {
         Ok(parts) => parts,
         Err(_) => {
            let next = index.checked_add(1).ok_or(Bip32Error::InvalidKey)?;
            return self.derive_child(next);
         }
      };

      let parent_res = self.key.unlock(|key_bytes| k256::NonZeroScalar::try_from(key_bytes));
      let mut parent_key = match parent_res {
         Ok(key) => key,
         Err(e) => {
            tweak.zeroize();
            return Err(e.into());
         }
      };

      let mut tweaked = tweak.add(&parent_key);
      tweak.zeroize();
      parent_key.zeroize();

      let mut tweaked_key: k256::NonZeroScalar =
         match Option::from(k256::NonZeroScalar::new(tweaked)) {
            Some(key) => key,
            None => {
               tweaked.zeroize();
               let next = index.checked_add(1).ok_or(Bip32Error::BadTweak)?;
               return self.derive_child(next);
            }
         };
      tweaked.zeroize();

      let mut bytes = tweaked_key.to_bytes();
      tweaked_key.zeroize();

      let sec_array = SecureArray::from_slice_mut(bytes.as_mut())
         .map_err(|e| Bip32Error::Custom(e.to_string()))?;

      let xkey_info = XKeyInfo {
         depth,
         index,
         parent: self.fingerprint()?,
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

#[cfg(test)]
mod tests {
   use super::*;

   struct Node {
      path: &'static [u32],
      key: &'static str,
      chain: &'static str,
      depth: u8,
      index: u32,
   }

   fn key_hex(xpriv: &SecureXPriv) -> String {
      xpriv.key.unlock(|k| hex::encode(k))
   }

   fn chain_hex(xpriv: &SecureXPriv) -> String {
      xpriv.xkey_info.chain_code.data.unlock(|c| hex::encode(c))
   }

   fn xpriv_from_seed(seed_hex: &str) -> SecureXPriv {
      let seed = hex::decode(seed_hex).unwrap();
      let (key, info) = root_from_seed(&seed, None).unwrap();
      SecureXPriv::new(key, info)
   }

   fn assert_node(root: &SecureXPriv, node: &Node) {
      let child = if node.path.is_empty() {
         root.clone()
      } else {
         root.derive_path(node.path).unwrap()
      };
      assert_eq!(key_hex(&child), node.key, "key mismatch");
      assert_eq!(
         chain_hex(&child),
         node.chain,
         "chain code mismatch"
      );
      assert_eq!(child.xkey_info.depth, node.depth);
      assert_eq!(child.xkey_info.index, node.index);
   }

   #[test]
   fn bip32_vector_1() {
      let root = xpriv_from_seed("000102030405060708090a0b0c0d0e0f");
      let nodes = [
         Node {
            path: &[],
            key: "e8f32e723decf4051aefac8e2c93c9c5b214313817cdb01a1494b917c8436b35",
            chain: "873dff81c02f525623fd1fe5167eac3a55a049de3d314bb42ee227ffed37d508",
            depth: 0,
            index: 0,
         },
         Node {
            path: &[BIP32_HARDEN],
            key: "edb2e14f9ee77d26dd93b4ecede8d16ed408ce149b6cd80b0715a2d911a0afea",
            chain: "47fdacbd0f1097043b78c63c20c34ef4ed9a111d980047ad16282c7ae6236141",
            depth: 1,
            index: BIP32_HARDEN,
         },
         Node {
            path: &[BIP32_HARDEN, 1],
            key: "3c6cb8d0f6a264c91ea8b5030fadaa8e538b020f0a387421a12de9319dc93368",
            chain: "2a7857631386ba23dacac34180dd1983734e444fdbf774041578e9b6adb37c19",
            depth: 2,
            index: 1,
         },
         Node {
            path: &[BIP32_HARDEN, 1, 2 + BIP32_HARDEN],
            key: "cbce0d719ecf7431d88e6a89fa1483e02e35092af60c042b1df2ff59fa424dca",
            chain: "04466b9cc8e161e966409ca52986c584f07e9dc81f735db683c3ff6ec7b1503f",
            depth: 3,
            index: 2 + BIP32_HARDEN,
         },
         Node {
            path: &[BIP32_HARDEN, 1, 2 + BIP32_HARDEN, 2],
            key: "0f479245fb19a38a1954c5c7c0ebab2f9bdfd96a17563ef28a6a4b1a2a764ef4",
            chain: "cfb71883f01676f587d023cc53a35bc7f88f724b1f8c2892ac1275ac822a3edd",
            depth: 4,
            index: 2,
         },
         Node {
            path: &[BIP32_HARDEN, 1, 2 + BIP32_HARDEN, 2, 1_000_000_000],
            key: "471b76e389e528d6de6d816857e012c5455051cad6660850e58372a6c3e6e7c8",
            chain: "c783e67b921d2beb8f6b389cc646d7263b4145701dadd2161548a8b078e65e9e",
            depth: 5,
            index: 1_000_000_000,
         },
      ];
      for node in &nodes {
         assert_node(&root, node);
      }

      assert_eq!(
         root.fingerprint().unwrap().0,
         [0x34, 0x42, 0x19, 0x3e]
      );
   }

   #[test]
   fn bip32_vector_2() {
      let root = xpriv_from_seed(
         "fffcf9f6f3f0edeae7e4e1dedbd8d5d2cfccc9c6c3c0bdbab7b4b1aeaba8a5a29f9c999693908d8a8784817e7b7875726f6c696663605d5a5754514e4b484542",
      );
      let nodes = [
         Node {
            path: &[],
            key: "4b03d6fc340455b363f51020ad3ecca4f0850280cf436c70c727923f6db46c3e",
            chain: "60499f801b896d83179a4374aeb7822aaeaceaa0db1f85ee3e904c4defbd9689",
            depth: 0,
            index: 0,
         },
         Node {
            path: &[0],
            key: "abe74a98f6c7eabee0428f53798f0ab8aa1bd37873999041703c742f15ac7e1e",
            chain: "f0909affaa7ee7abe5dd4e100598d4dc53cd709d5a5c2cac40e7412f232f7c9c",
            depth: 1,
            index: 0,
         },
         Node {
            path: &[0, 2147483647 + BIP32_HARDEN],
            key: "877c779ad9687164e9c2f4f0f4ff0340814392330693ce95a58fe18fd52e6e93",
            chain: "be17a268474a6bb9c61e1d720cf6215e2a88c5406c4aee7b38547f585c9a37d9",
            depth: 2,
            index: 2147483647 + BIP32_HARDEN,
         },
         Node {
            path: &[0, 2147483647 + BIP32_HARDEN, 1],
            key: "704addf544a06e5ee4bea37098463c23613da32020d604506da8c0518e1da4b7",
            chain: "f366f48f1ea9f2d1d3fe958c95ca84ea18e4c4ddb9366c336c927eb246fb38cb",
            depth: 3,
            index: 1,
         },
         Node {
            path: &[0, 2147483647 + BIP32_HARDEN, 1, 2147483646 + BIP32_HARDEN],
            key: "f1c7c871a54a804afe328b4c83a1c33b8e5ff48f5087273f04efa83b247d6a2d",
            chain: "637807030d55d01f9a0cb3a7839515d796bd07706386a6eddf06cc29a65a0e29",
            depth: 4,
            index: 2147483646 + BIP32_HARDEN,
         },
         Node {
            path: &[
               0,
               2147483647 + BIP32_HARDEN,
               1,
               2147483646 + BIP32_HARDEN,
               2,
            ],
            key: "bb7d39bdb83ecf58f2fd82b6d918341cbef428661ef01ab97c28a4842125ac23",
            chain: "9452b549be8cea3ecb7a84bec10dcfd94afe4d129ebfd3b3cb58eedf394ed271",
            depth: 5,
            index: 2,
         },
      ];
      for node in &nodes {
         assert_node(&root, node);
      }
   }

   #[test]
   fn bip32_vector_3() {
      let root = xpriv_from_seed(
         "4b381541583be4423346c643850da4b320e46a87ae3d2a4e6da11eba819cd4acba45d239319ac14f863b8d5ab5a0d0c64d2e8a1e7d1457df2e5a3c51c73235be",
      );
      let nodes = [
         Node {
            path: &[],
            key: "00ddb80b067e0d4993197fe10f2657a844a384589847602d56f0c629c81aae32",
            chain: "01d28a3e53cffa419ec122c968b3259e16b65076495494d97cae10bbfec3c36f",
            depth: 0,
            index: 0,
         },
         Node {
            path: &[BIP32_HARDEN],
            key: "491f7a2eebc7b57028e0d3faa0acda02e75c33b03c48fb288c41e2ea44e1daef",
            chain: "e5fea12a97b927fc9dc3d2cb0d1ea1cf50aa5a1fdc1f933e8906bb38df3377bd",
            depth: 1,
            index: BIP32_HARDEN,
         },
      ];
      for node in &nodes {
         assert_node(&root, node);
      }
   }

   #[test]
   fn bip32_vector_4() {
      let root =
         xpriv_from_seed("3ddd5602285899a946114506157c7997e5444528f3003f6134712147db19b678");
      let nodes = [
         Node {
            path: &[],
            key: "12c0d59c7aa3a10973dbd3f478b65f2516627e3fe61e00c345be9a477ad2e215",
            chain: "d0c8a1f6edf2500798c3e0b54f1b56e45f6d03e6076abd36e5e2f54101e44ce6",
            depth: 0,
            index: 0,
         },
         Node {
            path: &[BIP32_HARDEN],
            key: "00d948e9261e41362a688b916f297121ba6bfb2274a3575ac0e456551dfd7f7e",
            chain: "cdc0f06456a14876c898790e0b3b1a41c531170aec69da44ff7b7265bfe7743b",
            depth: 1,
            index: BIP32_HARDEN,
         },
         Node {
            path: &[BIP32_HARDEN, 1 + BIP32_HARDEN],
            key: "3a2086edd7d9df86c3487a5905a1712a9aa664bce8cc268141e07549eaa8661d",
            chain: "a48ee6674c5264a237703fd383bccd9fad4d9378ac98ab05e6e7029b06360c0d",
            depth: 2,
            index: 1 + BIP32_HARDEN,
         },
      ];
      for node in &nodes {
         assert_node(&root, node);
      }
   }

   #[test]
   fn seed_too_short() {
      assert!(matches!(
         root_from_seed(&[0u8; 15], None),
         Err(Bip32Error::SeedTooShort)
      ));
   }

   #[test]
   fn empty_path_returns_clone() {
      let root = xpriv_from_seed("000102030405060708090a0b0c0d0e0f");
      let same = root.derive_path(&[] as &[u32]).unwrap();
      assert!(root == same);
   }

   #[test]
   fn eq_includes_secret() {
      let a = xpriv_from_seed("000102030405060708090a0b0c0d0e0f");
      let b = xpriv_from_seed("000102030405060708090a0b0c0d0e0f");
      let c = xpriv_from_seed("101112131415161718191a1b1c1d1e1f");
      assert!(a == b);
      assert!(a != c);
   }

   #[test]
   fn depth_overflow() {
      let mut current = xpriv_from_seed("000102030405060708090a0b0c0d0e0f");
      for _ in 0..255 {
         current = current.derive_child(0).unwrap();
      }
      assert_eq!(current.xkey_info.depth, 255);
      assert!(matches!(
         current.derive_child(0),
         Err(Bip32Error::DepthOverflow)
      ));
   }
}
