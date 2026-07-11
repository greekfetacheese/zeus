pub mod indexed_account;
pub mod syncer;
pub mod txid_indexer;
pub mod utxo_indexer;

use crate::abi::railgun::RailgunSmartWallet;
use crate::indexer::syncer::{SyncEvent, SyncerError, types::*, normalize_tree_position::normalize_tree_position};

fn parse_shield(
   event: &RailgunSmartWallet::Shield,
   block_number: u64,
) -> Result<Vec<SyncEvent>, SyncerError> {
   let tree_number = event.treeNumber.to::<u32>();
   let start_position = event.startPosition.to::<u32>();

   let mut events = Vec::new();

   for (i, commitment) in event.commitments.iter().enumerate() {
      let shield_ciphertext = &event.shieldCiphertext[i];

      let (tree_number, leaf_index) =
         normalize_tree_position(tree_number, start_position + i as u32);
      let token = commitment.token.clone().into();

      let shield = Shield {
         tree_number,
         leaf_index,
         npk: commitment.npk.into(),
         token,
         value: commitment.value.saturating_to(),
         ciphertext: shield_ciphertext.clone().into(),
         shield_key: shield_ciphertext.shieldKey.into(),
         hash: None,
      };

      events.push(SyncEvent::Shield(shield, block_number));
   }

   Ok(events)
}

fn parse_transact(
   event: &RailgunSmartWallet::Transact,
   block_timestamp: u64,
) -> Result<Vec<SyncEvent>, SyncerError> {
   let tree_number = event.treeNumber.saturating_to();
   let start_position = event.startPosition.saturating_to::<u32>();

   let mut events = Vec::new();
   for (i, ciphertext) in event.ciphertext.clone().into_iter().enumerate() {
      let hash = event.hash[i].clone();
      let (tree_number, leaf_index) =
         normalize_tree_position(tree_number, start_position + i as u32);

      events.push(SyncEvent::Transact(
         Transact {
            tree_number,
            leaf_index,
            hash: hash.into(),
            ciphertext: ciphertext.clone().into(),
            blinded_receiver_viewing_key: ciphertext.blindedReceiverViewingKey.into(),
            blinded_sender_viewing_key: ciphertext.blindedSenderViewingKey.into(),
            annotation_data: ciphertext.annotationData.into(),
         },
         block_timestamp,
      ));
   }
   Ok(events)
}

fn parse_nullified(
   event: &RailgunSmartWallet::Nullified,
   block_timestamp: u64,
) -> Result<Vec<SyncEvent>, SyncerError> {
   let tree_number = event.treeNumber as u32;

   let mut events = Vec::new();
   for nullifier in event.nullifier.clone().into_iter() {
      events.push(SyncEvent::Nullified(
         Nullified {
            tree_number: tree_number,
            nullifier: nullifier,
         },
         block_timestamp,
      ));
   }
   Ok(events)
}
