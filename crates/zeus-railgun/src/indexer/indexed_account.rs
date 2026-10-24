use alloy_primitives::{B256, U256};
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use crate::{
   account::{address::RailgunAddress, signer::RailgunSigner},
   caip::{AssetId, TokenRegistry},
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
#[non_exhaustive]
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
#[non_exhaustive]
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
#[non_exhaustive]
pub struct IndexedAccountState {
   pub notes: Vec<NoteRecord>,
   pub synced_block: u64,
   /// Nullified notes, newest last.
   #[serde(default)]
   pub spent_notes: Vec<SpentNote>,
}

/// How a grouped private history row should be shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PrivateHistoryKind {
   /// zk → zk transfer labeled via the sender's change-note memo.
   Send,
   /// Net value exited Railgun to a public address.
   Unshield,
   /// Inputs were consolidated back to this 0zk (net ≈ 0).
   Merge,
   /// Spend we cannot classify (legacy notes, paymaster fee, exact-amount with no change).
   Spend,
   /// A note this account received from another 0zk, derived from the decrypted note — never
   /// authored into a memo by this wallet.
   Receive,
}

const HISTORY_KIND_TRANSFER: &str = "Private Transfer";
const HISTORY_KIND_UNSHIELD: &str = "Unshield";
const HISTORY_KIND_SEP: &str = " · ";

/// Memo written on the sender's change note so private history can label the spend.
///
/// Unshield notes have no ciphertext, and outgoing transfer notes are encrypted to
/// the recipient — only change (self) notes are decryptable by the sender.
pub fn encode_history_memo(kind: PrivateHistoryKind, user_memo: &str) -> String {
   let kind_s = match kind {
      PrivateHistoryKind::Unshield => HISTORY_KIND_UNSHIELD,
      PrivateHistoryKind::Merge => "Merged notes",
      PrivateHistoryKind::Receive => {
         debug_assert!(false, "a Receive has no authored memo");
         HISTORY_KIND_TRANSFER
      }
      PrivateHistoryKind::Send | PrivateHistoryKind::Spend => HISTORY_KIND_TRANSFER,
   };
   let user = user_memo.trim();
   if user.is_empty() {
      kind_s.to_string()
   } else {
      format!("{kind_s}{HISTORY_KIND_SEP}{user}")
   }
}

/// Split a change-note memo into `(kind hint, user-facing memo)`.
///
/// `"change"` and empty strings are treated as unlabeled.
pub fn decode_history_memo(raw: &str) -> (Option<PrivateHistoryKind>, String) {
   let raw = raw.trim();
   if raw.is_empty() || raw == "change" {
      return (None, String::new());
   }
   if raw == "merge notes" || raw == "Merged notes" {
      return (Some(PrivateHistoryKind::Merge), String::new());
   }
   if raw == HISTORY_KIND_UNSHIELD {
      return (Some(PrivateHistoryKind::Unshield), String::new());
   }
   if let Some(rest) = raw.strip_prefix(HISTORY_KIND_UNSHIELD)
      && let Some(user) = rest.strip_prefix(HISTORY_KIND_SEP)
   {
      return (
         Some(PrivateHistoryKind::Unshield),
         user.to_string(),
      );
   }
   if raw == HISTORY_KIND_TRANSFER {
      return (Some(PrivateHistoryKind::Send), String::new());
   }
   if let Some(rest) = raw.strip_prefix(HISTORY_KIND_TRANSFER)
      && let Some(user) = rest.strip_prefix(HISTORY_KIND_SEP)
   {
      return (Some(PrivateHistoryKind::Send), user.to_string());
   }
   (None, raw.to_string())
}

