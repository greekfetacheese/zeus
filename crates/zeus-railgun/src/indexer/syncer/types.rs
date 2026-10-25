use alloy_primitives::{B256, FixedBytes};
use ruint::aliases::U256;
use serde::{Deserialize, Serialize};

use crate::{
   caip::AssetId,
   crypto::{aes::Ciphertext, poseidon3},
   merkle_tree::UtxoLeafHash,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum SyncEvent {
   Shield(Shield, u64),
   Transact(Transact, u64),
   Nullified(Nullified, u64),
   Legacy(LegacyCommitment, u64),
}

/// A single Shield commitment event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Shield {
   pub tree_number: u32,
   pub leaf_index: u32,
   pub npk: U256,
   pub token: AssetId,
   pub value: U256,
   pub ciphertext: Ciphertext,
   pub shield_key: [u8; 32],
   pub hash: Option<UtxoLeafHash>,
   #[serde(default)]
   pub timestamp: u64,
   #[serde(default)]
   pub tx_hash: B256,
}

/// A single transact commitment event
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Transact {
   pub tree_number: u32,
   pub leaf_index: u32,
   pub hash: U256,
   pub ciphertext: Ciphertext,
   pub blinded_sender_viewing_key: [u8; 32],
   /// Same status as `annotation_data` (~6.5% of the payload): write-only here.
   /// POI is the only plausible future consumer.
   pub blinded_receiver_viewing_key: [u8; 32],
   /// Not read by Zeus today: no decoder touches it (`decrypt_transact` reads the
   /// sender key, and the memo comes from ciphertext block 3). Kept because dropping
   /// it would leave future annotation consumers needing an RPC refetch; it is
   /// ~12.8% of the events snapshot payload. Revisit when POI is wired.
   pub annotation_data: Vec<u8>,
   #[serde(default)]
   pub timestamp: u64,
   #[serde(default)]
   pub tx_hash: B256,
}

/// A single nullified event
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Nullified {
   pub tree_number: u32,
   pub nullifier: FixedBytes<32>,
   #[serde(default)]
   pub timestamp: u64,
   #[serde(default)]
   pub tx_hash: B256,
}

/// Legacy ciphertext format from pre-Mar23 CommitmentBatch events.
/// (ciphertext[4], ephemeralKeys[2], memo[])
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct LegacyCiphertext {
   pub ciphertext: [U256; 4],
   pub ephemeral_keys: [U256; 2],
   pub memo: Vec<U256>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[non_exhaustive]
pub struct LegacyCommitment {
   pub hash: U256,
   pub tree_number: u32,
   pub leaf_index: u32,
   /// Present only when decoded from legacy CommitmentBatch (RPC path).
   /// Subsquid path currently only provides the hash.
   pub ciphertext: Option<LegacyCiphertext>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Operation {
   pub block_number: u64,
   pub nullifiers: Vec<U256>,
   pub commitment_hashes: Vec<U256>,
   pub bound_params_hash: U256,
   pub utxo_tree_in: u32,
   pub utxo_tree_out: u32,
   pub utxo_out_start_index: u32,
}

#[derive(Debug)]
pub struct SyncerError(Box<dyn std::error::Error + Send + Sync>);

impl SyncerError {
   pub fn new<E: std::error::Error + Send + Sync + 'static>(e: E) -> Self {
      SyncerError(Box::new(e))
   }
}

impl std::fmt::Display for SyncerError {
   fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
      write!(f, "Syncer error: {}", self.0)
   }
}

impl std::error::Error for SyncerError {
   fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
      Some(self.0.as_ref())
   }
}

impl SyncEvent {
   pub fn block_number(&self) -> u64 {
      match self {
         SyncEvent::Shield(_, b) => *b,
         SyncEvent::Transact(_, b) => *b,
         SyncEvent::Nullified(_, b) => *b,
         SyncEvent::Legacy(_, b) => *b,
      }
   }
}

impl Shield {
   pub fn hash(&self) -> UtxoLeafHash {
      if let Some(hash) = self.hash {
         return hash;
      }

      poseidon3(
         self.npk,
         self.token.hash(),
         U256::from(self.value),
      )
      .into()
   }
}

#[cfg(test)]
mod syncer_error_tests {
   use std::error::Error as _;

   use super::*;

   #[test]
   fn renders_the_pinned_message() {
      let err = SyncerError::new(std::io::Error::other("boom"));
      assert_eq!(err.to_string(), "Syncer error: boom");
   }

   #[test]
   fn delegates_its_source_to_the_boxed_error() {
      let inner = std::io::Error::other("boom");
      let rendered = inner.to_string();
      let err = SyncerError::new(inner);
      assert_eq!(
         err.source().map(ToString::to_string),
         Some(rendered)
      );
      assert_eq!(
         err.source().map(ToString::to_string),
         Some("boom".to_owned())
      );
   }
}
