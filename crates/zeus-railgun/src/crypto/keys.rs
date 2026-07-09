use anyhow::anyhow;
use rand::Rng;
use rand::distr::{Distribution, StandardUniform};
use sha2::{Digest, Sha256, Sha512};
use std::hash::Hash;

use alloy_primitives::U256;
use ark_bn254::Fr;
use ark_ff::{BigInteger, PrimeField};
use curve25519_dalek::{EdwardsPoint, Scalar, edwards::CompressedEdwardsY};
use ed25519_dalek::SigningKey;
use secure_types::{SecureArray, Zeroize};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::poseidon_hash;
use crate::{
   account::derive_private_key,
   crypto::aes::{AesError, Ciphertext, CiphertextCtr, decrypt_gcm, encrypt_ctr, encrypt_gcm},
};

#[derive(Serialize, Deserialize)]
pub struct SpendingSignature {
   pub r8_x: U256,
   pub r8_y: U256,
   pub s: U256,
}

#[derive(Debug, Error)]
pub enum KeyError {
   #[error("Failed to decompress public key")]
   DecompressionFailed,
   #[error("Hex decoding error: {0}")]
   HexDecodingError(#[from] hex::FromHexError),
}

/// Symmetric key for AES encryption.
#[derive(Copy, Clone, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub struct SharedKey([u8; 32]);

/// Blinded public key for stealth addresses.
#[derive(Copy, Clone, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub struct BlindedKey([u8; 32]);

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ViewingPublicKey(pub [u8; 32]);

/// Key for nullifier derivation.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct NullifyingKey([u8; 32]);

/// Master public key (wallet identifier).
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub struct MasterPublicKey(pub [u8; 32]);

/// Private key for signing transactions (BabyJubJub curve).
#[derive(Clone)]
pub struct SpendingKey(pub SecureArray<u8, 32>);

impl Eq for SpendingKey {}

impl PartialEq for SpendingKey {
   fn eq(&self, other: &Self) -> bool {
      self.0.unlock(|slice_1| other.0.unlock(|slice_2| slice_1 == slice_2))
   }
}

impl Ord for SpendingKey {
   fn cmp(&self, other: &Self) -> std::cmp::Ordering {
      self.0.unlock(|slice_1| other.0.unlock(|slice_2| slice_1.cmp(slice_2)))
   }
}

impl PartialOrd for SpendingKey {
   fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
      Some(self.cmp(other))
   }
}

impl Hash for SpendingKey {
   fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
      self.0.unlock(|slice| state.write(slice))
   }
}

impl SpendingKey {
   pub fn new(key: SecureArray<u8, 32>) -> Self {
      Self(key)
   }

   pub fn from_bytes(mut bytes: [u8; 32]) -> Self {
      let sec_array = SecureArray::from_slice_mut(&mut bytes).unwrap();
      Self(sec_array)
   }

   pub fn derive(seed: &[u8], path: &str) -> Result<Self, anyhow::Error> {
      let key = derive_private_key(seed, path)?;

      Ok(Self(key))
   }

   pub fn public_key(&self) -> SpendingPublicKey {
      let mut bytes = self.0.unlock(|slice| {
         let mut arr = [0u8; 32];
         arr.copy_from_slice(slice);
         arr
      });

      let sk = super::babyjubjub::PrivateKey::new(bytes);
      bytes.zeroize();
      let pk = sk.public();

      let x = pk.x.into_bigint().to_bytes_be();
      let y = pk.y.into_bigint().to_bytes_be();

      let mut x32 = [0u8; 32];
      let mut y32 = [0u8; 32];

      x32[32 - x.len()..].copy_from_slice(&x);
      y32[32 - y.len()..].copy_from_slice(&y);

      SpendingPublicKey { x: x32, y: y32 }
   }

   pub fn sign(&self, message: U256) -> Result<SpendingSignature, anyhow::Error> {
      let mut bytes = self.0.unlock(|slice| {
         let mut arr = [0u8; 32];
         arr.copy_from_slice(slice);
         arr
      });

      let sk = super::babyjubjub::PrivateKey::new(bytes);
      bytes.zeroize();
      let sig = sk.sign(message.into()).map_err(|e| anyhow!("BabyJubJub sign error: {}", e))?;

      Ok(SpendingSignature {
         r8_x: U256::from_be_bytes(fr_to_be_bytes(sig.r_b8.x)),
         r8_y: U256::from_be_bytes(fr_to_be_bytes(sig.r_b8.y)),
         s: U256::from_le_slice(&sig.s.to_bytes_le().1),
      })
   }
}

