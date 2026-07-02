use alloy_primitives::{Address, U256};
use alloy_provider::Provider;
use anyhow::{Result, anyhow};
use redb::Database;
use std::collections::HashMap;
use std::path::PathBuf;

use zeus_eth::utils::client::RpcClient;
use zeus_eth::utils::get_next_base_fee;
use zeus_railgun_prover::RailgunProverClient;
use zeus_railgun_shared::{Chain, TxidVersion, RailgunKeys};
use zeus_waku_broadcaster::{SelectedBroadcaster, WakuSidecarClient, WakuTransactResponse};

use crate::SnarkProof;
use crate::builders::*;
use crate::note::TokenData;
use crate::scanner::{OwnedNote, RailgunScanner};
use crate::sidecar_assets;

use tracing::info;

/// High-level Railgun engine: wraps scanner state + builders for simple APIs.
///
/// One type to rule the protocol interaction.
/// Automatically updates scanner + merkle on apply (see add_own_shielded_note).
#[derive(Clone)]
pub struct RailgunEngine {
   /// The underlying scanner
   scanner: RailgunScanner,

   /// The RailgunKeys used for shield/unshield (viewing private + spending private).
   keys: RailgunKeys,

   /// The Waku sidecar client
   waku_client: WakuSidecarClient,

   /// The Railgun prover client
   prover_client: RailgunProverClient,

   clients_started: bool,

   syncing: bool,
}

impl RailgunEngine {
   /// Create a new engine with an empty state for the given keys.
   pub fn new(keys: RailgunKeys, chain_id: u64) -> Result<Self> {
      let scanner = RailgunScanner::new(&keys, chain_id)?;
      let waku_client = WakuSidecarClient::new(Chain::from(chain_id));
      let prover_client = RailgunProverClient::new();

      Ok(Self {
         scanner,
         keys,
         waku_client,
         prover_client,
         clients_started: false,
         syncing: false,
      })
   }

   /// Load the engine state from a database
   pub fn from_db(db_path: &str, keys: RailgunKeys, chain_id: u64) -> Result<Self> {
      let db = Database::create(db_path)?;
      let scanner = RailgunScanner::new(&keys, chain_id)?;
      let tree_id = RailgunEngine::tree_id(chain_id);

      // Best effort load
      let _ = scanner.load_merkle_tree(&db, &tree_id);
      let _ = scanner.load_state(&db, &tree_id);

      let waku_client = WakuSidecarClient::new(Chain::from(chain_id));
      let prover_client = RailgunProverClient::new();

      Ok(Self {
         scanner,
         keys: keys,
         waku_client,
         prover_client,
         clients_started: false,
         syncing: false,
      })
   }

   pub fn set_waku_client(&mut self, client: WakuSidecarClient) {
      self.waku_client = client;
   }

   pub fn set_prover_client(&mut self, client: RailgunProverClient) {
      self.prover_client = client;
   }

   /// Returns the tree id based on the chain
   fn tree_id(chain: u64) -> String {
      format!("railgun:{}", chain)
   }

   pub fn is_syncing(&self) -> bool {
      self.syncing
   }

   /// Starts the waku and prover clients.
   ///
   /// This function:
   /// 1. Extracts the embedded sidecar sources (only if they changed or are missing)
   /// 2. Automatically runs `npm install --production` if `node_modules` is missing
   /// 3. Starts the two Node.js sidecars
   ///
   /// If Node.js / npm is not installed on the user's machine, a clear
   /// error message is returned pointing to https://nodejs.org.
   pub async fn start_clients(&mut self) -> Result<(), anyhow::Error> {
      if self.clients_started {
         info!("Railgun engine already started");
         return Ok(());
      }

      // Smart extraction + automatic npm install when needed.
      // This is where we get a nice error if npm is missing.
      let (prover_dir, waku_dir) = sidecar_assets::ensure_sidecars_ready()?;

      // Extra early check (defensive)
      if !sidecar_assets::is_node_available() {
         return Err(anyhow!(
            "Node.js is required for Railgun privacy features.\n\n Please install Node.js from https://nodejs.org and restart Zeus."
         ));
      }

      // Start Waku sidecar
      let waku_entry = waku_dir.join("src/index.js");
      let _ = self.waku_client.start_sidecar(waku_entry.to_string_lossy().as_ref()).await?;
      let _ = self.waku_client.start_waku(self.chain_id().into(), None).await?;

      // Start prover sidecar
      let _ = self.prover_client.start(prover_dir.to_string_lossy().as_ref()).await?;

      self.clients_started = true;

      Ok(())
   }