/// One user-facing private history row: a spend's inputs minus change, or a received note.
///
/// The field names are shared by both kinds: `amount` is the net value for a spend and the full
/// note value(s) for a Receive; `input_count` is the inputs spent or the notes received;
/// `spent_block` / `spent_timestamp` are the spend's block and time, or for a Receive the block and
/// unix time of the creating tx; `tx_hash` is the spending tx, or for a Receive the creating tx.
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

   /// Replace decrypted note state from DB (e.g. after a failed sync window).
   ///
   /// Clears the dirty flag so a later save does not rewrite the discarded window.
   pub fn restore_state(&mut self, state: IndexedAccountState) {
      self.inner = state;
      self.dirty = false;
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

      // The event carries the leaf the chain actually inserted, and reading the ciphertext only proves *we*
      // can read it: the note is a reconstruction from the event's preimage — `poseidon(npk, asset, value)` —
      // so the two disagree wherever that reconstruction narrows or re-encodes, `value` being narrowed to
      // u128 and the asset rebuilt from the event's `TokenData`. Same stakes as the transact path: a note the
      // tree cannot witness is shown as a balance, never provable, never spendable, and never revisited,
      // because `knows_note` short-circuits every later pass. So it is dropped rather than stored.
      //
      // Shields parsed from an RPC log carry no commitment at all (`parse_shield`: the log event holds the
      // preimage, not the leaf), and those have nothing to be checked against.
      if let Some(expected) = event.hash {
         let commitment: U256 = note.hash().into();
         let expected: U256 = expected.into();
         if commitment != expected {
            warn!(
               "Dropped Shield note at tree {}, leaf {}: commitment does not match the event's",
               note.tree_number, note.leaf_index
            );
            return Ok(());
         }
      }

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
      known_tokens: &TokenRegistry,
   ) -> Result<(), NoteError> {
      let note = UtxoNote::decrypt_transact(&self.signer, event, known_tokens);

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

      // Reading the ciphertext only proves *we* can read it; the commitment proves the note is the one that
      // leaf holds. The two disagree when the asset could not be resolved and a 32-byte hash was read as an
      // ERC-20 instead (`resolve_transact_asset`): the guess is part of `note_hash`, so storing it stores a
      // note the tree will never witness again — shown as a balance, never provable, never spendable, and
      // never reprocessed, because `knows_note` short-circuits every later pass.
      //
      // So the note is dropped, and dropping it is final: the sync advances this account's watermark past
      // the event (`UtxoIndexer::commit_window_progress`) and only ever offers a block *after* the
      // watermark, so a later sync does not revisit it even once the registry knows the asset. Only a
      // resync — which drops the indexer state and replays from the chain's first block — recovers the
      // note. Skipping stays the lesser cost because the alternative stores a guess that corrupts it for
      // good.
      let commitment: U256 = note.hash().into();
      if commitment != event.hash {
         warn!(
            "Dropped Transact note at tree {}, leaf {}: commitment does not match the event's — unrecognized asset, only a resync retries it",
            note.tree_number, note.leaf_index
         );
         return Ok(());
      }

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

fn private_spends(unspent: &[NoteRecord], spent: &[SpentNote]) -> Vec<PrivateHistoryEntry> {
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
      let (memo_kind, display_memo) = decode_history_memo(&memo);
      let kind = if amount == 0 && change_amount > 0 {
         PrivateHistoryKind::Merge
      } else if memo_kind == Some(PrivateHistoryKind::Unshield) {
         PrivateHistoryKind::Unshield
      } else if memo_kind == Some(PrivateHistoryKind::Send) {
         PrivateHistoryKind::Send
      } else {
         PrivateHistoryKind::Spend
      };

      out.push(PrivateHistoryEntry {
         asset,
         amount,
         change_amount,
         input_count: inputs.len(),
         spent_block,
         spent_timestamp,
         tx_hash,
         memo: display_memo,
         kind,
      });
   }

   out
}

fn reconstruct_private_history(
   unspent: &[NoteRecord],
   spent: &[SpentNote],
) -> Vec<PrivateHistoryEntry> {
   let mut out = private_spends(unspent, spent);
   out.extend(received_notes(unspent, spent));
   out.sort_by(|a, b| {
      b.spent_timestamp
         .cmp(&a.spent_timestamp)
         .then_with(|| b.spent_block.cmp(&a.spent_block))
   });
   out
}