/// Private key for viewing transactions and ECDH.
#[derive(Clone)]
pub struct ViewingKey(pub SecureArray<u8, 32>);

impl std::fmt::Debug for ViewingKey {
   fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
      write!(f, "ViewingKey **** SECRET ****")
   }
}

impl Eq for ViewingKey {}

impl PartialEq for ViewingKey {
   fn eq(&self, other: &Self) -> bool {
      self.0.unlock(|slice_1| other.0.unlock(|slice_2| slice_1 == slice_2))
   }
}

impl Ord for ViewingKey {
   fn cmp(&self, other: &Self) -> std::cmp::Ordering {
      self.0.unlock(|slice_1| other.0.unlock(|slice_2| slice_1.cmp(slice_2)))
   }
}

impl PartialOrd for ViewingKey {
   fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
      Some(self.cmp(other))
   }
}

impl Hash for ViewingKey {
   fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
      self.0.unlock(|slice| state.write(slice))
   }
}

impl ViewingKey {
   pub fn new(key: SecureArray<u8, 32>) -> Self {
      Self(key)
   }

   pub fn derive(seed: &[u8], path: &str) -> Result<Self, anyhow::Error> {
      let key = derive_private_key(seed, path)?;

      Ok(Self(key))
   }

   pub fn from_bytes(mut bytes: [u8; 32]) -> Self {
      let sec_array = SecureArray::from_slice_mut(&mut bytes).unwrap();
      Self(sec_array)
   }

   pub fn public_key(&self) -> ViewingPublicKey {
      let mut bytes = self.0.unlock(|slice| {
         let mut arr = [0u8; 32];
         arr.copy_from_slice(slice);
         arr
      });

      let signing_key = SigningKey::from_bytes(&bytes);
      bytes.zeroize();

      ViewingPublicKey(signing_key.verifying_key().to_bytes())
   }

   pub fn to_u256(&self) -> U256 {
      let bytes = self.0.unlock(|slice| {
         let mut arr = [0u8; 32];
         arr.copy_from_slice(slice);
         arr
      });

      U256::from_be_bytes(bytes)
   }

   pub(crate) fn nullifying_key(&self) -> NullifyingKey {
      NullifyingKey::new(self.clone())
   }

   pub(crate) fn derive_shared_key(
      &self,
      their_public: ViewingPublicKey,
   ) -> Result<SharedKey, KeyError> {
      let point = CompressedEdwardsY(their_public.0)
         .decompress()
         .ok_or(KeyError::DecompressionFailed)?;
      Ok(SharedKey::new(self, point))
   }

   pub(crate) fn derive_shared_key_blinded(
      &self,
      blinded: BlindedKey,
   ) -> Result<SharedKey, KeyError> {
      let point = CompressedEdwardsY(blinded.0)
         .decompress()
         .ok_or(KeyError::DecompressionFailed)?;
      Ok(SharedKey::new(self, point))
   }

   pub fn encrypt_ctr(&self, plaintext: &[&[u8]], iv: &[u8; 16]) -> CiphertextCtr {
      let mut bytes = self.0.unlock(|slice| {
         let mut arr = [0u8; 32];
         arr.copy_from_slice(slice);
         arr
      });

      let cipher = encrypt_ctr(plaintext, &bytes, iv);
      bytes.zeroize();

      cipher
   }

   fn to_curve25519_scalar(&self) -> Scalar {
      let hash = self.0.unlock(|slice| Sha512::digest(slice));

      let mut head = [0u8; 32];
      head.copy_from_slice(&hash[..32]);

      // Clamp as per Ed25519
      head[0] &= 248;
      head[31] &= 63;
      head[31] |= 64;

      Scalar::from_bytes_mod_order(head)
   }
}

impl NullifyingKey {
   pub fn new(viewing_key: ViewingKey) -> Self {
      let hash = poseidon_hash(&[viewing_key.to_u256()]).unwrap();

      NullifyingKey::from_u256(hash)
   }

   pub fn to_u256(&self) -> U256 {
      U256::from_be_bytes(self.0)
   }

   pub fn from_u256(value: U256) -> Self {
      let bytes = value.to_be_bytes::<32>();
      Self(bytes)
   }