   /// Sync the scanner from the last known block (with a small reorg buffer) to latest.
   ///
   /// This is the recommended way to update after a shield or unshield transaction:
   /// - Processes new Shield/Transact commitments into the Merkle tree.
   /// - Processes Nullified events → removes spent notes from the unspent list
   ///   (exactly the Kohaku indexer model).
   ///
   /// Safe: does not optimistically change balances before on-chain confirmation.
   /// Use this (or the lower-level scanner.sync_from_block) after you have a tx receipt.
   pub async fn sync(&mut self, client: RpcClient) -> Result<()> {
      if self.syncing {
         info!("Railgun engine already syncing");
         return Ok(());
      }

      self.syncing = true;

      let last = self.scanner.last_synced_block();
      // Start a bit earlier to tolerate small reorgs / missed logs
      let from = last.saturating_sub(128);
      self.scanner.sync_from_block(client, from, None).await?;

      self.syncing = false;
      Ok(())
   }

   /// Save the full engine state to a redb Database.
   pub fn save_state(&self, db: &Database) -> Result<()> {
      let tree_id = RailgunEngine::tree_id(self.chain_id());
      self.scanner.save_state(db, &tree_id)?;
      self.scanner.save_merkle_tree(db, &tree_id)
   }

   /// Returns the chain this engine is configured for.
   pub fn chain_id(&self) -> u64 {
      self.scanner.chain_id()
   }

   /// Returns the RailgunSmartWallet contract address for the current chain.
   /// Returns None if the chain is not supported yet.
   pub fn railgun_contract_address(&self) -> Option<Address> {
      crate::contracts::railgun_address(self.chain_id())
   }

   /// Returns the keys used by this engine.
   pub fn keys(&self) -> &RailgunKeys {
      &self.keys
   }

   /// Access the underlying scanner (for advanced state inspection / sync).

   pub fn scanner(&self) -> &RailgunScanner {
      &self.scanner
   }

   /// Current private balances per token (recommended for UI).
   pub fn private_balances(&self) -> HashMap<TokenData, U256> {
      self.scanner.private_balances()
   }

   /// Private balance for one token.
   pub fn private_balance_for(&self, token: &TokenData) -> U256 {
      self.scanner.private_balance_for(token)
   }

   /// All unspent notes (for advanced selection).
   pub fn unspent_notes(&self) -> Vec<OwnedNote> {
      self.scanner.unspent_notes()
   }

   /// Unspent notes for a token (used when building unshield).
   pub fn unspent_notes_for(&self, token: &TokenData) -> Vec<crate::scanner::OwnedNote> {
      self.scanner.unspent_notes_for(token)
   }

   /// Mark a note as spent locally (call after successful unshield).
   pub fn mark_nullified(&mut self, nullifier: U256) {
      self.scanner.mark_nullified(nullifier);
   }

   /// Returns whether Node.js is available on this system.
   /// Useful for showing a friendly warning in the UI before enabling Privacy Mode.
   pub fn is_node_available(&self) -> bool {
      sidecar_assets::is_node_available()
   }

   /// Ensures the sidecars are extracted and npm dependencies are installed,
   /// without actually starting the clients yet.
   /// Returns the paths to the two sidecar directories.
   pub fn ensure_sidecars_ready(&self) -> Result<(PathBuf, PathBuf), anyhow::Error> {
      sidecar_assets::ensure_sidecars_ready()
   }

   /// Explicitly extract the embedded sidecars to the Zeus data directory.
   ///
   /// Returns (prover_dir, waku_dir).
   /// This is useful if you want to pre-extract or inspect the sidecars
   /// before calling `start_clients()`.
   pub fn extract_sidecars(&self) -> Result<(PathBuf, PathBuf), anyhow::Error> {
      sidecar_assets::extract_sidecars_to_zeus_data()
   }

   /// Apply shield result: updates owned notes + merkle tree automatically.
   pub fn apply_shield(&self, prepared: &PreparedShield, leaf_index: u64) -> Result<OwnedNote> {
      apply_shield_to_scanner(&self.scanner, prepared, leaf_index)
   }

