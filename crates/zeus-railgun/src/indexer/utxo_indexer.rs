use std::{
   collections::{BTreeMap, HashMap},
   sync::Arc,
   u64,
};

use alloy_primitives::{Log, U256};
use alloy_rpc_types::BlockId;
use alloy_sol_types::SolEvent;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::info;

use crate::{
   abi::{legacy::RailgunLegacy, railgun::RailgunSmartWallet},
   account::{address::RailgunAddress, signer::RailgunSigner},
   database::{Database, DatabaseError, RailgunDB},
   indexer::{
      indexed_account::IndexedAccount,
      syncer::{self, SyncEvent, SyncerError, UtxoSyncer},
   },
   merkle_tree::{MerkleTreeVerifier, UtxoLeafHash, UtxoMerkleTree},
   note::utxo::{NoteError, UtxoNote},
};

/// Utxo indexer that maintains the set of UTXO merkle trees and tracks accounts
/// and account notes / balances.
pub struct UtxoIndexer {
   synced_block: u64,
   pub utxo_trees: BTreeMap<u32, UtxoMerkleTree>,
   accounts: Vec<IndexedAccount>,

   db: Arc<dyn Database>,
   pub rpc_syncer: Arc<dyn UtxoSyncer>,
   subsquid_syncer: Option<Arc<dyn UtxoSyncer>>,
   pub utxo_verifier: Arc<dyn MerkleTreeVerifier>,
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
}

