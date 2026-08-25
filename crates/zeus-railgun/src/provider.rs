use std::{collections::HashMap, sync::Arc};
use tokio::sync::RwLock;

use alloy_primitives::{Address, B256, Bytes, Log, U256};
use alloy_provider::{Provider, network::Ethereum};
use alloy_rpc_types::{BlockId, TransactionRequest};
use alloy_sol_types::SolCall;

use anyhow::anyhow;
use rand::Rng;
use serde::Serialize;
use thiserror::Error;
use tracing::{debug, warn};
use userop_kit::{
   builder::UserOperationBuilder,
   bundler::{Bundler, BundlerError},
   signable_user_operation::SignableUserOperation,
   smart_account::SmartAccount,
};

use crate::{
   account::{address::RailgunAddress, signer::RailgunSigner},
   adapter_data::{encode_paymaster_data, encode_railgun_adapter_data, paymaster_railgun_address},
   caip::AssetId,
   chain_config::ChainConfig,
   circuit::groth16_prover::Groth16Prover,
   database::DatabaseError,
   indexer::{
      indexed_account::{PrivateHistoryEntry, SpentNote},
      utxo_indexer::{UtxoIndexer, UtxoIndexerError},
   },
   note::{OutputNote, utxo::UtxoNote},
   poi::{
      provider::{PoiProvider, PoiProviderError},
      types::{BlindedCommitmentType, PoiStatus},
   },
   transact::{
      ShieldBuilder, TransactionBuilder, TransactionBuilderError,
      proved_transaction::{ProvedOperation, ProvedTx},
   },
   types::Chain,
};

#[derive(Debug, Serialize)]
pub struct BalanceEntry {
   pub asset: AssetId,
   /// If POI is enabled, the spendability status of the note according to the POI provider.
   /// Otherwise None.
   #[serde(rename = "poiStatus")]
   pub poi_status: Option<PoiStatus>,
   pub amount: u128,
}

#[derive(Debug, Clone, Serialize)]
pub struct NoteEntry {
   pub asset: AssetId,
   /// If POI is enabled, the spendability status of the note according to the POI provider.
   /// Otherwise None.
   #[serde(rename = "poiStatus")]
   pub poi_status: Option<PoiStatus>,
   pub amount: u128,
   #[serde(rename = "treeNumber")]
   pub tree_number: u32,
   #[serde(rename = "leafIndex")]
   pub leaf_index: u32,
   #[serde(rename = "blindedCommitment")]
   pub blinded_commitment: String,
   #[serde(rename = "commitmentType")]
   pub commitment_type: BlindedCommitmentType,
   pub memo: String,
}

impl NoteEntry {
   fn from_note(note: UtxoNote, poi_status: Option<PoiStatus>) -> Self {
      Self {
         asset: note.asset(),
         poi_status,
         amount: note.value(),
         tree_number: note.tree_number,
         leaf_index: note.leaf_index,
         blinded_commitment: format!("0x{:064x}", note.blinded_commitment),
         commitment_type: note.commitment_type,
         memo: note.memo,
      }
   }
}

/// A spent UTXO note reconstructed from Railgun sync (Nullified + prior decrypt).
#[derive(Debug, Clone, Serialize)]
pub struct SpentNoteEntry {
   pub asset: AssetId,
   pub amount: u128,
   #[serde(rename = "treeNumber")]
   pub tree_number: u32,
   #[serde(rename = "leafIndex")]
   pub leaf_index: u32,
   #[serde(rename = "blindedCommitment")]
   pub blinded_commitment: String,
   #[serde(rename = "commitmentType")]
   pub commitment_type: BlindedCommitmentType,
   pub memo: String,
   #[serde(rename = "spentBlock")]
   pub spent_block: u64,
   #[serde(rename = "spentTimestamp")]
   pub spent_timestamp: u64,
   pub nullifier: String,
}

impl SpentNoteEntry {
   fn from_spent(spent: SpentNote) -> Self {
      let note = spent.note;
      Self {
         asset: note.asset(),
         amount: note.value(),
         tree_number: note.tree_number,
         leaf_index: note.leaf_index,
         blinded_commitment: format!("0x{:064x}", note.blinded_commitment),
         commitment_type: note.commitment_type,
         memo: note.memo,
         spent_block: spent.spent_block,
         spent_timestamp: spent.spent_timestamp,
         nullifier: format!("0x{:064x}", note.nullifier),
      }
   }
}