   /// Apply unshield result: marks nullifiers spent.
   pub fn apply_unshield(&self, prepared: &PreparedUnshield) {
      apply_unshield_to_scanner(&self.scanner, prepared);
      // ponytail: if change_note exists, caller can shield it or keep in scanner via other means
   }

   /// Returns the best (lowest fee) broadcaster quote for a token.
   ///
   /// Requires the waku client to be started (`start_clients`) and to have received
   /// fee messages from the Waku broadcaster network.
   ///
   /// Returns None if no suitable quote is available yet.
   pub async fn get_best_fee_quote(&self, token_address: &str) -> Option<SelectedBroadcaster> {
      self.waku_client.get_best_fee_quote(token_address).await
   }

   /// All fee quotes for a token, sorted by fee (lowest first).
   pub async fn get_all_fee_quotes(&self, token_address: &str) -> Vec<SelectedBroadcaster> {
      self.waku_client.get_all_fee_quotes(token_address).await
   }

   pub async fn estimate_min_gas_price(&self, client: &RpcClient) -> Result<U256> {
      let chain_id = self.chain_id();

      let base_or_legacy: U256 = if chain_id == 1 {
         let next_base = get_next_base_fee(client.clone()).await? as u128;
         U256::from(next_base)
      } else {
         // get_gas_price can return u128 in this project's provider setup
         let gp: u128 = client.get_gas_price().await.map_err(|e| anyhow::anyhow!("{:?}", e))?;
         U256::from(gp)
      };

      let priority: U256 = match client.get_max_priority_fee_per_gas().await {
         Ok(f) => {
            let p: u128 = f; // may be u128 from the provider
            U256::from(p)
         }
         Err(_) => U256::from(1_000_000_000u64),
      };

      let combined = base_or_legacy + priority;
      // 10% buffer
      let buffered = combined * U256::from(110) / U256::from(100);
      Ok(std::cmp::max(buffered, U256::from(1_000_000u64)))
   }

   /// Generate a real Groth16 proof using the prover sidecar for the given prepared unshield.
   ///
   /// This is the proof flow: build ProofRequest → prover.prove_with_inputs → snark_proof_from_sidecar.
   ///
   /// Requires clients to have been started.
   pub async fn generate_unshield_proof(
      &self,
      prepared: &PreparedUnshield,
      circuit_variant: Option<&str>,
   ) -> Result<SnarkProof> {
      if !self.clients_started {
         return Err(anyhow!(
            "Railgun sidecars not started. Call `engine.start_clients().await` first (and wait for prover artifacts to download on first run)."
         ));
      }

      let req = build_unshield_proof_request(
         &self.scanner,
         &self.keys,
         prepared,
         circuit_variant,
      )?;
      let proof_value = self.prover_client.prove_with_inputs(req).await?;

      // proof_value from sidecar should be the {"pi_a":..., "pi_b":..., "pi_c":...}
      snark_proof_from_sidecar(proof_value)
   }

   /// Generate a real Groth16 proof using the prover sidecar for a shield.
   ///
   /// This follows the same flow as unshield: build ProofRequest → prover.prove_with_inputs → snark_proof_from_sidecar.
   /// Requires clients to have been started.
   pub async fn generate_shield_proof(
      &self,
      prepared: &PreparedShield,
      circuit_variant: Option<&str>,
   ) -> Result<SnarkProof> {
      if !self.clients_started {
         return Err(anyhow!(
            "Railgun sidecars not started. Call `engine.start_clients().await` first (and wait for prover artifacts to download on first run)."
         ));
      }

      let req = build_shield_proof_request(
         &self.scanner,
         &self.keys,
         prepared,
         circuit_variant,
      )?;
      let proof_value = self.prover_client.prove_with_inputs(req).await?;

      snark_proof_from_sidecar(proof_value)
   }

   // =================================================================
   // Final High-Level Shield / Unshield APIs
   // =================================================================

   /// Shield (public → private). Returns the transact calldata.
   /// Caller is responsible for signing + broadcasting the tx.
   ///
   /// After the transaction confirms:
   ///   - Extract leaf_index from the emitted Shield event, then call `apply_shield(...)`, **or**
   ///   - Call `sync(client).await` to at least update the Merkle tree.
   ///
   /// `client` is used for realistic gas price estimation.
   pub async fn shield(
      &self,
      client: &RpcClient,
      token: TokenData,
      value: U256,
      memo: Option<String>,
   ) -> Result<Vec<u8>> {
      if !self.clients_started {
         return Err(anyhow!("Call start_clients() before shield."));
      }

      let prepared = prepare_shield(&self.keys, token, value, memo)?;

      let proof = self.generate_shield_proof(&prepared, Some("01x01")).await?;
      let min_gas_price = self.estimate_min_gas_price(client).await?;

      build_shield_transact_calldata(
         &self.scanner,
         &prepared,
         proof,
         self.chain_id(),
         min_gas_price,
      )
   }

