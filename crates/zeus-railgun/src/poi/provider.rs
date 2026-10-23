use std::{collections::HashMap, sync::Arc};
use tokio::sync::RwLock;

use alloy_primitives::ChainId;
use ruint::aliases::U256;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::{
   circuit::{
      groth16_prover::Groth16Prover,
      inputs::poi_inputs::{PoiCircuitInputs, PoiCircuitInputsError},
   },
   crypto::{
      keys::{NullifyingKey, SpendingPublicKey},
      railgun_txid::Txid,
   },
   database::{DatabaseError, RedbDatabase},
   indexer::{
      syncer::SubsquidSyncer,
      txid_indexer::{TxidIndexer, TxidIndexerError},
   },
   merkle_tree::{RailgunMerkleProof, TOTAL_LEAVES, TxidMerkleTree, UtxoTreeIndex},
   note::utxo::{self, UtxoNote},
   poi::{
      client::{PoiClient, PoiClientError},
      note::PoiNote,
      types::{BlindedCommitment, BlindedCommitmentType, ListKey, PoiStatus, TransactProofData},
   },
   transact::proved_transaction::ProvedOperation,
};

#[derive(Clone)]
pub struct PoiProvider {
   inner: Arc<RwLock<PoiProviderState>>,
   db: RedbDatabase,
   poi_client: PoiClient,
   txid_indexer: Arc<RwLock<TxidIndexer>>,
}

#[derive(Serialize, Deserialize, Default)]
#[non_exhaustive]
pub struct PoiProviderState {
   pub pending: Vec<PendingPoiEntry>,
   pub pois: HashMap<BlindedCommitment, HashMap<ListKey, PoiInfo>>,
}

#[derive(Debug)]
#[non_exhaustive]
pub enum PoiProviderError {
   TxidIndexer(TxidIndexerError),
   PoiClient(PoiClientError),
   ProofNotFound(BlindedCommitment, ListKey),
   Database(DatabaseError),
}

impl std::fmt::Display for PoiProviderError {
   fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
      match self {
         Self::TxidIndexer(err) => write!(f, "Txid indexer error: {err}"),
         Self::PoiClient(err) => write!(f, "POI Client error: {err}"),
         Self::ProofNotFound(blinded_commitment, list_key) => write!(
            f,
            "Merkle proof not found for blinded commitment {blinded_commitment} and list key {list_key}"
         ),
         Self::Database(err) => write!(f, "Database error: {err}"),
      }
   }
}

impl std::error::Error for PoiProviderError {
   fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
      match self {
         Self::TxidIndexer(err) => Some(err),
         Self::PoiClient(err) => Some(err),
         Self::Database(err) => Some(err),
         Self::ProofNotFound(..) => None,
      }
   }
}

impl From<TxidIndexerError> for PoiProviderError {
   fn from(err: TxidIndexerError) -> Self {
      Self::TxidIndexer(err)
   }
}

impl From<PoiClientError> for PoiProviderError {
   fn from(err: PoiClientError) -> Self {
      Self::PoiClient(err)
   }
}

impl From<DatabaseError> for PoiProviderError {
   fn from(err: DatabaseError) -> Self {
      Self::Database(err)
   }
}

#[derive(Clone, Serialize, Deserialize, Default)]
pub struct PoiInfo {
   status: Option<PoiStatus>,
   proof: Option<RailgunMerkleProof>,
}

/// Serializable snapshot needed to re-prove and submit a post-transaction POI
/// proof to the POI aggregator.
///
/// TODO: Consider privacy / security implications of storing this data on disk.
#[derive(Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PendingPoiEntry {
   pub txid: Txid,
   pub spending_pubkey: SpendingPublicKey,
   pub nullifying_key: NullifyingKey,
   pub utxo_tree_in: u32,
   pub bound_params_hash: U256,
   /// Input UTXO notes. Fresh POI proofs are re-fetched at process time.
   pub in_notes: Vec<UtxoNote>,
   pub out_commitments: Vec<U256>,
   pub out_npks: Vec<U256>,
   pub out_values: Vec<U256>,
   pub token_hash: U256,
   pub has_unshield: bool,
   pub list_keys: Vec<ListKey>,
}

#[derive(Debug)]
enum PendingPoiError {
   PoiClient(PoiClientError),
   CircuitInputs(PoiCircuitInputsError),
   Prover(Box<dyn std::error::Error + Send + Sync>),
   MissingTxid(Txid),
   MissingUtxoTree(u32),
   MissingTxidTree(u32),
}

