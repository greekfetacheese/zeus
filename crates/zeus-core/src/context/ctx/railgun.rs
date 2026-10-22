use super::super::{ZeusContext, ZeusCtx};
use super::{railgun_db_file, railgun_dir};
use crate::assets;
use crate::platform::malloc_trim;
use crate::types::*;
use crate::urls::ZeusUrl;
use anyhow::anyhow;
use std::time::{Duration, Instant};
use zeus_eth::{alloy_provider::Provider, types::ChainId, utils::client::RpcClient};
use zeus_railgun::{
   ChainConfig, Groth16Prover, RailgunDbKey, RailgunProvider, RailgunSigner, RedbDatabase,
   RootVerifier, RpcSyncer, SnapshotLoader, SubsquidSyncer, UtxoIndexer,
};

pub async fn create_railgun_provider(
   client: RpcClient,
   chain: u64,
   db_key: RailgunDbKey,
   allow_circuit_download: bool,
) -> Result<RailgunProvider<RpcClient>, anyhow::Error> {
   let time = std::time::Instant::now();

   let db_file = railgun_db_file(chain)?;
   let railgun_dir = railgun_dir()?;

   let snapshot_loader = SnapshotLoader::new(railgun_dir.clone());
   let chain_config = match ChainConfig::from_chain_id(chain) {
      Some(chain_config) => chain_config,
      None => {
         return Err(anyhow!("Chain {} not supported", chain));
      }
   };

   // Acquire the redb lock on the events snapshot now instead of lazily on the
   // first sync: redb allows a single open handle per file, and a stale provider
   // may still hold it. Opening it here surfaces that contention so the caller's
   // retry can wait it out. A non-lock failure (e.g. corrupt file) is logged and
   // ignored — the sync path already degrades to a fresh snapshot.
   if let Err(e) = snapshot_loader.acquire(chain) {
      if zeus_railgun::is_database_already_open(&e) {
         return Err(e.context(format!("events snapshot for chain {chain}")));
      }
      tracing::warn!(
         "Failed to open events snapshot for chain {} (will start fresh): {}",
         chain,
         e
      );
   }

   let utxo_verifier = RootVerifier::new(client.clone(), chain_config.railgun_smart_wallet);
   let rpc_syncer = RpcSyncer::new(
      client.clone(),
      chain,
      chain_config.railgun_smart_wallet,
   )
   .with_snapshot_loader(snapshot_loader.clone());

   let subsquid_syncer = Some(
      SubsquidSyncer::new(&chain_config.subsquid_endpoint, chain)
         .with_snapshot_loader(snapshot_loader.clone()),
   );

   let db = RedbDatabase::new(db_file, db_key)?;
   let utxo_indexer = UtxoIndexer::new(db, rpc_syncer, subsquid_syncer, utxo_verifier).await?;

   let prover = Groth16Prover::new(
      ZeusUrl::RailgunCircuitArtifacts.base(),
      Some(railgun_dir),
   )
   .with_embedded_circuits(assets::installed().railgun_circuits())
   .with_allow_download(allow_circuit_download);

   let railgun_provider = RailgunProvider::new(
      chain_config,
      client.clone(),
      utxo_indexer,
      prover,
      None,
      snapshot_loader,
   )
   .await?;

   tracing::info!(
      "Railgun provider created for chain {} in {}ms",
      chain,
      time.elapsed().as_millis()
   );

   Ok(railgun_provider)
}

fn railgun_supported(chain: ChainId) -> bool {
   matches!(
      chain,
      ChainId::Ethereum | ChainId::EthereumSepolia
   )
}

impl ZeusCtx {
   pub fn railgun_is_supported(&self, chain: ChainId) -> bool {
      railgun_supported(chain)
   }

   pub fn is_railgun_enabled(&self, chain: u64) -> bool {
      self.read(|ctx| ctx.is_railgun_enabled(chain))
   }

