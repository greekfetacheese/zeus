use alloy_primitives::{B256, U256};
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::{
   account::{address::RailgunAddress, signer::RailgunSigner},
   caip::AssetId,
   indexer::syncer,
   note::utxo::{NoteError, UtxoNote},
   poi::types::BlindedCommitmentType,
};

/// IndexerAccount represents a Railgun account being tracked by the indexer.
///
/// The indexer will use the contained signer to decrypt notes and track the
/// account's balance and UTXOs.
pub struct IndexedAccount {
   signer: RailgunSigner,
   inner: IndexedAccountState,
   /// Set when notes or synced_block change and cleared after a successful DB save.
   dirty: bool,
}

/// An owned note plus when/where it was created.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct NoteRecord {
   pub note: UtxoNote,
   pub created_block: u64,
   pub created_timestamp: u64,
   pub created_tx_hash: B256,
}

/// A note this account previously owned that has been nullified on-chain.
///
/// `spent_block` is the chain block of the Nullified event.
/// `spent_timestamp` is unix seconds when the RPC log provided it (0 if unknown).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SpentNote {
   pub note: UtxoNote,
   pub spent_block: u64,
   pub spent_timestamp: u64,
   pub spent_tx_hash: B256,
   pub created_block: u64,
   pub created_timestamp: u64,
   pub created_tx_hash: B256,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct IndexedAccountState {
   pub notes: Vec<NoteRecord>,
   pub synced_block: u64,
   /// Nullified notes, newest last.
   #[serde(default)]
   pub spent_notes: Vec<SpentNote>,
}

/// How a grouped private spend should be shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivateHistoryKind {
   /// Net value left this 0zk (transfer, unshield, or paymaster fee).
   Send,
   /// Inputs were consolidated back to this 0zk (net ≈ 0).
   Merge,
}

/// One user-facing private spend: input UTXOs minus change that came back.
#[derive(Debug, Clone)]
pub struct PrivateHistoryEntry {
   pub asset: AssetId,
   pub amount: u128,
   pub change_amount: u128,
   pub input_count: usize,
   pub spent_block: u64,
   pub spent_timestamp: u64,
   pub tx_hash: B256,
   pub memo: String,
   pub kind: PrivateHistoryKind,
}

impl IndexedAccountState {
   /// Decode account state written by this crate.
   ///
   /// Current blobs are `{ notes: NoteRecord[], synced_block, spent_notes }`.
   /// Older blobs use bare `UtxoNote`s (created/spend tx meta = 0).
   pub fn from_bincode(bytes: &[u8]) -> Result<Self, String> {
      let cfg = bincode_next::config::standard();
      if let Ok((state, _)) = bincode_next::serde::decode_from_slice::<Self, _>(bytes, cfg) {
         return Ok(state);
      }

      #[derive(Deserialize)]
      struct SpentNoteV1 {
         note: UtxoNote,
         spent_block: u64,
         spent_timestamp: u64,
      }
      #[derive(Deserialize)]
      struct Mid {
         notes: Vec<UtxoNote>,
         synced_block: u64,
         spent_notes: Vec<SpentNoteV1>,
      }
      if let Ok((mid, _)) = bincode_next::serde::decode_from_slice::<Mid, _>(bytes, cfg) {
         return Ok(Self {
            notes: mid.notes.into_iter().map(NoteRecord::from_note).collect(),
            synced_block: mid.synced_block,
            spent_notes: mid
               .spent_notes
               .into_iter()
               .map(|s| SpentNote::from_parts(s.note, s.spent_block, s.spent_timestamp))
               .collect(),
         });
      }

      #[derive(Deserialize)]
      struct Legacy {
         notes: Vec<UtxoNote>,
         synced_block: u64,
      }
      let (legacy, _) = bincode_next::serde::decode_from_slice::<Legacy, _>(bytes, cfg)
         .map_err(|e| e.to_string())?;
      Ok(Self {
         notes: legacy.notes.into_iter().map(NoteRecord::from_note).collect(),
         synced_block: legacy.synced_block,
         spent_notes: Vec::new(),
      })
   }
}