   pub fn to_hex(&self) -> String {
      hex::encode(self.0)
   }
}

impl BlindedKey {
   pub fn from_bytes(bytes: [u8; 32]) -> Self {
      Self(bytes)
   }

   pub fn to_u256(&self) -> U256 {
      U256::from_be_bytes(self.0)
   }

   pub fn to_hex(&self) -> String {
      hex::encode(self.0)
   }
}

impl ViewingPublicKey {
   pub fn to_hex(&self) -> String {
      hex::encode(self.0)
   }

   pub fn from_bytes(bytes: [u8; 32]) -> Self {
      Self(bytes)
   }

   pub fn as_bytes(&self) -> &[u8; 32] {
      &self.0
   }

   pub fn to_u256(&self) -> U256 {
      U256::from_be_bytes(self.0)
   }

   pub fn from_hex(hex: &str) -> Result<Self, KeyError> {
      let hex = hex.strip_prefix("0x").unwrap_or(hex);

      if hex.len() != 64 {
         return Err(KeyError::HexDecodingError(
            hex::FromHexError::InvalidStringLength,
         ));
      }

      let bytes = hex::decode(hex)?;
      let arr: [u8; 32] = bytes
         .as_slice()
         .try_into()
         .map_err(|_| hex::FromHexError::InvalidStringLength)?;

      Ok(Self(arr))
   }
}

impl SharedKey {
   pub fn new(viewing_key: &ViewingKey, their_point: EdwardsPoint) -> Self {
      let scalar = viewing_key.to_curve25519_scalar();
      let shared = their_point * scalar;
      let digest = Sha256::digest(shared.compress().to_bytes());
      SharedKey(digest.into())
   }

   pub fn encrypt_gcm(&self, plaintext: &[&[u8]], iv: &[u8; 16]) -> Result<Ciphertext, AesError> {
      encrypt_gcm(plaintext, &self.0, iv)
   }

   pub fn decrypt_gcm(&self, ciphertext: &Ciphertext) -> Result<Vec<Vec<u8>>, AesError> {
      decrypt_gcm(ciphertext, &self.0)
   }

   pub fn to_hex(&self) -> String {
      hex::encode(self.0)
   }
}

impl MasterPublicKey {
   pub(crate) fn new(spending_pubkey: SpendingPublicKey, nullifying_key: NullifyingKey) -> Self {
      let hash = poseidon_hash(&[
         spending_pubkey.x_u256(),
         spending_pubkey.y_u256(),
         nullifying_key.to_u256(),
      ])
      .unwrap();
      MasterPublicKey::from_u256(hash)
   }

   pub fn as_bytes(&self) -> &[u8; 32] {
      &self.0
   }

   pub fn to_u256(&self) -> U256 {
      U256::from_be_bytes(self.0)
   }

   fn from_u256(value: U256) -> Self {
      let bytes = value.to_be_bytes::<32>();
      Self(bytes)
   }

   pub fn to_hex(&self) -> String {
      hex::encode(self.0)
   }
}

#[derive(Copy, Clone, Eq, PartialEq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SpendingPublicKey {
   x: [u8; 32],
   y: [u8; 32],
}

impl std::fmt::Debug for SpendingPublicKey {
   fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
      write!(
         f,
         "SpendingPublicKey {{ x: {:?}, y: {:?} }}",
         self.x, self.y
      )
   }
}

impl SpendingPublicKey {
   pub fn new(x: [u8; 32], y: [u8; 32]) -> Self {
      Self { x, y }
   }

   pub fn x_hex(&self) -> String {
      hex::encode(self.x)
   }

   pub fn y_hex(&self) -> String {
      hex::encode(self.y)
   }

   pub fn x_u256(&self) -> U256 {
      U256::from_be_bytes(self.x)
   }

   pub fn y_u256(&self) -> U256 {
      U256::from_be_bytes(self.y)
   }
}

impl Distribution<ViewingKey> for StandardUniform {
   fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> ViewingKey {
      let mut bytes: [u8; 32] = rng.random();
      bytes[0] &= 0x1F; // Mask for BN254 range (matching kohaku)
      let array = SecureArray::from_slice(&bytes).expect("32 byte array is valid");
      ViewingKey(array)
   }
}

