use alloy_primitives::B256;

use crate::abi::railgun::RailgunSmartWallet;
use crate::indexer::syncer::{
   SyncEvent, SyncerError, normalize_tree_position::normalize_tree_position,
   types::{Nullified, Shield, Transact},
};

pub(crate) fn parse_shield(
   event: &RailgunSmartWallet::Shield,
   block_number: u64,
   timestamp: u64,
   tx_hash: B256,
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
         timestamp,
         tx_hash,
      };

      events.push(SyncEvent::Shield(shield, block_number));
   }

   Ok(events)
}

pub(crate) fn parse_transact(
   event: &RailgunSmartWallet::Transact,
   block_number: u64,
   timestamp: u64,
   tx_hash: B256,
) -> Result<Vec<SyncEvent>, SyncerError> {
   let tree_number = event.treeNumber.saturating_to();
   let start_position = event.startPosition.saturating_to::<u32>();

   let mut events = Vec::new();
   for (i, ciphertext) in event.ciphertext.clone().into_iter().enumerate() {
      let hash = event.hash[i];
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
            timestamp,
            tx_hash,
         },
         // Always the real chain block. Snapshot range filters and apply_tree
         // watermarks use SyncEvent::block_number() — a unix timestamp here drops
         // every Transact on historical snapshot replay (timestamp >> tip block).
         block_number,
      ));
   }
   Ok(events)
}

pub(crate) fn parse_nullified(
   event: &RailgunSmartWallet::Nullified,
   block_number: u64,
   timestamp: u64,
   tx_hash: B256,
) -> Result<Vec<SyncEvent>, SyncerError> {
   let tree_number = event.treeNumber as u32;

   let mut events = Vec::new();
   for nullifier in event.nullifier.clone().into_iter() {
      events.push(SyncEvent::Nullified(
         Nullified {
            tree_number,
            nullifier,
            timestamp,
            tx_hash,
         },
         block_number,
      ));
   }
   Ok(events)
}