impl NoteRecord {
   fn from_note(note: UtxoNote) -> Self {
      Self {
         note,
         created_block: 0,
         created_timestamp: 0,
         created_tx_hash: B256::ZERO,
      }
   }
}

impl SpentNote {
   fn from_parts(note: UtxoNote, spent_block: u64, spent_timestamp: u64) -> Self {
      Self {
         note,
         spent_block,
         spent_timestamp,
         spent_tx_hash: B256::ZERO,
         created_block: 0,
         created_timestamp: 0,
         created_tx_hash: B256::ZERO,
      }
   }
}

impl IndexedAccount {
   pub fn from_state(signer: RailgunSigner, state: IndexedAccountState) -> Self {
      IndexedAccount {
         signer,
         inner: state,
         dirty: false,
      }
   }

   pub fn state(&self) -> IndexedAccountState {
      self.inner.clone()
   }

   pub fn address(&self) -> &RailgunAddress {
      self.signer.address()
   }

   /// Returns all unspent notes for this account.
   pub fn unspent(&self) -> Vec<UtxoNote> {
      self.inner.notes.iter().map(|n| n.note.clone()).collect()
   }

   /// Returns notes this account has spent (nullified), in spend order.
   pub fn spent(&self) -> Vec<SpentNote> {
      self.inner.spent_notes.clone()
   }

   /// Grouped private spends: input UTXOs minus change that returned to this 0zk.
   pub fn private_history(&self) -> Vec<PrivateHistoryEntry> {
      reconstruct_private_history(&self.inner.notes, &self.inner.spent_notes)
   }

   fn knows_note(&self, tree_number: u32, leaf_index: u32) -> bool {
      self
         .inner
         .notes
         .iter()
         .any(|n| n.note.tree_number == tree_number && n.note.leaf_index == leaf_index)
         || self
            .inner
            .spent_notes
            .iter()
            .any(|s| s.note.tree_number == tree_number && s.note.leaf_index == leaf_index)
   }

   /// Returns the latest synced block for this account.
   pub fn synced_block(&self) -> u64 {
      self.inner.synced_block
   }

   pub fn set_synced_block(&mut self, block: u64) {
      if self.inner.synced_block != block {
         self.inner.synced_block = block;
         self.dirty = true;
      }
   }

   pub fn is_dirty(&self) -> bool {
      self.dirty
   }

   pub fn clear_dirty(&mut self) {
      self.dirty = false;
   }

   pub fn handle_shield_event(
      &mut self,
      event: &syncer::Shield,
      created_block: u64,
   ) -> Result<(), NoteError> {
      let note = UtxoNote::decrypt_shield(&self.signer, event);
      let note = match note {
         Err(NoteError::Aes(_)) => {
            return Ok(());
         }
         Err(e) => {
            debug!(
               "Failed to decrypt Shield note at tree {}, leaf {}: {}",
               event.tree_number, event.leaf_index, e
            );
            return Ok(());
         }
         Ok(n) => n,
      };

      if self.knows_note(note.tree_number, note.leaf_index) {
         return Ok(());
      }
      self.inner.notes.push(NoteRecord {
         note,
         created_block,
         created_timestamp: event.timestamp,
         created_tx_hash: event.tx_hash,
      });
      self.dirty = true;

      Ok(())
   }

   pub fn handle_transact_event(
      &mut self,
      event: &syncer::Transact,
      created_block: u64,
   ) -> Result<(), NoteError> {
      let note = UtxoNote::decrypt_transact(&self.signer, &event);

      let note = match note {
         Err(NoteError::Aes(_)) => {
            return Ok(());
         }
         Err(e) => {
            debug!(
               "Failed to decrypt Transact note at tree {}, leaf {}: {}",
               event.tree_number, event.leaf_index, e
            );
            return Ok(());
         }
         Ok(n) => n,
      };

      if self.knows_note(note.tree_number, note.leaf_index) {
         return Ok(());
      }
      self.inner.notes.push(NoteRecord {
         note,
         created_block,
         created_timestamp: event.timestamp,
         created_tx_hash: event.tx_hash,
      });
      self.dirty = true;

      Ok(())
   }