/// Notes this account **received** from another 0zk.
///
/// A note we can decrypt, of `commitment_type == Transact`, whose creating tx is not one of our own
/// spends: change and self-transfer outputs are created by a tx we spent in, so they are excluded, and
/// a `Shield` commitment is a public deposit, not a transfer. `spent_notes` is scanned as well as
/// `notes` — a received note that was later spent moves there and must still produce its receive row.
/// Notes from before `created_tx_hash` was recorded carry `B256::ZERO` and cannot be classified.
fn received_notes(unspent: &[NoteRecord], spent: &[SpentNote]) -> Vec<PrivateHistoryEntry> {
   use std::collections::{BTreeMap, HashSet};

   let spent_txs: HashSet<B256> =
      spent.iter().map(|s| s.spent_tx_hash).filter(|h| *h != B256::ZERO).collect();

   struct Received {
      amount: u128,
      notes: usize,
      memo: String,
      block: u64,
      timestamp: u64,
   }

   let mut groups: BTreeMap<(B256, AssetId), Received> = BTreeMap::new();

   let records = unspent
      .iter()
      .map(|r| {
         (
            &r.note,
            r.created_block,
            r.created_timestamp,
            r.created_tx_hash,
         )
      })
      .chain(spent.iter().map(|s| {
         (
            &s.note,
            s.created_block,
            s.created_timestamp,
            s.created_tx_hash,
         )
      }));

   for (note, created_block, created_ts, created_tx) in records {
      if note.commitment_type != BlindedCommitmentType::Transact {
         continue;
      }
      if created_tx == B256::ZERO || spent_txs.contains(&created_tx) {
         continue;
      }

      let group = groups.entry((created_tx, note.asset)).or_insert_with(|| Received {
         amount: 0,
         notes: 0,
         memo: String::new(),
         block: created_block,
         timestamp: created_ts,
      });
      group.amount = group.amount.saturating_add(note.value);
      group.notes += 1;
      group.block = group.block.max(created_block);
      group.timestamp = group.timestamp.max(created_ts);
      if group.memo.is_empty() && !note.memo.is_empty() {
         group.memo = note.memo.clone();
      }
   }

   groups
      .into_iter()
      .map(|((tx_hash, asset), group)| PrivateHistoryEntry {
         asset,
         amount: group.amount,
         change_amount: 0,
         input_count: group.notes,
         spent_block: group.block,
         spent_timestamp: group.timestamp,
         tx_hash,
         memo: group.memo,
         kind: PrivateHistoryKind::Receive,
      })
      .collect()
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
      merkle_tree::UtxoLeafHash,
      note::{encrypt::encrypt_shield, transfer::TransferNote},
   };

   /// A note whose asset the wallet cannot recognize is not stored at all.
   ///
   /// A transact note's ciphertext kept only `asset.hash()`, and an unrecognized hash is read as an ERC-20
   /// at the hash's low 20 bytes — a *guess*, and the guess is part of the note's commitment. Stored, it is
   /// a note the tree can never witness: shown as a balance, never provable, never spendable, and never
   /// reprocessed because `knows_note` short-circuits every later pass. The commitment check turns that
   /// permanent corruption into a skip that a later pass — one with the asset in the registry — undoes.
   #[test]
   fn a_transact_note_whose_asset_is_unrecognized_is_skipped_not_stored() {
      let seed: [u8; 64] = random();
      let sec_array = SecureArray::from_slice(&seed).unwrap();
      let sender = RailgunSigner::from_seed(&sec_array, 0, 1).unwrap();

      let seed: [u8; 64] = random();
      let sec_array = SecureArray::from_slice(&seed).unwrap();
      let recipient = RailgunSigner::from_seed(&sec_array, 0, 1).unwrap();

      // An NFT: the unrecognized reading is then provably not the asset the sender used.
      let asset = AssetId::Erc721(
         address!("0xaf5aa7b670ef209e23d3f7b39a8f42f84bd002ac"),
         U256::from(1071),
      );

      let rng = &mut rand::rng();
      let transfer = TransferNote::new(
         sender.keys().viewing_private_key.clone(),
         recipient.address().clone(),
         asset,
         1u128,
         random(),
         "",
      );
      let ciphertext = transfer.encrypt(rng).unwrap();
      let event = syncer::Transact {
         tree_number: 1,
         leaf_index: 0,
         hash: transfer.hash().into(),
         ciphertext: ciphertext.clone().into(),
         blinded_sender_viewing_key: *ciphertext.blindedSenderViewingKey,
         blinded_receiver_viewing_key: *ciphertext.blindedReceiverViewingKey,
         annotation_data: ciphertext.annotationData.to_vec(),
         timestamp: 0,
         tx_hash: B256::ZERO,
      };

      let mut account = IndexedAccount {
         signer: recipient.clone(),
         inner: Default::default(),
         dirty: false,
      };

      // Nothing has been seen shielded, so the asset cannot be recognized: skip rather than store a guess.
      account.handle_transact_event(&event, 10, &TokenRegistry::new()).unwrap();
      assert!(
         account.unspent().is_empty(),
         "a note whose commitment does not match its event must not be stored"
      );

      // With the asset known — what replaying the shields gives a wallet — the same event stores the note
      // it really is.
      let mut known = TokenRegistry::new();
      known.insert(asset.hash(), asset);
      account.handle_transact_event(&event, 10, &known).unwrap();

      let notes = account.unspent();
      assert_eq!(notes.len(), 1);
      assert_eq!(notes[0].asset, asset);

      let stored: U256 = notes[0].hash().into();
      assert_eq!(stored, event.hash);
   }

   /// A shield whose note does not reproduce its event's commitment is skipped, not stored.
   ///
   /// The note is a reconstruction from the event's preimage, `poseidon(npk, asset, value)`, so this is the
   /// transact path's check applied where the source hands the commitment over — the subsquid shield
   /// commitment carries it (`hash: Some(value.hash)`), a shield parsed from an RPC log cannot.
   #[test]
   fn a_shield_note_that_does_not_reproduce_its_commitment_is_not_stored() {
      let seed: [u8; 64] = random();
      let sec_array = SecureArray::from_slice(&seed).unwrap();
      let recipient = RailgunSigner::from_seed(&sec_array, 0, 1).unwrap();

      let asset = AssetId::erc20(address!(
         "0xDEADDEADDEADDEADDEADDEADDEADDEADDEADDEAD"
      ));
      let value: u128 = 100;
      let rng = &mut rand::rng();
      let shield = encrypt_shield(recipient.address().clone(), asset, value, rng).unwrap();

      let mut event = syncer::Shield {
         tree_number: 1,
         leaf_index: 0,
         npk: shield.preimage.npk.into(),
         token: shield.preimage.token.into(),
         value: U256::from(shield.preimage.value),
         ciphertext: shield.ciphertext.clone().into(),
         shield_key: *shield.ciphertext.shieldKey,
         hash: None,
         timestamp: 0,
         tx_hash: B256::ZERO,
      };

      let account = || IndexedAccount {
         signer: recipient.clone(),
         inner: Default::default(),
         dirty: false,
      };

      // What the leaf really holds: the commitment a stored note hashes to.
      let mut reference = account();
      reference.handle_shield_event(&event, 10).unwrap();
      let expected = reference.unspent()[0].hash();

      // An event the note does not agree with is skipped rather than stored as a note no tree can witness.
      event.hash = Some(UtxoLeafHash::from(U256::from(0xdead_beefu64)));
      let mut account = account();
      account.handle_shield_event(&event, 10).unwrap();
      assert!(
         account.unspent().is_empty(),
         "a note whose commitment does not match its event must not be stored"
      );

      // And the commitment it does reproduce is stored.
      event.hash = Some(expected);
      account.handle_shield_event(&event, 10).unwrap();
      let notes = account.unspent();
      assert_eq!(notes.len(), 1);
      assert_eq!(notes[0].hash(), expected);
   }

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
         token: shield.preimage.token.into(),
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
         token: other_shield.preimage.token.into(),
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

      account.handle_transact_event(&event, 20, &TokenRegistry::default()).unwrap();
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
      account.handle_transact_event(&event, 20, &TokenRegistry::default()).unwrap();
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
         token: shield.preimage.token.into(),
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
            &TokenRegistry::default(),
         )
         .unwrap();

      let history = account.private_history();
      assert_eq!(history.len(), 1);
      assert_eq!(history[0].kind, PrivateHistoryKind::Spend);
      assert_eq!(history[0].amount, 100);
      assert_eq!(history[0].change_amount, 133);
      assert_eq!(history[0].input_count, 1);
      assert!(history.iter().all(|e| e.kind != PrivateHistoryKind::Receive));
   }

   fn test_signer() -> RailgunSigner {
      let seed: [u8; 64] = random();
      let sec_array = SecureArray::from_slice(&seed).unwrap();
      RailgunSigner::from_seed(&sec_array, 0, 1).unwrap()
   }

   fn test_asset() -> AssetId {
      AssetId::erc20(address!(
         "0xDEADDEADDEADDEADDEADDEADDEADDEADDEADDEAD"
      ))
   }

   fn account_for(signer: &RailgunSigner) -> IndexedAccount {
      IndexedAccount {
         signer: signer.clone(),
         inner: Default::default(),
         dirty: false,
      }
   }

   fn transact_event(
      sender: &RailgunSigner,
      recipient: &RailgunSigner,
      asset: AssetId,
      value: u128,
      memo: &str,
      leaf_index: u32,
      timestamp: u64,
      tx_hash: B256,
   ) -> syncer::Transact {
      let rng = &mut rand::rng();
      let transfer = TransferNote::new(
         sender.keys().viewing_private_key.clone(),
         recipient.address().clone(),
         asset,
         value,
         random(),
         memo,
      );
      let ciphertext = transfer.encrypt(rng).unwrap();
      syncer::Transact {
         tree_number: 1,
         leaf_index,
         hash: transfer.hash().into(),
         ciphertext: ciphertext.clone().into(),
         blinded_sender_viewing_key: *ciphertext.blindedSenderViewingKey,
         blinded_receiver_viewing_key: *ciphertext.blindedReceiverViewingKey,
         annotation_data: ciphertext.annotationData.to_vec(),
         timestamp,
         tx_hash,
      }
   }

   fn shield_event(
      recipient: &RailgunSigner,
      asset: AssetId,
      value: u128,
      leaf_index: u32,
      timestamp: u64,
      tx_hash: B256,
   ) -> syncer::Shield {
      let rng = &mut rand::rng();
      let shield = encrypt_shield(recipient.address().clone(), asset, value, rng).unwrap();
      syncer::Shield {
         tree_number: 1,
         leaf_index,
         npk: shield.preimage.npk.into(),
         token: shield.preimage.token.into(),
         value: U256::from(shield.preimage.value),
         ciphertext: shield.ciphertext.clone().into(),
         shield_key: *shield.ciphertext.shieldKey,
         hash: None,
         timestamp,
         tx_hash,
      }
   }

   #[test]
   fn received_note_from_a_foreign_transact_is_one_row() {
      let sender = test_signer();
      let recipient = test_signer();
      let asset = test_asset();
      let tx_hash = B256::from([7u8; 32]);
      let mut account = account_for(&recipient);

      let event = transact_event(
         &sender,
         &recipient,
         asset,
         100,
         "lunch",
         0,
         1_700_000_000,
         tx_hash,
      );
      account.handle_transact_event(&event, 20, &TokenRegistry::default()).unwrap();

      let history = account.private_history();
      assert_eq!(history.len(), 1);
      let entry = &history[0];
      assert_eq!(entry.kind, PrivateHistoryKind::Receive);
      assert_eq!(entry.asset, asset);
      assert_eq!(entry.amount, 100);
      assert_eq!(entry.change_amount, 0);
      assert_eq!(entry.input_count, 1);
      assert_eq!(entry.spent_block, 20);
      assert_eq!(entry.spent_timestamp, 1_700_000_000);
      assert_eq!(entry.tx_hash, tx_hash);
      assert_eq!(entry.memo, "lunch");
   }

   #[test]
   fn a_shield_note_is_not_received() {
      let recipient = test_signer();
      let asset = test_asset();
      let mut account = account_for(&recipient);

      let event = shield_event(
         &recipient,
         asset,
         100,
         0,
         1_700_000_000,
         B256::from([1u8; 32]),
      );
      account.handle_shield_event(&event, 20).unwrap();

      assert_eq!(account.unspent().len(), 1);
      assert!(
         account.private_history().is_empty(),
         "a public deposit is not a private transfer"
      );
   }

   #[test]
   fn a_change_note_is_not_received() {
      let sender = test_signer();
      let asset = test_asset();
      let spend_tx = B256::from([9u8; 32]);
      let mut account = account_for(&sender);

      let shield = shield_event(&sender, asset, 233, 0, 1, B256::from([1u8; 32]));
      account.handle_shield_event(&shield, 10).unwrap();
      let spent_note = account.unspent()[0].clone();
      account.handle_nullified_event(
         &syncer::Nullified {
            tree_number: 1,
            nullifier: spent_note.nullifier.into(),
            timestamp: 100,
            tx_hash: spend_tx,
         },
         20,
      );

      let change = transact_event(
         &sender,
         &sender,
         asset,
         133,
         &encode_history_memo(PrivateHistoryKind::Send, ""),
         5,
         100,
         spend_tx,
      );
      account.handle_transact_event(&change, 20, &TokenRegistry::default()).unwrap();

      let history = account.private_history();
      assert_eq!(history.len(), 1);
      assert_eq!(history[0].kind, PrivateHistoryKind::Send);
      assert_eq!(history[0].amount, 100);
      assert_eq!(history[0].change_amount, 133);
      assert!(history.iter().all(|e| e.kind != PrivateHistoryKind::Receive));
   }

   #[test]
   fn a_received_note_spent_later_shows_both_rows() {
      let sender = test_signer();
      let recipient = test_signer();
      let asset = test_asset();
      let receive_tx = B256::from([7u8; 32]);
      let spend_tx = B256::from([8u8; 32]);
      let mut account = account_for(&recipient);

      let event = transact_event(
         &sender, &recipient, asset, 100, "lunch", 0, 1_000, receive_tx,
      );
      account.handle_transact_event(&event, 20, &TokenRegistry::default()).unwrap();

      let note = account.unspent()[0].clone();
      account.handle_nullified_event(
         &syncer::Nullified {
            tree_number: 1,
            nullifier: note.nullifier.into(),
            timestamp: 2_000,
            tx_hash: spend_tx,
         },
         30,
      );

      let history = account.private_history();
      assert_eq!(history.len(), 2);
      let receive = history
         .iter()
         .find(|e| e.kind == PrivateHistoryKind::Receive)
         .expect("the received note still owes its Receive row after being spent");
      assert_eq!(receive.tx_hash, receive_tx);
      assert_eq!(receive.amount, 100);
      let spend = history
         .iter()
         .find(|e| e.kind == PrivateHistoryKind::Spend)
         .expect("the nullification is a spend row");
      assert_eq!(spend.tx_hash, spend_tx);
   }

   #[test]
   fn two_notes_received_in_one_tx_are_one_row() {
      let sender = test_signer();
      let recipient = test_signer();
      let asset = test_asset();
      let tx_hash = B256::from([7u8; 32]);
      let mut account = account_for(&recipient);

      for (leaf_index, value) in [(0u32, 10u128), (1, 20)] {
         let event = transact_event(
            &sender, &recipient, asset, value, "", leaf_index, 500, tx_hash,
         );
         account.handle_transact_event(&event, 20, &TokenRegistry::default()).unwrap();
      }

      let history = account.private_history();
      assert_eq!(history.len(), 1);
      assert_eq!(history[0].kind, PrivateHistoryKind::Receive);
      assert_eq!(history[0].asset, asset);
      assert_eq!(history[0].amount, 30);
      assert_eq!(history[0].input_count, 2);
      assert_eq!(history[0].tx_hash, tx_hash);
   }

   #[test]
   fn a_note_without_a_creating_tx_is_not_received() {
      let signer = test_signer();
      let asset = test_asset();
      let mut account = account_for(&signer);

      let note = UtxoNote::new(
         &signer,
         crate::note::utxo::UtxoNoteParams {
            tree_number: 1,
            leaf_index: 0,
            asset,
            value: 100,
            random: random(),
            memo: "lunch".to_string(),
            commitment_type: BlindedCommitmentType::Transact,
         },
      );
      account.inner.notes.push(NoteRecord {
         note,
         created_block: 20,
         created_timestamp: 1_700_000_000,
         created_tx_hash: B256::ZERO,
      });

      let history = account.private_history();
      assert!(
         history.iter().all(|e| e.kind != PrivateHistoryKind::Receive),
         "a note from before created_tx_hash was recorded cannot be classified"
      );
      assert!(history.is_empty());
   }

   #[test]
   fn history_memo_roundtrip() {
      assert_eq!(
         encode_history_memo(PrivateHistoryKind::Unshield, ""),
         "Unshield"
      );
      assert_eq!(
         encode_history_memo(PrivateHistoryKind::Unshield, " lunch "),
         "Unshield · lunch"
      );
      assert_eq!(
         encode_history_memo(PrivateHistoryKind::Send, "rent"),
         "Private Transfer · rent"
      );
      assert_eq!(
         decode_history_memo("Unshield · lunch"),
         (
            Some(PrivateHistoryKind::Unshield),
            "lunch".to_string()
         )
      );
      assert_eq!(
         decode_history_memo("Private Transfer"),
         (Some(PrivateHistoryKind::Send), String::new())
      );
      assert_eq!(
         decode_history_memo("change"),
         (None, String::new())
      );
      assert_eq!(
         decode_history_memo("fee"),
         (None, "fee".to_string())
      );
   }
}
