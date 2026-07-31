use std::{
   collections::{BTreeSet, HashMap},
   sync::Arc,
};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{info, warn};

use crate::{
   crypto::railgun_txid::Txid,
   database::{
      Database, DatabaseError, RailgunDB, WriteBatch, WriteDurability,
      railgun_db::{
         all_chunk_indices, dirty_chunks_for_range, push_txid_tree_save, put_txid_indexer,
      },
   },
   indexer::syncer::{Operation, SyncerError, TxidSyncer},
   merkle_tree::{TOTAL_LEAVES, TxidLeafHash, TxidMerkleTree, UtxoTreeIndex},
   poi::client::{PoiClientError, PoiNodeClient},
};

pub struct TxidIndexer {
   trees: HashMap<u32, TxidMerkleTree>,
   inner: TxidIndexerState,

   /// Tree number → dirty leaf-chunk indices since last successful save.
   dirty_chunks: HashMap<u32, BTreeSet<u32>>,
   /// Trees loaded from legacy monolithic blobs; next save migrates fully to chunks.
   legacy_trees: BTreeSet<u32>,

   db: Arc<dyn Database>,
   txid_syncer: Arc<dyn TxidSyncer>,
}

#[derive(Serialize, Deserialize, Default)]
pub struct TxidIndexerState {
   pub synced_block: u64,
   pub trees: Vec<u32>,
   pub pending: Vec<Operation>,
   pub txid_to_utxo_position: HashMap<Txid, (u32, u32)>,
   pub txid_to_txid_position: HashMap<Txid, (u32, u32)>,
}