impl std::fmt::Display for PendingPoiError {
   fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
      match self {
         Self::PoiClient(err) => write!(f, "POI client error: {err}"),
         Self::CircuitInputs(err) => write!(f, "Circuit inputs error: {err}"),
         Self::Prover(err) => write!(f, "Prover error: {err}"),
         Self::MissingTxid(txid) => write!(f, "Missing txid position for txid {txid:?}"),
         Self::MissingUtxoTree(tree_number) => write!(f, "Missing UTXO tree {tree_number}"),
         Self::MissingTxidTree(tree_number) => write!(f, "Missing TXID tree {tree_number}"),
      }
   }
}

impl std::error::Error for PendingPoiError {
   fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
      match self {
         Self::PoiClient(err) => Some(err),
         Self::CircuitInputs(err) => Some(err),
         // `Prover` deliberately exposes no source although its payload is an error:
         // the previous derive carried no `#[source]`, so `None` here is the behaviour
         // this impl preserves.
         Self::Prover(_)
         | Self::MissingTxid(_)
         | Self::MissingUtxoTree(_)
         | Self::MissingTxidTree(_) => None,
      }
   }
}

impl From<PoiClientError> for PendingPoiError {
   fn from(err: PoiClientError) -> Self {
      Self::PoiClient(err)
   }
}

impl From<PoiCircuitInputsError> for PendingPoiError {
   fn from(err: PoiCircuitInputsError) -> Self {
      Self::CircuitInputs(err)
   }
}

#[cfg(test)]
mod poi_provider_error_tests {
   use std::error::Error as _;

   use super::*;

   fn txid_indexer_error() -> TxidIndexerError {
      TxidIndexerError::RootMismatch { tree_number: 3 }
   }

   fn poi_client_error() -> PoiClientError {
      PoiClientError::NullResult
   }

   fn database_error() -> DatabaseError {
      DatabaseError::MissingCryptoKey
   }

   #[test]
   fn renders_the_pinned_messages() {
      assert_eq!(
         PoiProviderError::TxidIndexer(txid_indexer_error()).to_string(),
         "Txid indexer error: TXID tree root mismatch for tree 3"
      );
      assert_eq!(
         PoiProviderError::PoiClient(poi_client_error()).to_string(),
         "POI Client error: Null result from RPC"
      );

      let commitment = BlindedCommitment::from(U256::from(7));
      let list_key = ListKey::from("0xabc");
      assert_eq!(
         PoiProviderError::ProofNotFound(commitment, list_key.clone()).to_string(),
         format!(
            "Merkle proof not found for blinded commitment {commitment} and list key {list_key}"
         )
      );

      assert_eq!(
         PoiProviderError::Database(database_error()).to_string(),
         "Database error: Missing Railgun DB crypto key"
      );
   }

   #[test]
   fn the_from_variants_delegate_their_source() {
      let inner = txid_indexer_error();
      let rendered = inner.to_string();
      assert_eq!(
         PoiProviderError::TxidIndexer(inner).source().map(ToString::to_string),
         Some(rendered)
      );

      let inner = poi_client_error();
      let rendered = inner.to_string();
      assert_eq!(
         PoiProviderError::PoiClient(inner).source().map(ToString::to_string),
         Some(rendered)
      );

      let inner = database_error();
      let rendered = inner.to_string();
      assert_eq!(
         PoiProviderError::Database(inner).source().map(ToString::to_string),
         Some(rendered)
      );
   }

   #[test]
   fn the_proof_not_found_variant_has_no_source() {
      let err = PoiProviderError::ProofNotFound(
         BlindedCommitment::from(U256::from(7)),
         ListKey::from("0xabc"),
      );
      assert!(err.source().is_none());
   }
}

#[cfg(test)]
mod pending_poi_error_tests {
   use std::error::Error as _;

   use super::*;

   fn poi_client_error() -> PoiClientError {
      PoiClientError::NullResult
   }

   fn circuit_inputs_error() -> PoiCircuitInputsError {
      PoiCircuitInputsError::MissingPoiProofs(ListKey::from("0xabc"))
   }

   #[test]
   fn renders_the_pinned_messages() {
      assert_eq!(
         PendingPoiError::PoiClient(poi_client_error()).to_string(),
         "POI client error: Null result from RPC"
      );
      assert_eq!(
         PendingPoiError::CircuitInputs(circuit_inputs_error()).to_string(),
         "Circuit inputs error: Missing POI proofs for list key ListKey(0xabc)"
      );
      assert_eq!(
         PendingPoiError::Prover("boom".into()).to_string(),
         "Prover error: boom"
      );

      let txid = Txid::from(U256::from(7));
      assert_eq!(
         PendingPoiError::MissingTxid(txid).to_string(),
         format!("Missing txid position for txid {txid:?}")
      );

      assert_eq!(
         PendingPoiError::MissingUtxoTree(5).to_string(),
         "Missing UTXO tree 5"
      );
      assert_eq!(
         PendingPoiError::MissingTxidTree(6).to_string(),
         "Missing TXID tree 6"
      );
   }

