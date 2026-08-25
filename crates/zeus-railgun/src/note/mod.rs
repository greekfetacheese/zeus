pub mod encrypt;
pub mod operation;
pub mod transfer;
pub mod unshield;
pub mod utxo;

use ruint::aliases::U256;

use crate::{
   caip::AssetId, merkle_tree::UtxoLeafHash, note::transfer::TransferNote,
   note::unshield::UnshieldNote,
};

/// Mixed circuit-output note: a private transfer or an unshield.
#[derive(Debug, Clone)]
pub enum OutputNote {
   Transfer(TransferNote),
   Unshield(UnshieldNote),
}

impl OutputNote {
   pub fn asset(&self) -> AssetId {
      match self {
         OutputNote::Transfer(n) => n.asset(),
         OutputNote::Unshield(n) => n.asset(),
      }
   }

   pub fn value(&self) -> u128 {
      match self {
         OutputNote::Transfer(n) => n.value(),
         OutputNote::Unshield(n) => n.value(),
      }
   }

   pub fn memo(&self) -> String {
      match self {
         OutputNote::Transfer(n) => n.memo(),
         OutputNote::Unshield(n) => n.memo(),
      }
   }

   pub fn random(&self) -> [u8; 16] {
      match self {
         OutputNote::Transfer(n) => n.random(),
         OutputNote::Unshield(n) => n.random(),
      }
   }

   pub fn hash(&self) -> UtxoLeafHash {
      match self {
         OutputNote::Transfer(n) => n.hash(),
         OutputNote::Unshield(n) => n.hash(),
      }
   }

   pub fn note_public_key(&self) -> U256 {
      match self {
         OutputNote::Transfer(n) => n.note_public_key(),
         OutputNote::Unshield(n) => n.note_public_key(),
      }
   }
}
