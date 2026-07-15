use secure_types::SecureArray;

use crate::{
   account::compute_public_spending_key,
   crypto::keys::{
      MasterPublicKey, NullifyingKey, SpendingKey, SpendingPublicKey, ViewingKey, ViewingPublicKey,
   },
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
   pub spending_public_key: SpendingPublicKey,

   /// Private key for viewing transactions and ECDH.
   pub viewing_private_key: ViewingKey,

   /// Viewing public key (compressed Ed25519 point, 32 bytes) - used in 0zk address.
   pub viewing_public_key: ViewingPublicKey,

   /// Nullifying key = Poseidon(viewing private).
   pub nullifying_key: NullifyingKey,

   /// Master public key = Poseidon(spend_x, spend_y, nullifying_key).
   pub master_public_key: MasterPublicKey,
}

impl RailgunKeys {
   /// Generate the complete set of Railgun keys (private + public) for a given seed and index.
   pub fn new(seed: &SecureArray<u8, 64>, index: u32) -> Result<RailgunKeys, anyhow::Error> {
      let spending_path = spending_key_path(index);
      let viewing_path = viewing_key_path(index);

      let spending_priv =
         seed.unlock(|seed_bytes| SpendingKey::derive(seed_bytes, &spending_path))?;
      let viewing_priv = seed.unlock(|seed_bytes| ViewingKey::derive(seed_bytes, &viewing_path))?;

      let (spend_x, spend_y) = compute_public_spending_key(&seed, &spending_path)?;
      let viewing_public_key = viewing_priv.public_key();
      let nullifying_key = viewing_priv.nullifying_key();

      let x = spend_x.to_be_bytes();
      let y = spend_y.to_be_bytes();
      let spending_public_key = SpendingPublicKey::new(x, y);

      let master_public_key = MasterPublicKey::new(spending_public_key, nullifying_key);

      Ok(RailgunKeys {
         spending_private_key: spending_priv,
         spending_public_key,
         viewing_private_key: viewing_priv,
         viewing_public_key: viewing_public_key,
         nullifying_key,
         master_public_key,
      })
   }
}
