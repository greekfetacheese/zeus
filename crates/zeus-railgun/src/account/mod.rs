pub mod address;
mod derive;
pub mod keys;
pub mod signer;

use crate::crypto::poseidon_hash;
use crate::types::Key64;

use alloy_primitives::U256;
use anyhow::anyhow;
use curve25519_dalek::{EdwardsPoint, Scalar, constants::ED25519_BASEPOINT_TABLE};
use secure_types::Zeroize;
use sha2::Digest as ShaDigest;

pub use derive::{compute_public_spending_key, derive_private_key};

type NullifyingKey = U256;
type ViewPublicKey = [u8; 32];

pub fn compute_public_viewing_key(
   seed: &Key64,
   path: &str,
) -> Result<(ViewPublicKey, NullifyingKey), anyhow::Error> {
   let viewing_priv_res = seed.unlock(|seed_bytes| derive_private_key(seed_bytes, path));

   let viewing_priv = viewing_priv_res.map_err(|e| anyhow!("Derive viewing key {}", e))?;
   let mut view_priv_bytes = viewing_priv.unlock(|bytes| {
      let mut slice = [0u8; 32];
      slice.copy_from_slice(bytes);
      slice
   });

   // Compute scalar per Ed25519 spec (SHA-512 hash, clamp lower 32 bytes)
   let h_view = sha2::Sha512::digest(view_priv_bytes);
   let mut scalar_bytes: [u8; 32] = [0; 32];
   scalar_bytes.copy_from_slice(&h_view[0..32]);
   scalar_bytes[0] &= 248;
   scalar_bytes[31] &= 127;
   scalar_bytes[31] |= 64;

   let scalar = Scalar::from_bytes_mod_order(scalar_bytes);
   let point: EdwardsPoint = ED25519_BASEPOINT_TABLE * &scalar;

   let viewing_public_key = point.compress().to_bytes();
   let view_priv_u256 = U256::from_be_bytes(view_priv_bytes);
   view_priv_bytes.zeroize();

   let nullifying_key = poseidon_hash(&[view_priv_u256])?;

   Ok((viewing_public_key, nullifying_key))
}