   /// Register all the available `RailgunSigners` from the vault's wallets
   ///
   /// This should be called at the startup and whenever a wallet is added.
   /// Also drops Railgun DB account state for wallets that no longer exist.
   ///
   /// Make sure after calling this also call to sync railgun and update private data
   pub async fn register_railgun_signers(
      &self,
      chain: u64,
      ignore_resync: bool,
   ) -> Result<(), anyhow::Error> {
      let wallets = self.read_vault(|vault| vault.clone_all_wallets());

      for wallet in wallets {
         if let Ok(seed) = wallet.seed() {
            let signer = RailgunSigner::from_seed(&seed, 0, chain)?;
            self.register_railgun_signer(signer, chain.into(), ignore_resync).await?;
         }
      }

      Ok(())
   }

   /// Register a RailgunSigner to the RailgunProvider for all supported chains
   ///
   /// Subsquent calls with the same signer are no-ops
   pub async fn register_railgun_signer(
      &self,
      signer: RailgunSigner,
      chain: ChainId,
      ignore_resync: bool,
   ) -> Result<(), anyhow::Error> {
      if !self.railgun_is_supported(chain) {
         return Ok(());
      }

      if !self.is_railgun_enabled(chain.id()) {
         return Ok(());
      }

      let mut provider = self.get_railgun_provider(chain.id(), ignore_resync).await?;
      provider.register(signer.clone()).await?;

      Ok(())
   }

   /// Re-sync the [RailgunProvider] from scratch for the given chain.
   ///
   /// Attempt 0 deletes `railgun:{chain}.db` and replays from
   /// `events-snapshot:{chain}.db`. Attempt 1 also deletes the events snapshot
   /// (true from-scratch). Leftover `.data` / `.meta` blobs are dropped when the
   /// snapshot loader opens.
   ///
   /// We do a best effort to make sure we have the only handle to the provider,
   /// if we don't the resynced db will not be found on disk and on next startup
   /// the RailgunProvider will be re-created and resynced from scratch again.
   pub async fn resync_railgun(&self, chain: u64) -> Result<(), anyhow::Error> {
      if !self.railgun_is_supported(chain.into()) {
         return Ok(());
      }

      if !self.is_railgun_enabled(chain) {
         return Ok(());
      }

      let is_resyncing = self.read(|ctx| ctx.railgun_status.resync_in_progress(chain));

      if is_resyncing {
         return Ok(());
      }

      let attempts = self.read(|ctx| ctx.railgun_resync_attempts.get(&chain).cloned()).unwrap_or(0);

      // 0 = First attempt
      // 1 = Second attempt that deletes the events snapshot
      // syncing completely from scratch
      // After that if still fails we stop trying
      if attempts >= 2 {
         return Ok(());
      }

      self.write(|ctx| ctx.railgun_status.set_resync_in_progress(chain, true));
      let old_provider = self.write(|ctx| ctx.railgun_provider.remove(&chain));

      if let Some(provider) = old_provider {
         // Best effort to make sure we have the only handle
         // ? Maybe at some point we should wrap the RailgunProvider with an Arc<Mutex>

         let timeout = Duration::from_secs(120);
         let start = Instant::now();

         loop {
            let is_syncing = provider.is_syncing().await;
            let is_verifying = provider.is_verifying().await;
            let op_in_progress = self.read(|ctx| ctx.railgun_status.op_in_progress(chain));

            if !is_syncing && !is_verifying && !op_in_progress {
               break;
            }

            let now = Instant::now();
            if now.duration_since(start) > timeout {
               self.write(|ctx| ctx.railgun_status.set_resync_in_progress(chain, false));
               return Err(anyhow!(
                  "Railgun Resync aborted: timeout for chain {}",
                  chain
               ));
            }

            tokio::time::sleep(Duration::from_millis(250)).await;
         }

         tracing::info!(
            "Dropped cached RailgunProvider for chain {} before db wipe",
            chain
         );
      }

      let res = async {
         let railgun_dir = railgun_dir()?;
         let snapshot_loader = SnapshotLoader::new(railgun_dir.clone());
         let snapshot_db = railgun_dir.join(snapshot_loader.db_filename(chain));

         if attempts > 0 {
            if let Err(e) = tokio::fs::remove_file(&snapshot_db).await {
               if e.kind() != std::io::ErrorKind::NotFound {
                  return Err(e.into());
               }
            }
         }

         let db_file = railgun_db_file(chain)?;
         tokio::fs::remove_file(&db_file).await?;

         self.register_railgun_signers(chain, true).await?;
         self.sync_railgun(chain, true).await?;

         Ok(())
      }
      .await;

      let success = res.is_ok();

      if success {
         self.write(|ctx| ctx.railgun_resync_attempts.remove(&chain));

         // Free memory
         malloc_trim();
      } else {
         let new_attempts = attempts + 1;
         self.write(|ctx| {
            ctx.railgun_resync_attempts.insert(chain, new_attempts);
         });
      }

      if !success && attempts > 1 {
         tracing::error!(
            "Railgun resync failed after {} attempts, even with complete resync",
            attempts
         );

         // Free memory
         malloc_trim();
      }

      self.write(|ctx| ctx.railgun_status.set_resync_in_progress(chain, false));

      res
   }

