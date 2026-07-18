pub mod indexed_account;
pub mod syncer;
pub mod txid_indexer;
pub mod utxo_indexer;

use crate::abi::{legacy::RailgunLegacy, railgun::RailgunSmartWallet};
use crate::indexer::syncer::{
   SyncEvent, SyncerError, normalize_tree_position::normalize_tree_position, types::*,
};
use alloy_primitives::U256;

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

fn parse_legacy_commitment_batch(
   event: &RailgunLegacy::CommitmentBatch,
   block_number: u64,
) -> Result<Vec<SyncEvent>, SyncerError> {
   let tree_number = event.treeNumber.saturating_to();
   let start_position = event.startPosition.saturating_to::<u32>();

   let mut events = Vec::new();
   for (i, &hash_alloy) in event.hash.iter().enumerate() {
      let hash: U256 = U256::from_be_bytes::<32>(hash_alloy.to_be_bytes::<32>());

      let (tree_number, leaf_index) =
         normalize_tree_position(tree_number, start_position + i as u32);

      // Attach legacy ciphertext so accounts can attempt decryption
      let ct = if !event.ciphertext.is_empty() && i < event.ciphertext.len() {
         let c = &event.ciphertext[i];
         Some(LegacyCiphertext {
            ciphertext: [
               U256::from_be_bytes::<32>(c.ciphertext[0].to_be_bytes::<32>()),
               U256::from_be_bytes::<32>(c.ciphertext[1].to_be_bytes::<32>()),
               U256::from_be_bytes::<32>(c.ciphertext[2].to_be_bytes::<32>()),
               U256::from_be_bytes::<32>(c.ciphertext[3].to_be_bytes::<32>()),
            ],
            ephemeral_keys: [
               U256::from_be_bytes::<32>(c.ephemeralKeys[0].to_be_bytes::<32>()),
               U256::from_be_bytes::<32>(c.ephemeralKeys[1].to_be_bytes::<32>()),
            ],
            memo: c
               .memo
               .iter()
               .map(|m| U256::from_be_bytes::<32>(m.to_be_bytes::<32>()))
               .collect(),
         })
      } else {
         None
      };

      events.push(SyncEvent::Legacy(
         LegacyCommitment {
            hash,
            tree_number,
            leaf_index,
            ciphertext: ct,
         },
         block_number,
      ));
   }
   Ok(events)
}

fn parse_legacy_generated_commitment_batch(
   event: &RailgunLegacy::GeneratedCommitmentBatch,
   block_number: u64,
) -> Result<Vec<SyncEvent>, SyncerError> {
   use crate::crypto::poseidon_hash;

   let tree_number = event.treeNumber.saturating_to();
   let start_position = event.startPosition.saturating_to::<u32>();

   let mut events = Vec::new();

   for (i, commitment) in event.commitments.iter().enumerate() {
      // Convert legacy token to AssetId style for hashing
      // For now map simply (legacy only had ERC20 mostly in very early days)
      let token_addr = commitment.token.tokenAddress;
      let asset = crate::caip::AssetId::Erc20(token_addr); // extend for ERC721/1155 if needed

      let npk = commitment.npk;
      let value = U256::from(commitment.value);

      let computed_hash =
         poseidon_hash(&[npk, asset.hash(), value]).map_err(|e| SyncerError::new(e))?;

      let (tree_number, leaf_index) =
         normalize_tree_position(tree_number, start_position + i as u32);

      events.push(SyncEvent::Legacy(
         LegacyCommitment {
            hash: computed_hash,
            tree_number,
            leaf_index,
            ciphertext: None,
         },
         block_number,
      ));
   }

   Ok(events)
}

fn parse_legacy_nullifiers(
   event: &RailgunLegacy::Nullifiers,
   block_timestamp: u64,
) -> Result<Vec<SyncEvent>, SyncerError> {
   let tree_number = event.treeNumber.saturating_to::<u32>();

   let mut events = Vec::new();
   for nullifier in &event.nullifier {
      // Legacy nullifier is uint256, modern is bytes32. Take low 256 bits as-is.
      let n: alloy_primitives::FixedBytes<32> =
         alloy_primitives::FixedBytes::from_slice(&nullifier.to_be_bytes::<32>());
      events.push(SyncEvent::Nullified(
         Nullified {
            tree_number,
            nullifier: n,
         },
         block_timestamp,
      ));
   }
   Ok(events)
}

fn parse_legacy_transact(
   event: &RailgunLegacy::Transact,
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

fn parse_legacy_shield(
   event: &RailgunLegacy::Shield,
   block_number: u64,
) -> Result<Vec<SyncEvent>, SyncerError> {
   let tree_number = event.treeNumber.saturating_to();
   let start_position = event.startPosition.saturating_to::<u32>();

   let mut events = Vec::new();

   for (i, commitment) in event.commitments.iter().enumerate() {
      let shield_ciphertext = &event.shieldCiphertext[i];

      let (tree_number, leaf_index) =
         normalize_tree_position(tree_number, start_position + i as u32);

      let token: crate::caip::AssetId = commitment.token.clone().into();

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

fn parse_legacy_unshield(
   _event: &RailgunLegacy::Unshield,
   _block_number: u64,
) -> Result<Vec<SyncEvent>, SyncerError> {
   // Unshields do not add leaves to the UTXO tree.
   // We parse them to avoid "Unknown Log" and for future use (e.g. full history).
   // If we ever need to track outgoing value per account, we can extend here.
   Ok(vec![])
}