#[derive(Debug, Error)]
pub enum TxidIndexerError {
   #[error("Syncer error: {0}")]
   SyncerError(#[from] SyncerError),
   #[error("POI client error: {0}")]
   PoiClient(#[from] PoiClientError),
   #[error("TXID tree root mismatch for tree {tree_number}")]
   RootMismatch { tree_number: u32 },
   #[error("Database error: {0}")]
   DatabaseError(#[from] DatabaseError),
}

impl TxidIndexer {
   pub async fn new(
      db: Arc<dyn Database>,
      txid_syncer: Arc<dyn TxidSyncer>,
   ) -> Result<Self, TxidIndexerError> {
      let inner = db.get_txid_indexer().await?;

      let mut txid_trees = HashMap::new();
      let mut legacy_trees = BTreeSet::new();
      for number in inner.trees.clone() {
         if let Some((leaves, from_legacy)) = db.get_txid_tree_leaves(number).await? {
            if from_legacy {
               legacy_trees.insert(number);
            }
            txid_trees.insert(
               number,
               TxidMerkleTree::from_leaves(number, leaves),
            );
         }
      }

      info!(
         "Loaded Txid indexer state: synced_block={}, trees={:?}, pending_ops={}, legacy_migrate={}",
         inner.synced_block,
         inner.trees,
         inner.pending.len(),
         legacy_trees.len()
      );
      Ok(TxidIndexer {
         inner,
         trees: txid_trees,
         dirty_chunks: HashMap::new(),
         legacy_trees,
         db,
         txid_syncer,
      })
   }

   pub fn tree(&self, tree_number: u32) -> Option<&TxidMerkleTree> {
      self.trees.get(&tree_number)
   }

   pub fn txid_position(&self, txid: &Txid) -> Option<(u32, u32)> {
      self.inner.txid_to_txid_position.get(txid).cloned()
   }

   pub fn utxo_position(&self, txid: &Txid) -> Option<(u32, u32)> {
      self.inner.txid_to_utxo_position.get(txid).cloned()
   }

   fn mark_tree_range_dirty(&mut self, tree_number: u32, start: usize, end: usize) {
      let chunks = dirty_chunks_for_range(start, end);
      if chunks.is_empty() {
         return;
      }
      self.dirty_chunks.entry(tree_number).or_default().extend(chunks);
   }

   #[tracing::instrument(name = "txid_sync", skip_all)]
   pub async fn sync_to(
      &mut self,
      to_block: u64,
      poi_client: &impl PoiNodeClient,
   ) -> Result<(), TxidIndexerError> {
      let from_block = self.inner.synced_block + 1;

      let syncer = self.txid_syncer.clone();
      let latest_block = syncer.latest_block().await?;
      let to_block = to_block.min(latest_block);

      let ops = syncer.sync(from_block, to_block).await?;
      info!("Fetched {} operations from syncer", ops.len());
      for op in ops {
         self.inner.pending.push(op);
      }
      self.inner.synced_block = to_block;

      self.update(poi_client).await?;
      self.save().await?;
      Ok(())
   }

   #[tracing::instrument(name = "txid_update", skip_all)]
   async fn update(&mut self, poi_client: &impl PoiNodeClient) -> Result<(), TxidIndexerError> {
      let validated = poi_client.validated_txid().await?;
      info!(
         "Latest validated txid index from POI: tree {}, leaf {}",
         validated.tree(),
         validated.leaf_index()
      );

      let current_total = self.trees.values().map(|t| t.leaves_len() as u32).sum();
      let target_total = validated.tree() * TOTAL_LEAVES + validated.leaf_index() + 1;

      let to_drain = target_total.saturating_sub(current_total) as usize;
      if to_drain == 0 {
         return Ok(());
      }

      let drain_count = to_drain.min(self.inner.pending.len());
      let drained: Vec<_> = self.inner.pending.drain(..drain_count).collect();

      let mut total = current_total;
      let mut tree_leaves: HashMap<u32, Vec<(u32, TxidLeafHash)>> = HashMap::new();
      for op in drained {
         let txid = Txid::new(
            &op.nullifiers,
            &op.commitment_hashes,
            op.bound_params_hash,
         );

         if let Some(&existing_pos) = self.inner.txid_to_txid_position.get(&txid) {
            warn!(
               "Skipping duplicate operation: txid {:?} already at tree {}, leaf {}",
               txid, existing_pos.0, existing_pos.1
            );
            continue;
         }

         let included = UtxoTreeIndex::included(op.utxo_tree_out, op.utxo_out_start_index);
         let leaf = TxidLeafHash::new(txid, op.utxo_tree_in, included);

         let tree_number = total / TOTAL_LEAVES;
         let leaf_index = total % TOTAL_LEAVES;

         tree_leaves.entry(tree_number).or_default().push((leaf_index as u32, leaf));

         self.inner.txid_to_txid_position.insert(txid, (tree_number, leaf_index as u32));
         self
            .inner
            .txid_to_utxo_position
            .insert(txid, (op.utxo_tree_out, op.utxo_out_start_index));

         if total % 10000 == 0 {
            info!(
               "Draining operation {}/{}",
               total - current_total,
               target_total
            );
         }
         total += 1;
      }

      info!("Inserting leaves into TXID trees");
      for (tree_number, mut leaves) in tree_leaves {
         leaves.sort_by_key(|(idx, _)| *idx);
         let start = leaves[0].0 as usize;
         let end = leaves.last().map(|(i, _)| *i as usize + 1).unwrap_or(start);
         let hashes: Vec<_> = leaves.into_iter().map(|(_, hash)| hash).collect();
         self
            .trees
            .entry(tree_number)
            .or_insert_with(|| TxidMerkleTree::new(tree_number))
            .insert_leaves(&hashes, start);
         self.mark_tree_range_dirty(tree_number, start, end);
      }
      info!("Drained {} operations", drain_count);

      info!("Validating TXID trees");
      for (tree_number, tree) in self.trees.iter() {
         let index = tree.leaves_len() as u32 - 1;
         let merkleroot = tree.root();
         let validated =
            poi_client.validate_txid_merkleroot(*tree_number, index, merkleroot).await?;

         if !validated {
            return Err(TxidIndexerError::RootMismatch {
               tree_number: *tree_number,
            });
         }

         info!(
            "Validated TXID tree up to tree {}, leaf {} (total {})",
            tree_number,
            index,
            total - 1
         );
      }

      Ok(())
   }

   async fn save(&mut self) -> Result<(), DatabaseError> {
      let mut batch = WriteBatch::new();

      let state = TxidIndexerState {
         synced_block: self.inner.synced_block,
         trees: self.trees.keys().cloned().collect(),
         pending: self.inner.pending.clone(),
         txid_to_utxo_position: self.inner.txid_to_utxo_position.clone(),
         txid_to_txid_position: self.inner.txid_to_txid_position.clone(),
      };
      put_txid_indexer(&mut batch, &state)?;

      // Indexer state holds pending ops / position maps always Immediate.
      // Only rewrite dirty trees, legacy trees migrate when first dirtied.
      let trees_to_write: Vec<u32> = self.dirty_chunks.keys().copied().collect();
      let mut migrated = Vec::new();

      for tree_number in trees_to_write {
         let Some(tree) = self.trees.get(&tree_number) else {
            continue;
         };
         let migrate = self.legacy_trees.contains(&tree_number);
         let dirty = if migrate {
            all_chunk_indices(tree.leaves_len())
         } else {
            self.dirty_chunks.get(&tree_number).cloned().unwrap_or_default()
         };
         if dirty.is_empty() && !migrate {
            continue;
         }
         push_txid_tree_save(
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

      self.db.apply_batch(batch, WriteDurability::Immediate).await?;

      self.dirty_chunks.clear();
      Ok(())
   }
}