   pub fn handle_nullified_event(&mut self, event: &syncer::Nullified, spent_block: u64) {
      let nullifier: U256 = event.nullifier.into();
      let mut spent = Vec::new();
      self.inner.notes.retain(|record| {
         if record.note.tree_number == event.tree_number && record.note.nullifier == nullifier {
            spent.push(record.clone());
            return false;
         }
         true
      });
      if spent.is_empty() {
         return;
      }

      for record in spent {
         let already = self.inner.spent_notes.iter().any(|s| {
            s.note.tree_number == record.note.tree_number
               && s.note.leaf_index == record.note.leaf_index
         });
         if !already {
            self.inner.spent_notes.push(SpentNote {
               note: record.note,
               spent_block,
               spent_timestamp: event.timestamp,
               spent_tx_hash: event.tx_hash,
               created_block: record.created_block,
               created_timestamp: record.created_timestamp,
               created_tx_hash: record.created_tx_hash,
            });
         }
      }
      self.dirty = true;
   }

   /// Attempt to decrypt a legacy encrypted commitment.
   /// Currently a no-op stub — full implementation requires legacy key
   /// derivation (ephemeralKeys + different AES usage).
   pub fn handle_legacy_event(
      &mut self,
      _event: &syncer::LegacyCommitment,
   ) -> Result<(), NoteError> {
      // TODO: Implement proper legacy decryption using LegacyCiphertext
      // if let Some(ct) = &_event.ciphertext {
      //     let note = UtxoNote::decrypt_legacy(self.signer.clone(), _event, ct)?;
      //     self.inner.notes.push(note);
      // }
      Ok(())
   }
}

