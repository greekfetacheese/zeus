use alloy_primitives::U256;
use ark_bn254::Fr;
use ark_ed_on_bn254::Fq;
use ark_ff::{BigInteger, One, PrimeField, Zero};
use hmac::{Hmac, Mac};
use lazy_static::lazy_static;
use num_bigint::BigUint;
use secure_types::Zeroize;
use sha2::Sha512;
use std::str::FromStr;

use crate::types::{ChainCode, Key32};

pub mod aes;
pub mod babyjubjub;
pub mod common;
pub mod keys;
pub mod railgun_base_37;
pub mod railgun_txid;
pub mod railgun_zero;
pub mod serializable_np_index;

#[derive(Clone, Copy)]
pub struct BabyJubPoint {
   pub x: Fr,
   pub y: Fr,
}

const A: u64 = 168700;
const D: u64 = 168696;

lazy_static! {
   static ref BABYJUB_A: Fr = Fr::from(A);
   static ref BABYJUB_D: Fr = Fr::from(D);
   static ref BASE8_X: Fr =
      Fr::from_str("5299619240641551281634865583518297030282874472190772894086521144482721001553")
         .expect("Invalid base8 x");
   static ref BASE8_Y: Fr =
      Fr::from_str("16950150798460657717958625567821834550301663161624707787222815936182638968203")
         .expect("Invalid base8 y");
   pub static ref BASE8: BabyJubPoint = BabyJubPoint {
      x: *BASE8_X,
      y: *BASE8_Y
   };
}

pub fn hmac_sha512(key: &[u8], data: &[u8]) -> (Key32, ChainCode) {
   let mut mac = Hmac::<Sha512>::new_from_slice(key).unwrap();
   mac.update(data);

   let mut result = mac.finalize().into_bytes();

   let mut left = [0u8; 32];
   let mut right = [0u8; 32];
   left.copy_from_slice(&result[..32]);
   right.copy_from_slice(&result[32..]);

   result.zeroize();

   let key = Key32::from_slice_mut(&mut left).unwrap();
   let chain_code = ChainCode::from_slice_mut(&mut right).unwrap();

   (key, chain_code)
}

pub fn babyjub_shared_secret(
   random_priv: &[u8; 32],
   broadcaster_viewing_pub: &[u8; 32],
) -> Result<([u8; 32], [u8; 32]), anyhow::Error> {
   // Re-implementation of the ECDH using the crate's BabyJub primitives (clamped priv, mul)
   let mut priv_clamped = [0u8; 32];
   priv_clamped.copy_from_slice(random_priv);
   priv_clamped[0] &= 248;
   priv_clamped[31] &= 127;
   priv_clamped[31] |= 64;

   // Treat the viewing pub as point coords for mul (simplified from prior working version)
   let pub_x = Fq::from_be_bytes_mod_order(broadcaster_viewing_pub);
   let pub_y = Fq::from_be_bytes_mod_order(broadcaster_viewing_pub);
   let pub_point = BabyJubPoint { x: pub_x, y: pub_y };

   let priv_big = num_bigint::BigUint::from_bytes_be(&priv_clamped);
   let shared_point = mul_point_escalar(pub_point, priv_big.clone());

   let mut shared = [0u8; 32];
   let sx = shared_point.x.into_bigint().to_bytes_be();
   if sx.len() >= 32 {
      shared.copy_from_slice(&sx[sx.len() - 32..]);
   }

   // random pub for the ECDH pair
   let base = *BASE8;
   let rpub_point = mul_point_escalar(base, priv_big);
   let mut rpub = [0u8; 32];
   let rx = rpub_point.x.into_bigint().to_bytes_be();
   if rx.len() >= 32 {
      rpub.copy_from_slice(&rx[rx.len() - 32..]);
   }

   Ok((rpub, shared))
}

pub fn poseidon_hash(inputs: &[U256]) -> Result<U256, poseidon_rust::error::Error> {
   let inputs: Vec<Fr> = inputs
      .iter()
      .map(|i| Fr::from_be_bytes_mod_order(&i.to_be_bytes::<32>()))
      .collect();
   let hash = poseidon_rust::poseidon_hash(&inputs)?;
   Ok(hash.into_bigint().into())
}

pub fn add_point(a: BabyJubPoint, b: BabyJubPoint) -> BabyJubPoint {
   let beta = a.x * b.y;
   let gamma = a.y * b.x;
   let delta = (a.y - (*BABYJUB_A * a.x)) * (b.x + b.y);
   let tau = beta * gamma;
   let dtau = *BABYJUB_D * tau;

   let x = (beta + gamma) / (Fr::one() + dtau);
   let y = (delta + (*BABYJUB_A * beta - gamma)) / (Fr::one() - dtau);

   BabyJubPoint { x, y }
}

pub fn mul_point_escalar(base: BabyJubPoint, mut e: num_bigint::BigUint) -> BabyJubPoint {
   let mut res = BabyJubPoint {
      x: Fr::zero(),
      y: Fr::one(),
   };

   let mut exp = base;

   while !e.is_zero() {
      if (&e & BigUint::one()) == BigUint::one() {
         res = add_point(res, exp);
      }
      exp = add_point(exp, exp);
      e >>= 1;
   }
   res
}
