use rand::RngCore;
use ruint::aliases::U256;

use crate::{
   abi::railgun::CommitmentCiphertext,
   account::address::RailgunAddress,
   caip::AssetId,
   crypto::keys::ViewingKey,
   crypto::{poseidon2, poseidon3},
   merkle_tree::UtxoLeafHash,
   note::encrypt::{EncryptError, EncryptNoteParams, encrypt_note},
};

/// Transfer notes represent value being sent from one Railgun account to another.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct TransferNote {
   pub from_key: ViewingKey,
   pub to: RailgunAddress,
   pub asset: AssetId,
   pub value: u128,
   pub random: [u8; 16],
   pub memo: String,
}

impl TransferNote {
   pub fn new(
      from_key: ViewingKey,
      to: RailgunAddress,
      asset: AssetId,
      value: u128,
      random: [u8; 16],
      memo: &str,
   ) -> Self {
      TransferNote {
         from_key,
         to,
         asset,
         value,
         random,
         memo: memo.to_string(),
      }
   }

   pub fn encrypt(&self, rng: &mut dyn RngCore) -> Result<CommitmentCiphertext, EncryptError> {
      encrypt_note(
         EncryptNoteParams {
            receiver: &self.to,
            shared_random: &self.random,
            value: self.value,
            asset: &self.asset,
            memo: &self.memo,
            viewing_key: self.from_key.clone(),
            blind: false,
         },
         rng,
      )
   }

   pub fn asset(&self) -> AssetId {
      self.asset
   }

   pub fn value(&self) -> u128 {
      self.value
   }

   pub fn memo(&self) -> String {
      self.memo.clone()
   }

   pub fn random(&self) -> [u8; 16] {
      self.random
   }

   pub fn hash(&self) -> UtxoLeafHash {
      poseidon3(
         self.note_public_key(),
         self.asset.hash(),
         U256::from(self.value),
      )
      .into()
   }

   pub fn note_public_key(&self) -> U256 {
      poseidon2(
         self.to.master_pubkey().to_u256(),
         U256::from_be_slice(&self.random),
      )
   }
}

#[cfg(test)]
mod tests {
   use alloy_primitives::address;
   use ruint::uint;

   use crate::{
      account::address::RailgunAddress,
      caip::AssetId,
      crypto::keys::{SpendingKey, ViewingKey},
      merkle_tree::UtxoLeafHash,
      note::transfer::TransferNote,
      types::Chain,
   };

   #[test]
   fn test_transfer_note_hash() {
      let note = TransferNote::new(
         ViewingKey::from_bytes([3u8; 32]),
         RailgunAddress::from_private_keys(
            SpendingKey::from_bytes([1u8; 32]),
            ViewingKey::from_bytes([2u8; 32]),
            Some(Chain::ETHEREUM_MAINNET),
         ),
         AssetId::Erc20(address!(
            "0x1234567890123456789012345678901234567890"
         )),
         90,
         [2u8; 16],
         "memo",
      );
      let hash: UtxoLeafHash = note.hash();

      let expected: UtxoLeafHash =
         uint!(1005027091991696937637380235791481806966626119421670561695028901610612069057_U256)
            .into();
      assert_eq!(hash, expected);
   }
}
