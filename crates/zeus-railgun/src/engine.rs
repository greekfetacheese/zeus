use alloy_primitives::{Address, U256};
use anyhow::{Result, anyhow};
use redb::Database;

use zeus_eth::utils::client::RpcClient;
use zeus_railgun_prover::{ProofRequest, RailgunProverClient};
use zeus_railgun_shared::{Chain, RailgunKeys};
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
            "Node.js is required for Railgun privacy features.\n\n             Please install Node.js from https://nodejs.org and restart Zeus."
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

   /// Start syncing the scanner and merkle tree
   pub async fn sync(&mut self, client: RpcClient) -> Result<(), anyhow::Error> {
      if self.syncing {
        info!("Railgun engine already syncing");
         return Ok(());
      }

      self.syncing = true;
      let last_synced_block = self.scanner.last_synced_block();
      self.scanner.sync_from_block(client, last_synced_block, None).await?;

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

   /// Returns the keys used by this engine.
   pub fn keys(&self) -> &RailgunKeys {
      &self.keys
   }

   /// Access the underlying scanner (for advanced state inspection / sync).
   pub fn scanner(&self) -> &RailgunScanner {
      &self.scanner
   }

   /// Returns whether Node.js is available on this system.
   /// Useful for showing a friendly warning in the UI before enabling Privacy Mode.
   pub fn is_node_available(&self) -> bool {
      sidecar_assets::is_node_available()
   }

   /// Ensures the sidecars are extracted and npm dependencies are installed,
   /// without actually starting the clients yet.
   /// Returns the paths to the two sidecar directories.
   pub fn ensure_sidecars_ready(
      &self,
   ) -> Result<(std::path::PathBuf, std::path::PathBuf), anyhow::Error> {
      sidecar_assets::ensure_sidecars_ready()
   }

   /// Explicitly extract the embedded sidecars to the Zeus data directory.
   ///
   /// Returns (prover_dir, waku_dir).
   /// This is useful if you want to pre-extract or inspect the sidecars
   /// before calling `start_clients()`.
   pub fn extract_sidecars(
      &self,
   ) -> Result<(std::path::PathBuf, std::path::PathBuf), anyhow::Error> {
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

   /// Convenience for the full gas-sponsored (broadcaster) flow.
   ///
   /// - Fetches the best fee quote automatically from the waku broadcaster client.
   /// - Prepares unshield for broadcaster.
   /// - Generates real proof.
   /// - Builds calldata.
   ///
   /// Returns the calldata ready to be sent to the broadcaster (you still need to use
   /// the broadcaster client to post the transact message with the fees_id).
   ///
   /// Error if no fee quote is available.
   // TODO: remove this?
   pub async fn _build_unshield_calldata_via_broadcaster(
      &self,
      to: Address,
      token: TokenData,
      amount: U256,
      min_gas_price: U256,
   ) -> Result<Vec<u8>> {
      if !self.clients_started {
         return Err(anyhow!(
            "Call start_clients() before using broadcaster flow."
         ));
      }

      let quote = self
         .get_best_fee_quote(&token.token_address)
         .await
         .ok_or_else(|| {
            anyhow!(
               "No broadcaster fee quote available for token {}. Make sure the waku client is started, connected to the Railgun Waku network,                 and has received recent fee messages. You can also use build_unshield_calldata with use_broadcaster=false.",
               token.token_address
            )
         })?;

      // We log the chosen broadcaster for transparency (user can inspect)
      info!(
         "[railgun] Using broadcaster {} (fees_id={}) for token {}",
         quote.railgun_address, quote.fees_id, token.token_address
      );

      // Prepare + real proof + calldata
      let prepared = prepare_unshield(&self.scanner, &self.keys, to, token, amount)?;
      let proof = self.generate_unshield_proof(&prepared, Some("01x01")).await?;

      // Note: we could also construct a PreparedBroadcasterUnshield here with the quote info
      build_unshield_transact_calldata(
         &self.scanner,
         &prepared,
         proof,
         self.chain_id(),
         min_gas_price,
      )
   }

   /// Returns a PreparedBroadcasterUnshield that includes a real proof and calldata.
   /// Useful if you want to keep the fees_id + broadcaster info together with the calldata.
   // TODO: remove this?
   pub async fn _prepare_broadcaster_unshield_with_proof(
      &self,
      to: Address,
      token: TokenData,
      amount: U256,
      min_gas_price: U256,
   ) -> Result<PreparedBroadcasterUnshield> {
      let quote = self
         .get_best_fee_quote(&token.token_address)
         .await
         .ok_or_else(|| anyhow!("No broadcaster fee quote available"))?;

      let prepared = prepare_unshield(&self.scanner, &self.keys, to, token, amount)?;
      let proof = self.generate_unshield_proof(&prepared, Some("01x01")).await?;

      let calldata = build_unshield_transact_calldata(
         &self.scanner,
         &prepared,
         proof,
         self.chain_id(),
         min_gas_price,
      )?;

      Ok(PreparedBroadcasterUnshield {
         prepared_unshield: prepared,
         fees_id: quote.fees_id,
         broadcaster_address: quote.railgun_address,
         min_gas_price,
         transact_calldata: Some(calldata),
      })
   }

   // =================================================================
   // Final High-Level Shield / Unshield APIs
   // =================================================================

   /// Shield (public → private). Returns calldata only.
   /// Caller is responsible for signing and sending the tx.
   pub async fn shield(
      &self,
      token: TokenData,
      value: U256,
      memo: Option<String>,
   ) -> Result<Vec<u8>> {
      if !self.clients_started {
         return Err(anyhow!("Call start_clients() before shield."));
      }

      let prepared = prepare_shield(&self.keys, token, value, memo)?;

      // Real proof via the prover sidecar (0-input / 1-output shield witness)
      let proof = self.generate_shield_proof(&prepared, Some("01x01")).await?;

      build_shield_transact_calldata(
         &self.scanner,
         &prepared,
         proof,
         self.chain_id(),
         U256::from(1u64),
      )
   }

   /// Simple unshield (self-pay gas). Returns calldata.
   pub async fn unshield(&self, to: Address, token: TokenData, amount: U256) -> Result<Vec<u8>> {
      if !self.clients_started {
         return Err(anyhow!(
            "Call start_clients() before generating private calldata."
         ));
      }

      let prepared = prepare_unshield(&self.scanner, &self.keys, to, token, amount)?;
      let proof = self.generate_unshield_proof(&prepared, Some("01x01")).await?;

      // TODO: adjust this
      let min_gas_price = U256::from(1u64);

      build_unshield_transact_calldata(
         &self.scanner,
         &prepared,
         proof,
         self.chain_id(),
         min_gas_price,
      )
   }

   /// Full gas-abstracted private unshield via broadcaster.
   ///
   /// Handles quote → prepare → proof → calldata → Waku transact in one call.
   /// Returns the response from the broadcaster (tx_hash or error).
   pub async fn unshield_via_broadcaster(
      &mut self,
      to: Address,
      token: TokenData,
      amount: U256,
   ) -> Result<WakuTransactResponse> {
      if !self.clients_started {
         return Err(anyhow!(
            "Call start_clients() before unshield_via_broadcaster."
         ));
      }

      let quote = self.get_best_fee_quote(&token.token_address).await.ok_or_else(|| {
         anyhow!(
            "No fee quote available for token {}",
            token.token_address
         )
      })?;

      let prepared = prepare_unshield(&self.scanner, &self.keys, to, token, amount)?;
      let proof = self.generate_unshield_proof(&prepared, Some("01x01")).await?;
      // TODO: adjust this
      let min_gas_price = U256::from(1u64);

      let calldata = build_unshield_transact_calldata(
         &self.scanner,
         &prepared,
         proof,
         self.chain_id(),
         min_gas_price,
      )?;

      let calldata_hex = format!("0x{}", hex::encode(&calldata));
      let nullifiers: Vec<String> = prepared.nullifiers.iter().map(|n| n.to_string()).collect();

      // Note: the "to" here should be the RailgunSmartWallet address for the chain.
      // We pass a placeholder; production code should supply the real address.
      let railgun_wallet_address = "0x0000000000000000000000000000000000000000"; // TODO

      self
         .waku_client
         .transact(
            "V2_PoseidonMerkle",
            railgun_wallet_address,
            &calldata_hex,
            &quote,
            nullifiers,
            1u128,
            false,
         )
         .await
   }
}