   /// Sync the [RailgunProvider] for the given chain
   pub async fn sync_railgun(&self, chain: u64, ignore_resync: bool) -> Result<(), anyhow::Error> {
      if !self.railgun_is_supported(chain.into()) {
         return Ok(());
      }

      if !self.is_railgun_enabled(chain) {
         return Ok(());
      }

      let mut provider = match self.get_railgun_provider(chain, ignore_resync).await {
         Ok(provider) => provider,
         Err(err) => {
            self.write(|ctx| {
               ctx.railgun_status.set_synced(chain, false);
               ctx.railgun_status.set_sync_error(chain, err.to_string());
            });
            return Err(err);
         }
      };

      let accounts_count = provider.accounts_count().await;
      if accounts_count == 0 {
         tracing::info!("No Railgun registered accounts yet, skipping sync");
         return Ok(());
      }

      match provider.sync().await {
         Ok(()) => self.write(|ctx| {
            ctx.railgun_status.set_synced(chain, true);
            ctx.railgun_status.clear_last_error(chain);
         }),
         Err(err) => {
            self.write(|ctx| {
               ctx.railgun_status.set_synced(chain, false);
               ctx.railgun_status.set_sync_error(chain, err.to_string());
            });
            return Err(err.into());
         }
      }

      let synced_block = provider.global_synced_block().await;
      self.write(|ctx| {
         ctx.railgun_status.set_synced_block(chain, synced_block);
      });

      Ok(())
   }

