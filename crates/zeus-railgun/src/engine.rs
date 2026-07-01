use alloy_primitives::{Address, U256};
use anyhow::{Result, anyhow};
use redb::Database;

use zeus_eth::utils::client::RpcClient;
use zeus_railgun_prover::{ProofRequest, RailgunProverClient};
use zeus_railgun_shared::{Chain, RailgunKeys};
use zeus_waku_broadcaster::WakuSidecarClient;

use crate::builders::*;
use crate::note::TokenData;
use crate::scanner::{OwnedNote, RailgunScanner};
use crate::sidecar_assets;

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
      let last_synced_block = self.scanner.last_synced_block();
      self.scanner.sync_from_block(client, last_synced_block, None).await?;

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

   /// High-level shield.
   pub(crate) fn prepare_shield(
      &self,
      token: TokenData,
      value: U256,
      memo: Option<String>,
   ) -> Result<PreparedShield> {
      prepare_shield(&self.keys, token, value, memo)
   }

   /// High-level unshield (multi-note + change note support).
   ///
   /// `use_broadcaster`: if true, the unshield is prepared for gas-sponsored
   /// execution via a Waku broadcaster (use `build_unshield_transact_calldata` after).
   pub(crate) fn prepare_unshield(
      &self,
      to: Address,
      token: TokenData,
      amount: U256,
      _use_broadcaster: bool,
   ) -> Result<PreparedUnshield> {
      prepare_unshield(
         &self.scanner,
         &self.keys,
         to,
         token,
         amount,
         _use_broadcaster,
      )
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

   /// High-level gas-sponsored unshield (optional broadcaster path).
   ///
   /// Pass the fee information obtained from the waku broadcaster client
   /// (via `get_best_fee_quote` or `find_broadcasters_for_token`).
   ///
   /// This is only for unshield operations. Shields are almost always self-broadcast.
   pub fn prepare_unshield_gas_sponsored(
      &self,
      to: Address,
      token: TokenData,
      amount: U256,
      fees_id: String,
      broadcaster_address: String,
      min_gas_price: U256,
   ) -> Result<PreparedBroadcasterUnshield> {
      prepare_unshield_for_broadcaster(
         &self.scanner,
         &self.keys,
         to,
         token,
         amount,
         fees_id,
         broadcaster_address,
         min_gas_price,
      )
   }

   /// Build the full `transact` calldata for an unshield (used for gas-sponsored via broadcaster).
   ///
   /// Call this after `prepare_unshield(..., use_broadcaster: true)`.
   /// Uses the chain_id stored in this engine.
   pub fn build_unshield_transact_calldata(
      &self,
      prepared: &PreparedUnshield,
      proof: crate::contracts::SnarkProof,
      min_gas_price: U256,
      use_broadcaster: bool,
   ) -> Result<Vec<u8>> {
      build_unshield_transact_calldata(
         &self.scanner,
         prepared,
         proof,
         self.chain_id(),
         min_gas_price,
         use_broadcaster,
      )
   }

   pub fn build_unshield_proof_request(
      &self,
      prepared: &PreparedUnshield,
      v: Option<&str>,
   ) -> Result<ProofRequest> {
      build_unshield_proof_request(&self.scanner, &self.keys, prepared, v)
   }
}