   #[test]
   fn the_from_variants_delegate_their_source() {
      let inner = poi_client_error();
      let rendered = inner.to_string();
      assert_eq!(
         PendingPoiError::PoiClient(inner).source().map(ToString::to_string),
         Some(rendered)
      );

      let inner = circuit_inputs_error();
      let rendered = inner.to_string();
      assert_eq!(
         PendingPoiError::CircuitInputs(inner).source().map(ToString::to_string),
         Some(rendered)
      );
   }

   #[test]
   fn the_other_variants_have_no_source() {
      assert!(PendingPoiError::Prover("boom".into()).source().is_none());
      assert!(PendingPoiError::MissingTxid(Txid::from(U256::from(7))).source().is_none());
      assert!(PendingPoiError::MissingUtxoTree(5).source().is_none());
      assert!(PendingPoiError::MissingTxidTree(6).source().is_none());
   }
}

impl PoiProvider {
   pub async fn new(
      chain_id: ChainId,
      db: RedbDatabase,
      txid_syncer: SubsquidSyncer,
      poi_endpoint: impl Into<String>,
      list_keys: Vec<ListKey>,
   ) -> Result<Self, PoiProviderError> {
      let inner = db.get_poi_provider().await?;
      let txid_indexer = TxidIndexer::new(db.clone(), txid_syncer).await?;
      let poi_client = PoiClient::new(chain_id, poi_endpoint, list_keys);

      Ok(Self {
         inner: Arc::new(RwLock::new(inner)),
         db,
         poi_client,
         txid_indexer: Arc::new(RwLock::new(txid_indexer)),
      })
   }

   pub async fn sync_to(
      &mut self,
      prover: &Groth16Prover,
      to_block: u64,
   ) -> Result<(), PoiProviderError> {
      let poi_client = &self.poi_client;
      self.txid_indexer.write().await.sync_to(to_block, poi_client).await?;
      self.submit_pending(prover).await;
      self.save().await?;
      Ok(())
   }

   pub async fn register_ops(
      &mut self,
      operations: &[ProvedOperation],
   ) -> Result<(), PoiProviderError> {
      let list_keys = self.poi_client.list_keys();
      for op in operations {
         self.register(op, list_keys.clone()).await;
      }
      self.save().await?;
      Ok(())
   }

   pub fn list_keys(&self) -> Vec<ListKey> {
      self.poi_client.list_keys()
   }

   /// Returns the worst-case POI status across all configured list keys.
   pub async fn status(
      &mut self,
      blinded_commitment: BlindedCommitment,
      commitment_type: BlindedCommitmentType,
   ) -> Result<PoiStatus, PoiProviderError> {
      let mut worst = PoiStatus::Valid;
      for list_key in self.list_keys() {
         let status = self
            .poi_client
            .poi_status(&list_key, blinded_commitment, commitment_type)
            .await?;
         debug!(
            "POI status for {} ({list_key}): {status:?}",
            blinded_commitment
         );
         worst = worst.max(status);
      }
      Ok(worst)
   }

   async fn register(&mut self, op: &ProvedOperation, list_keys: Vec<ListKey>) {
      let spending_pubkey = op.inner.from.keys().spending_public_key;
      let txid = Txid::from_operation(op);
      let in_notes = op.inner.in_notes().to_vec();
      let out_notes = op.inner.out_notes();
      let encryptable_notes = op.inner.out_encryptable_notes();

      info!(
         "Registered POI for {:?}",
         op.circuit_inputs.bound_params_hash
      );
      self.inner.write().await.pending.push(PendingPoiEntry {
         txid,
         spending_pubkey,
         nullifying_key: op.inner.from.keys().nullifying_key,
         utxo_tree_in: op.inner.utxo_tree_number,
         bound_params_hash: op.circuit_inputs.bound_params_hash,
         in_notes,
         out_commitments: out_notes.iter().map(|n| n.hash().into()).collect(),
         out_npks: encryptable_notes.iter().map(|n| n.note_public_key()).collect(),
         out_values: encryptable_notes.iter().map(|n| U256::from(n.value())).collect(),
         token_hash: op.inner.asset.hash(),
         has_unshield: op.inner.unshield_note().is_some(),
         list_keys,
      });
   }

   async fn submit_pending(&mut self, prover: &Groth16Prover) {
      let mut inner = self.inner.write().await;
      for i in (0..inner.pending.len()).rev() {
         let entry = inner.pending[i].clone();
         match self.submit_poi(prover, &entry).await {
            Ok(_) => {
               info!("Submitted POI for {:?}", entry.txid);
               inner.pending.remove(i);
            }
            Err(PendingPoiError::MissingTxid(_)) => {
               info!("Waiting for txid to be indexed: {:?}", entry.txid);
            }
            Err(e) => {
               warn!("Failed to submit POI for pending entry: {:?}", e);
            }
         }
      }
   }