fn reconstruct_private_history(
   unspent: &[NoteRecord],
   spent: &[SpentNote],
) -> Vec<PrivateHistoryEntry> {
   use std::collections::BTreeMap;

   #[derive(Clone, Ord, PartialOrd, Eq, PartialEq)]
   enum GroupKey {
      Tx(B256, AssetId),
      Block(u64, u64, AssetId),
   }

   let mut groups: BTreeMap<GroupKey, Vec<&SpentNote>> = BTreeMap::new();
   for note in spent {
      let asset = note.note.asset;
      let key = if note.spent_tx_hash != B256::ZERO {
         GroupKey::Tx(note.spent_tx_hash, asset)
      } else {
         GroupKey::Block(note.spent_block, note.spent_timestamp, asset)
      };
      groups.entry(key).or_default().push(note);
   }

   let mut out = Vec::with_capacity(groups.len());
   for (key, inputs) in groups {
      let asset = inputs[0].note.asset;
      let spent_sum: u128 = inputs.iter().map(|n| n.note.value).sum();
      let spent_block = inputs.iter().map(|n| n.spent_block).max().unwrap_or(0);
      let spent_timestamp = inputs.iter().map(|n| n.spent_timestamp).max().unwrap_or(0);
      let tx_hash = match key {
         GroupKey::Tx(hash, _) => hash,
         GroupKey::Block(_, _, _) => inputs
            .iter()
            .map(|n| n.spent_tx_hash)
            .find(|h| *h != B256::ZERO)
            .unwrap_or(B256::ZERO),
      };

      let input_keys: Vec<(u32, u32)> =
         inputs.iter().map(|n| (n.note.tree_number, n.note.leaf_index)).collect();

      let is_change =
         |created_tx: B256, created_block: u64, created_ts: u64, typ, tree, leaf, note_asset| {
            if note_asset != asset {
               return false;
            }
            if typ != BlindedCommitmentType::Transact {
               return false;
            }
            if input_keys.contains(&(tree, leaf)) {
               return false;
            }
            if tx_hash != B256::ZERO {
               return created_tx == tx_hash;
            }
            created_block == spent_block
               && (spent_timestamp == 0 || created_ts == 0 || created_ts == spent_timestamp)
         };

      let mut change_amount = 0u128;
      let mut memo = String::new();

      for record in unspent {
         if is_change(
            record.created_tx_hash,
            record.created_block,
            record.created_timestamp,
            record.note.commitment_type,
            record.note.tree_number,
            record.note.leaf_index,
            record.note.asset,
         ) {
            change_amount = change_amount.saturating_add(record.note.value);
            if memo.is_empty() && !record.note.memo.is_empty() {
               memo = record.note.memo.clone();
            }
         }
      }

      for record in spent {
         if is_change(
            record.created_tx_hash,
            record.created_block,
            record.created_timestamp,
            record.note.commitment_type,
            record.note.tree_number,
            record.note.leaf_index,
            record.note.asset,
         ) {
            change_amount = change_amount.saturating_add(record.note.value);
            if memo.is_empty() && !record.note.memo.is_empty() {
               memo = record.note.memo.clone();
            }
         }
      }

      let amount = spent_sum.saturating_sub(change_amount);
      let kind = if amount == 0 && change_amount > 0 {
         PrivateHistoryKind::Merge
      } else {
         PrivateHistoryKind::Send
      };

      out.push(PrivateHistoryEntry {
         asset,
         amount,
         change_amount,
         input_count: inputs.len(),
         spent_block,
         spent_timestamp,
         tx_hash,
         memo,
         kind,
      });
   }

   out.sort_by(|a, b| {
      b.spent_timestamp
         .cmp(&a.spent_timestamp)
         .then_with(|| b.spent_block.cmp(&a.spent_block))
   });
   out
}

#[cfg(test)]
mod tests {
   use alloy_primitives::{B256, address};
   use rand::random;
   use secure_types::SecureArray;

   use super::*;
   use crate::{
      account::signer::RailgunSigner,
      caip::AssetId,
      note::{EncryptableNote, Note, encrypt::encrypt_shield, transfer::TransferNote},
   };

