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
   abi::railgun::RailgunSmartWallet,
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
   rpc_syncer: Arc<dyn UtxoSyncer>,
   subsquid_syncer: Option<Arc<dyn UtxoSyncer>>,
   utxo_verifier: Arc<dyn MerkleTreeVerifier>,
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

   /// Returns the latest synced block
   pub fn synced_block(&self) -> u64 {
      let mut min_synced = self.synced_block;
      for account in self.accounts.iter() {
         min_synced = min_synced.min(account.synced_block());
      }
      min_synced
   }

   /// Registers a signer with the indexer. The indexer will track UTXOs for the associated
   /// address.
   ///
   /// The account state (including any previously decrypted notes and its synced_block)
   /// is loaded from the database. We immediately persist the (possibly loaded) account
   /// state so that progress for this account survives process restarts.
   pub async fn register(&mut self, signer: RailgunSigner) -> Result<(), UtxoIndexerError> {
      // Borrow only for the lookup; the address value is owned after this statement.
      let state = self.db.get_account(&signer.address()).await?;

      let account = IndexedAccount::from_state(signer, state);
      let addr = account.address();

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
      _deployment_block: u64,
      use_subsquid: bool,
   ) -> Result<(), UtxoIndexerError> {
      let from_block = self.synced_block() + 1;

      info!(
         "Effective sync range for utxo: from_block={} to_block={} use_subsquid={}",
         from_block, to_block, use_subsquid
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
         self.handle_event(&event, &mut tree_leaves)?;
      }

      info!("Inserting leaves into UTXO trees");
      for (tree_number, mut leaves) in tree_leaves {
         leaves.sort_by_key(|(idx, _)| *idx);
         let start = leaves[0].0;
         let hashes: Vec<_> = leaves.into_iter().map(|(_, hash)| hash).collect();

         self
            .utxo_trees
            .entry(tree_number)
            .or_insert(UtxoMerkleTree::new(tree_number))
            .insert_leaves(&hashes, start as usize);
      }

      for (tn, tree) in &self.utxo_trees {
         info!(
            "Tree {} now has {} leaves (root={})",
            tn,
            tree.leaves_len(),
            tree.root()
         );
      }

      // Verify against latest (root history is append-only / immutable once written)
      info!("Verifying UTXO trees");
      self.verify(None).await?;

      info!("Synced to block {}", to_block);
      self.synced_block = to_block;
      for account in self.accounts.iter_mut() {
         account.set_synced_block(to_block);
      }

      // Save
      self.save().await?;

      match self.db.compact().await {
         Ok(true) => info!("Database compaction performed"),
         Ok(false) => {
            info!("Database does need compaction");
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
      }

      let mut tree_leaves: HashMap<u32, Vec<(u32, UtxoLeafHash)>> = HashMap::new();
      for (_, event) in events.iter().enumerate() {
         self.handle_event(&event, &mut tree_leaves)?;
      }

      for (tree_number, mut leaves) in tree_leaves {
         leaves.sort_by_key(|(idx, _)| *idx);
         let start = leaves[0].0;
         let hashes: Vec<_> = leaves.into_iter().map(|(_, hash)| hash).collect();

         self
            .utxo_trees
            .entry(tree_number)
            .or_insert(UtxoMerkleTree::new(tree_number))
            .insert_leaves(&hashes, start as usize);
      }

      Ok(())
   }

   fn handle_event(
      &mut self,
      event: &SyncEvent,
      tree_leaves: &mut HashMap<u32, Vec<(u32, UtxoLeafHash)>>,
   ) -> Result<(), UtxoIndexerError> {
      match event {
         SyncEvent::Shield(shield, _) => self.handle_shield(shield, tree_leaves)?,
         SyncEvent::Transact(transact, _) => self.handle_transact(transact, tree_leaves)?,
         SyncEvent::Nullified(nullified, ts) => self.handle_nullified(nullified, *ts),
         SyncEvent::Legacy(legacy, _) => self.handle_legacy(legacy, tree_leaves),
      };

      Ok(())
   }

   fn handle_shield(
      &mut self,
      event: &syncer::Shield,
      tree_leaves: &mut HashMap<u32, Vec<(u32, UtxoLeafHash)>>,
   ) -> Result<(), UtxoIndexerError> {
      tree_leaves
         .entry(event.tree_number)
         .or_default()
         .push((event.leaf_index, event.hash()));

      for account in self.accounts.iter_mut() {
         account.handle_shield_event(event)?;
      }

      Ok(())
   }

   fn handle_transact(
      &mut self,
      event: &syncer::Transact,
      tree_leaves: &mut HashMap<u32, Vec<(u32, UtxoLeafHash)>>,
   ) -> Result<(), UtxoIndexerError> {
      tree_leaves
         .entry(event.tree_number)
         .or_default()
         .push((event.leaf_index, event.hash.into()));

      for account in self.accounts.iter_mut() {
         account.handle_transact_event(event)?;
      }

      Ok(())
   }

   fn handle_nullified(&mut self, event: &syncer::Nullified, timestamp: u64) {
      for account in self.accounts.iter_mut() {
         account.handle_nullified_event(event, timestamp);
      }
   }

   fn handle_legacy(
      &mut self,
      _event: &syncer::LegacyCommitment,
      tree_leaves: &mut HashMap<u32, Vec<(u32, UtxoLeafHash)>>,
   ) {
      tree_leaves
         .entry(_event.tree_number)
         .or_default()
         .push((_event.leaf_index, _event.hash.into()));

      // TODO: Forward legacy to accounts
   }

   pub async fn verify(&self, block_id: Option<BlockId>) -> Result<(), UtxoIndexerError> {
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
