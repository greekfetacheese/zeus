pub mod address;
pub mod keys;
pub mod signer;

use crate::crypto::{BASE8, mul_point_escalar, poseidon_hash};
use crate::crypto::{babyjubjub::CURVE_SEED, hmac_sha512};
use crate::types::{Key32, Key64};

use alloy_primitives::U256;
use anyhow::anyhow;
use ark_ed_on_bn254::Fr as BabyJubScalar;
use ark_ff::{BigInteger, PrimeField};
use blake_hash::{Blake512, Digest};
use curve25519_dalek::{EdwardsPoint, Scalar, constants::ED25519_BASEPOINT_TABLE};
use num_bigint::BigUint;
use secure_types::Zeroize;
use sha2::Digest as ShaDigest;

type SpendX = U256;
type SpendY = U256;

type NullifyingKey = U256;
type ViewPublicKey = [u8; 32];

pub fn derive_private_key(seed: &[u8], path: &str) -> Result<Key32, anyhow::Error> {
   let (mut current_key, mut current_chain_code) = hmac_sha512(CURVE_SEED, seed);

   // Parse and derive for each segment in path (skip 'm')
   let segments: Vec<&str> = path.split('/').skip(1).collect();
   for segment in segments {
      let index_str = segment.trim_end_matches("'");
      let mut index: u32 = index_str.parse()?;

      index += 0x80000000;

      let index_bytes = index.to_be_bytes();

      // Data = 0x00 + current_key + index_bytes
      let mut data = vec![0u8];
      current_key.unlock(|slice| data.extend_from_slice(slice));
      data.extend_from_slice(&index_bytes);

      let (key, chain_code) = current_chain_code.unlock(|code| {
         let (new_key, new_code) = hmac_sha512(code, &data);
         (new_key, new_code)
      });

      data.zeroize();

      current_key = key;
      current_chain_code = chain_code;
   }

   Ok(current_key)
}

pub fn compute_public_spending_key(
   seed: &Key64,
   path: &str,
) -> Result<(SpendX, SpendY), anyhow::Error> {
   let spending_priv_res = seed.unlock(|seed_bytes| derive_private_key(seed_bytes, path));
   let spending_priv = spending_priv_res.map_err(|e| anyhow!("Derive spending key {}", e))?;

   let mut scalar_bytes = spending_priv.unlock(|priv_bytes| {
      let hash = Blake512::digest(priv_bytes);
      let mut sb = [0u8; 32];
      sb.copy_from_slice(&hash[..32]);
      // prune / clamp
      sb[0] &= 248;
      sb[31] &= 127;
      sb[31] |= 64;
      sb
   });

   // LE bigint
   let scalar_big = BigUint::from_bytes_le(&scalar_bytes);
   scalar_bytes.zeroize();
   let scalar_shifted: BigUint = scalar_big >> 3;

   // Convert back to Fr
   let scalar_bytes_shifted = scalar_shifted.to_bytes_le();
   let mut padded = [0u8; 32];
   padded[0..scalar_bytes_shifted.len()].copy_from_slice(&scalar_bytes_shifted);
   let scalar = BabyJubScalar::from_le_bytes_mod_order(&padded);

   padded.zeroize();

   let point = mul_point_escalar(*BASE8, scalar.into_bigint().into());
   let affine = point;

   let mut x_bytes = affine.x.into_bigint().to_bytes_le();
   let mut y_bytes = affine.y.into_bigint().to_bytes_le();

   let mut spend_x_bytes = [0u8; 32];
   let mut spend_y_bytes = [0u8; 32];

   spend_x_bytes[0..x_bytes.len()].copy_from_slice(&x_bytes);
   spend_y_bytes[0..y_bytes.len()].copy_from_slice(&y_bytes);

   let spend_x = U256::from_le_bytes(spend_x_bytes);
   let spend_y = U256::from_le_bytes(spend_y_bytes);

   x_bytes.zeroize();
   y_bytes.zeroize();
   spend_x_bytes.zeroize();
   spend_y_bytes.zeroize();

   Ok((spend_x, spend_y))
}

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
   let h_view = sha2::Sha512::digest(&view_priv_bytes);
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

   let nullifying_key = poseidon_hash(&vec![view_priv_u256])?;

   Ok((viewing_public_key, nullifying_key))
}
