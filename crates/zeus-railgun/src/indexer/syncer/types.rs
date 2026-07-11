use alloy_primitives::FixedBytes;
use ruint::aliases::U256;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
   caip::AssetId,
   crypto::{aes::Ciphertext, poseidon_hash},
   merkle_tree::UtxoLeafHash,
};

// TODO: impl a snapshot to store all the SyncEvent for faster syncing
#[derive(Debug, Clone, Serialize, Deserialize)]
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
}

/// A single transact commitment event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transact {
   pub tree_number: u32,
   pub leaf_index: u32,
   pub hash: U256,
   pub ciphertext: Ciphertext,
   pub blinded_sender_viewing_key: [u8; 32],
   pub blinded_receiver_viewing_key: [u8; 32],
   pub annotation_data: Vec<u8>,
}

/// A single nullified event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Nullified {
   pub tree_number: u32,
   pub nullifier: FixedBytes<32>,
}


/// Legacy ciphertext format from pre-Mar23 CommitmentBatch events.
/// (ciphertext[4], ephemeralKeys[2], memo[])
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LegacyCiphertext {
   pub ciphertext: [U256; 4],
   pub ephemeral_keys: [U256; 2],
   pub memo: Vec<U256>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LegacyCommitment {
   pub hash: U256,
   pub tree_number: u32,
   pub leaf_index: u32,
   /// Present only when decoded from legacy CommitmentBatch (RPC path).
   /// Subsquid path currently only provides the hash.
   pub ciphertext: Option<LegacyCiphertext>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Operation {
   pub block_number: u64,
   pub nullifiers: Vec<U256>,
   pub commitment_hashes: Vec<U256>,
   pub bound_params_hash: U256,
   pub utxo_tree_in: u32,
   pub utxo_tree_out: u32,
   pub utxo_out_start_index: u32,
}

#[derive(Debug, Error)]
#[error("Syncer error: {0}")]
pub struct SyncerError(#[source] Box<dyn std::error::Error + Send + Sync>);

impl SyncerError {
   pub fn new<E: std::error::Error + Send + Sync + 'static>(e: E) -> Self {
      SyncerError(Box::new(e))
   }
}

impl Shield {
   pub fn hash(&self) -> UtxoLeafHash {
      if let Some(hash) = self.hash {
         return hash;
      }

      poseidon_hash(&[self.npk, self.token.hash(), U256::from(self.value)])
         .unwrap()
         .into()
   }
}