   #[test]
   fn test_event_handling() {
      let seed: [u8; 64] = random();
      let sec_array = SecureArray::from_slice(&seed).unwrap();
      let sender = RailgunSigner::from_seed(&sec_array, 0, 1).unwrap();

      let seed: [u8; 64] = random();
      let sec_array = SecureArray::from_slice(&seed).unwrap();
      let recipient = RailgunSigner::from_seed(&sec_array, 0, 1).unwrap();

      let seed: [u8; 64] = random();
      let sec_array = SecureArray::from_slice(&seed).unwrap();
      let other_recipient = RailgunSigner::from_seed(&sec_array, 0, 1).unwrap();

      let asset = AssetId::erc20(address!(
         "0xDEADDEADDEADDEADDEADDEADDEADDEADDEADDEAD"
      ));
      let value = 100;
      let rng = &mut rand::rng();
      let mut account = IndexedAccount {
         signer: recipient.clone(),
         inner: Default::default(),
         dirty: false,
      };

      // Ingest a shield note
      let shield = encrypt_shield(recipient.address().clone(), asset, value, rng).unwrap();
      let event = syncer::Shield {
         tree_number: 1,
         leaf_index: 0,
         npk: shield.preimage.npk.into(),
         token: shield.preimage.token.try_into().unwrap(),
         value: U256::from(shield.preimage.value),
         ciphertext: shield.ciphertext.clone().into(),
         shield_key: *shield.ciphertext.shieldKey,
         hash: None,
         timestamp: 0,
         tx_hash: B256::ZERO,
      };

      account.handle_shield_event(&event, 10).unwrap();
      let notes = account.unspent();
      assert_eq!(notes.len(), 1);

      let note = &notes[0];
      assert_eq!(note.tree_number, 1);
      assert_eq!(note.leaf_index, 0);
      assert_eq!(note.asset, asset);
      assert_eq!(note.value, value);

      // Ingest a shield note for a different recipient
      let other_shield = encrypt_shield(
         other_recipient.address().clone(),
         asset,
         value,
         rng,
      )
      .unwrap();
      let other_event = syncer::Shield {
         tree_number: 1,
         leaf_index: 1,
         npk: other_shield.preimage.npk.into(),
         token: other_shield.preimage.token.try_into().unwrap(),
         value: U256::from(other_shield.preimage.value),
         ciphertext: other_shield.ciphertext.clone().into(),
         shield_key: *other_shield.ciphertext.shieldKey,
         hash: None,
         timestamp: 0,
         tx_hash: B256::ZERO,
      };

      account.handle_shield_event(&other_event, 10).unwrap();
      let notes = account.unspent();
      assert_eq!(notes.len(), 1); // Should still only have the first note

      // Ingest a transact note
      let memo = "Test transfer";
      let transact = TransferNote::new(
         sender.keys().viewing_private_key.clone(),
         recipient.address().clone(),
         asset,
         value,
         random(),
         memo,
      );

      let ciphertext = transact.encrypt(rng).unwrap();
      let event = syncer::Transact {
         tree_number: 1,
         leaf_index: 2,
         hash: transact.hash().into(),
         ciphertext: ciphertext.clone().into(),
         blinded_sender_viewing_key: *ciphertext.blindedSenderViewingKey,
         blinded_receiver_viewing_key: *ciphertext.blindedReceiverViewingKey,
         annotation_data: ciphertext.annotationData.to_vec(),
         timestamp: 1_700_000_000,
         tx_hash: B256::from([7u8; 32]),
      };

      account.handle_transact_event(&event, 20).unwrap();
      let notes = account.unspent();
      assert_eq!(notes.len(), 2);

      let note = notes.iter().find(|n| n.leaf_index == 2).unwrap();
      assert_eq!(note.tree_number, 1);
      assert_eq!(note.leaf_index, 2);
      assert_eq!(note.asset, asset);
      assert_eq!(note.value, value);
      assert_eq!(note.memo, memo.to_string());

      // Ingest a nullifier for the transact
      let nullified_event = syncer::Nullified {
         tree_number: 1,
         nullifier: note.nullifier.into(),
         timestamp: 1_700_000_000,
         tx_hash: B256::from([7u8; 32]),
      };

      account.handle_nullified_event(&nullified_event, 1_234_567);
      let notes = account.unspent();
      assert_eq!(notes.len(), 1);

      let remaining_note = &notes[0];
      assert_eq!(remaining_note.tree_number, 1);
      assert_eq!(remaining_note.leaf_index, 0);

      let spent = account.spent();
      assert_eq!(spent.len(), 1);
      assert_eq!(spent[0].spent_block, 1_234_567);
      assert_eq!(spent[0].spent_timestamp, 1_700_000_000);
      assert_eq!(spent[0].note.leaf_index, 2);
      assert_eq!(spent[0].note.value, value);
      assert_eq!(spent[0].note.memo, memo.to_string());

      // Replaying the same nullifier must not duplicate spent notes
      account.handle_nullified_event(&nullified_event, 1_234_567);
      assert_eq!(account.spent().len(), 1);

      // Ingest a nullifier for an unrelated note
      let unrelated_nullified_event = syncer::Nullified {
         tree_number: 1,
         nullifier: U256::from(1234567890).into(),
         timestamp: 0,
         tx_hash: B256::ZERO,
      };

      account.handle_nullified_event(&unrelated_nullified_event, 0);
      let notes = account.unspent();
      assert_eq!(notes.len(), 1); // Should still have the original note
      assert_eq!(account.spent().len(), 1);

      // Replaying the spent transact must not resurrect it as unspent
      account.handle_transact_event(&event, 20).unwrap();
      assert_eq!(account.unspent().len(), 1);
      assert_eq!(account.spent().len(), 1);
   }