   async fn submit_poi(
      &self,
      prover: &Groth16Prover,
      entry: &PendingPoiEntry,
   ) -> Result<(), PendingPoiError> {
      let txid_indexer = self.txid_indexer.read().await;

      let txid_tree_number = match txid_indexer.txid_position(&entry.txid) {
         Some((tree_number, _)) => tree_number,
         None => return Err(PendingPoiError::MissingTxid(entry.txid)),
      };

      let (utxo_tree_number, utxo_leaf_index) = match txid_indexer.utxo_position(&entry.txid) {
         Some((tree_number, leaf_index)) => (tree_number, leaf_index),
         None => {
            return Err(PendingPoiError::MissingUtxoTree(
               entry.utxo_tree_in,
            ));
         }
      };

      let txid_tree = match txid_indexer.tree(txid_tree_number) {
         Some(tree) => tree,
         None => return Err(PendingPoiError::MissingTxidTree(txid_tree_number)),
      };

      let utxo_tree_out = UtxoTreeIndex::included(utxo_tree_number, utxo_leaf_index);

      let proof_data = self
         .create_proof(
            prover,
            entry,
            txid_tree_number,
            utxo_tree_number,
            utxo_leaf_index,
            txid_tree,
            utxo_tree_out,
         )
         .await?;

      self.poi_client.submit_proof(proof_data).await?;
      Ok(())
   }

   // One proof per pending entry needs the txid tree, the UTXO position and both trees at once; they
   // are all the inputs `PoiCircuitInputs::from_inputs` wants, kept positional to match the circuit.
   #[allow(clippy::too_many_arguments)]
   async fn create_proof(
      &self,
      prover: &Groth16Prover,
      entry: &PendingPoiEntry,
      txid_tree_number: u32,
      utxo_tree_number: u32,
      utxo_leaf_index: u32,
      txid_tree: &TxidMerkleTree,
      utxo_tree_out: UtxoTreeIndex,
   ) -> Result<HashMap<ListKey, TransactProofData>, PendingPoiError> {
      let mut proof_data = HashMap::new();

      for list_key in &entry.list_keys {
         let mut in_notes = Vec::new();
         for note in entry.in_notes.clone() {
            let proof =
               self.poi_client.merkle_proof(list_key, note.blinded_commitment.into()).await?;
            in_notes.push(PoiNote::new(
               note,
               HashMap::from([(list_key.clone(), proof)]),
            ));
         }

         let inputs = PoiCircuitInputs::from_inputs(
            entry.spending_pubkey,
            entry.nullifying_key,
            entry.utxo_tree_in,
            entry.bound_params_hash,
            &in_notes,
            &entry.out_commitments,
            &entry.out_npks,
            &entry.out_values,
            entry.token_hash,
            entry.has_unshield,
            list_key.clone(),
            utxo_tree_out,
            txid_tree,
         )?;

         let proof = prover
            .prove_poi(&inputs)
            .await
            .map_err(|e| PendingPoiError::Prover(Box::new(e)))?;
         let blinded_commitments_out =
            blinded_commitments(entry, utxo_tree_number, utxo_leaf_index);

         let txid_merkleroot_index =
            txid_tree_number as u64 * TOTAL_LEAVES as u64 + (txid_tree.leaves_len() as u64 - 1);

         proof_data.insert(
            list_key.clone(),
            TransactProofData {
               proof,
               poi_merkleroots: inputs.poi_merkleroots,
               txid_merkleroot: inputs.railgun_txid_merkleroot_after_transaction,
               txid_merkleroot_index,
               blinded_commitments_out,
               railgun_txid_if_has_unshield: inputs.railgun_txid_if_has_unshield,
            },
         );
      }
      Ok(proof_data)
   }

   async fn save(&self) -> Result<(), PoiProviderError> {
      let inner = self.inner.read().await;
      self.db.set_poi_provider(&inner).await?;
      Ok(())
   }
}

fn blinded_commitments(
   entry: &PendingPoiEntry,
   utxo_tree_number: u32,
   utxo_leaf_index: u32,
) -> Vec<BlindedCommitment> {
   let mut blinded_commitments_out = Vec::new();
   for (i, (commitment, npk)) in entry.out_commitments.iter().zip(entry.out_npks.iter()).enumerate()
   {
      let blinded_commitment = utxo::blinded_commitment(
         *commitment,
         *npk,
         utxo_tree_number,
         utxo_leaf_index + i as u32,
      )
      .into();
      blinded_commitments_out.push(blinded_commitment);
   }
   blinded_commitments_out
}