   pub async fn get_railgun_provider(
      &self,
      chain: u64,
      ignore_resync: bool,
   ) -> Result<RailgunProvider<RpcClient>, anyhow::Error> {
      if self.is_chain_disabled(chain) {
         return Err(anyhow!("Chain {} is disabled", chain));
      }

      if !self.railgun_is_supported(chain.into()) {
         return Err(anyhow!(
            "Railgun is not supported for the {} network",
            chain
         ));
      }

      if !self.is_railgun_enabled(chain) {
         return Err(anyhow!(
            "Railgun is disabled for this network. Enable it in Settings/Railgun."
         ));
      }

      if !ignore_resync {
         let is_resyncing = self.read(|ctx| ctx.railgun_status.resync_in_progress(chain));
         if is_resyncing {
            return Err(anyhow!("Railgun resyncing in progress"));
         }
      }

      let mut retries = 0;
      let max_retries = 20;
      let wait_time = Duration::from_millis(500);

      let client = match self.get_archive_client(chain, false).await {
         Ok(client) => client,
         Err(e) => {
            let error = format!(
               "Railgun needs access to an archive node, check your Network Settings: {}",
               e
            );
            self.write(|ctx| {
               ctx.railgun_status.set_sync_error(chain, error);
            });
            return Err(e);
         }
      };

      // Ensure vault has a Railgun DB key (legacy vaults get one on first use).
      let (db_key, key_was_generated) = self.write_vault(|vault| {
         let generated = vault.ensure_railgun_db_key()?;
         Ok::<_, anyhow::Error>((vault.railgun_db_key()?, generated))
      })?;
      if key_was_generated {
         // Save the vault to persist the key.
         if let Err(e) = self.encrypt_and_save_vault(None, None) {
            return Err(anyhow!("Failed to save vault: {e}"));
         }
      }

      loop {
         if retries >= max_retries {
            return Err(anyhow!(
               "Failed to create Railgun provider for chain {}",
               chain
            ));
         }

         let client = client.clone();

         let provider_opt = self.read(|ctx| ctx.railgun_provider.get(&chain).cloned());
         if let Some(mut provider) = provider_opt {
            provider
               .prover()
               .set_allow_download(self.read(|ctx| ctx.railgun_config.allow_circuit_download()));
            provider.set_provider(client.clone());

            let is_syncing = provider.is_syncing().await;
            let is_verifying = provider.is_verifying().await;

            if !is_syncing && !is_verifying {
               {
                  let range = self.read(|ctx| ctx.railgun_config.rpc_syncer_block_range(chain));
                  let concurrency = self.read(|ctx| ctx.railgun_config.rpc_syncer_concurrency());

                  let indexer = provider.utxo_indexer.lock().await;

                  indexer.rpc_syncer.set_provider(client.clone().erased()).await;
                  indexer.rpc_syncer.set_block_range(range).await;
                  indexer.rpc_syncer.set_concurrency(concurrency).await;

                  indexer.utxo_verifier.set_provider(client.clone().erased()).await;
               }
            }
            return Ok(provider);
         }

         let has_loaded = self.read(|ctx| ctx.railgun_provider.get(&chain).is_some());
         if !has_loaded {
            self.write(|ctx| {
               ctx.railgun_status.set_loading_db_in_progress(chain, true);
            });
         }

         let allow_circuit_download = self.read(|ctx| ctx.railgun_config.allow_circuit_download());

         let provider = match create_railgun_provider(
            client,
            chain,
            db_key.clone(),
            allow_circuit_download,
         )
         .await
         {
            Ok(provider) => provider,
            Err(e) => {
               // A live handle still holding a redb file (another provider mid-teardown)
               // is not a real failure — this attempt will retry. Report anything else.
               if !zeus_railgun::is_database_already_open(&e) {
                  let error_str = e.to_string();
                  self.write(|ctx| {
                     ctx.railgun_status.set_sync_error(chain, error_str);
                  });
               }

               self.write(|ctx| {
                  ctx.railgun_status.set_loading_db_in_progress(chain, false);
               });

               retries += 1;
               tokio::time::sleep(wait_time).await;
               continue;
            }
         };

         self.write(|ctx| {
            ctx.railgun_provider.insert(chain, provider.clone());
            ctx.railgun_status.ui_can_check.insert(chain, true);
            ctx.railgun_status.loading_db_in_progress.insert(chain, false);
         });

         return Ok(provider);
      }
   }
}

impl ZeusContext {
   pub fn railgun_is_supported(&self, chain: ChainId) -> bool {
      railgun_supported(chain)
   }

   pub fn railgun_status(&self) -> &RailgunStatus {
      &self.railgun_status
   }

   /// Returns true if we need to check if a Railgun provider is syncing
   pub fn should_check_railgun_provider_sync(
      &mut self,
      now_millis: u64,
      chain: u64,
      threshold: u64,
   ) -> bool {
      let last_check = self.railgun_provider_sync_last_check.get(&chain).cloned();

      if let Some(last_check) = last_check {
         let elapsed = now_millis.saturating_sub(last_check);
         if elapsed < threshold {
            return false;
         }
      }

      self.railgun_provider_sync_last_check.insert(chain, now_millis);

      true
   }

   pub fn is_railgun_provider_syncing(&self, chain: u64) -> bool {
      self.railgun_status.sync_in_progress(chain)
   }

   pub fn is_railgun_db_loading(&self, chain: u64) -> bool {
      self.railgun_status.loading_db_in_progress(chain)
   }

   pub fn is_railgun_enabled(&self, chain: u64) -> bool {
      self.railgun_config.is_enabled(chain)
   }
}
