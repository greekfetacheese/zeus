use alloy_primitives::U256;
use secure_types::SecureArray;

use super::address::{compute_public_spending_key, compute_public_viewing_key};
use crate::crypto::{
   keys::{MasterPublicKey, NullifyingKey, SpendingKey, ViewingKey, ViewingPublicKey},
   poseidon_hash,
};

/// BIP-32 spending path
pub fn spending_key_path(index: u32) -> String {
   format!("m/44'/1984'/0'/0'/{}'", index)
}

/// BIP-32 viewing path
pub fn viewing_key_path(index: u32) -> String {
   format!("m/420'/1984'/0'/0'/{}'", index)
}

/// Full set of Railgun keys derived from a seed + index.
#[derive(Clone)]
pub struct RailgunKeys {
   /// Spending Private key for signing transactions (BabyJubJub curve).
   pub spending_private_key: SpendingKey,
   /// Spending public key point (x, y) on BabyJubJub.
   pub spending_public: (U256, U256),
   /// Private key for viewing transactions and ECDH.
   pub viewing_private: ViewingKey,
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
   pub fn new(seed: &SecureArray<u8, 64>, index: u32) -> Result<RailgunKeys, anyhow::Error> {
      let spending_path = spending_key_path(index);
      let viewing_path = viewing_key_path(index);

      let spending_priv =
         seed.unlock(|seed_bytes| SpendingKey::derive(seed_bytes, &spending_path))?;
      let viewing_priv = seed.unlock(|seed_bytes| ViewingKey::derive(seed_bytes, &viewing_path))?;

      let (spend_x, spend_y) = compute_public_spending_key(&seed, index)?;
      let (viewing_public, nullifying_key) = compute_public_viewing_key(&seed, index)?;

      let master_public_key = poseidon_hash(&vec![spend_x, spend_y, nullifying_key])?;

      Ok(RailgunKeys {
         spending_private_key: spending_priv,
         spending_public: (spend_x, spend_y),
         viewing_private: viewing_priv,
         viewing_public,
         nullifying_key,
         master_public_key,
      })
   }
}