   #[test]
   fn test_account_state_bincode_legacy_and_current() {
      let current = IndexedAccountState {
         notes: vec![],
         synced_block: 99,
         spent_notes: vec![],
      };
      let bytes =
         bincode_next::serde::encode_to_vec(&current, bincode_next::config::standard()).unwrap();
      let loaded = IndexedAccountState::from_bincode(&bytes).unwrap();
      assert_eq!(loaded.synced_block, 99);
      assert!(loaded.spent_notes.is_empty());

      #[derive(Serialize)]
      struct Legacy {
         notes: Vec<UtxoNote>,
         synced_block: u64,
      }
      let legacy = Legacy {
         notes: vec![],
         synced_block: 42,
      };
      let legacy_bytes =
         bincode_next::serde::encode_to_vec(&legacy, bincode_next::config::standard()).unwrap();
      let migrated = IndexedAccountState::from_bincode(&legacy_bytes).unwrap();
      assert_eq!(migrated.synced_block, 42);
      assert!(migrated.notes.is_empty());
      assert!(migrated.spent_notes.is_empty());
   }

   #[test]
   fn test_private_history_nets_change() {
      let seed: [u8; 64] = random();
      let sec_array = SecureArray::from_slice(&seed).unwrap();
      let sender = RailgunSigner::from_seed(&sec_array, 0, 1).unwrap();

      let asset = AssetId::erc20(address!(
         "0xDEADDEADDEADDEADDEADDEADDEADDEADDEADDEAD"
      ));
      let rng = &mut rand::rng();
      let mut account = IndexedAccount {
         signer: sender.clone(),
         inner: Default::default(),
         dirty: false,
      };

      let shield = encrypt_shield(sender.address().clone(), asset, 233, rng).unwrap();
      let shield_event = syncer::Shield {
         tree_number: 1,
         leaf_index: 0,
         npk: shield.preimage.npk.into(),
         token: shield.preimage.token.try_into().unwrap(),
         value: U256::from(shield.preimage.value),
         ciphertext: shield.ciphertext.clone().into(),
         shield_key: *shield.ciphertext.shieldKey,
         hash: None,
         timestamp: 1,
         tx_hash: B256::from([1u8; 32]),
      };
      account.handle_shield_event(&shield_event, 10).unwrap();

      let spent_note = account.unspent()[0].clone();
      let spend_tx = B256::from([9u8; 32]);
      account.handle_nullified_event(
         &syncer::Nullified {
            tree_number: 1,
            nullifier: spent_note.nullifier.into(),
            timestamp: 100,
            tx_hash: spend_tx,
         },
         20,
      );

      let change = TransferNote::new(
         sender.keys().viewing_private_key.clone(),
         sender.address().clone(),
         asset,
         133,
         random(),
         "",
      );
      let ciphertext = change.encrypt(rng).unwrap();
      account
         .handle_transact_event(
            &syncer::Transact {
               tree_number: 1,
               leaf_index: 5,
               hash: change.hash().into(),
               ciphertext: ciphertext.clone().into(),
               blinded_sender_viewing_key: *ciphertext.blindedSenderViewingKey,
               blinded_receiver_viewing_key: *ciphertext.blindedReceiverViewingKey,
               annotation_data: ciphertext.annotationData.to_vec(),
               timestamp: 100,
               tx_hash: spend_tx,
            },
            20,
         )
         .unwrap();

      let history = account.private_history();
      assert_eq!(history.len(), 1);
      assert_eq!(history[0].kind, PrivateHistoryKind::Send);
      assert_eq!(history[0].amount, 100);
      assert_eq!(history[0].change_amount, 133);
      assert_eq!(history[0].input_count, 1);
   }
}
