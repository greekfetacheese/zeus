use std::{
   collections::{BTreeMap, BTreeSet, HashMap},
   u64,
};

use alloy_primitives::{Log, U256};
use alloy_rpc_types::BlockId;
use alloy_sol_types::SolEvent;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::debug;

use crate::{
   abi::{legacy::RailgunLegacy, railgun::RailgunSmartWallet},
   account::{address::RailgunAddress, signer::RailgunSigner},
   database::{
      DatabaseError, RailgunDbKey, RedbDatabase, WriteBatch, WriteDurability,
      railgun_db::{
         account_key, all_chunk_indices, dirty_chunks_for_range, push_utxo_tree_save, put_account,
         put_utxo_indexer, put_utxo_note_proof,
      },
   },
   indexer::{
      indexed_account::{IndexedAccount, PrivateHistoryEntry, SpentNote},
      syncer::{self, RpcSyncer, SubsquidSyncer, SyncEvent, SyncerError},
   },
   merkle_tree::{
      MerkleRoot, MerkleTreeError, RailgunMerkleProof, RootVerifier, UtxoLeafHash, UtxoMerkleTree,
   },
   note::utxo::{NoteError, UtxoNote},
};

/// Cached leaf count + root for a UTXO tree (resident or unloaded).
#[derive(Clone, Copy, Debug)]
pub struct UtxoTreeSummary {
   pub leaf_count: usize,
   pub root: MerkleRoot,
}

/// Utxo indexer that maintains the set of UTXO merkle trees and tracks accounts
/// and account notes / balances.
pub struct UtxoIndexer {
   synced_block: u64,
   /// Fully rebuilt trees currently in RAM. Sealed trees with no unspent notes are
   /// unloaded; `known_trees` is the durable catalog.
   pub utxo_trees: BTreeMap<u32, UtxoMerkleTree>,
   /// All tree numbers that exist on disk / have been observed, including unloaded ones.
   known_trees: BTreeSet<u32>,
   /// Leaf count + root for known trees (filled from disk meta and live trees).
   tree_summaries: BTreeMap<u32, UtxoTreeSummary>,
   /// Inclusion proofs for unspent notes on sealed trees that are not in RAM.
   frozen_proofs: HashMap<(u32, u32), RailgunMerkleProof>,
   accounts: Vec<IndexedAccount>,

   /// Tree number → dirty leaf-chunk indices since last successful save.
   dirty_chunks: HashMap<u32, BTreeSet<u32>>,
   /// Trees loaded from legacy monolithic blobs, next save migrates fully to chunks.
   legacy_trees: BTreeSet<u32>,

   db: RedbDatabase,
   /// AEAD key for sealing account note state in the DB.
   db_key: RailgunDbKey,
   pub rpc_syncer: RpcSyncer,
   subsquid_syncer: Option<SubsquidSyncer>,
   pub utxo_verifier: RootVerifier,
}

#[derive(Serialize, Deserialize, Default)]
pub struct UtxoIndexerState {
   pub synced_block: u64,
   pub trees: Vec<u32>,
}

