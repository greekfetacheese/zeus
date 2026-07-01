use alloy_primitives::U256;
use secure_types::SecureArray;

use crate::address::{
   compute_spending_key, compute_viewing_key, derive_spending_private_key,
   derive_viewing_private_key,
};
use crate::crypto::poseidon_hash;

/// Full set of Railgun keys derived from a seed + index.
/// This is what the engine will use for note decryption, nullifier creation, etc.
#[derive(Clone)]
pub struct RailgunKeys {
   /// Raw spending private key (before final BabyJub clamping for scalar).
   pub spending_private: SecureArray<u8, 32>,
   /// Spending public key point (x, y) on BabyJubJub.
   pub spending_public: (U256, U256),
   /// Raw viewing private key.
   pub viewing_private: SecureArray<u8, 32>,
   /// Viewing public key (compressed Ed25519 point, 32 bytes) - used in 0zk address.
   pub viewing_public: [u8; 32],
   /// Nullifying key = Poseidon(viewing private).
   pub nullifying_key: U256,
   /// Master public key = Poseidon(spend_x, spend_y, nullifying_key).
   pub master_public_key: U256,
}

impl RailgunKeys {
   /// Generate the complete set of Railgun keys (private + public) for a given seed and index.
   /// This is the recommended entry point for the engine.
   pub fn new(seed: SecureArray<u8, 64>, index: u32) -> Result<RailgunKeys, anyhow::Error> {
      let spending_priv = derive_spending_private_key(&seed, index)?;
      let viewing_priv = derive_viewing_private_key(&seed, index)?;

      let (spend_x, spend_y) = compute_spending_key(&seed, index)?;
      let (viewing_public, nullifying_key) = compute_viewing_key(&seed, index)?;

      let master_public_key = poseidon_hash(vec![spend_x, spend_y, nullifying_key])?;

      Ok(RailgunKeys {
         spending_private: spending_priv,
         spending_public: (spend_x, spend_y),
         viewing_private: viewing_priv,
         viewing_public,
         nullifying_key,
         master_public_key,
      })
   }
}