impl Distribution<SpendingKey> for StandardUniform {
   fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> SpendingKey {
      let mut bytes: [u8; 32] = rng.random();
      bytes[0] &= 0x1F; // Mask for BN254 range (matching kohaku)
      let array = SecureArray::from_slice(&bytes).expect("32 byte array is valid");
      SpendingKey(array)
   }
}

fn fr_to_be_bytes(f: Fr) -> [u8; 32] {
   let mut out = [0u8; 32];
   let le = f.into_bigint().to_bytes_le();
   out[..le.len()].copy_from_slice(&le);
   out.reverse(); // convert LE -> BE
   out
}

#[cfg(test)]
mod tests {
   use super::*;
   use ruint::uint;

   // Test key and key derivation correctness against known values. Known values
   // were generated using the Railgun JS SDK.
   #[test]
   fn test_spending_key() {
      let array = SecureArray::from_slice(&[1u8; 32]).unwrap();
      let spending_key = SpendingKey::new(array);

      let spending_pubkey = spending_key.public_key();
      let expected_x = "234056d968baf183fe8d237d496d1c04188220cd33e8f8d14df9b84479736b20";
      let expected_y = "2624393fad9b71c04b3b14d8ac45202dbb4eaff4c2d1350c9453fc08d18651fe";

      assert_eq!(expected_x, spending_pubkey.x_hex());
      assert_eq!(expected_y, spending_pubkey.y_hex());
   }

   #[test]
   fn test_viewing_key() {
      let array = SecureArray::from_slice(&[2u8; 32]).unwrap();
      let viewing_key = ViewingKey::new(array);

      let viewing_pubkey = viewing_key.public_key();
      let expected_pubkey = "8139770ea87d175f56a35466c34c7ecccb8d8a91b4ee37a25df60f5b8fc9b394";

      assert_eq!(expected_pubkey, viewing_pubkey.to_hex());
   }

   #[test]
   fn test_master_public_key() {
      let array = SecureArray::from_slice(&[1u8; 32]).unwrap();
      let spending_key = SpendingKey::new(array);

      let array = SecureArray::from_slice(&[2u8; 32]).unwrap();
      let viewing_key = ViewingKey::new(array);

      let master_key = MasterPublicKey::new(
         spending_key.public_key(),
         viewing_key.nullifying_key(),
      );
      let expected_master_key = "21532725e608f56b562244d61ef15288a3ab3f01b7790586f9ed0c2e7baa6b29";

      assert_eq!(expected_master_key, master_key.to_hex());
   }

   #[test]
   fn test_shared_key() {
      let array = SecureArray::from_slice(&[2u8; 32]).unwrap();
      let viewing_key = ViewingKey::new(array);

      let array = SecureArray::from_slice(&[3u8; 32]).unwrap();
      let their_viewing = ViewingKey::new(array);

      let shared_key_ab = viewing_key.derive_shared_key(their_viewing.public_key()).unwrap();
      let shared_key_ba = their_viewing.derive_shared_key(viewing_key.public_key()).unwrap();

      let expected_shared_key = "b8d9b27ccb6161ba969a646553ad1b7221b4113ac83bdd603985ce44923456f1";

      assert_eq!(expected_shared_key, shared_key_ab.to_hex());
      assert_eq!(shared_key_ab.to_hex(), shared_key_ba.to_hex());
   }

   #[test]
   fn test_nullifying_key() {
      let array = SecureArray::from_slice(&[2u8; 32]).unwrap();
      let viewing_key = ViewingKey::new(array);
      let nullifying_key = viewing_key.nullifying_key();

      let expected = "186ab99ece60e112b37c660eaf7ca6dbcb04dc434e04aa5e106e94abc6c81936";
      assert_eq!(expected, nullifying_key.to_hex());
   }

   #[test]
   fn test_sign() {
      let array = SecureArray::from_slice(&[1u8; 32]).unwrap();
      let spending_key = SpendingKey::new(array);
      let message = U256::from(42u64);
      let signature = spending_key.sign(message).unwrap();

      let expected_r8_x =
         uint!(14021219264176114698656285200925183015004950119566700345808626607587007258652_U256);
      let expected_r8_y =
         uint!(722845713210012403245093368934831287436133400350912012728600696178479669333_U256);
      let expected_s =
         uint!(719423466960100536815219984091461547618047721989819848960065284130969424009_U256);

      assert_eq!(expected_r8_x, signature.r8_x);
      assert_eq!(expected_r8_y, signature.r8_y);
      assert_eq!(expected_s, signature.s);
   }
}