   /// Simple unshield (self-pay gas).
   /// Returns the raw transact calldata.
   ///
   /// After broadcasting and confirmation, prefer calling `sync(client).await`
   /// so the scanner processes the Nullified event(s) and removes the spent notes
   /// (correct private balance update, no optimistic risk if the tx reverts).
   ///
   /// `client` is used for realistic gas price estimation.
   pub async fn unshield(
      &self,
      client: &RpcClient,
      to: Address,
      token: TokenData,
      amount: U256,
   ) -> Result<Vec<u8>> {
      if !self.clients_started {
         return Err(anyhow!(
            "Call start_clients() before generating private calldata."
         ));
      }

      let prepared = prepare_unshield(&self.scanner, &self.keys, to, token, amount)?;
      let proof = self.generate_unshield_proof(&prepared, Some("01x01")).await?;

      let min_gas_price = self.estimate_min_gas_price(client).await?;

      build_unshield_transact_calldata(
         &self.scanner,
         &prepared,
         proof,
         self.chain_id(),
         min_gas_price,
      )
   }

   /// Full private unshield via broadcaster (gas-sponsored + maximum anonymity).
   ///
   /// Complete flow (see above). Returns WakuTransactResponse (contains tx hash on success).
   ///
   /// **Balance update strategy**:
   /// Do **not** optimistically mark notes spent here. After you receive a successful
   /// response (and the tx is confirmed on-chain), call:
   ///   engine.sync(&client).await
   /// This lets the scanner see the on-chain Nullified event(s), matching Kohaku behavior.
   /// This prevents showing an incorrect (too-low) private balance if the tx fails.
   ///
   /// `client` is used for realistic on-chain gas price estimation.
   pub async fn unshield_via_broadcaster(
      &mut self,
      client: &RpcClient,
      to: Address,
      token: TokenData,
      amount: U256,
   ) -> Result<WakuTransactResponse> {
      if !self.clients_started {
         return Err(anyhow!(
            "Call start_clients() before unshield_via_broadcaster."
         ));
      }

      let quote = self.get_best_fee_quote(token.address()).await.ok_or_else(|| {
         anyhow!(
            "No fee quote available for token {}. Wait for fee messages from the Waku broadcaster or use unshield() for self-broadcast.",
            token.address()
         )
      })?;

      let prepared = prepare_unshield(&self.scanner, &self.keys, to, token, amount)?;
      let proof = self.generate_unshield_proof(&prepared, Some("01x01")).await?;

      // Use realistic on-chain gas price (mainnet uses get_next_base_fee + priority)
      let min_gas_price = self.estimate_min_gas_price(client).await?;

      let calldata = build_unshield_transact_calldata(
         &self.scanner,
         &prepared,
         proof,
         self.chain_id(),
         min_gas_price,
      )?;

      let calldata_hex = format!("0x{}", hex::encode(&calldata));
      let nullifiers: Vec<String> = prepared.nullifiers.iter().map(|n| n.to_string()).collect();

      let railgun_contract = self.railgun_contract_address()
         .ok_or_else(|| anyhow!(
            "Railgun contract address not known for chain {}. Supported chains: Ethereum mainnet (1), Polygon (137), etc.",
            self.chain_id()
         ))?;

      let railgun_contract_hex = format!("{:?}", railgun_contract);

      info!(
         "[railgun] Sending unshield via broadcaster {} (fees_id={}) to contract {} | minGasPrice={}",
         quote.railgun_address, quote.fees_id, railgun_contract_hex, min_gas_price
      );

      let overall_min_gp = min_gas_price.to::<u128>();
      let txid_version = TxidVersion::V2PoseidonMerkle;

      self
         .waku_client
         .transact(
            txid_version,
            &railgun_contract_hex,
            &calldata_hex,
            &quote,
            nullifiers,
            overall_min_gp,
            false,
         )
         .await
   }
}