/// Interfaces with the RAILGUN protocol.
#[derive(Clone)]
pub struct RailgunProvider<P: Provider<Ethereum>> {
   chain: ChainConfig,
   provider: P,
   pub utxo_indexer: Arc<RwLock<UtxoIndexer>>,
   prover: Groth16Prover,
   poi_provider: Option<PoiProvider>,
   is_syncing: Arc<RwLock<bool>>,
   is_verifying: Arc<RwLock<bool>>,
}

#[derive(Debug, Error)]
pub enum RailgunProviderError {
   #[error("Utxo indexer error: {0}")]
   UtxoIndexer(#[from] UtxoIndexerError),
   #[error("Build error: {0}")]
   Build(#[from] TransactionBuilderError),
   #[error("POI provider error: {0}")]
   PoiProvider(#[from] PoiProviderError),
   #[error("Unable to construct valid note configuration for fee payment")]
   FeeNoteNotFound,
   #[error(
      "Not enough private balance for unshield + bundler fee (asset {asset}): need {needed} wei total, have {available} wei. Leave some private WETH for the paymaster fee, or unshield a smaller amount."
   )]
   InsufficientForFee {
      asset: AssetId,
      needed: u128,
      available: u128,
   },
   #[error("Signer Error: {0}")]
   Signer(#[from] alloy_signer::Error),
   #[error("Bundler error: {0}")]
   Bundler(#[from] BundlerError),
   #[error("RPC error: {0}")]
   Rpc(#[from] anyhow::Error),
   #[error("Privacy Paymaster not configured for chain: {0}")]
   PrivacyPaymasterNotConfigured(u64),
   #[error("Other: {0}")]
   Other(Box<dyn std::error::Error + Send + Sync>),
}

impl<P: Provider<Ethereum> + Clone> RailgunProvider<P> {
   pub async fn new(
      chain: ChainConfig,
      provider: P,
      utxo_indexer: UtxoIndexer,
      prover: Groth16Prover,
      poi_provider: Option<PoiProvider>,
   ) -> Result<Self, RailgunProviderError> {
      Ok(Self {
         chain,
         provider,
         utxo_indexer: Arc::new(RwLock::new(utxo_indexer)),
         prover,
         poi_provider,
         is_syncing: Arc::new(RwLock::new(false)),
         is_verifying: Arc::new(RwLock::new(false)),
      })
   }

   /// Returns the shield fee in %
   pub fn shield_fee(&self) -> f64 {
      0.25
   }

   /// Returns the unshield fee in %
   pub fn unshield_fee(&self) -> f64 {
      0.25
   }

   /// Returns the chain id
   pub fn chain_id(&self) -> u64 {
      self.chain.id
   }

   /// Shared Groth16 prover / artifact loader for this provider.
   pub fn prover(&self) -> &Groth16Prover {
      &self.prover
   }

   /// Max notes per merge pack from circuits cached on disk (`Nx01`).
   /// Falls back to the full artifact pack size when nothing is cached yet.
   pub fn max_merge_inputs(&self) -> usize {
      self.prover.max_merge_inputs()
   }

   /// Returns the chain configuration
   pub fn chain_config(&self) -> ChainConfig {
      self.chain.clone()
   }

   /// Returns the railgun contract address based on the chain configuration
   pub fn railgun_address(&self) -> Address {
      self.chain.railgun_smart_wallet
   }

   pub fn set_provider(&mut self, provider: P) {
      self.provider = provider;
   }

   /// Register a signer with the provider. The provider will index and track
   /// UTXOs for the associated address.
   pub async fn register(&mut self, signer: RailgunSigner) -> Result<(), RailgunProviderError> {
      self.utxo_indexer.write().await.register(signer).await?;
      Ok(())
   }

   /// Drop account state for any registered/persisted account that is not in `keep_signers`.
   ///
   /// Compares signers against the provider's account set (memory + DB),
   /// deletes orphans, and persists the DB immediately. Returns how many account
   /// DB keys were removed.
   pub async fn cleanup_orphaned_accounts(
      &self,
      keep_signers: &[RailgunSigner],
   ) -> Result<usize, RailgunProviderError> {
      let keep: Vec<RailgunAddress> = keep_signers.iter().map(|s| s.address().clone()).collect();
      let removed = self.utxo_indexer.write().await.retain_accounts(&keep).await?;
      Ok(removed)
   }

   /// Returns true if the [UtxoIndexer] is syncing
   pub async fn is_syncing(&self) -> bool {
      *self.is_syncing.read().await
   }

   /// Returns true if the [UtxoIndexer] is verifying
   pub async fn is_verifying(&self) -> bool {
      *self.is_verifying.read().await
   }

   /// Last minimum synced block for the registered accounts
   pub async fn min_account_synced_block(&self) -> u64 {
      self.utxo_indexer.read().await.min_account_synced_block()
   }

   /// Returns the number of registered accounts
   pub async fn accounts_count(&self) -> usize {
      self.utxo_indexer.read().await.accounts_count()
   }

   /// Last synced block for the given account
   pub async fn account_synced_block(&self, address: &RailgunAddress) -> Option<u64> {
      self.utxo_indexer.read().await.account_synced_block(address)
   }

   /// Last global synced block
   pub async fn global_synced_block(&self) -> u64 {
      self.utxo_indexer.read().await.global_synced_block()
   }

   /// Syncs the provider to the latest block.
   pub async fn sync(&mut self) -> Result<(), RailgunProviderError> {
      {
         let mut is_syncing = self.is_syncing.write().await;

         if *is_syncing {
            return Ok(());
         }

         *is_syncing = true;
      }

      let block_res = self.provider.get_block_number().await;

      let block = match block_res {
         Ok(block) => block,
         Err(e) => {
            return {
               {
                  let mut is_syncing = self.is_syncing.write().await;
                  *is_syncing = false;
               }
               Err(RailgunProviderError::Rpc(e.into()))
            };
         }
      };

      let res = self.sync_to(block, false).await;

      {
         let mut is_syncing = self.is_syncing.write().await;
         *is_syncing = false;
      }

      res
   }

   /// Syncs the provider to the specified block.
   pub async fn sync_to(
      &mut self,
      to_block: u64,
      use_subsquid: bool,
   ) -> Result<(), RailgunProviderError> {
      let deployment_block = self.chain.deployment_block;

      {
         let mut utxo_indexer = self.utxo_indexer.write().await;
         utxo_indexer.sync_to(to_block, deployment_block, use_subsquid).await?;
      }

      if let Some(poi_provider) = &mut self.poi_provider {
         poi_provider.sync_to(&self.prover, to_block).await?;
      }

      Ok(())
   }

   /// Syncs the provider directly from logs
   ///
   /// This should only be used for evm simulations on a instance of the provider that
   /// will not be used for real transactions.
   pub async fn sync_from_logs(
      &mut self,
      logs: Vec<Log>,
      synced_block: u64,
      timestamp: u64,
   ) -> Result<(), RailgunProviderError> {
      let mut utxo_indexer = self.utxo_indexer.write().await;
      utxo_indexer.sync_from_logs(logs, synced_block, timestamp)?;

      Ok(())
   }

   /// Compact the db to save space
   pub async fn compact(&self) -> Result<bool, DatabaseError> {
      self.utxo_indexer.write().await.compact().await
   }

   /// Save the db to disk
   pub async fn save(&self, trees_mutated: bool) -> Result<(), DatabaseError> {
      self.utxo_indexer.write().await.save(trees_mutated).await
   }

   /// Verify root withing the given `block_id`
   ///
   /// If `block_id` is none the latest block will be used
   pub async fn verify_root(&self, block_id: Option<BlockId>) -> Result<(), anyhow::Error> {
      {
         let mut is_verifying = self.is_verifying.write().await;
         *is_verifying = true;
      }

      let utxo_indexer = self.utxo_indexer.read().await;
      let res = utxo_indexer.verify(block_id).await;

      {
         let mut is_verifying = self.is_verifying.write().await;
         *is_verifying = false;
      }

      res.map_err(|e| anyhow!("{:?}", e))
   }

   /// Returns all unspent notes for the given address.
   pub async fn notes(&mut self, address: RailgunAddress) -> Vec<NoteEntry> {
      self
         .unspent(address)
         .await
         .into_iter()
         .map(|(note, poi_status)| NoteEntry::from_note(note, poi_status))
         .collect()
   }

   /// Returns notes this address has spent (nullified).
   ///
   /// Survives a wallet-state wipe: a full Railgun resync rebuilds this from
   /// Shield/Transact decrypt + Nullified events.
   pub async fn spent_notes(&self, address: RailgunAddress) -> Vec<SpentNoteEntry> {
      self
         .utxo_indexer
         .read()
         .await
         .spent(address)
         .into_iter()
         .map(SpentNoteEntry::from_spent)
         .collect()
   }

   /// Grouped private spends: input UTXOs minus change that returned to this 0zk.
   pub async fn private_history(&self, address: RailgunAddress) -> Vec<PrivateHistoryEntry> {
      self.utxo_indexer.read().await.private_history(address)
   }

   /// Returns the balance for the given address.
   ///
   /// If POI is enabled, only returns the spendable balance according to the POI provider.
   pub async fn balance(&mut self, address: RailgunAddress) -> Vec<BalanceEntry> {
      let mut balance_map = HashMap::new();
      for note in self.notes(address).await {
         *balance_map.entry((note.asset, note.poi_status)).or_insert(0) += note.amount;
      }

      balance_map
         .into_iter()
         .map(|((asset, poi_status), amount)| BalanceEntry {
            asset,
            poi_status,
            amount,
         })
         .collect()
   }

   /// Returns the balance for the given railgun address and erc20 token.
   pub async fn balance_erc20(&mut self, address: RailgunAddress, token: AssetId) -> u128 {
      let mut balance_map = HashMap::new();
      for note in self.notes(address).await {
         *balance_map.entry((note.asset, note.poi_status)).or_insert(0) += note.amount;
      }
      balance_map
         .into_iter()
         .filter(|((asset, _), _)| asset == &token)
         .map(|(_, amount)| amount)
         .sum()
   }

   /// Helper to create a shield builder.
   pub fn shield(&self) -> ShieldBuilder {
      ShieldBuilder::new(self.chain.clone())
   }

   /// Helper to create a transaction builder.
   pub fn transact(&self) -> TransactionBuilder {
      TransactionBuilder::new()
   }

   /// Build a transaction builder into a proved, signable transaction.
   pub async fn build<R: Rng>(
      &mut self,
      builder: TransactionBuilder,
      rng: &mut R,
   ) -> Result<ProvedTx, RailgunProviderError> {
      let operations = self.build_operation(builder, rng).await?;
      if let Some(poi_provider) = &mut self.poi_provider {
         poi_provider.register_ops(&operations).await?;
      }

      let proved_tx = ProvedTx::new(self.chain.railgun_smart_wallet, operations);
      Ok(proved_tx)
   }

   /// Build a transaction builder into a broadcastable 7702 UserOperation.
   ///
   /// Constructs a UserOperation sent from the `delegator_address` that executes the provided
   /// transaction, with an additional fee note transfer to cover the bundler fees. The
   /// `fee_payer` is the signer that will authorize the fee note transfer to the bundler's
   /// address for the estimated fee amount in `fee_token`.
   pub async fn prepare_userop<S: SmartAccount>(
      &mut self,
      builder: TransactionBuilder,
      bundler: &dyn Bundler,
      sender: &S,
      fee_payer: RailgunSigner,
      fee_token: Address,
      calldata: S::CallData,
      rng: &mut impl Rng,
   ) -> Result<SignableUserOperation, RailgunProviderError> {
      let privacy_paymaster = self
         .chain
         .privacy_paymaster
         .ok_or(RailgunProviderError::PrivacyPaymasterNotConfigured(self.chain.id))?;
      let railgun_fee_adapter = self
         .chain
         .railgun_fee_adapter
         .ok_or(RailgunProviderError::PrivacyPaymasterNotConfigured(self.chain.id))?;

      if fee_token != self.chain.wrapped_base_token {
         return Err(RailgunProviderError::Other(Box::new(
            std::io::Error::new(
               std::io::ErrorKind::InvalidInput,
               "Currently only the wrapped base token is supported for fee payment",
            ),
         )));
      }

      let paymaster_railgun_address = paymaster_railgun_address(Chain::from(self.chain.id));
      let fee_asset = AssetId::Erc20(fee_token);
      let calldata = sender.encode_call_data(calldata);

      //? Initial arbitrary estimation of fee note value.
      //? IMPORTANT: Needs to be high enough to not cause a revert. Most
      //? bundlers seem to use a fixed maxCost value for estimation (IE 27_000_000
      //? for pimlico). Setting this too low causes an unrecoverable estimation
      //? failure.
      let mut fee_value = 100_000_000u128;
      // Pin gas fees after the first real bundler quote so the fee loop doesn't
      // chase a moving max_fee_per_gas (public Pimlico prices jitter enough to
      // break a tight convergence window).
      let mut pinned_max_fee_per_gas: Option<u128> = None;
      let mut pinned_max_priority_fee_per_gas: Option<u128> = None;

      let base_builder = builder.adapt(railgun_fee_adapter, *sender.address().into_word());
      // When the unshield/transfer already spends the fee asset, the fee note is
      // merged into that operation and every fee bump must re-prove it. Otherwise
      // the fee is a separate op, prove the base intents once and only re-prove
      // the fee transfer on later iterations.
      let fee_merged = base_builder.spends_asset(fee_asset);

      // Snapshot spendable notes once for the whole fee loop (notes don't change
      // mid-prepare). Avoids repeated indexer/POI scans across prove iterations.
      let spendable_notes = self.spendable_notes().await;
      let fee_asset_balance: u128 =
         spendable_notes.iter().filter(|n| n.asset == fee_asset).map(|n| n.value()).sum();
      let base_fee_asset_spend = base_builder.total_value_for_asset(fee_asset);

      // Cached base (non-fee) proofs when fee is a separate asset/op.
      let mut cached_base_ops: Option<Vec<ProvedOperation>> = None;

      debug!("Iteratively building UserOperation to converge on accurate fee estimate");
      // Typical path is 2 proves: seed gas quote → fee=cost*1.08 → accept.
      // Cap at 8 to bound worst-case proving cost.
      for iter in 0..8 {
         let needed = base_fee_asset_spend.saturating_add(fee_value);
         if needed > fee_asset_balance {
            return Err(RailgunProviderError::InsufficientForFee {
               asset: fee_asset,
               needed,
               available: fee_asset_balance,
            });
         }

         let operations = if fee_merged {
            let broadcast_builder = base_builder.clone().transfer(
               fee_payer.clone(),
               paymaster_railgun_address.clone(),
               fee_asset,
               fee_value,
               "fee",
            );
            debug!(
               "Building merged fee+spend tx with fee value: {} (iter {})",
               fee_value, iter
            );
            self
               .build_operation_with_notes(broadcast_builder, &spendable_notes, rng)
               .await?
         } else {
            if cached_base_ops.is_none() {
               debug!("Proving base (non-fee) operations once");
               cached_base_ops = Some(
                  self
                     .build_operation_with_notes(base_builder.clone(), &spendable_notes, rng)
                     .await?,
               );
            }

            let fee_builder = TransactionBuilder::new()
               .adapt(railgun_fee_adapter, *sender.address().into_word())
               .transfer(
                  fee_payer.clone(),
                  paymaster_railgun_address.clone(),
                  fee_asset,
                  fee_value,
                  "fee",
               );
            debug!(
               "Building separate fee transfer with fee value: {} (iter {})",
               fee_value, iter
            );
            let fee_ops =
               self.build_operation_with_notes(fee_builder, &spendable_notes, rng).await?;

            let mut ops = cached_base_ops.clone().expect("base ops cached before fee prove");
            ops.extend(fee_ops);
            ops
         };

         // Get the fee operation & note so the decrypted commitment data can be sent to the
         // paymaster.
         let fee_operation = get_fee_operation(&operations, fee_asset, fee_value)?;
         let fee_note = get_fee_note(fee_operation, fee_asset, fee_value)?;

         let random = fee_note.random();
         let asset = fee_token;
         let value = fee_value;
         let transactions = operations.iter().map(|op| op.transaction.clone()).collect();

         let paymaster_data = encode_paymaster_data(
            railgun_fee_adapter,
            encode_railgun_adapter_data(random, asset, value, transactions),
         );

         // Construct UserOperation
         let mut signable = UserOperationBuilder::new_with_smart_account(sender)
            .await
            .map_err(|e| RailgunProviderError::Other(Box::new(e)))?
            .with_calldata(calldata.clone())
            .with_paymaster(privacy_paymaster, paymaster_data)
            .with_gas_estimate(bundler)
            .await?
            .build();

         // Recalculate fee and check for convergence.
         let estimated_paymaster_verification_gas_limit =
            estimate_paymaster_verification_gas_limit(self.provider.clone(), &signable).await?;
         // Prefer the higher of bundler vs local eth_estimateGas so we don't under-budget
         // paymaster verification if either side is optimistic.
         let bundler_pm_vgl = signable.user_op.paymaster_verification_gas_limit.unwrap_or(0);
         let pm_vgl = estimated_paymaster_verification_gas_limit.max(bundler_pm_vgl);
         signable.user_op.paymaster_verification_gas_limit = Some(pm_vgl);

         // Pin gas fees from the first successful quote (take max if a later quote is higher).
         let quote_max_fee = signable.user_op.max_fee_per_gas;
         let quote_priority = signable.user_op.max_priority_fee_per_gas;
         let max_fee = match pinned_max_fee_per_gas {
            Some(prev) => prev.max(quote_max_fee),
            None => quote_max_fee,
         };
         let priority = match pinned_max_priority_fee_per_gas {
            Some(prev) => prev.max(quote_priority),
            None => quote_priority,
         };
         pinned_max_fee_per_gas = Some(max_fee);
         pinned_max_priority_fee_per_gas = Some(priority);
         signable.user_op.max_fee_per_gas = max_fee;
         signable.user_op.max_priority_fee_per_gas = priority;

         let total_gas = signable.total_gas_limit();
         let new_fee = total_gas.saturating_mul(max_fee);
         debug!(
            "Fee iter {}: note={} cost={} gas={} max_fee={}",
            iter, fee_value, new_fee, total_gas, max_fee
         );

         // Accept when the private fee note covers estimated gas cost.
         // Prefer tight headroom (5%) on the next prove so we usually finish in 2
         // iterations without a shrink/re-prove cycle.
         if new_fee > 0 && new_fee <= fee_value {
            debug!(
               "Fee converged at note={}, cost={}, total gas: {}",
               fee_value, new_fee, total_gas
            );
            if let Some(poi_provider) = &mut self.poi_provider {
               poi_provider.register_ops(&operations).await?;
            }
            return Ok(signable);
         }

         // Fee note too small: set next prove to cost + 5% headroom.
         let bumped = new_fee.saturating_mul(105) / 100;
         fee_value = bumped.max(100_000_000);
         debug!("Fee updated to {} for next iteration", fee_value);
      }

      return Err(RailgunProviderError::Other(Box::new(
         std::io::Error::new(
            std::io::ErrorKind::Other,
            "Failed to converge on fee estimate",
         ),
      )));
   }

   async fn all_unspent(&mut self) -> Vec<(UtxoNote, Option<PoiStatus>)> {
      let addresses = self.utxo_indexer.read().await.registered();
      let mut all_notes = Vec::new();

      for address in addresses {
         let mut notes = self.unspent(address).await;
         all_notes.append(&mut notes);
      }
      all_notes
   }

   async fn unspent(&mut self, address: RailgunAddress) -> Vec<(UtxoNote, Option<PoiStatus>)> {
      let notes = self.utxo_indexer.read().await.unspent(address);

      let Some(poi_provider) = &mut self.poi_provider else {
         return notes.into_iter().map(|note| (note, None)).collect();
      };

      let mut annotated_notes = Vec::new();
      for note in notes {
         let status = poi_provider
            .status(
               note.blinded_commitment.into(),
               note.commitment_type,
            )
            .await;
         match status {
            Ok(status) => {
               annotated_notes.push((note, Some(status)));
            }
            Err(e) => {
               warn!("Error checking POI for note {}: {}", note, e);
               annotated_notes.push((note, Some(PoiStatus::Missing)));
            }
         }
      }

      annotated_notes
   }

   /// Collect currently spendable notes (POI-valid when POI is enabled).
   async fn spendable_notes(&mut self) -> Vec<UtxoNote> {
      let in_notes = self.all_unspent().await;
      if self.poi_provider.is_some() {
         in_notes
            .into_iter()
            .filter(|(_, status)| *status == Some(PoiStatus::Valid))
            .map(|(note, _)| note)
            .collect()
      } else {
         in_notes.into_iter().map(|(note, _)| note).collect()
      }
   }

   async fn build_operation<R: Rng>(
      &mut self,
      builder: TransactionBuilder,
      rng: &mut R,
   ) -> Result<Vec<ProvedOperation>, RailgunProviderError> {
      let spendable_notes = self.spendable_notes().await;
      self.build_operation_with_notes(builder, &spendable_notes, rng).await
   }

   async fn build_operation_with_notes<R: Rng>(
      &mut self,
      builder: TransactionBuilder,
      spendable_notes: &[UtxoNote],
      rng: &mut R,
   ) -> Result<Vec<ProvedOperation>, RailgunProviderError> {
      let utxo_indexer = self.utxo_indexer.read().await;

      let operations = builder
         .build(
            &self.prover,
            self.chain.id,
            spendable_notes,
            &utxo_indexer.utxo_trees,
            rng,
         )
         .await?;

      Ok(operations)
   }
}

/// Gets the operation containing the fee note
fn get_fee_operation<'a>(
   operations: &'a Vec<ProvedOperation>,
   fee_asset: AssetId,
   fee_value: u128,
) -> Result<&'a ProvedOperation, RailgunProviderError> {
   let Some(fee_note_pos) = operations
      .iter()
      .position(|o| o.inner.out_notes().iter().any(|n| is_fee_note(n, fee_asset, fee_value)))
   else {
      return Err(RailgunProviderError::FeeNoteNotFound);
   };
   Ok(&operations[fee_note_pos])
}

/// Gets the fee note from the operation
fn get_fee_note(
   operation: &ProvedOperation,
   fee_asset: AssetId,
   fee_value: u128,
) -> Result<OutputNote, RailgunProviderError> {
   operation
      .inner
      .out_notes()
      .into_iter()
      .find(|n| is_fee_note(n, fee_asset, fee_value))
      .ok_or(RailgunProviderError::FeeNoteNotFound)
}

fn is_fee_note(note: &OutputNote, fee_asset: AssetId, fee_value: u128) -> bool {
   note.asset() == fee_asset && note.value() == fee_value && note.memo() == "fee"
}

async fn estimate_paymaster_verification_gas_limit<P: Provider<Ethereum>>(
   provider: P,
   user_op: &SignableUserOperation,
) -> Result<u128, RailgunProviderError> {
   let entry_point = user_op.entry_point;
   // Kohaku: eth_estimateGas({ to: paymaster, from: entryPoint, data: validatePaymasterUserOp… })
   // Calling EntryPoint with this selector reverts with empty data ("error code 3").
   let Some(paymaster) = user_op.user_op.paymaster else {
      return Ok(0);
   };

   let packed = user_op.user_op.into_packed();
   let data = abi::PrivacyPaymaster::validatePaymasterUserOpCall {
      userOp: abi::PrivacyPaymaster::PackedUserOperation {
         sender: packed.sender,
         nonce: packed.nonce,
         initCode: packed.initCode,
         callData: packed.callData,
         accountGasLimits: packed.accountGasLimits,
         preVerificationGas: packed.preVerificationGas,
         gasFees: packed.gasFees,
         paymasterAndData: packed.paymasterAndData,
         signature: Bytes::new(),
      },
      userOpHash: B256::ZERO,
      maxCost: U256::ZERO,
   }
   .abi_encode();

   let tx = TransactionRequest::default().from(entry_point).to(paymaster).input(data.into());
   let res = provider
      .estimate_gas(tx)
      .await
      .map_err(|e| RailgunProviderError::Other(Box::new(e)))?;

   Ok(res as u128)
}

mod abi {
   use alloy_sol_types::sol;

   sol!(
       contract PrivacyPaymaster {
           function validatePaymasterUserOp(
               PackedUserOperation calldata userOp,
               bytes32 userOpHash,
               uint256 maxCost
           ) external returns (bytes memory context, uint256 validationData);

           struct PackedUserOperation {
               address sender;
               uint256 nonce;
               bytes initCode;
               bytes callData;
               bytes32 accountGasLimits;
               uint256 preVerificationGas;
               bytes32 gasFees;
               bytes paymasterAndData;
               bytes signature;
           }
       }
   );
}