impl UtxoIndexer {
   pub async fn new(
      db: Arc<dyn Database>,
      rpc_syncer: Arc<dyn UtxoSyncer>,
      subsquid_syncer: Option<Arc<dyn UtxoSyncer>>,
      utxo_verifier: Arc<dyn MerkleTreeVerifier>,
   ) -> Result<Self, UtxoIndexerError> {
      let state = db.get_utxo_indexer().await?;

      let mut utxo_trees = BTreeMap::new();
      for number in state.trees.clone() {
         let tree_state = db.get_utxo_tree(number).await?;
         if let Some(tree_state) = tree_state {
            utxo_trees.insert(number, UtxoMerkleTree::from_state(tree_state));
         }
      }

      info!(
         "Loaded UTXO indexer state: synced_block={}, trees={:?}",
         state.synced_block, state.trees
      );
      Ok(UtxoIndexer {
         synced_block: state.synced_block,
         utxo_trees,
         accounts: vec![],
         db,
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
   pub fn account_synced_block(&self) -> u64 {
      let mut min_synced = self.synced_block;
      for account in self.accounts.iter() {
         min_synced = min_synced.min(account.synced_block());
      }
      min_synced
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
      if self.accounts.iter().any(|a| a.address() == addr) {
         return Ok(());
      }

      let state = self.db.get_account(&addr).await?;
      let account = IndexedAccount::from_state(signer, state);

      // Persist right away. This records the account's notes + its per-account synced_block on disk.
      self.db.set_account(&addr, &account.state()).await?;
      self.accounts.push(account);
      Ok(())
   }

   /// Lists all registered accounts
   pub fn registered(&self) -> Vec<RailgunAddress> {
      self.accounts.iter().map(|a| a.address()).collect()
   }

   /// Lists all unspent notes for a given address. Returns an empty list if the address is not
   /// registered.
   pub fn unspent(&self, address: RailgunAddress) -> Vec<UtxoNote> {
      for account in self.accounts.iter() {
         if account.address() == address {
            return account.unspent();
         }
      }

      vec![]
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

      info!(
         "Effective sync range for utxo: from_block={} to_block={} use_subsquid={} global_synced={} tree_from={} account_from={} accounts={}",
         from_block,
         to_block,
         use_subsquid,
         global_synced,
         tree_from,
         account_from,
         self.accounts.len()
      );

      let syncer: Arc<dyn UtxoSyncer> = if use_subsquid {
         self.subsquid_syncer.clone().ok_or_else(|| {
            SyncerError::new(std::io::Error::new(
               std::io::ErrorKind::Other,
               "subsquid syncer not configured",
            ))
         })?
      } else {
         self.rpc_syncer.clone()
      };

      let latest_block = syncer.latest_block().await?;

      let to_block = to_block.min(latest_block);

      if from_block > to_block {
         return Ok(());
      }

      // Sync
      let events = syncer.sync(from_block, to_block).await?;
      info!("Fetched {} events from syncer", events.len());

      let mut tree_leaves: HashMap<u32, Vec<(u32, UtxoLeafHash)>> = HashMap::new();
      for (i, event) in events.iter().enumerate() {
         if i % 20000 == 0 {
            info!("Processing event {}/{}", i, events.len());
         }
         // Only mutate trees for blocks the global indexer has not applied yet.
         let apply_tree = event.block_number() > global_synced;
         self.handle_event(event, &mut tree_leaves, apply_tree)?;
      }

      let trees_mutated = !tree_leaves.is_empty();

      if trees_mutated {
         info!("Inserting leaves into UTXO trees");
         for (tree_number, mut leaves) in tree_leaves {
            leaves.sort_by_key(|(idx, _)| *idx);
            // Exact leaf indices — dense packing from min index corrupts gapped/legacy ranges
            // and also corrupts a re-scan that is not the full leaf set.
            let tree = self
               .utxo_trees
               .entry(tree_number)
               .or_insert_with(|| UtxoMerkleTree::new(tree_number));
            for (leaf_index, hash) in leaves {
               tree.insert_leaves(&[hash], leaf_index as usize);
            }
         }
      } else {
         info!("No new tree leaves (account catch-up and/or empty delta)");
      }

      for (tn, tree) in &self.utxo_trees {
         info!(
            "Tree {} now has {} leaves (root={})",
            tn,
            tree.leaves_len(),
            tree.root()
         );
      }

      // Verify only when trees changed. Account-only catch-up must not risk failing
      // on an unrelated tree state, and root history is immutable once written.
      if trees_mutated {
         info!("Verifying UTXO trees");
         self.verify(None).await?;
      }

      info!(
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

      // Save
      self.save().await?;

      match self.db.compact().await {
         Ok(true) => info!("Database compaction performed"),
         Ok(false) => {
            info!("Database does not need compaction");
         }
         Err(e) => tracing::warn!("Compaction failed: {}", e),
      }

      Ok(())
   }

   /// Syncs the indexer directly from logs
   ///
   /// This should be used only for evm simulations on a new instance of the indexer
   pub fn sync_from_logs(
      &mut self,
      logs: Vec<Log>,
      block: u64,
      timestamp: u64,
   ) -> Result<(), UtxoIndexerError> {
      let mut events = Vec::new();

      for log in logs {
         if let Ok(decoded) = <RailgunSmartWallet::Shield as SolEvent>::decode_log(&log) {
            let mut shield_events = super::parse_shield(&decoded.data, block)?;
            events.append(&mut shield_events);
            continue;
         }

         if let Ok(decoded) = <RailgunSmartWallet::Transact as SolEvent>::decode_log(&log) {
            let mut tx_events = super::parse_transact(&decoded.data, timestamp)?;
            events.append(&mut tx_events);
            continue;
         }

         if let Ok(decoded) = <RailgunSmartWallet::Nullified as SolEvent>::decode_log(&log) {
            let mut null_events = super::parse_nullified(&decoded.data, timestamp)?;
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
            let mut null_events = super::parse_legacy_nullifiers(&decoded.data, timestamp)?;
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
            let mut tx_events = super::parse_legacy_transact(&decoded.data, timestamp)?;
            events.append(&mut tx_events);
            continue;
         }

         if let Ok(decoded) = <RailgunLegacy::Shield as SolEvent>::decode_log(&log) {
            let mut shield_events = super::parse_legacy_shield(&decoded.data, block)?;
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

      for (tree_number, mut leaves) in tree_leaves {
         leaves.sort_by_key(|(idx, _)| *idx);
         let tree = self
            .utxo_trees
            .entry(tree_number)
            .or_insert_with(|| UtxoMerkleTree::new(tree_number));
         for (leaf_index, hash) in leaves {
            tree.insert_leaves(&[hash], leaf_index as usize);
         }
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
         SyncEvent::Nullified(nullified, ts) => self.handle_nullified(nullified, *ts, block),
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
            account.handle_shield_event(event)?;
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
            account.handle_transact_event(event)?;
         }
      }

      Ok(())
   }

   fn handle_nullified(&mut self, event: &syncer::Nullified, timestamp: u64, block: u64) {
      for account in self.accounts.iter_mut() {
         if block > account.synced_block() {
            account.handle_nullified_event(event, timestamp);
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
                  // Ignore decryption failures (not our note) — same pattern as shield/transact
                  if !matches!(e, NoteError::Aes(_)) {
                     tracing::debug!("Legacy note handling error: {}", e);
                  }
               }
            }
         }
      }
   }

   pub async fn verify(&self, block_id: Option<BlockId>) -> Result<(), UtxoIndexerError> {
      // TODO: Make this a batch call
      for tree in self.utxo_trees.values() {
         if tree.leaves_len() == 0 {
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
   async fn save(&self) -> Result<(), DatabaseError> {
      let state = UtxoIndexerState {
         synced_block: self.synced_block,
         trees: self.utxo_trees.keys().cloned().collect(),
      };
      self.db.set_utxo_indexer(&state).await?;

      for (tree_number, tree) in self.utxo_trees.iter() {
         self.db.set_utxo_tree(*tree_number, tree.state()).await?;
      }

      for account in self.accounts.iter() {
         self.db.set_account(&account.address(), &account.state()).await?;
      }

      Ok(())
   }
}