#[derive(Debug, Error)]
pub enum UtxoIndexerError {
   #[error("Syncer error: {0}")]
   SyncerError(#[from] SyncerError),
   #[error("Verification error: {0}")]
   VerificationError(#[source] Box<dyn std::error::Error + Send + Sync + 'static>),
   #[error("Note error: {0}")]
   NoteError(#[from] NoteError),
   #[error("Database error: {0}")]
   DatabaseError(#[from] DatabaseError),
   #[error("Timed out waiting for commitments")]
   Timeout,
   #[error("Invalid root for tree {0} root {1}")]
   InvalidRoot(u32, U256),
   #[error("UTXO tree {0} is required but is not in memory and not on disk")]
   MissingTree(u32),
   #[error("Merkle tree error: {0}")]
   MerkleTree(#[from] MerkleTreeError),
   #[error("Frozen merkle proof for tree {0} leaf {1} is invalid")]
   InvalidFrozenProof(u32, u32),
}

impl UtxoIndexer {
   pub async fn new(
      db: RedbDatabase,
      rpc_syncer: RpcSyncer,
      subsquid_syncer: Option<SubsquidSyncer>,
      utxo_verifier: RootVerifier,
   ) -> Result<Self, UtxoIndexerError> {
      let db_key = db.crypto_key().clone();
      let state = db.get_utxo_indexer().await?;

      let known_trees: BTreeSet<u32> = state.trees.iter().copied().collect();
      let mut tree_summaries = BTreeMap::new();
      let mut utxo_trees = BTreeMap::new();
      let mut legacy_trees = BTreeSet::new();

      for number in state.trees.iter().copied() {
         if let Some(meta) = db.get_utxo_tree_meta(number).await? {
            tree_summaries.insert(
               number,
               UtxoTreeSummary {
                  leaf_count: meta.leaf_count as usize,
                  root: MerkleRoot::new(meta.root),
               },
            );
         }
      }

      // Only rebuild the open (highest) tree. Sealed trees stay on disk until a
      // registered account has unspent notes there (see compact_utxo_trees).
      if let Some(open) = known_trees.iter().next_back().copied() {
         if let Some((leaves, from_legacy)) = db.get_utxo_tree_leaves(open).await? {
            if from_legacy {
               legacy_trees.insert(open);
            }
            let tree = UtxoMerkleTree::from_leaves(open, leaves);
            tree_summaries.insert(
               open,
               UtxoTreeSummary {
                  leaf_count: tree.leaves_len(),
                  root: tree.root(),
               },
            );
            utxo_trees.insert(open, tree);
         }
      }

      debug!(
         "Loaded UTXO indexer state: synced_block={}, known_trees={:?}, resident={:?}, legacy_migrate={}",
         state.synced_block,
         known_trees,
         utxo_trees.keys().cloned().collect::<Vec<_>>(),
         legacy_trees.len()
      );
      Ok(UtxoIndexer {
         synced_block: state.synced_block,
         utxo_trees,
         known_trees,
         tree_summaries,
         frozen_proofs: HashMap::new(),
         accounts: vec![],
         dirty_chunks: HashMap::new(),
         legacy_trees,
         db,
         db_key,
         rpc_syncer,
         subsquid_syncer,
         utxo_verifier,
      })
   }

   /// Overall resume watermark: min(global tree progress, registered accounts).
   ///
   /// Used for reporting / "are we fully caught up". Tree mutation must NOT use this
   /// alone — a newly registered account at block 0 would otherwise force a full
   /// historical re-insert into already-loaded merkle trees and corrupt the root.
   pub fn min_account_synced_block(&self) -> u64 {
      let mut min_synced = self.synced_block;
      for account in self.accounts.iter() {
         min_synced = min_synced.min(account.synced_block());
      }
      min_synced
   }

   pub fn accounts_count(&self) -> usize {
      self.accounts.len()
   }

   /// Returns the synced block for the given account
   pub fn account_synced_block(&self, address: &RailgunAddress) -> Option<u64> {
      self.accounts.iter().find(|a| a.address() == address).map(|a| a.synced_block())
   }

   /// Global UTXO tree progress only (ignores per-account catch-up).
   pub fn global_synced_block(&self) -> u64 {
      self.synced_block
   }

   /// Registers a signer with the indexer. The indexer will track UTXOs for the associated
   /// address.
   ///
   /// The account state (including any previously decrypted notes and its synced_block)
   /// is loaded from the database. We immediately persist the (possibly loaded) account
   /// state so that progress for this account survives process restarts.
   ///
   /// Idempotent: registering an address that is already loaded is a no-op.
   pub async fn register(&mut self, signer: RailgunSigner) -> Result<(), UtxoIndexerError> {
      let addr = signer.address().clone();
      if self.accounts.iter().any(|a| a.address().address == addr.address) {
         return Ok(());
      }

      let state = self.db.get_account(&addr).await?;
      let account = IndexedAccount::from_state(signer, state);

      // Persist right away. This records the account's notes + its per-account synced_block on disk.
      self.db.set_account(&addr, &account.state()).await?;
      self.accounts.push(account);
      self.ensure_trees_for_unspent().await?;
      self.compact_utxo_trees().await?;
      Ok(())
   }

   /// Lists all registered accounts
   pub fn registered(&self) -> Vec<RailgunAddress> {
      self.accounts.iter().map(|a| a.address().clone()).collect()
   }

   /// Keep only accounts whose addresses are in `keep`.
   ///
   /// - Drops matching entries from the in-memory registered set
   /// - Deletes orphan account blobs from the DB (including ones never loaded this session)
   ///
   /// Returns how many DB account keys were removed.
   pub async fn retain_accounts(
      &mut self,
      keep: &[RailgunAddress],
   ) -> Result<usize, UtxoIndexerError> {
      let keep_keys: std::collections::HashSet<Vec<u8>> = keep.iter().map(account_key).collect();

      // Drop from memory first so we never re-save an orphan after this.
      self.accounts.retain(|a| keep_keys.contains(&account_key(a.address())));

      let stored = self.db.list_account_keys().await?;
      let mut batch = WriteBatch::new();
      let mut removed = 0usize;
      for key in stored {
         if !keep_keys.contains(&key) {
            batch.delete(key);
            removed += 1;
         }
      }

      if removed > 0 {
         self.db.apply_batch(batch, WriteDurability::Immediate).await?;
      }

      self.compact_utxo_trees().await?;
      Ok(removed)
   }

   /// Lists all unspent notes for a given address. Returns an empty list if the address is not
   /// registered.
   pub fn unspent(&self, address: RailgunAddress) -> Vec<UtxoNote> {
      for account in self.accounts.iter() {
         if account.address().address == address.address {
            return account.unspent();
         }
      }

      vec![]
   }

   /// Lists spent (nullified) notes for a given address. Empty if the address is not registered.
   pub fn spent(&self, address: RailgunAddress) -> Vec<SpentNote> {
      for account in self.accounts.iter() {
         if account.address().address == address.address {
            return account.spent();
         }
      }

      vec![]
   }

   /// Grouped private spends for a registered address.
   pub fn private_history(&self, address: RailgunAddress) -> Vec<PrivateHistoryEntry> {
      for account in self.accounts.iter() {
         if account.address().address == address.address {
            return account.private_history();
         }
      }

      vec![]
   }

   fn mark_tree_range_dirty(&mut self, tree_number: u32, start: usize, end: usize) {
      let chunks = dirty_chunks_for_range(start, end);
      if chunks.is_empty() {
         return;
      }
      self.dirty_chunks.entry(tree_number).or_default().extend(chunks);
   }

   /// Highest known tree number — the only tree that still accepts new leaves.
   pub fn open_tree_number(&self) -> Option<u32> {
      self.known_trees.iter().next_back().copied()
   }

   /// Tree numbers currently rebuilt in RAM.
   pub fn resident_trees(&self) -> Vec<u32> {
      self.utxo_trees.keys().copied().collect()
   }

   /// All catalogued tree numbers, including sealed trees that are not in RAM.
   pub fn known_tree_numbers(&self) -> Vec<u32> {
      self.known_trees.iter().copied().collect()
   }

   /// Cached root/leaf-count for a tree (live tree preferred over disk meta).
   pub fn tree_summary(&self, tree_number: u32) -> Option<UtxoTreeSummary> {
      if let Some(tree) = self.utxo_trees.get(&tree_number) {
         return Some(UtxoTreeSummary {
            leaf_count: tree.leaves_len(),
            root: tree.root(),
         });
      }
      self.tree_summaries.get(&tree_number).copied()
   }

   fn is_sealed(&self, tree_number: u32) -> bool {
      self.open_tree_number().map(|open| tree_number < open).unwrap_or(false)
   }

   fn capture_summary(&mut self, tree_number: u32, tree: &UtxoMerkleTree) {
      self.tree_summaries.insert(
         tree_number,
         UtxoTreeSummary {
            leaf_count: tree.leaves_len(),
            root: tree.root(),
         },
      );
   }

   fn desired_resident_trees(&self) -> BTreeSet<u32> {
      let mut keep = BTreeSet::new();
      if let Some(open) = self.open_tree_number() {
         keep.insert(open);
      }
      for account in &self.accounts {
         for note in account.unspent() {
            if self.can_spend_without_tree(&note) {
               continue;
            }
            keep.insert(note.tree_number);
         }
      }
      keep
   }

   fn can_spend_without_tree(&self, note: &UtxoNote) -> bool {
      self.is_sealed(note.tree_number)
         && self.frozen_proofs.contains_key(&(note.tree_number, note.leaf_index))
   }

   /// Freeze inclusion proofs for sealed resident trees, then drop them from RAM.
   ///
   /// Disk leaves stay. Proofs are extra keys (`utxo_proof:{tree}:{leaf}`), so old
   /// DBs without them still load the tree.
   pub async fn compact_utxo_trees(&mut self) -> Result<(), UtxoIndexerError> {
      self.freeze_sealed_resident_trees().await?;
      let keep = self.desired_resident_trees();
      let drop: Vec<u32> = self
         .utxo_trees
         .keys()
         .copied()
         .filter(|n| !keep.contains(n) && !self.dirty_chunks.contains_key(n))
         .collect();
      if drop.is_empty() {
         return Ok(());
      }
      for n in &drop {
         if let Some(tree) = self.utxo_trees.remove(n) {
            self.capture_summary(*n, &tree);
         }
      }
      debug!(
         "Unloaded sealed UTXO trees from memory: {:?} (resident={:?} known={:?})",
         drop,
         self.resident_trees(),
         self.known_tree_numbers()
      );
      Ok(())
   }

   async fn freeze_sealed_resident_trees(&mut self) -> Result<(), UtxoIndexerError> {
      let sealed: Vec<u32> = self
         .utxo_trees
         .keys()
         .copied()
         .filter(|n| self.is_sealed(*n) && !self.dirty_chunks.contains_key(n))
         .collect();
      if sealed.is_empty() {
         return Ok(());
      }

      let notes: Vec<UtxoNote> = self.accounts.iter().flat_map(|a| a.unspent()).collect();
      let mut batch = WriteBatch::new();
      let mut new_proofs: Vec<((u32, u32), RailgunMerkleProof)> = Vec::new();

      for tree_number in sealed {
         let Some(tree) = self.utxo_trees.get(&tree_number) else {
            continue;
         };
         for note in notes.iter().filter(|n| n.tree_number == tree_number) {
            if self.frozen_proofs.contains_key(&(tree_number, note.leaf_index)) {
               continue;
            }
            let proof = tree.generate_proof(note.hash())?;
            let leaf: ruint::aliases::U256 = note.hash().into();
            if proof.element != leaf || proof.root != tree.root() || !proof.verify() {
               return Err(UtxoIndexerError::InvalidFrozenProof(
                  tree_number,
                  note.leaf_index,
               ));
            }
            put_utxo_note_proof(
               &mut batch,
               tree_number,
               note.leaf_index,
               &proof,
               &self.db_key,
            )?;
            new_proofs.push(((tree_number, note.leaf_index), proof));
         }
      }

      if !batch.is_empty() {
         self.db.apply_batch(batch, WriteDurability::Immediate).await?;
      }
      for (key, proof) in new_proofs {
         self.frozen_proofs.insert(key, proof);
      }
      Ok(())
   }

   async fn hydrate_frozen_proofs(&mut self, notes: &[UtxoNote]) -> Result<(), UtxoIndexerError> {
      for note in notes {
         if !self.is_sealed(note.tree_number) {
            continue;
         }
         let key = (note.tree_number, note.leaf_index);
         if self.frozen_proofs.contains_key(&key) {
            continue;
         }
         if let Some(proof) = self.db.get_utxo_note_proof(note.tree_number, note.leaf_index).await?
         {
            let leaf: ruint::aliases::U256 = note.hash().into();
            if proof.element != leaf || !proof.verify() {
               return Err(UtxoIndexerError::InvalidFrozenProof(
                  note.tree_number,
                  note.leaf_index,
               ));
            }
            self.frozen_proofs.insert(key, proof);
         }
      }
      Ok(())
   }

   /// Make sure we can prove `notes`: live tree or a frozen inclusion proof.
   pub async fn ensure_resident_for_notes(
      &mut self,
      notes: &[UtxoNote],
   ) -> Result<(), UtxoIndexerError> {
      self.hydrate_frozen_proofs(notes).await?;
      let mut trees: BTreeSet<u32> = BTreeSet::new();
      if let Some(open) = self.open_tree_number() {
         trees.insert(open);
      }
      for note in notes {
         if !self.can_spend_without_tree(note) {
            trees.insert(note.tree_number);
         }
      }
      for n in trees {
         self.ensure_tree_loaded(n, false).await?;
      }
      self.freeze_sealed_resident_trees().await?;
      Ok(())
   }

   pub fn merkle_witnesses(
      &self,
      notes: &[UtxoNote],
   ) -> Result<crate::transact::MerkleWitnesses, UtxoIndexerError> {
      use crate::transact::MerkleWitnesses;

      let mut witnesses = MerkleWitnesses::default();
      for note in notes {
         let proof = if let Some(tree) = self.utxo_trees.get(&note.tree_number) {
            tree.generate_proof(note.hash())?
         } else if let Some(p) = self.frozen_proofs.get(&(note.tree_number, note.leaf_index)) {
            p.clone()
         } else {
            return Err(UtxoIndexerError::MissingTree(note.tree_number));
         };
         witnesses.roots.insert(note.tree_number, proof.root);
         witnesses.proofs.insert((note.tree_number, note.leaf_index), proof);
      }
      if let Some(open) = self.open_tree_number() {
         if let Some(tree) = self.utxo_trees.get(&open) {
            witnesses.roots.entry(open).or_insert(tree.root());
         } else if let Some(s) = self.tree_summaries.get(&open) {
            witnesses.roots.entry(open).or_insert(s.root);
         }
      }
      Ok(witnesses)
   }

   async fn ensure_trees_for_unspent(&mut self) -> Result<(), UtxoIndexerError> {
      let notes: Vec<UtxoNote> = self.accounts.iter().flat_map(|a| a.unspent()).collect();
      self.ensure_resident_for_notes(&notes).await
   }

   async fn ensure_tree_loaded(
      &mut self,
      tree_number: u32,
      allow_empty: bool,
   ) -> Result<(), UtxoIndexerError> {
      if self.utxo_trees.contains_key(&tree_number) {
         self.known_trees.insert(tree_number);
         return Ok(());
      }

      match self.db.get_utxo_tree_leaves(tree_number).await? {
         Some((leaves, from_legacy)) => {
            if from_legacy {
               self.legacy_trees.insert(tree_number);
            }
            let tree = UtxoMerkleTree::from_leaves(tree_number, leaves);
            debug!(
               "Loaded UTXO tree {} into memory ({} leaves, root={})",
               tree_number,
               tree.leaves_len(),
               tree.root()
            );
            self.capture_summary(tree_number, &tree);
            self.utxo_trees.insert(tree_number, tree);
            self.known_trees.insert(tree_number);
            Ok(())
         }
         None if allow_empty => {
            self.utxo_trees.insert(tree_number, UtxoMerkleTree::new(tree_number));
            self.known_trees.insert(tree_number);
            Ok(())
         }
         None => Err(UtxoIndexerError::MissingTree(tree_number)),
      }
   }

   fn insert_sorted_leaves(&mut self, tree_number: u32, mut leaves: Vec<(u32, UtxoLeafHash)>) {
      leaves.sort_by_key(|(idx, _)| *idx);
      let mut dirty_ranges = Vec::new();
      {
         let tree = self
            .utxo_trees
            .entry(tree_number)
            .or_insert_with(|| UtxoMerkleTree::new(tree_number));
         for (leaf_index, hash) in leaves {
            let start = leaf_index as usize;
            tree.insert_leaves(&[hash], start);
            dirty_ranges.push((start, start + 1));
         }
         tree.shrink_to_fit();
      }
      self.known_trees.insert(tree_number);
      for (start, end) in dirty_ranges {
         self.mark_tree_range_dirty(tree_number, start, end);
      }
   }

   /// Syncs the indexer to a specific block. If the indexer is already synced past that block,
   /// this is a no-op.
   ///
   /// # Arguments
   ///
   /// * `to_block` - The block to sync to.
   /// * `deployment_block` - The block at which the Railgun contract was deployed.
   /// * `use_subsquid` - Whether to use the subsquid syncer.
   #[tracing::instrument(name = "utxo_sync", skip_all)]
   pub async fn sync_to(
      &mut self,
      to_block: u64,
      deployment_block: u64,
      use_subsquid: bool,
   ) -> Result<(), UtxoIndexerError> {
      // Tree progress is global and independent of accounts. A brand-new registered
      // account (synced_block=0) must re-scan history for note decryption, but must
      // NOT rebuild/re-insert into merkle trees that are already loaded from DB.
      let global_synced = self.synced_block;

      let mut tree_from = global_synced.saturating_add(1);
      if tree_from <= 1 && !use_subsquid {
         tree_from = deployment_block;
      }

      let account_min = self.accounts.iter().map(|a| a.synced_block()).min();

      let account_from = match account_min {
         Some(min_synced) => {
            let mut from = min_synced.saturating_add(1);
            if from <= 1 && !use_subsquid {
               from = deployment_block;
            }
            from
         }
         // No accounts registered: only advance the global tree.
         None => tree_from,
      };

      let from_block = tree_from.min(account_from);

      debug!(
         "Effective sync range for utxo: from_block={} to_block={} use_subsquid={} global_synced={} tree_from={} account_from={} accounts={}",
         from_block,
         to_block,
         use_subsquid,
         global_synced,
         tree_from,
         account_from,
         self.accounts.len()
      );

      let latest_block = if use_subsquid {
         let syncer = self.subsquid_syncer.as_ref().ok_or_else(|| {
            SyncerError::new(std::io::Error::new(
               std::io::ErrorKind::Other,
               "subsquid syncer not configured",
            ))
         })?;
         syncer.latest_block().await?
      } else {
         self.rpc_syncer.latest_block().await?
      };

      let to_block = to_block.min(latest_block);

      if from_block > to_block {
         return Ok(());
      }

      let events = if use_subsquid {
         self
            .subsquid_syncer
            .as_ref()
            .expect("checked above")
            .sync(from_block, to_block)
            .await?
      } else {
         self.rpc_syncer.sync(from_block, to_block).await?
      };
      debug!("Fetched {} events from syncer", events.len());

      let mut tree_leaves: HashMap<u32, Vec<(u32, UtxoLeafHash)>> = HashMap::new();
      for (i, event) in events.iter().enumerate() {
         if i % 20000 == 0 {
            debug!("Processing event {}/{}", i, events.len());
         }
         // Only mutate trees for blocks the global indexer has not applied yet.
         let apply_tree = event.block_number() > global_synced;
         self.handle_event(event, &mut tree_leaves, apply_tree)?;
      }

      let trees_mutated = !tree_leaves.is_empty();

      if trees_mutated {
         debug!("Inserting leaves into UTXO trees");
         let mutated: Vec<u32> = tree_leaves.keys().copied().collect();
         for tree_number in &mutated {
            self.ensure_tree_loaded(*tree_number, true).await?;
         }
         for (tree_number, leaves) in tree_leaves {
            self.insert_sorted_leaves(tree_number, leaves);
         }
      } else {
         debug!("No new tree leaves (account catch-up and/or empty delta)");
      }

      for tn in self.known_tree_numbers() {
         if let Some(summary) = self.tree_summary(tn) {
            debug!(
               "Tree {} now has {} leaves (root={}){}",
               tn,
               summary.leaf_count,
               summary.root,
               if self.utxo_trees.contains_key(&tn) {
                  ""
               } else {
                  " [unloaded]"
               }
            );
         }
      }

      // Verify only trees that received new leaves. Sealed trees are immutable;
      // account-only catch-up must not risk failing on an unrelated tree.
      if trees_mutated {
         self.warn_if_trees_not_sequentially_full();
         debug!("Verifying UTXO trees");
         let mutated: Vec<u32> = self.dirty_chunks.keys().copied().collect();
         self.verify_trees(&mutated, None).await?;
         let updates: Vec<(u32, UtxoTreeSummary)> = mutated
            .iter()
            .filter_map(|n| {
               self.utxo_trees.get(n).map(|tree| {
                  (
                     *n,
                     UtxoTreeSummary {
                        leaf_count: tree.leaves_len(),
                        root: tree.root(),
                     },
                  )
               })
            })
            .collect();
         for (n, summary) in updates {
            self.tree_summaries.insert(n, summary);
         }
      }

      debug!(
         "Synced to block {} (trees_mutated={})",
         to_block, trees_mutated
      );

      if tree_from <= to_block {
         self.synced_block = to_block;
      }

      for account in self.accounts.iter_mut() {
         if account.synced_block() < to_block {
            account.set_synced_block(to_block);
         }
      }

      // Save trees only when mutated, accounts only when dirty.
      self.save(trees_mutated).await?;
      self.compact_utxo_trees().await?;

      // ! Dont call compact because it fucks up with mem usage

      Ok(())
   }

   /// Syncs the indexer directly from logs
   ///
   /// This should be used only for evm simulations on a new instance of the indexer
   pub fn sync_from_logs(
      &mut self,
      logs: Vec<Log>,
      block: u64,
      _timestamp: u64,
   ) -> Result<(), UtxoIndexerError> {
      let mut events = Vec::new();

      for log in logs {
         if let Ok(decoded) = <RailgunSmartWallet::Shield as SolEvent>::decode_log(&log) {
            let mut shield_events = super::parse_shield(
               &decoded.data,
               block,
               _timestamp,
               Default::default(),
            )?;
            events.append(&mut shield_events);
            continue;
         }

         if let Ok(decoded) = <RailgunSmartWallet::Transact as SolEvent>::decode_log(&log) {
            // Store chain block, not the fork timestamp — same contract as RpcSyncer.
            let mut tx_events = super::parse_transact(
               &decoded.data,
               block,
               _timestamp,
               Default::default(),
            )?;
            events.append(&mut tx_events);
            continue;
         }

         if let Ok(decoded) = <RailgunSmartWallet::Nullified as SolEvent>::decode_log(&log) {
            let mut null_events = super::parse_nullified(
               &decoded.data,
               block,
               _timestamp,
               Default::default(),
            )?;
            events.append(&mut null_events);
            continue;
         }

         // Legacy events
         if let Ok(decoded) = <RailgunLegacy::CommitmentBatch as SolEvent>::decode_log(&log) {
            let mut legacy_events = super::parse_legacy_commitment_batch(&decoded.data, block)?;
            events.append(&mut legacy_events);
            continue;
         }

         if let Ok(decoded) = <RailgunLegacy::Nullifiers as SolEvent>::decode_log(&log) {
            let mut null_events = super::parse_legacy_nullifiers(
               &decoded.data,
               block,
               _timestamp,
               Default::default(),
            )?;
            events.append(&mut null_events);
            continue;
         }

         if let Ok(decoded) =
            <RailgunLegacy::GeneratedCommitmentBatch as SolEvent>::decode_log(&log)
         {
            let mut legacy_events =
               super::parse_legacy_generated_commitment_batch(&decoded.data, block)?;
            events.append(&mut legacy_events);
            continue;
         }

         if let Ok(decoded) = <RailgunLegacy::Transact as SolEvent>::decode_log(&log) {
            let mut tx_events = super::parse_legacy_transact(
               &decoded.data,
               block,
               _timestamp,
               Default::default(),
            )?;
            events.append(&mut tx_events);
            continue;
         }

         if let Ok(decoded) = <RailgunLegacy::Shield as SolEvent>::decode_log(&log) {
            let mut shield_events = super::parse_legacy_shield(
               &decoded.data,
               block,
               _timestamp,
               Default::default(),
            )?;
            events.append(&mut shield_events);
            continue;
         }

         if let Ok(decoded) = <RailgunLegacy::Unshield as SolEvent>::decode_log(&log) {
            let _ = super::parse_legacy_unshield(&decoded.data, block);
            continue;
         }
      }

      let mut tree_leaves: HashMap<u32, Vec<(u32, UtxoLeafHash)>> = HashMap::new();
      for (_, event) in events.iter().enumerate() {
         self.handle_event(event, &mut tree_leaves, true)?;
      }

      for (tree_number, leaves) in tree_leaves {
         self.insert_sorted_leaves(tree_number, leaves);
      }

      Ok(())
   }

   fn handle_event(
      &mut self,
      event: &SyncEvent,
      tree_leaves: &mut HashMap<u32, Vec<(u32, UtxoLeafHash)>>,
      apply_tree: bool,
   ) -> Result<(), UtxoIndexerError> {
      let block = event.block_number();
      match event {
         SyncEvent::Shield(shield, _) => {
            self.handle_shield(shield, block, tree_leaves, apply_tree)?
         }
         SyncEvent::Transact(transact, _) => {
            self.handle_transact(transact, block, tree_leaves, apply_tree)?
         }
         SyncEvent::Nullified(nullified, _) => self.handle_nullified(nullified, block),
         SyncEvent::Legacy(legacy, _) => self.handle_legacy(legacy, block, tree_leaves, apply_tree),
      };

      Ok(())
   }

   fn handle_shield(
      &mut self,
      event: &syncer::Shield,
      block: u64,
      tree_leaves: &mut HashMap<u32, Vec<(u32, UtxoLeafHash)>>,
      apply_tree: bool,
   ) -> Result<(), UtxoIndexerError> {
      if apply_tree {
         tree_leaves
            .entry(event.tree_number)
            .or_default()
            .push((event.leaf_index, event.hash()));
      }

      for account in self.accounts.iter_mut() {
         if block > account.synced_block() {
            account.handle_shield_event(event, block)?;
         }
      }

      Ok(())
   }

   fn handle_transact(
      &mut self,
      event: &syncer::Transact,
      block: u64,
      tree_leaves: &mut HashMap<u32, Vec<(u32, UtxoLeafHash)>>,
      apply_tree: bool,
   ) -> Result<(), UtxoIndexerError> {
      if apply_tree {
         tree_leaves
            .entry(event.tree_number)
            .or_default()
            .push((event.leaf_index, event.hash.into()));
      }

      for account in self.accounts.iter_mut() {
         if block > account.synced_block() {
            account.handle_transact_event(event, block)?;
         }
      }

      Ok(())
   }

   fn handle_nullified(&mut self, event: &syncer::Nullified, block: u64) {
      for account in self.accounts.iter_mut() {
         if block > account.synced_block() {
            account.handle_nullified_event(event, block);
         }
      }
   }

   // This is still WIP ( see handle_legacy_event )
   fn handle_legacy(
      &mut self,
      event: &syncer::LegacyCommitment,
      block: u64,
      tree_leaves: &mut HashMap<u32, Vec<(u32, UtxoLeafHash)>>,
      apply_tree: bool,
   ) {
      if apply_tree {
         tree_leaves
            .entry(event.tree_number)
            .or_default()
            .push((event.leaf_index, event.hash.into()));
      }

      // Forward to accounts so they can attempt decryption for private balances
      // (only when we have the ciphertext from legacy CommitmentBatch)
      if event.ciphertext.is_some() {
         for account in self.accounts.iter_mut() {
            if block > account.synced_block() {
               if let Err(e) = account.handle_legacy_event(event) {
                  // Ignore decryption failures (not our note)
                  if !matches!(e, NoteError::Aes(_)) {
                     tracing::debug!("Legacy note handling error: {}", e);
                  }
               }
            }
         }
      }
   }

   /// On-chain trees usually fill sequentially, but a short intermediate tree is
   /// not proof of corruption (tree 1 ending at 65535 has been observed with a
   /// valid rootHistory). Detect real holes: zero-padded gaps inside level-0.
   fn warn_if_trees_not_sequentially_full(&self) {
      use crate::merkle_tree::{MerkleConfig, RailgunMerkleConfig, TOTAL_LEAVES};

      let zero = RailgunMerkleConfig::zero();
      for (n, tree) in &self.utxo_trees {
         let leaves = tree.leaves();
         if leaves.is_empty() {
            continue;
         }
         // Sparse insert pads with zero between min and max index — a middle zero
         // with non-zeros after it means a missing commitment.
         let mut saw_nonzero_after_gap = false;
         let mut in_gap = false;
         for (i, leaf) in leaves.iter().enumerate() {
            if *leaf == zero {
               if i + 1 < leaves.len() {
                  in_gap = true;
               }
            } else if in_gap {
               saw_nonzero_after_gap = true;
               break;
            }
         }
         if saw_nonzero_after_gap {
            tracing::warn!(
               "UTXO tree {} has internal zero gaps in leaves (len={}); \
                event history is likely missing commitments — root verify may fail.",
               n,
               leaves.len()
            );
         }
      }

      let max_tree = match self.known_trees.iter().next_back().copied() {
         Some(n) => n,
         None => return,
      };
      for n in 0..max_tree {
         let len = self
            .utxo_trees
            .get(&n)
            .map(|t| t.leaves_len())
            .or_else(|| self.tree_summaries.get(&n).map(|s| s.leaf_count))
            .unwrap_or(0);
         if len > 0 && len < TOTAL_LEAVES as usize {
            tracing::debug!(
               "UTXO tree {} has {} leaves (TOTAL_LEAVES={}); higher tree present",
               n,
               len,
               TOTAL_LEAVES
            );
         }
      }
   }

   fn should_skip_root_verify(&self, tree_number: u32, tree: &UtxoMerkleTree) -> bool {
      if !self.is_sealed(tree_number) {
         return false;
      }
      match self.tree_summaries.get(&tree_number) {
         Some(s) if s.root == tree.root() && s.leaf_count == tree.leaves_len() => true,
         _ => false,
      }
   }

   pub async fn verify(&self, block_id: Option<BlockId>) -> Result<(), UtxoIndexerError> {
      let trees: Vec<u32> = self.utxo_trees.keys().copied().collect();
      self.verify_trees(&trees, block_id).await
   }

   async fn verify_trees(
      &self,
      trees: &[u32],
      block_id: Option<BlockId>,
   ) -> Result<(), UtxoIndexerError> {
      // TODO: Make this a batch call
      for &tree_number in trees {
         let Some(tree) = self.utxo_trees.get(&tree_number) else {
            continue;
         };
         if tree.leaves_len() == 0 {
            continue;
         }
         if self.should_skip_root_verify(tree_number, tree) {
            debug!(
               "Skipping root verify for sealed UTXO tree {} (cached root={})",
               tree_number,
               tree.root()
            );
            continue;
         }

         let exists = self
            .utxo_verifier
            .verify_root(tree.number(), 0, tree.root(), block_id)
            .await
            .map_err(|e| UtxoIndexerError::VerificationError(e))?;

         if !exists {
            return Err(UtxoIndexerError::InvalidRoot(
               tree.number(),
               tree.root().into(),
            ));
         }
      }
      Ok(())
   }

   /// Compact the db to save space
   pub async fn compact(&self) -> Result<bool, DatabaseError> {
      self.db.compact().await
   }

   /// Saves the current state of the indexer to the database.
   ///
   /// - Always writes the lightweight indexer watermark.
   /// - Only dirty trees are touched; for each, only dirty leaf chunks (+ meta).
   /// - Account state is only rewritten when the account is dirty.
   /// - Everything goes through one redb write transaction.
   /// - Watermark-only saves use non-durable commits (safe to re-sync).
   /// - Tree/account saves use immediate durability.
   pub async fn save(&mut self, _trees_mutated: bool) -> Result<(), DatabaseError> {
      let mut batch = WriteBatch::new();

      let state = UtxoIndexerState {
         synced_block: self.synced_block,
         trees: self.known_trees.iter().copied().collect(),
      };
      put_utxo_indexer(&mut batch, &state)?;

      let mut has_critical = false;

      // Only rewrite dirty trees. Legacy trees migrate to chunked format the first
      // time they become dirty (not every tree on the first post-upgrade save).
      let trees_to_write: Vec<u32> = self.dirty_chunks.keys().copied().collect();
      let mut migrated = Vec::new();

      for tree_number in trees_to_write {
         let Some(tree) = self.utxo_trees.get(&tree_number) else {
            continue;
         };
         has_critical = true;
         let migrate = self.legacy_trees.contains(&tree_number);
         let dirty = if migrate {
            all_chunk_indices(tree.leaves_len())
         } else {
            self.dirty_chunks.get(&tree_number).cloned().unwrap_or_default()
         };
         if dirty.is_empty() && !migrate {
            continue;
         }
         push_utxo_tree_save(
            &mut batch,
            tree_number,
            tree.leaves(),
            tree.root().into(),
            &dirty,
            migrate,
         )?;
         if migrate {
            migrated.push(tree_number);
         }
      }
      for tree_number in migrated {
         self.legacy_trees.remove(&tree_number);
      }

      for account in self.accounts.iter() {
         if account.is_dirty() {
            has_critical = true;
            put_account(
               &mut batch,
               account.address(),
               &account.state(),
               &self.db_key,
            )?;
         }
      }

      let durability = if has_critical {
         WriteDurability::Immediate
      } else {
         // Pure watermark bump, can be reconstructed by re-sync after crash.
         WriteDurability::None
      };

      self.db.apply_batch(batch, durability).await?;

      self.dirty_chunks.clear();
      for account in self.accounts.iter_mut() {
         if account.is_dirty() {
            account.clear_dirty();
         }
      }

      Ok(())
   }
}

#[cfg(test)]
mod tests {
   use super::*;
   use alloy_primitives::{Address, B256, address};
   use alloy_provider::ProviderBuilder;
   use rand::random;
   use ruint::aliases::U256 as Ru256;
   use secure_types::SecureArray;

   use crate::{
      account::signer::RailgunSigner,
      caip::AssetId,
      database::{
         RailgunDbKey, RedbDatabase, WriteBatch, WriteDurability,
         railgun_db::{all_chunk_indices, push_utxo_tree_save},
      },
      indexer::indexed_account::{IndexedAccountState, NoteRecord},
      indexer::syncer::RpcSyncer,
      merkle_tree::{RailgunMerkleProof, RootVerifier, UtxoLeafHash, UtxoMerkleTree},
      poi::types::BlindedCommitmentType,
   };

   fn dummy_syncer() -> (RpcSyncer, RootVerifier) {
      let url: reqwest::Url = "http://127.0.0.1:1".parse().unwrap();
      let provider = ProviderBuilder::new().connect_http(url);
      let rpc = RpcSyncer::new(provider.clone(), 1, Address::ZERO);
      let verifier = RootVerifier::new(provider, Address::ZERO);
      (rpc, verifier)
   }

   async fn persist_tree_leaves(db: &RedbDatabase, number: u32, hashes: &[UtxoLeafHash]) {
      let mut tree = UtxoMerkleTree::new(number);
      if !hashes.is_empty() {
         tree.insert_leaves(hashes, 0);
      }
      let mut batch = WriteBatch::new();
      let chunks = all_chunk_indices(hashes.len());
      let root: ruint::aliases::U256 = tree.root().into();
      push_utxo_tree_save(
         &mut batch,
         number,
         tree.leaves(),
         root,
         &chunks,
         false,
      )
      .unwrap();
      db.apply_batch(batch, WriteDurability::Immediate).await.unwrap();
   }

   async fn persist_tree(db: &RedbDatabase, number: u32, n_leaves: usize) {
      let hashes: Vec<UtxoLeafHash> =
         (0..n_leaves as u64).map(|i| Ru256::from(i + 1).into()).collect();
      persist_tree_leaves(db, number, &hashes).await;
   }

   async fn indexer_from_db(db: RedbDatabase) -> UtxoIndexer {
      let (rpc, verifier) = dummy_syncer();
      UtxoIndexer::new(db, rpc, None, verifier).await.unwrap()
   }

   #[tokio::test]
   async fn loads_only_open_tree_into_memory() {
      let db = RedbDatabase::in_memory(RailgunDbKey::generate().unwrap()).unwrap();
      persist_tree(&db, 0, 16).await;
      persist_tree(&db, 1, 8).await;
      persist_tree(&db, 2, 4).await;
      db.set_utxo_indexer(&UtxoIndexerState {
         synced_block: 99,
         trees: vec![0, 1, 2],
      })
      .await
      .unwrap();

      let indexer = indexer_from_db(db).await;
      assert_eq!(indexer.known_tree_numbers(), vec![0, 1, 2]);
      assert_eq!(indexer.resident_trees(), vec![2]);
      assert_eq!(indexer.open_tree_number(), Some(2));
      assert_eq!(indexer.tree_summary(0).unwrap().leaf_count, 16);
      assert_eq!(indexer.tree_summary(1).unwrap().leaf_count, 8);
      assert_eq!(indexer.tree_summary(2).unwrap().leaf_count, 4);
      assert!(indexer.utxo_trees.contains_key(&2));
      assert!(!indexer.utxo_trees.contains_key(&0));
      assert!(!indexer.utxo_trees.contains_key(&1));
   }

   #[tokio::test]
   async fn register_loads_trees_with_unspent_notes_then_compact_unloads() {
      let db = RedbDatabase::in_memory(RailgunDbKey::generate().unwrap()).unwrap();

      let seed: [u8; 64] = random();
      let sec = SecureArray::from_slice(&seed).unwrap();
      let signer = RailgunSigner::from_seed(&sec, 0, 1).unwrap();
      let asset = AssetId::erc20(address!(
         "0xDEADDEADDEADDEADDEADDEADDEADDEADDEADDEAD"
      ));
      let note = UtxoNote::new(
         0,
         0,
         &signer,
         asset,
         1,
         [7u8; 16],
         "",
         BlindedCommitmentType::Shield,
      );

      let mut tree0_leaves = vec![note.hash()];
      tree0_leaves.extend((1..16u64).map(|i| UtxoLeafHash::from(Ru256::from(i + 1))));
      persist_tree_leaves(&db, 0, &tree0_leaves).await;
      persist_tree(&db, 1, 8).await;
      persist_tree(&db, 2, 4).await;
      db.set_utxo_indexer(&UtxoIndexerState {
         synced_block: 99,
         trees: vec![0, 1, 2],
      })
      .await
      .unwrap();

      let state = IndexedAccountState {
         notes: vec![NoteRecord {
            note: note.clone(),
            created_block: 1,
            created_timestamp: 0,
            created_tx_hash: B256::ZERO,
         }],
         synced_block: 99,
         spent_notes: vec![],
      };
      db.set_account(signer.address(), &state).await.unwrap();

      let mut indexer = indexer_from_db(db).await;
      assert_eq!(indexer.resident_trees(), vec![2]);

      indexer.register(signer.clone()).await.unwrap();
      // Sealed tree 0 is frozen then unloaded; only the open tree stays resident.
      assert_eq!(indexer.resident_trees(), vec![2]);
      assert_eq!(indexer.known_tree_numbers(), vec![0, 1, 2]);
      let witnesses = indexer.merkle_witnesses(&[note.clone()]).unwrap();
      assert!(witnesses.proofs.contains_key(&(0, 0)));
      assert!(witnesses.proofs[&(0, 0)].verify());

      indexer.retain_accounts(&[]).await.unwrap();
      assert_eq!(indexer.resident_trees(), vec![2]);
      assert_eq!(indexer.known_tree_numbers(), vec![0, 1, 2]);

      indexer.save(false).await.unwrap();
      let state = indexer.db.get_utxo_indexer().await.unwrap();
      assert_eq!(state.trees, vec![0, 1, 2]);
   }

   fn dummy_leaf(i: u64) -> UtxoLeafHash {
      UtxoLeafHash::from(Ru256::from(i.wrapping_mul(0x9E37) + 0xC0FFEE))
   }

   fn test_signer() -> RailgunSigner {
      let seed: [u8; 64] = random();
      let sec = SecureArray::from_slice(&seed).unwrap();
      RailgunSigner::from_seed(&sec, 0, 1).unwrap()
   }

   fn test_asset() -> AssetId {
      AssetId::erc20(address!(
         "0xDEADDEADDEADDEADDEADDEADDEADDEADDEADDEAD"
      ))
   }

   fn make_note(signer: &RailgunSigner, tree: u32, leaf: u32, value: u128) -> UtxoNote {
      UtxoNote::new(
         tree,
         leaf,
         signer,
         test_asset(),
         value,
         [leaf as u8; 16],
         "",
         BlindedCommitmentType::Shield,
      )
   }

   fn note_record(note: UtxoNote) -> NoteRecord {
      NoteRecord {
         note,
         created_block: 1,
         created_timestamp: 0,
         created_tx_hash: B256::ZERO,
      }
   }

   fn leaves_with_notes(len: usize, notes: &[(u32, UtxoLeafHash)]) -> Vec<UtxoLeafHash> {
      let mut leaves: Vec<UtxoLeafHash> = (0..len as u64).map(dummy_leaf).collect();
      for &(idx, hash) in notes {
         leaves[idx as usize] = hash;
      }
      leaves
   }

   async fn persist_mainnet_like(db: &RedbDatabase, placements: &[(u32, u32, UtxoLeafHash)]) {
      // Topology matches Ethereum mainnet: trees 0/2/3 filled, tree 1 one short,
      // tree 4 the open (partial) tree.
      let lens = [2048usize, 2047, 2048, 2048, 256];
      for (tree, &len) in lens.iter().enumerate() {
         let tree = tree as u32;
         let placed: Vec<(u32, UtxoLeafHash)> = placements
            .iter()
            .filter(|(t, idx, _)| *t == tree && (*idx as usize) < len)
            .map(|(_, idx, h)| (*idx, *h))
            .collect();
         persist_tree_leaves(db, tree, &leaves_with_notes(len, &placed)).await;
      }
      db.set_utxo_indexer(&UtxoIndexerState {
         synced_block: 25_924_250,
         trees: vec![0, 1, 2, 3, 4],
      })
      .await
      .unwrap();
   }

   fn assert_proof_matches_tree(
      tree: &UtxoMerkleTree,
      note: &UtxoNote,
      proof: &RailgunMerkleProof,
   ) {
      let live = tree.generate_proof(note.hash()).unwrap();
      assert_eq!(proof.element, live.element);
      assert_eq!(proof.elements, live.elements);
      assert_eq!(proof.indices, live.indices);
      assert_eq!(proof.root, live.root);
      assert_eq!(proof.root, tree.root());
      assert!(proof.verify());
   }

   fn try_01x01_circuit() -> Option<crate::circuit::remote_artifact_loader::EmbeddedCircuit> {
      use crate::circuit::remote_artifact_loader::EmbeddedCircuit;
      let dir =
         std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../embedded/railgun/01x01");
      let wasm = std::fs::read(dir.join("wasm.br")).ok()?;
      let pk = std::fs::read(dir.join("proving_key.bin.br")).ok()?;
      let matrices = std::fs::read(dir.join("matrices.bin.br")).ok()?;
      if wasm.len() < 256 || pk.len() < 256 || matrices.len() < 256 {
         return None;
      }
      Some(EmbeddedCircuit::new(
         "railgun/01x01",
         Box::leak(wasm.into_boxed_slice()),
         Box::leak(pk.into_boxed_slice()),
         Box::leak(matrices.into_boxed_slice()),
      ))
   }

   /// Mainnet-shaped forest: sealed 0–3, open 4. Covers load, freeze, reload,
   /// tip insert, new-tree seal, second signer, and Groth16 01x01 if artifacts exist.
   #[tokio::test]
   async fn mainnet_like_residency_freeze_reload_tip_and_prove() {
      use crate::{
         circuit::groth16_prover::Groth16Prover,
         circuit::inputs::transact_inputs::TransactCircuitInputs,
         note::{OutputNote, unshield::UnshieldNote},
         rand::SeedableRng,
         rand_chacha::ChaCha12Rng,
         transact::TransactionBuilder,
      };

      let db = RedbDatabase::in_memory(RailgunDbKey::generate().unwrap()).unwrap();
      let signer_a = test_signer();
      let signer_b = test_signer();

      let note0 = make_note(&signer_a, 0, 100, 1_000);
      let note3 = make_note(&signer_a, 3, 50, 2_000);
      let note4 = make_note(&signer_a, 4, 10, 3_000);
      let note2_b = make_note(&signer_b, 2, 7, 4_000);

      persist_mainnet_like(
         &db,
         &[
            (0, 100, note0.hash()),
            (3, 50, note3.hash()),
            (4, 10, note4.hash()),
            (2, 7, note2_b.hash()),
         ],
      )
      .await;

      let indexer = indexer_from_db(db.clone()).await;
      assert_eq!(indexer.known_tree_numbers(), vec![0, 1, 2, 3, 4]);
      assert_eq!(indexer.resident_trees(), vec![4]);
      assert_eq!(indexer.open_tree_number(), Some(4));
      assert_eq!(indexer.tree_summary(0).unwrap().leaf_count, 2048);
      assert_eq!(indexer.tree_summary(1).unwrap().leaf_count, 2047);
      assert_eq!(indexer.tree_summary(2).unwrap().leaf_count, 2048);
      assert_eq!(indexer.tree_summary(3).unwrap().leaf_count, 2048);
      assert_eq!(indexer.tree_summary(4).unwrap().leaf_count, 256);
      drop(indexer);

      db.set_account(
         signer_a.address(),
         &IndexedAccountState {
            notes: vec![
               note_record(note0.clone()),
               note_record(note3.clone()),
               note_record(note4.clone()),
            ],
            synced_block: 25_924_250,
            spent_notes: vec![],
         },
      )
      .await
      .unwrap();
      db.set_account(
         signer_b.address(),
         &IndexedAccountState {
            notes: vec![note_record(note2_b.clone())],
            synced_block: 25_924_250,
            spent_notes: vec![],
         },
      )
      .await
      .unwrap();

      let mut indexer = indexer_from_db(db.clone()).await;
      indexer.register(signer_a.clone()).await.unwrap();
      indexer.register(signer_b.clone()).await.unwrap();

      assert_eq!(
         indexer.resident_trees(),
         vec![4],
         "sealed trees with frozen proofs must not stay in RAM"
      );
      assert_eq!(indexer.known_tree_numbers(), vec![0, 1, 2, 3, 4]);

      let witnesses = indexer
         .merkle_witnesses(&[note0.clone(), note3.clone(), note4.clone(), note2_b.clone()])
         .unwrap();
      for note in [&note0, &note3, &note4, &note2_b] {
         let proof = &witnesses.proofs[&(note.tree_number, note.leaf_index)];
         assert!(
            proof.verify(),
            "frozen/live proof must verify for {note}"
         );
         let leaf: ruint::aliases::U256 = note.hash().into();
         assert_eq!(proof.element, leaf);
      }
      assert!(
         !indexer.utxo_trees.contains_key(&0)
            && !indexer.utxo_trees.contains_key(&2)
            && !indexer.utxo_trees.contains_key(&3)
      );
      assert!(indexer.utxo_trees.contains_key(&4));

      let raw = db.get_utxo_tree_leaves(0).await.unwrap().unwrap().0;
      let rebuilt0 = UtxoMerkleTree::from_leaves(0, raw);
      assert_proof_matches_tree(&rebuilt0, &note0, &witnesses.proofs[&(0, 100)]);

      let sealed = db.get_utxo_note_proof(0, 100).await.unwrap().unwrap();
      assert_eq!(
         sealed.elements,
         witnesses.proofs[&(0, 100)].elements
      );

      indexer.save(false).await.unwrap();
      assert_eq!(
         indexer.db.get_utxo_indexer().await.unwrap().trees,
         vec![0, 1, 2, 3, 4]
      );

      drop(indexer);
      let mut indexer = indexer_from_db(db.clone()).await;
      assert_eq!(indexer.resident_trees(), vec![4]);
      indexer.register(signer_a.clone()).await.unwrap();
      indexer.register(signer_b.clone()).await.unwrap();
      assert_eq!(indexer.resident_trees(), vec![4]);
      let reloaded = indexer.merkle_witnesses(&[note0.clone()]).unwrap();
      assert!(reloaded.proofs[&(0, 100)].verify());
      assert!(!indexer.utxo_trees.contains_key(&0));

      let before0 = indexer.tree_summary(0).unwrap();
      indexer.insert_sorted_leaves(
         4,
         vec![(256, dummy_leaf(9_001)), (257, dummy_leaf(9_002))],
      );
      assert_eq!(indexer.resident_trees(), vec![4]);
      assert_eq!(
         indexer.utxo_trees.get(&4).unwrap().leaves_len(),
         258
      );
      assert_eq!(
         indexer.tree_summary(0).unwrap().leaf_count,
         before0.leaf_count
      );
      assert_eq!(
         indexer.tree_summary(0).unwrap().root,
         before0.root
      );

      indexer.insert_sorted_leaves(5, vec![(0, dummy_leaf(50_000))]);
      indexer.save(true).await.unwrap();
      indexer.compact_utxo_trees().await.unwrap();
      assert_eq!(indexer.open_tree_number(), Some(5));
      assert_eq!(
         indexer.resident_trees(),
         vec![5],
         "opening tree 5 seals tree 4; freeze then unload"
      );
      let after_roll = indexer.merkle_witnesses(&[note0.clone(), note4.clone()]).unwrap();
      assert!(after_roll.proofs[&(0, 100)].verify());
      assert!(after_roll.proofs[&(4, 10)].verify());
      assert!(!indexer.utxo_trees.contains_key(&4));

      indexer.retain_accounts(&[signer_a.address().clone()]).await.unwrap();
      assert!(indexer.merkle_witnesses(&[note0.clone()]).is_ok());
      assert!(
         !indexer.utxo_trees.contains_key(&2),
         "dropping signer B must not reload their sealed tree"
      );

      let outs = vec![OutputNote::Unshield(UnshieldNote::new(
         Address::from([0xAB; 20]),
         test_asset(),
         note0.value(),
      ))];
      let inputs = TransactCircuitInputs::from_notes_and_proofs(
         after_roll.roots[&0],
         Ru256::from(1u64),
         &signer_a,
         test_asset(),
         &[note0.clone()],
         &outs,
         &[after_roll.proofs[&(0, 100)].clone()],
      )
      .unwrap();
      assert_eq!(inputs.nullifiers.len(), 1);
      assert_eq!(inputs.commitments_out.len(), 1);

      if let Some(circuit) = try_01x01_circuit() {
         let prover = Groth16Prover::new(None).with_embedded_circuits([circuit]);
         let mut rng = ChaCha12Rng::from_os_rng();
         let proved = TransactionBuilder::new()
            .unshield(
               signer_a.clone(),
               Address::from([0xAB; 20]),
               test_asset(),
               note0.value(),
            )
            .unwrap()
            .build(
               &prover,
               1,
               &[note0.clone()],
               &indexer.merkle_witnesses(&[note0.clone()]).unwrap(),
               &mut rng,
            )
            .await
            .expect("Groth16 prove from frozen sealed-tree witness");
         assert_eq!(proved.len(), 1);
         assert_eq!(proved[0].inner.utxo_tree_number, 0);
      } else {
         eprintln!(
            "skipping Groth16: embedded/railgun/01x01 artifacts not found next to the crate"
         );
      }
   }

   #[tokio::test]
   async fn dirty_sealed_tree_stays_resident_until_save() {
      let db = RedbDatabase::in_memory(RailgunDbKey::generate().unwrap()).unwrap();
      persist_tree(&db, 0, 16).await;
      persist_tree(&db, 1, 8).await;
      db.set_utxo_indexer(&UtxoIndexerState {
         synced_block: 1,
         trees: vec![0, 1],
      })
      .await
      .unwrap();

      let mut indexer = indexer_from_db(db).await;
      indexer.ensure_tree_loaded(0, false).await.unwrap();
      indexer.insert_sorted_leaves(0, vec![(16, dummy_leaf(99))]);
      indexer.compact_utxo_trees().await.unwrap();
      assert!(
         indexer.utxo_trees.contains_key(&0),
         "dirty tree 0 must not unload before save"
      );

      indexer.save(true).await.unwrap();
      indexer.compact_utxo_trees().await.unwrap();
      assert_eq!(indexer.resident_trees(), vec![1]);
      assert_eq!(indexer.known_tree_numbers(), vec![0, 1]);
   }

   #[tokio::test]
   async fn missing_disk_tree_errors_and_live_proof_used_for_open_tree() {
      let db = RedbDatabase::in_memory(RailgunDbKey::generate().unwrap()).unwrap();
      let signer = test_signer();
      let note = make_note(&signer, 0, 0, 5);
      persist_tree_leaves(&db, 0, &[note.hash()]).await;
      db.set_utxo_indexer(&UtxoIndexerState {
         synced_block: 1,
         trees: vec![0],
      })
      .await
      .unwrap();
      db.set_account(
         signer.address(),
         &IndexedAccountState {
            notes: vec![note_record(note.clone())],
            synced_block: 1,
            spent_notes: vec![],
         },
      )
      .await
      .unwrap();

      let mut indexer = indexer_from_db(db).await;
      indexer.register(signer).await.unwrap();
      assert_eq!(indexer.resident_trees(), vec![0]);
      let w = indexer.merkle_witnesses(&[note.clone()]).unwrap();
      assert!(w.proofs[&(0, 0)].verify());
      assert!(matches!(
         indexer.ensure_tree_loaded(9, false).await,
         Err(UtxoIndexerError::MissingTree(9))
      ));
   }
}
