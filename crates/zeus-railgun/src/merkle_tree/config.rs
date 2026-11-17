use std::fmt::Debug;

use alloy_primitives::utils::keccak256_cached;
use ruint::aliases::U256;

use crate::crypto::{poseidon2, railgun_zero::SNARK_PRIME};

pub const TREE_DEPTH: usize = 16;
pub const TOTAL_LEAVES: u32 = 1 << TREE_DEPTH;

pub trait MerkleConfig: Debug + Clone + PartialEq + Eq {
   const DEPTH: usize;

   fn hash(left: U256, right: U256) -> U256;
   fn zero() -> U256;
}

/// Merkle tree configuration for Railgun.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RailgunMerkleConfig;

impl MerkleConfig for RailgunMerkleConfig {
   const DEPTH: usize = TREE_DEPTH;

   fn hash(left: U256, right: U256) -> U256 {
      poseidon2(left, right)
   }

   fn zero() -> U256 {
      let hash = U256::from_be_bytes(*keccak256_cached(b"Railgun"));
      hash % SNARK_PRIME
   }
}
