use super::{
   AddressBookHandle, ApprovalManagerHandle, BalanceManagerHandle, ClientKind, ClientManager,
   CurrencyDB, EnsCache, NftDB, PoolManagerHandle, WalletPortfolio,
   price_manager::PriceManagerHandle, tx::TxDBHandle,
};

use crate::core::persisted::{self, PersistedFile};
use crate::core::{TransactionRich, WalletState, WalletValue};
use crate::core::{Vault, WalletInfo, client::Rpc, types::*};
use crate::server::SERVER_PORT;
use crate::utils::{TimeStamp, create_railgun_provider, malloc_trim};
use anyhow::anyhow;
use egui_elements::theme::ThemeKind;
use ncrypt_me::Argon2;
use std::{
   collections::{HashMap, HashSet},
   str::FromStr,
   sync::{Arc, Mutex, RwLock},
   time::{Duration, Instant},
};
use zeus_wallet::Wallet;

use zeus_eth::{
   alloy_primitives::{Address, Bytes, FixedBytes, U256},
   alloy_provider::Provider,
   alloy_rpc_types::{
      Block as RpcBlock, BlockId, Transaction, TransactionReceipt, TransactionRequest,
   },
   amm::uniswap::{
      AnyUniswapPool, DexKind, FeeAmount, State, UniswapPool, UniswapV2Pool, UniswapV3Pool,
      UniswapV4Pool,
   },
   currency::{Currency, NativeCurrency, erc20::ERC20Token},
   nft::{NftCollection, NftToken},
   types::{ChainId, SUPPORTED_CHAINS},
   utils::{NumericValue, client::RpcClient, ens::ENS_CHAIN},
};
use zeus_railgun::{RailgunAddress, RailgunProvider, RailgunSigner, SnapshotLoader};

pub use persisted::{
   bundler_url_dir, data_dir, disabled_chains_dir, misc_config_dir, pool_data_dir,
   railgun_config_dir, railgun_db_file, railgun_dir, security_dir, theme_kind_dir,
};

/// This is the minimum USD value in a base currency that a pool needs to have in order to be considered sufficiently liquid
pub const DEFAULT_POOL_MINIMUM_LIQUIDITY: f64 = 10_000.0;

pub const DELEGATE_WALLET_CHECK_TIMEOUT: u64 = 600;

/// Max entries in the wallet-connector RPC caches (`eth_call`, gas, code, …).
const CONNECTOR_CACHE_CAP: usize = 20;

fn railgun_supported(chain: ChainId) -> bool {
   matches!(
      chain,
      ChainId::Ethereum | ChainId::EthereumSepolia
   )
}

fn cache_elapsed_fresh(now: u64, old: u64, ttl_ms: u64) -> bool {
   let elapsed = if now > old {
      now - old
   } else {
      tracing::warn!("System time is behind block timestamp");
      u64::MAX
   };
   elapsed < ttl_ms
}

fn cache_insert_capped<K, V>(map: &mut HashMap<K, V>, key: K, value: V)
where
   K: Eq + std::hash::Hash + Clone,
{
   map.insert(key, value);
   if map.len() >= CONNECTOR_CACHE_CAP {
      if let Some(oldest) = map.keys().next().cloned() {
         map.remove(&oldest);
      }
   }
}

pub fn load_theme_kind() -> Result<ThemeKind, anyhow::Error> {
   let dir = theme_kind_dir()?;
   let theme_kind_str = std::fs::read_to_string(dir)?;
   let theme_kind = serde_json::from_str(&theme_kind_str)?;
   Ok(theme_kind)
}

/// Thread-safe handle to the [ZeusContext]
#[derive(Clone)]
pub struct ZeusCtx(Arc<RwLock<ZeusContext>>);

impl ZeusCtx {
   pub fn new() -> Self {
      Self(Arc::new(RwLock::new(ZeusContext::new())))
   }

   /// Shared access to the context
   pub fn read<R>(&self, reader: impl FnOnce(&ZeusContext) -> R) -> R {
      reader(&self.0.read().unwrap())
   }

   /// Exclusive mutable access to the context
   pub fn write<R>(&self, writer: impl FnOnce(&mut ZeusContext) -> R) -> R {
      writer(&mut self.0.write().unwrap())
   }

   pub fn pool_manager(&self) -> PoolManagerHandle {
      self.read(|ctx| ctx.pool_manager.clone())
   }

   pub fn price_manager(&self) -> PriceManagerHandle {
      self.read(|ctx| ctx.price_manager.clone())
   }

   pub fn balance_manager(&self) -> BalanceManagerHandle {
      self.read_wallet_state(|ws| ws.balance_manager.clone())
   }

   /// Cheap clone of the vault handle (`Arc<Mutex<Vault>>`).
   fn vault_handle(&self) -> Arc<Mutex<Vault>> {
      self.read(|ctx| Arc::clone(&ctx.vault))
   }

   /// Cheap clone of the wallet-state handle.
   pub fn wallet_state(&self) -> WalletState {
      self.read(|ctx| ctx.wallet_state.clone())
   }

   /// Shared access to the vault without cloning its contents.
   pub fn read_vault<R>(&self, reader: impl FnOnce(&Vault) -> R) -> R {
      let vault = self.vault_handle();
      reader(&vault.lock().unwrap())
   }

   /// Shared access to wallet app state (contacts, balances, portfolios, txs, …).
   pub fn read_wallet_state<R>(
      &self,
      reader: impl FnOnce(&crate::core::WalletStateInner) -> R,
   ) -> R {
      self.wallet_state().read(reader)
   }

   /// Mutable access to wallet app state (does not hold the ZeusContext lock).
   pub fn write_wallet_state<R>(
      &self,
      writer: impl FnOnce(&mut crate::core::WalletStateInner) -> R,
   ) -> R {
      self.wallet_state().write(writer)
   }

   /// Replace wallet-state contents in-place (same `Arc`).
   pub fn set_wallet_state(&self, state: WalletState) {
      self.write(|ctx| {
         // Keep the same outer handle so clones stay valid, copy inner into live Arc.
         let live = ctx.wallet_state.clone();
         live.set(state.clone_inner());
      });
   }

   pub fn vault_exists(&self) -> bool {
      self.read(|ctx| ctx.vault_exists)
   }

   pub fn vault_unlocked(&self) -> bool {
      self.read(|ctx| ctx.vault_unlocked)
   }

   /// True while the UI is locked by auto-lock.
   pub fn is_locked(&self) -> bool {
      self.read(|ctx| ctx.locked)
   }

   /// Guard for anything that signs, sends, or authorizes on the user's behalf.
   ///
   /// Auto-lock gates the UI, but headless callers — the dapp server, a built-in
   /// flow still mid-simulation — have no window to be hidden by, so every
   /// signing / broadcasting chokepoint re-checks the lock here.
   pub fn ensure_unlocked(&self) -> Result<(), anyhow::Error> {
      if self.is_locked() {
         return Err(anyhow::anyhow!("Zeus is locked"));
      }
      Ok(())
   }

   /// Verify `other` against the in-memory vault, counting the attempt.
   ///
   /// This is the Argon2-free re-login / confirm check (the lock screen and the
   /// confirm-password dialogs). Because it is pure equality — no KDF to slow a
   /// guess — it is capped: after `MAX_CREDENTIAL_ATTEMPTS` failures Zeus
   /// refuses further tries and shuts down.
   ///
   /// `login_only` picks [`Vault::credentials_match_login`] (username +
   /// password) over [`Vault::credentials_match`], which also compares the
   /// confirm field the login form does not have.
   pub fn check_credentials(
      &self,
      other: &ncrypt_me::Credentials,
      login_only: bool,
   ) -> CredentialCheck {
      // Refuse before comparing once the cap is reached.
      if self.read(|ctx| ctx.security.credential_attempts) >= MAX_CREDENTIAL_ATTEMPTS {
         Self::credential_lockout_shutdown();
         return CredentialCheck::LockedOut;
      }

      let matched = self.read_vault(|vault| {
         if login_only {
            vault.credentials_match_login(other)
         } else {
            vault.credentials_match(other)
         }
      });

      let result = self.write(|ctx| ctx.security.note_credential_result(matched));

      if result == CredentialCheck::LockedOut {
         Self::credential_lockout_shutdown();
      }

      result
   }

   /// Tell the user and shut Zeus down (see [`Self::check_credentials`]).
   ///
   /// The notice window stays up while `ZeusApp::on_shutdown` saves the vault,
   /// so it is readable before the window closes.
   fn credential_lockout_shutdown() {
      tracing::warn!("Too many failed credential attempts, shutting Zeus down");
      crate::gui::SHARED_GUI.write(|gui| {
         gui.open_msg_window("Too many failed attempts. Zeus will close.");
         gui.shutdown();
      });
   }

   pub fn server_running(&self) -> bool {
      self.read(|ctx| ctx.server_running)
   }

   pub fn railgun_is_supported(&self, chain: ChainId) -> bool {
      railgun_supported(chain)
   }

   pub fn is_chain_supported(&self, chain: u64) -> bool {
      SUPPORTED_CHAINS.contains(&chain)
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

   /// Encrypt and save the vault (Argon2 — wallets + AEAD keys only).
   ///
   /// If `new_vault` is None, the current vault will be encrypted
   ///
   /// If `new_params` is None, the current [Argon2] params will be used
   pub fn encrypt_and_save_vault(
      &self,
      new_vault: Option<Vault>,
      new_params: Option<Argon2>,
   ) -> Result<(), anyhow::Error> {
      if self.save_vault_in_progress() {
         return Err(anyhow!(
            "Saving vault in progress, try again later"
         ));
      }

      self.write(|ctx| ctx.save_vault_in_progress = true);

      // Wallet mutations pass `new_vault` that may lack AEAD keys — merge from live.
      // Snapshot under a short vault lock; Argon2 encrypt must not hold it.
      let vault = match new_vault {
         Some(mut vault) => {
            self.read_vault(|live| vault.persisted_state_from(live));
            vault
         }
         None => self.clone_vault(),
      };

      let encrypted_data = match vault.encrypt(new_params) {
         Ok(data) => data,
         Err(e) => {
            self.write(|ctx| ctx.save_vault_in_progress = false);
            return Err(e);
         }
      };

      if let Err(e) = vault.save(None, encrypted_data) {
         self.write(|ctx| ctx.save_vault_in_progress = false);
         return Err(e);
      }

      self.write(|ctx| ctx.save_vault_in_progress = false);
      Ok(())
   }

   pub fn save_wallet_state(&self) -> Result<(), anyhow::Error> {
      if self.save_wallet_state_in_progress() {
         return Err(anyhow!(
            "Saving wallet state in progress, try again later"
         ));
      }

      self.write(|ctx| {
         ctx.save_wallet_state_in_progress = true;
      });

      let key = self.read_vault(|vault| vault.wallet_state_key())?;
      let state = self.wallet_state();

      let res = state.encrypt_and_save(&key);

      self.write(|ctx| {
         ctx.save_wallet_state_in_progress = false;
      });

      res
   }

   pub fn save_vault_in_progress(&self) -> bool {
      self.read(|ctx| ctx.save_vault_in_progress)
   }

   pub fn save_wallet_state_in_progress(&self) -> bool {
      self.read(|ctx| ctx.save_wallet_state_in_progress)
   }

   /// Load `data/security.data` with the vault-held key.
   ///
   /// The file is optional: a missing or undecryptable file falls back to the
   /// defaults. Returns an error only when the vault has no `wallet_state_key`
   /// (i.e. it was not unlocked).
   pub fn load_security_settings(&self) -> Result<(), anyhow::Error> {
      let key = self.read_vault(|vault| vault.wallet_state_key())?;

      let settings = match SecuritySettings::load_from_file(&key) {
         Ok(settings) => settings,
         Err(e) => {
            if persisted::file_path(PersistedFile::Security).is_ok_and(|path| path.exists()) {
               tracing::error!("Failed to load security settings: {:?}", e);
            }
            SecuritySettings::default()
         }
      };

      self.write(|ctx| ctx.security = settings);
      Ok(())
   }

   /// Seal and write `data/security.data` with the vault-held key.
   pub fn save_security_settings(&self) -> Result<(), anyhow::Error> {
      let key = self.read_vault(|vault| vault.wallet_state_key())?;
      let settings = self.read(|ctx| ctx.security.clone());
      settings.save(&key)
   }

   /// Mutable access to the vault (does not hold the ZeusContext lock).
   pub fn write_vault<R>(&self, writer: impl FnOnce(&mut Vault) -> R) -> R {
      let vault = self.vault_handle();
      writer(&mut vault.lock().unwrap())
   }

   /// Replace vault contents in-place (same `Arc`).
   pub fn set_vault(&self, new_vault: Vault) {
      self.write_vault(|vault| {
         *vault = new_vault;
      });
   }

   /// Deep-clone vault contents. Prefer [`Self::read_vault`] / [`Self::write_vault`]
   /// for ordinary access; use this only for offline mutate → encrypt snapshots.
   pub fn clone_vault(&self) -> Vault {
      self.read_vault(|vault| vault.clone())
   }

   /// Alias for [`Self::clone_vault`] (legacy name used at wallet mutation sites).
   pub fn get_vault(&self) -> Vault {
      self.clone_vault()
   }

   pub fn master_wallet_address(&self) -> Address {
      self.read_vault(|vault| vault.master_wallet_address())
   }

   /// Get the wallet with the given address
   pub fn get_wallet(&self, address: Address) -> Option<Wallet> {
      self.read_vault(|vault| {
         vault
            .all_wallets()
            .into_iter()
            .find(|wallet| wallet.address() == address)
            .cloned()
      })
   }

   /// Is this wallet selected as the current wallet
   pub fn is_current_wallet(&self, address: Address) -> bool {
      self.read(|ctx| ctx.current_wallet.address == address)
   }

   /// Clone the current wallet (including key material) from the vault.
   pub fn get_current_wallet(&self) -> Wallet {
      self.read(|ctx| ctx.get_current_wallet())
   }

   pub fn current_wallet_info(&self) -> WalletInfo {
      self.read(|ctx| {
         if !ctx.vault_unlocked {
            return WalletInfo::default();
         }
         ctx.current_wallet_info()
      })
   }

   pub fn wallet_exists(&self, address: Address) -> bool {
      self.read_vault(|vault| vault.wallet_address_exists(address))
   }

   /// Build the wallet info cache for all the wallets currently
   /// in the vault.
   ///
   /// This should be called at the startup and whenever a wallet is added or removed.
   pub fn build_wallet_info_cache(&self) {
      let previous: HashSet<Address> =
         self.read(|ctx| ctx.wallet_info_cache.map.keys().copied().collect());

      let mut new_map = HashMap::new();
      let mut new_ordered = Vec::new();

      let wallets = self.read_vault(|vault| vault.clone_all_wallets());
      for wallet in wallets {
         let info = WalletInfo::from_wallet(&wallet, true);
         new_map.insert(wallet.address(), info.clone());
         new_ordered.push(info);
      }

      let book = self.address_book();
      if book.is_persisted() {
         book.apply_wallet_diff(&previous, &new_ordered);
         self.save_address_book();
      }

      let new_wallet_info_cache = WalletInfoCache::new(new_map, new_ordered);

      self.write(|ctx| {
         ctx.wallet_info_cache = new_wallet_info_cache;
      });
   }

   pub fn address_book(&self) -> AddressBookHandle {
      self.read(|ctx| ctx.address_book.clone())
   }

   /// The session-only ENS name cache. See [`EnsCache`].
   pub fn ens_cache(&self) -> EnsCache {
      self.read(|ctx| ctx.ens_cache.clone())
   }

   pub fn wallet_with_zk_address_exists(&self, zk_address: &RailgunAddress) -> bool {
      self.read(|ctx| {
         for wallet in ctx.wallet_info_cache.ordered_slice() {
            if wallet.zk_address_ref() == zk_address.address.as_str() {
               return true;
            }
         }

         false
      })
   }

   /// Get all wallets info without cloning the private key
   pub fn get_all_wallets_info(&self) -> Vec<WalletInfo> {
      self.read(|ctx| ctx.wallet_info_cache.ordered_vec.clone())
   }

   pub fn contacts(&self) -> Vec<Contact> {
      self.read_wallet_state(|ws| ws.contacts.clone())
   }

   pub fn remove_contact(&self, evm_address: &str) {
      self.write_wallet_state(|ws| {
         ws.contacts.retain(|c| c.evm_address != evm_address);
      });
      if let Ok(address) = Address::from_str(evm_address) {
         self.address_book().remove_identity(address);
         self.save_address_book();
      }
   }

   pub fn add_contact(&self, contact: Contact) -> Result<(), anyhow::Error> {
      if contact.name.is_empty() {
         return Err(anyhow!("Contact name cannot be empty"));
      }

      let contacts = self.contacts();

      // make sure name and address are unique
      if contacts.iter().any(|c| c.name == contact.name) {
         return Err(anyhow!(
            "Contact with name {} already exists",
            contact.name
         ));
      } else if contacts.iter().any(|c| c.evm_address == contact.evm_address) {
         return Err(anyhow!(
            "Contact with address {} already exists",
            contact.evm_address
         ));
      }

      if let Ok(address) = Address::from_str(&contact.evm_address) {
         self.address_book().insert_identity(address, contact.name.as_str());
         self.save_address_book();
      }

      self.write_wallet_state(|ws| {
         ws.contacts.push(contact);
      });
      Ok(())
   }

   /// Get a contact by it's address
   pub fn get_contact(&self, evm_address: &str) -> Option<Contact> {
      self.read(|ctx| ctx.get_contact_by_address(evm_address))
   }

   pub fn get_contact_by_zk_address(&self, zk_address: &str) -> Option<Contact> {
      let zk_address = zk_address.to_lowercase();
      self.read_wallet_state(|ws| {
         ws.contacts.iter().find(|c| c.zk_address.to_lowercase() == zk_address).cloned()
      })
   }

   /// Check if any RPC client is available for the given chain
   pub fn client_available(&self, chain: u64) -> bool {
      let z_client = self.get_client_manager();
      z_client.rpc_available(chain)
   }

   pub fn get_client_manager(&self) -> ClientManager {
      self.read(|ctx| ctx.client_manager.clone())
   }

   pub async fn get_client(&self, chain: u64) -> Result<RpcClient, anyhow::Error> {
      let z_client = self.get_client_manager();
      z_client.get_client(chain).await
   }

   pub async fn connect_to_rpc(&self, rpc: &Rpc) -> Result<RpcClient, anyhow::Error> {
      let z_client = self.get_client_manager();
      z_client.connect_to(rpc).await
   }

   /// Get an archive client for the given chain.
   ///
   /// If `http` is true, it will use an http endpoint.
   pub async fn get_archive_client(
      &self,
      chain: u64,
      http: bool,
   ) -> Result<RpcClient, anyhow::Error> {
      let z_client = self.get_client_manager();
      z_client.get_archive_client(chain, http).await
   }

   pub async fn get_mev_protect_client(&self, chain: u64) -> Result<RpcClient, anyhow::Error> {
      let z_client = self.get_client_manager();
      z_client.get_mev_protect_client(chain).await
   }

   pub fn chain(&self) -> ChainId {
      self.read(|ctx| ctx.chain)
   }

   pub fn is_chain_disabled(&self, chain: u64) -> bool {
      self.read(|ctx| ctx.is_chain_disabled(chain))
   }

   pub fn is_chain_syncing(&self, chain: u64) -> bool {
      self.read(|ctx| ctx.state_sync.get(&chain).cloned().unwrap_or(false))
   }

   pub fn set_chain_syncing(&self, chain: u64, syncing: bool) {
      self.write(|ctx| {
         ctx.state_sync.insert(chain, syncing);
      });
   }

   pub fn save_currency_db(&self) {
      let key = match self.read_vault(|vault| vault.wallet_state_key()) {
         Ok(k) => k,
         Err(e) => {
            tracing::error!("Error saving CurrencyDB: {:?}", e);
            return;
         }
      };
      let db = self.read(|ctx| ctx.currency_db.clone());
      match db.save(&key) {
         Ok(_) => tracing::trace!("CurrencyDB saved"),
         Err(e) => tracing::error!("Error saving CurrencyDB: {:?}", e),
      }
   }

   /// Load sealed `tokens.data` into the live context (no-op if the file is missing).
   pub fn load_currency_db(&self) {
      match CurrencyDB::exists() {
         Ok(true) => {}
         Ok(false) => {
            tracing::warn!("Token data file missing, skipping load");
            return;
         }
         Err(e) => {
            tracing::error!("Error checking CurrencyDB: {:?}", e);
            return;
         }
      }

      let key = match self.read_vault(|vault| vault.wallet_state_key()) {
         Ok(k) => k,
         Err(e) => {
            tracing::error!("Error loading CurrencyDB: {:?}", e);
            return;
         }
      };

      match CurrencyDB::load_from_file(&key) {
         Ok(db) => self.write(|ctx| ctx.currency_db = db),
         Err(e) => tracing::error!("Error loading CurrencyDB: {:?}", e),
      }
   }

   pub fn save_nft_db(&self) {
      let key = match self.read_vault(|vault| vault.wallet_state_key()) {
         Ok(k) => k,
         Err(e) => {
            tracing::error!("Error saving NftDB: {:?}", e);
            return;
         }
      };
      let db = self.read(|ctx| ctx.nft_db.clone());
      match db.save(&key) {
         Ok(_) => tracing::trace!("NftDB saved"),
         Err(e) => tracing::error!("Error saving NftDB: {:?}", e),
      }
   }

   /// Load sealed `nft_db.data` into the live context (no-op if the file is missing).
   pub fn load_nft_db(&self) {
      match NftDB::exists() {
         Ok(true) => {}
         Ok(false) => {
            tracing::warn!("NFT data file missing, skipping load");
            return;
         }
         Err(e) => {
            tracing::error!("Error checking NftDB: {:?}", e);
            return;
         }
      }

      let key = match self.read_vault(|vault| vault.wallet_state_key()) {
         Ok(k) => k,
         Err(e) => {
            tracing::error!("Error loading NftDB: {:?}", e);
            return;
         }
      };

      match NftDB::load_from_file(&key) {
         Ok(db) => self.write(|ctx| ctx.nft_db = db),
         Err(e) => tracing::error!("Error loading NftDB: {:?}", e),
      }
   }

   pub fn save_address_book(&self) {
      let key = match self.read_vault(|vault| vault.wallet_state_key()) {
         Ok(k) => k,
         Err(_) => return,
      };
      let book = self.address_book();
      match book.save(&key) {
         Ok(_) => tracing::trace!("AddressBook saved"),
         Err(e) => tracing::error!("Error saving AddressBook: {:?}", e),
      }
   }

   /// Load `address_book.data`, or create it from well-known contracts, wallets, and contacts.
   ///
   /// Call after vault unlock (wallets + contacts + currency db are available).
   pub fn load_or_create_address_book(&self) {
      let key = match self.read_vault(|vault| vault.wallet_state_key()) {
         Ok(k) => k,
         Err(e) => {
            tracing::error!("Error loading AddressBook: {:?}", e);
            return;
         }
      };

      let book = self.address_book();
      match AddressBookHandle::exists() {
         Ok(true) => match AddressBookHandle::load_from_file(&key) {
            Ok(loaded) => book.replace_from(&loaded),
            Err(e) => tracing::error!("Error loading AddressBook: {:?}", e),
         },
         Ok(false) => {}
         Err(e) => {
            tracing::error!("Error checking AddressBook: {:?}", e);
            return;
         }
      }

      book.seed_well_known();
      let wallets = self.get_all_wallets_info();
      book.seed_wallets(&wallets);
      let contacts = self.contacts();
      book.seed_contacts(&contacts);

      if let Err(e) = book.save(&key) {
         tracing::error!("Error saving AddressBook: {:?}", e);
      }
   }

   pub fn save_client_manager(&self) {
      let key = match self.read_vault(|vault| vault.wallet_state_key()) {
         Ok(k) => k,
         Err(e) => {
            tracing::error!("Error saving ClientManager: {:?}", e);
            return;
         }
      };
      let client = self.get_client_manager();
      match client.save_to_file(&key) {
         Ok(_) => tracing::trace!("ClientManager saved"),
         Err(e) => tracing::error!("Error saving ClientManager: {:?}", e),
      }

      if let Err(e) = client.save_settings() {
         tracing::error!("Error saving client settings: {:?}", e);
      }
   }

   /// Load sealed `providers.data` into the live client (no-op if the file is missing).
   pub fn load_client_manager(&self) {
      let client = self.get_client_manager();
      client.load_settings();

      match ClientManager::exists() {
         Ok(true) => {}
         Ok(false) => {
            tracing::warn!("ClientManager file missing, skipping load");
            return;
         }
         Err(e) => {
            tracing::error!("Error checking ClientManager: {:?}", e);
            return;
         }
      }

      let key = match self.read_vault(|vault| vault.wallet_state_key()) {
         Ok(k) => k,
         Err(e) => {
            tracing::error!("Error loading ClientManager: {:?}", e);
            return;
         }
      };

      if let Err(e) = client.load_from_file(&key) {
         tracing::error!("Error loading ClientManager: {:?}", e);
      }
   }

   pub fn save_pool_manager(&self) {
      let key = match self.read_vault(|vault| vault.wallet_state_key()) {
         Ok(k) => k,
         Err(e) => {
            tracing::error!("Error saving Pool Manager: {:?}", e);
            return;
         }
      };
      let manager = self.pool_manager();
      match manager.save_to_file(&key) {
         Ok(_) => {}
         Err(e) => tracing::error!("Error saving Pool Manager: {:?}", e),
      }
   }

   /// Load sealed `pool_data.data` into the live handle (no-op if the file is missing).
   pub fn load_pool_manager(&self) {
      match PoolManagerHandle::exists() {
         Ok(true) => {}
         Ok(false) => {
            tracing::warn!("Pool Manager file missing, skipping load");
            return;
         }
         Err(e) => {
            tracing::error!("Error checking Pool Manager: {:?}", e);
            return;
         }
      }

      let key = match self.read_vault(|vault| vault.wallet_state_key()) {
         Ok(k) => k,
         Err(e) => {
            tracing::error!("Error loading Pool Manager: {:?}", e);
            return;
         }
      };

      let manager = self.pool_manager();
      if let Err(e) = manager.load_from_file(&key) {
         tracing::error!("Error loading Pool Manager: {:?}", e);
      }
   }

   /// After an import has written `data/`, drop the live vault/state and load
   /// the imported files. `vault` is already unlocked by [`crate::core::data_import::import_data_from_zip`].
   pub fn reload_from_data_dir(
      &self,
      mut vault: Vault,
   ) -> Result<(WalletInfo, Argon2), anyhow::Error> {
      let info = vault.encrypted_info()?;
      vault.ensure_wallet_state_key()?;
      let key = vault.wallet_state_key()?;
      let (wallet_state, _) = WalletState::load_or_migrate(&key, None)?;
      let master_wallet = vault.get_master_wallet();

      let mut new_wallet_info_map = HashMap::new();
      let mut new_wallet_info_ordered = Vec::new();

      for wallet in vault.clone_all_wallets() {
         let info = WalletInfo::from_wallet(&wallet, true);
         new_wallet_info_map.insert(wallet.address(), info.clone());
         new_wallet_info_ordered.push(info);
      }

      let new_wallet_info_cache =
         WalletInfoCache::new(new_wallet_info_map, new_wallet_info_ordered);

      let master_info = new_wallet_info_cache
         .get(&master_wallet.address())
         .cloned()
         .unwrap_or_else(|| WalletInfo::from_wallet(&master_wallet, true));

      self.write_vault(|old| {
         old.erase();
      });
      self.set_vault(vault);
      self.set_wallet_state(wallet_state);
      self.load_tx_db();
      self.load_security_settings()?;

      self.address_book().replace_from(&AddressBookHandle::default());
      self.ens_cache().clear();

      let now = TimeStamp::now_as_millis().unwrap_or_default().timestamp();

      self.write(|ctx| {
         ctx.currency_db = CurrencyDB::default();
         ctx.nft_db = NftDB::default();
         ctx.delegated_wallets = DelegatedWallets::new();
         ctx.railgun_provider.clear();
         ctx.railgun_status = RailgunStatus::new();
         ctx.railgun_resync_attempts.clear();
         ctx.wallet_info_cache = new_wallet_info_cache;
         ctx.current_wallet = master_info.clone();
         ctx.security.argon_params = info.argon2.clone();
         ctx.locked = false;
         ctx.last_activity_ms = now;
         ctx.last_autolock_notice_ms = now;
         ctx.autolock_notice_shown = false;
         ctx.vault_exists = true;
         ctx.vault_unlocked = true;
         ctx.eth_calls.clear();
         ctx.estimate_gas.clear();
         ctx.codes.clear();
         ctx.storage.clear();
         ctx.transactions.clear();
         ctx.receipts.clear();
         ctx.blocks_by_hash.clear();
         ctx.blocks_by_number.clear();
         ctx.tx_counts.clear();
      });

      let defaults = ClientManager::default();
      self.get_client_manager().write(|map| {
         defaults.read(|d| *map = d.clone());
      });

      let fresh_pools = PoolManagerHandle::default();
      self.pool_manager().write(|manager| {
         fresh_pools.read(|fresh| *manager = fresh.clone());
      });

      let fresh_prices = PriceManagerHandle::new();
      self.price_manager().write(|manager| {
         fresh_prices.read(|fresh| *manager = fresh.clone());
      });

      match DisabledChains::load_from_file() {
         Ok(chains) => self.write(|ctx| ctx.disabled_chains = chains),
         Err(_) => self.write(|ctx| ctx.disabled_chains = DisabledChains::default()),
      }
      match RailgunConfig::load_from_file() {
         Ok(config) => self.write(|ctx| ctx.railgun_config = config),
         Err(_) => self.write(|ctx| ctx.railgun_config = RailgunConfig::default()),
      }
      match MiscConfig::load_from_file() {
         Ok(config) => self.write(|ctx| ctx.misc_config = config),
         Err(_) => self.write(|ctx| ctx.misc_config = MiscConfig::default()),
      }

      self.load_currency_db();
      self.load_nft_db();
      self.load_pool_manager();
      self.load_client_manager();
      self.load_price_manager();
      self.load_or_create_address_book();

      Ok((master_info, info.argon2))
   }

   pub fn save_price_manager(&self) {
      let key = match self.read_vault(|vault| vault.wallet_state_key()) {
         Ok(k) => k,
         Err(e) => {
            tracing::error!("Error saving Price Manager: {:?}", e);
            return;
         }
      };
      let manager = self.price_manager();
      match manager.save_to_file(&key) {
         Ok(_) => tracing::trace!("Price Manager saved"),
         Err(e) => tracing::error!("Error saving Price Manager: {:?}", e),
      }
   }

   /// Load sealed `price_data.data` into the live handle (no-op if the file is missing).
   pub fn load_price_manager(&self) {
      match PriceManagerHandle::exists() {
         Ok(true) => {}
         Ok(false) => {
            tracing::warn!("Price Manager file missing, skipping load");
            return;
         }
         Err(e) => {
            tracing::error!("Error checking Price Manager: {:?}", e);
            return;
         }
      }

      let key = match self.read_vault(|vault| vault.wallet_state_key()) {
         Ok(k) => k,
         Err(e) => {
            tracing::error!("Error loading Price Manager: {:?}", e);
            return;
         }
      };

      let manager = self.price_manager();
      if let Err(e) = manager.load_from_file(&key) {
         tracing::error!("Error loading Price Manager: {:?}", e);
      }
   }

   /// Open `tx_history.db`, migrate any txs still embedded in WalletState, then
   /// clear that field and re-save wallet state so the old blob shrinks.
   pub fn load_tx_db(&self) {
      let key = match self.read_vault(|vault| vault.wallet_state_key()) {
         Ok(k) => k,
         Err(e) => {
            tracing::error!("Error loading Tx history: {:?}", e);
            return;
         }
      };

      let tx_db = match TxDBHandle::open(&key) {
         Ok(db) => db,
         Err(e) => {
            tracing::error!("Error opening Tx history db: {:?}", e);
            let legacy = self.read_wallet_state(|ws| ws.tx_db.clone());
            self.write(|ctx| ctx.tx_db = legacy);
            return;
         }
      };

      let legacy = self.read_wallet_state(|ws| ws.tx_db.clone());
      match tx_db.import_legacy(&legacy) {
         Ok(true) => {
            self.write_wallet_state(|ws| {
               ws.tx_db = TxDBHandle::new();
            });
            self.write(|ctx| ctx.tx_db = tx_db);
            if let Err(e) = self.save_wallet_state() {
               tracing::error!(
                  "Error saving WalletState after tx db migration: {:?}",
                  e
               );
            } else {
               tracing::info!("Migrated transaction history to tx_history.db");
            }
         }
         Ok(false) => {
            self.write(|ctx| ctx.tx_db = tx_db);
         }
         Err(e) => {
            tracing::error!("Error migrating Tx history to redb: {:?}", e);
            self.write(|ctx| ctx.tx_db = legacy);
         }
      }
   }

   /// Append a rich transaction.
   ///
   /// Also feeds ERC20 / Permit2 approvals into the approval manager.
   pub fn add_transaction(&self, chain: u64, owner: Address, tx: TransactionRich) {
      self.approval_manager().add_from_tx(&tx);
      let db = self.tx_db();
      if let Err(e) = db.add_tx(chain, owner, tx) {
         tracing::error!("Error adding transaction to TxDB: {:?}", e);
      }

      match self.save_wallet_state() {
         Ok(_) => {}
         Err(e) => tracing::error!("Error saving WalletState: {:?}", e),
      }
   }

   pub fn tx_db(&self) -> TxDBHandle {
      self.read(|ctx| ctx.tx_db.clone())
   }

   pub fn approval_manager(&self) -> ApprovalManagerHandle {
      self.read_wallet_state(|ws| ws.approval_manager.clone())
   }

   pub fn save_disabled_chains(&self) {
      let chains = self.read(|ctx| ctx.disabled_chains.clone());
      match chains.save_to_file() {
         Ok(_) => tracing::trace!("Disabled Chains saved"),
         Err(e) => tracing::error!("Error saving disabled chains: {:?}", e),
      }
   }

   /// Return the chains which the owner has balance in
   pub fn get_chains_that_have_balance(&self, owner: Address) -> Vec<u64> {
      let mut chains = Vec::new();
      for chain in SUPPORTED_CHAINS {
         let balance = self.get_eth_balance(chain, owner);
         if !balance.is_zero() {
            chains.push(chain);
         }
      }
      chains
   }

   pub fn get_eth_balance(&self, chain: u64, owner: Address) -> NumericValue {
      self.read(|ctx| ctx.get_eth_balance(chain, owner))
   }

   pub fn get_token_balance(&self, chain: u64, owner: Address, token: Address) -> NumericValue {
      self.read(|ctx| ctx.get_token_balance(chain, owner, token))
   }

   pub fn get_currencies(&self, chain: u64) -> Vec<Currency> {
      self.read(|ctx| ctx.currency_db.get_currencies(chain))
   }

   pub fn get_portfolio(&self, chain: u64, owner: Address) -> WalletPortfolio {
      self.read_wallet_state(|ws| ws.portfolio_db.get(chain, owner))
   }

   pub fn has_portfolio(&self, chain: u64, owner: Address) -> bool {
      self.read_wallet_state(|ws| ws.portfolio_db.portfolios.contains_key(&(chain, owner)))
   }

   /// Get the total value for the given owner across all of its wallets and chains
   pub fn get_total_value(&self, owner: Address, include_testnets: bool) -> WalletValue {
      self.read(|ctx| ctx.get_total_value(owner, include_testnets))
   }

   /// Get all tokens from all portfolios
   pub fn get_all_tokens_from_portfolios(&self, chain: u64) -> Vec<ERC20Token> {
      let mut tokens = Vec::new();
      let portfolios = self.read_wallet_state(|ws| ws.portfolio_db.get_all(chain));

      for portfolio in portfolios {
         tokens.extend(portfolio.tokens().iter().cloned());
      }
      tokens
   }

   /// Update the public data of a portfolio for the given chain and owner
   ///
   /// What it does:
   ///
   /// - Calculates the public token list and sorts it by value
   /// - Updates the portfolio public value based on the latest price data
   pub fn update_public_data(&self, chain: u64, owner: Address) {
      let mut portfolio = self.get_portfolio(chain, owner);
      portfolio.update_public_data(self.clone());
      self.write_wallet_state(|ws| {
         ws.portfolio_db.insert_portfolio(chain, owner, portfolio);
      });
   }

   /// Update the private data (Railgun) of a portfolio for the given chain and owner
   ///
   /// What it does:
   ///
   /// - Indexes the private tokens and sorts them by value
   /// - Updates the portfolio private value based on the latest price data
   pub async fn update_private_data(&self, chain: u64, owner: Address) {
      let mut portfolio = self.get_portfolio(chain, owner);
      portfolio.update_private_data(self.clone()).await;
      self.write_wallet_state(|ws| {
         ws.portfolio_db.insert_portfolio(chain, owner, portfolio);
      });
   }

   pub fn get_eth_price(&self, chain: u64) -> NumericValue {
      let token = ERC20Token::wrapped_native_token(chain);
      self.get_token_price(&token)
   }

   /// Get the USD price of an ERC20 token
   pub fn get_token_price(&self, token: &ERC20Token) -> NumericValue {
      self.read(|ctx| ctx.get_token_price(token))
   }

   /// Get the USD price of a currency
   pub fn get_currency_price(&self, currency: &Currency) -> NumericValue {
      self.read(|ctx| ctx.get_currency_price(currency))
   }

   pub fn get_token_value_for_owner(
      &self,
      chain: u64,
      owner: Address,
      token: &ERC20Token,
   ) -> NumericValue {
      let price = self.get_token_price(token);
      let balance = self.get_token_balance(chain, owner, token.address);
      NumericValue::value(balance.f64(), price.f64())
   }

   pub fn get_currency_value_for_amount(&self, amount: f64, currency: &Currency) -> NumericValue {
      self.read(|ctx| ctx.get_currency_value_for_amount(amount, currency))
   }

   pub fn get_token_value_for_amount(&self, amount: f64, token: &ERC20Token) -> NumericValue {
      self.read(|ctx| ctx.get_token_value_for_amount(amount, token))
   }

   pub fn get_currency_balance(
      &self,
      chain: u64,
      owner: Address,
      currency: &Currency,
   ) -> NumericValue {
      self.read(|ctx| ctx.get_currency_balance(chain, owner, currency))
   }

   pub fn pool_has_sufficient_liquidity(&self, pool: &AnyUniswapPool) -> Option<bool> {
      if pool.state().is_none() {
         return None;
      }

      let base_balance = pool.base_balance();
      let base_price = self.get_token_price(&pool.base_currency().to_erc20());
      let base_value = NumericValue::value(base_balance.f64(), base_price.f64());

      Some(base_value.f64() >= DEFAULT_POOL_MINIMUM_LIQUIDITY)
   }

   pub fn get_base_fee(&self, chain: u64) -> Option<BaseFee> {
      self.read(|ctx| ctx.base_fee.get(&chain).cloned())
   }

   pub fn get_priority_fee(&self, chain: u64) -> Option<NumericValue> {
      self.read(|ctx| ctx.priority_fee.get(chain).cloned())
   }

   pub fn update_base_fee(&self, chain: u64, base_fee: u64, next_base_fee: u64) {
      self.write(|ctx| {
         ctx.base_fee.insert(chain, BaseFee::new(base_fee, next_base_fee));
      });
   }

   pub fn update_priority_fee(&self, chain: u64, fee: NumericValue) {
      self.write(|ctx| {
         ctx.priority_fee.fee.insert(chain, fee);
      });
   }

   /// Return the name of this address if its known
   pub fn get_address_name(&self, chain: u64, address: Address) -> Option<Arc<str>> {
      self.read(|ctx| ctx.get_address_name(chain, address))
   }

   /// Off-frame ERC-7730 / Sourcify / ENS fill. No-op if already named or already attempted.
   ///
   /// A token or an NFT collection Zeus already has cached is named by [`Self::get_address_name`],
   /// which the UI reads first, so this never runs for one — it is the names that need a registry,
   /// Sourcify or ENS to learn.
   ///
   /// Returns true if a new name was stored.
   pub async fn lookup_address_name(&self, chain: u64, address: Address) -> bool {
      if address.is_zero() {
         return false;
      }

      if self.get_address_name(chain, address).is_some() {
         return false;
      }

      // A reverse label whose binding has lapsed is forgotten and re-resolved once — after its
      // grace period the name may belong to someone else. Any other address keeps the
      // once-per-session guard, so an address with no name is never looked up on every frame.
      let relapsed = self.ens_cache().remove_lapsed(chain, address);

      let book = self.address_book();

      if !relapsed && !book.mark_pending(chain, address) {
         return false;
      }

      if self.read(|ctx| ctx.currency_db.get_erc20_token(chain, address).is_some()) {
         return false;
      }

      // Read the opt-in once: it gates the registry's remote fallback as well as Sourcify, while the
      // registry's embedded descriptors answer offline either way.
      let fetch_names = self.read(|ctx| ctx.misc_config.fetch_contract_names());

      let contract_name = if let Some(name) =
         crate::core::clear_signing::registry::resolve_contract_label(chain, address, fetch_names)
            .await
      {
         Some(name)
      } else if fetch_names {
         crate::core::clear_signing::sourcify::contract_name(chain, address).await
      } else {
         None
      };

      // A contract label is durable, so it belongs in the address book. ENS is only asked for when
      // there is no registry label, so a contract keeps the name it already has.
      if let Some(name) = contract_name {
         if name.trim().is_empty() {
            return false;
         }

         if !book.insert_contract(chain, address, name.as_str()) {
            return false;
         }

         self.save_address_book();
         return true;
      }

      // ENS reverse, asked for on mainnet (ENS only exists there). The name goes to the session
      // cache rather than the book: ENS names expire and a reverse record outlives the name it
      // points at, so a persisted label would outlive the name. The cache also refuses a name
      // whose binding has already lapsed. See [`EnsCache`].
      let Some((name, takeover_at)) = self.lookup_ens_name(address).await else {
         return false;
      };

      self.ens_cache().insert(chain, address, &name, takeover_at)
   }

   /// ENS reverse lookup for [`Self::lookup_address_name`], with the name's takeover time.
   ///
   /// Onchain only (`zeus_eth::utils::ens` refuses offchain redirects) and mainnet
   /// only. `None` covers every ordinary outcome: mainnet disabled, no RPC
   /// reachable, or the address simply has no primary name — none of them errors.
   ///
   /// The returned `u64` is the name's takeover time (`0` = no onchain expiry known, e.g. a
   /// non-`.eth` name, or an expiry read that failed). It bounds the label's life in [`EnsCache`]:
   /// a *reverse* record is owned by the address rather than the name, so it survives the name's
   /// expiry and would otherwise keep labelling the address forever.
   async fn lookup_ens_name(&self, address: Address) -> Option<(String, u64)> {
      if self.is_chain_disabled(ENS_CHAIN) {
         return None;
      }

      let client = self.get_client(ENS_CHAIN).await.ok()?;

      let name = match zeus_eth::utils::ens::lookup_name(&client, &address).await {
         Ok(name) => name?,
         Err(e) => {
            tracing::error!("ENS reverse lookup failed {}", e);
            return None;
         }
      };

      let takeover_at = match zeus_eth::utils::ens::name_expiry(&client, &name).await {
         Ok(Some(expiry)) => expiry.takeover_at,
         Ok(None) => 0,
         Err(e) => {
            tracing::warn!("ENS expiry lookup failed for {}: {}", name, e);
            0
         }
      };

      Some((name, takeover_at))
   }

   /// Get the V2 pool for the given address
   ///
   /// If the pool is not found in cache, it will be retrieved from the blockchain
   pub async fn get_v2_pool(
      &self,
      chain: u64,
      address: Address,
   ) -> Result<AnyUniswapPool, anyhow::Error> {
      let z_client = self.get_client_manager();
      let cached = self.read(|ctx| ctx.pool_manager.get_v2_pool_from_address(chain, address));

      if let Some(pool) = cached {
         return Ok(pool);
      } else {
         let pool = z_client
            .request(chain, |client| async move {
               UniswapV2Pool::from_address(client, chain, address).await
            })
            .await?;
         let pool = AnyUniswapPool::from_pool(pool);
         self.write(|ctx| {
            ctx.pool_manager.add_pool(pool.clone());
            ctx.currency_db.insert_currency(chain, pool.currency0().clone());
            ctx.currency_db.insert_currency(chain, pool.currency1().clone());
         });

         return Ok(pool);
      };
   }

   /// Get the V3 pool for the given address
   ///
   /// If the pool is not found in cache, it will be retrieved from the blockchain
   pub async fn get_v3_pool(
      &self,
      chain: u64,
      address: Address,
   ) -> Result<AnyUniswapPool, anyhow::Error> {
      let z_client = self.get_client_manager();
      let cached = self.read(|ctx| ctx.pool_manager.get_v3_pool_from_address(chain, address));

      if let Some(pool) = cached {
         return Ok(pool);
      } else {
         let pool = z_client
            .request(chain, |client| async move {
               UniswapV3Pool::from_address(client, chain, address).await
            })
            .await?;
         let pool = AnyUniswapPool::from_pool(pool);
         self.write(|ctx| {
            ctx.pool_manager.add_pool(pool.clone());
            ctx.currency_db.insert_currency(chain, pool.currency0().clone());
            ctx.currency_db.insert_currency(chain, pool.currency1().clone());
         });

         return Ok(pool);
      };
   }

   /// Get the V4 pool for the given pool id
   ///
   /// If the pool is not found in cache, it does a best-effort search over known
   /// tokens and common fee / tick-spacing combos.
   ///
   /// `fee` is typically the Swap-event fee (charged LP + protocol). That is **not**
   /// the PoolKey fee — e.g. Base ETH/VIRTUAL is PoolKey.fee=500 but Swap.fee=625.
   pub async fn get_v4_pool(
      &self,
      chain: u64,
      fee: u32,
      expected_id: FixedBytes<32>,
   ) -> Result<AnyUniswapPool, anyhow::Error> {
      let cached = self.read(|ctx| ctx.pool_manager.get_v4_pool_from_id(chain, expected_id));

      if let Some(pool) = cached {
         return Ok(pool);
      }

      let mut fees = vec![100u32, 500, 3000, 10000, fee];
      fees.sort_unstable();
      fees.dedup();

      let spacings_for = |f: u32| -> Vec<i32> {
         let mut spacings = vec![1, 10, 60, 200, FeeAmount::new(f).tick_spacing_i32()];
         spacings.sort_unstable();
         spacings.dedup();
         spacings
      };

      let mut base_tokens = ERC20Token::base_tokens(chain);

      // remove WETH since in V4 is not used
      let weth = ERC20Token::wrapped_native_token(chain);
      base_tokens.retain(|t| t.address != weth.address);

      let mut base_currencies: Vec<Currency> =
         base_tokens.iter().map(|t| Currency::from(t.clone())).collect();

      // Add ETH native
      let currency = Currency::from(NativeCurrency::from(chain));
      base_currencies.push(currency);

      let quote_currencies = self.get_currencies(chain);

      // Best effort pool finding from all known tokens
      for quote_currency in quote_currencies.iter() {
         for base_currency in &base_currencies {
            if quote_currency.address() == base_currency.address() {
               continue;
            }

            for &cand_fee in &fees {
               for spacing in spacings_for(cand_fee) {
                  let pool = UniswapV4Pool::new_with_spacing(
                     chain,
                     FeeAmount::new(cand_fee),
                     spacing,
                     DexKind::UniswapV4,
                     base_currency.clone(),
                     quote_currency.clone(),
                     State::none(),
                     Address::ZERO,
                  );

                  if pool.id() == expected_id {
                     let pool_manager = self.pool_manager();
                     pool_manager.add_pool(pool.clone());
                     return Ok(pool.into());
                  }
               }
            }
         }
      }

      Err(anyhow!("V4 Pool not found"))
   }

   /// Get the ERC20 token for the given address
   ///
   /// If the token is not found in cache, it will be retrieved from the blockchain
   pub async fn get_token(
      &self,
      chain: u64,
      address: Address,
   ) -> Result<ERC20Token, anyhow::Error> {
      let cached = self.read(|ctx| ctx.currency_db.get_erc20_token(chain, address));

      if let Some(token) = cached {
         return Ok(token);
      } else {
         let z_client = self.get_client_manager();
         let rpc = z_client.get_best_rpc(chain).ok_or(anyhow!("No available RPC found"))?;
         let client = z_client.client_for(&rpc, ClientKind::Short).await?;

         let token = ERC20Token::new(client, address, chain).await?;

         self.write(|ctx| ctx.currency_db.insert_currency(chain, Currency::from(token.clone())));

         return Ok(token);
      };
   }

   /// Get an NFT
   ///
   /// If it is not cached, the collection is resolved first — the standard costs an ERC-165 sweep and
   /// cannot differ per token — and then the token itself. Both are stored in `nft_db`.
   pub async fn get_nft(
      &self,
      chain: u64,
      collection: Address,
      token_id: U256,
   ) -> Result<NftToken, anyhow::Error> {
      if let Some(nft) = self.read(|ctx| ctx.nft_db.get_nft(chain, collection, token_id)) {
         return Ok(nft);
      }

      let z_client = self.get_client_manager();
      let rpc = z_client.get_best_rpc(chain).ok_or(anyhow!("No available RPC found"))?;
      let client = z_client.client_for(&rpc, ClientKind::Short).await?;

      let collection = NftCollection::fetch(client.clone(), chain, collection).await?;
      let nft = NftToken::fetch(client, &collection, token_id).await?;

      self.write(|ctx| {
         ctx.nft_db.insert_collection(chain, collection);
         ctx.nft_db.insert_nft(nft.clone());
      });
      self.save_nft_db();

      Ok(nft)
   }

   pub fn get_connected_dapps(&self) -> Vec<String> {
      self.read(|ctx| ctx.connected_dapps.connected_dapps())
   }

   pub fn connect_dapp(&self, dapp: String) {
      self.write(|ctx| {
         ctx.connected_dapps.connect_dapp(dapp);
      });
   }

   pub fn disconnect_dapp(&self, dapp: &str) {
      self.write(|ctx| {
         ctx.connected_dapps.disconnect_dapp(dapp);
      });
   }

   pub fn disconnect_all_dapps(&self) {
      self.write(|ctx| {
         ctx.connected_dapps.disconnect_all();
      });
   }

   pub fn is_dapp_connected(&self, dapp: &str) -> bool {
      self.read(|ctx| ctx.connected_dapps.is_connected(dapp))
   }

   /// The account Zeus gave `origin`, if the app has connected before.
   pub fn dapp_account(&self, origin: &str) -> Option<Address> {
      self.read_wallet_state(|ws| ws.dapp_accounts.get(origin))
   }

   /// Every account `origin` has been given, oldest first.
   pub fn dapp_seen_accounts(&self, origin: &str) -> Vec<Address> {
      self.read_wallet_state(|ws| ws.dapp_accounts.seen(origin).to_vec())
   }

   /// Remember `address` as the account exposed to `origin` and persist it.
   ///
   /// Disconnecting an app does not clear this, and the accounts the app was
   /// given before are kept too, so reconnecting offers the same account and the
   /// prompt can still mark the earlier ones.
   pub fn set_dapp_account(&self, origin: &str, address: Address) -> Result<(), anyhow::Error> {
      self.write_wallet_state(|ws| ws.dapp_accounts.record(origin, address));
      self.save_wallet_state()
   }

   /// Derive a fresh child wallet to serve as a new app's dedicated account.
   ///
   /// The wallet keeps the usual auto-generated name (e.g. `Wallet 5`) so
   /// per-app accounts stay unobtrusive in the wallet list. The vault write runs
   /// Argon2, so callers must keep this off the GUI thread.
   pub fn create_app_account(&self) -> Result<Address, anyhow::Error> {
      let mut new_vault = self.get_vault();
      let address = new_vault.derive_child_wallet(String::new())?;
      self.encrypt_and_save_vault(Some(new_vault.clone()), None)?;
      self.set_vault(new_vault);
      self.build_wallet_info_cache();
      Ok(address)
   }

   pub fn should_check_delegated_wallet_status(&self, chain: u64, account: Address) -> bool {
      self.read(|ctx| ctx.delegated_wallets.should_check(chain, account))
   }

   pub async fn check_delegated_wallet_status(
      &self,
      chain: u64,
      account: Address,
   ) -> Result<(), anyhow::Error> {
      if self.is_chain_disabled(chain) {
         return Ok(());
      }

      let client = self.get_client_manager();
      let code = client
         .request(chain, |client| async move {
            client.get_code_at(account).await.map_err(|e| anyhow!("{:?}", e))
         })
         .await?;

      if code.is_empty() {
         self.write(|ctx| {
            ctx.delegated_wallets.remove(chain, account);
         });
         return Ok(());
      }

      let addr_slice = &code[3..];
      let delegated_address = Address::from_slice(&addr_slice);

      self.write(|ctx| {
         ctx.delegated_wallets.add(chain, account, delegated_address);
      });

      Ok(())
   }

   /// Get the receipt for the given transaction hash
   ///
   /// If the receipt is not found in cache, it will be retrieved from the blockchain
   pub async fn get_receipt_by_hash(
      &self,
      hash: FixedBytes<32>,
   ) -> Result<Option<TransactionReceipt>, anyhow::Error> {
      let chain = self.chain().id();

      let receipt = self.read(|ctx| ctx.receipts.get(&(chain, hash)).cloned());

      if let Some(receipt) = receipt {
         return Ok(Some(receipt));
      }

      let client = self.get_client_manager();
      let receipt = client
         .request(chain, |client| async move {
            client.get_transaction_receipt(hash).await.map_err(|e| anyhow!("{:?}", e))
         })
         .await?;

      if let Some(receipt) = &receipt {
         self.write(|ctx| {
            cache_insert_capped(&mut ctx.receipts, (chain, hash), receipt.clone());
         });
      }

      Ok(receipt)
   }

   /// Get the transaction for the given transaction hash
   ///
   /// If the transaction is not found in cache, it will be retrieved from the blockchain
   pub async fn get_tx_by_hash(
      &self,
      hash: FixedBytes<32>,
   ) -> Result<Option<Transaction>, anyhow::Error> {
      let chain = self.chain().id();

      let transaction = self.read(|ctx| ctx.transactions.get(&(chain, hash)).cloned());

      if let Some(transaction) = transaction {
         return Ok(Some(transaction));
      }

      let client = self.get_client_manager();
      let transaction = client
         .request(chain, |client| async move {
            client.get_transaction_by_hash(hash).await.map_err(|e| anyhow!("{:?}", e))
         })
         .await?;

      if let Some(transaction) = &transaction {
         self.write(|ctx| {
            cache_insert_capped(
               &mut ctx.transactions,
               (chain, hash),
               transaction.clone(),
            );
         });
      }

      Ok(transaction)
   }

   /// Get the storage value for the given address and slot
   ///
   /// If the storage value is not found in cache, it will be retrieved from the blockchain
   pub async fn get_storage(
      &self,
      block_id: BlockId,
      address: Address,
      slot: U256,
   ) -> Result<U256, anyhow::Error> {
      let chain = self.chain().id();

      let block = if let Some(block) = block_id.as_u64() {
         block
      } else {
         self.get_latest_block().await?.number
      };

      let storage = self.read(|ctx| ctx.storage.get(&(chain, block, address, slot)).cloned());

      if let Some(storage) = storage {
         return Ok(storage);
      }

      let client = self.get_client_manager();
      let storage = client
         .request(chain, |client| async move {
            client
               .get_storage_at(address, slot)
               .block_id(block_id)
               .await
               .map_err(|e| anyhow!("{:?}", e))
         })
         .await?;

      self.write(|ctx| {
         cache_insert_capped(
            &mut ctx.storage,
            (chain, block, address, slot),
            storage.clone(),
         );
      });

      Ok(storage)
   }

   /// Get the code for the given address
   ///
   /// If the code is not found in cache, it will be retrieved from the blockchain
   pub async fn get_code(
      &self,
      block_id: BlockId,
      address: Address,
   ) -> Result<Bytes, anyhow::Error> {
      let chain = self.chain().id();

      let block = if let Some(block) = block_id.as_u64() {
         block
      } else {
         self.get_latest_block().await?.number
      };

      let code = self.read(|ctx| ctx.codes.get(&(chain, block, address)).cloned());

      if let Some(code) = code {
         return Ok(code);
      }

      let client = self.get_client_manager();
      let code = client
         .request(chain, |client| async move {
            client
               .get_code_at(address)
               .block_id(block_id)
               .await
               .map_err(|e| anyhow!("{:?}", e))
         })
         .await?;

      self.write(|ctx| {
         cache_insert_capped(
            &mut ctx.codes,
            (chain, block, address),
            code.clone(),
         );
      });

      Ok(code)
   }

   /// Estimate the gas for the given transaction
   ///
   /// If the gas is not found in cache, it will be estimated from the blockchain
   pub async fn estimate_gas(&self, tx: TransactionRequest) -> Result<u64, anyhow::Error> {
      let chain = self.chain();
      let res = self.read(|ctx| ctx.estimate_gas.get(&(chain.id(), tx.clone())).cloned());
      let block_time = chain.block_time_millis();
      let now = TimeStamp::now_as_millis()?.timestamp();

      if let Some(res) = res {
         if cache_elapsed_fresh(now, res.timestamp, block_time) {
            return Ok(res.gas);
         }
      }

      let client = self.get_client_manager();
      let gas = client
         .request_with(chain.id(), tx.clone(), |client, tx| async move {
            client.estimate_gas(tx).await.map_err(|e| anyhow!("{:?}", e))
         })
         .await?;

      let now = TimeStamp::now_as_millis()?.timestamp();

      self.write(|ctx| {
         cache_insert_capped(
            &mut ctx.estimate_gas,
            (chain.id(), tx),
            EstimateGas {
               timestamp: now,
               gas,
            },
         );
      });

      Ok(gas)
   }

   /// Get the eth_call for the given transaction
   ///
   /// If the eth_call is not found in cache, it will be retrieved from the blockchain
   pub async fn get_eth_call(&self, tx: TransactionRequest) -> Result<EthCall, anyhow::Error> {
      let chain = self.chain();
      let block_time = chain.block_time_millis();
      let now = TimeStamp::now_as_millis()?.timestamp();
      let eth_call = self.read(|ctx| ctx.eth_calls.get(&(chain.id(), tx.clone())).cloned());

      if let Some(eth_call) = eth_call {
         if cache_elapsed_fresh(now, eth_call.timestamp, block_time) {
            return Ok(eth_call);
         }
      }

      let z_client = self.get_client_manager();
      let result = z_client
         .request_with(chain.id(), tx.clone(), |client, tx| async move {
            client.call(tx).await.map_err(|e| anyhow!("{:?}", e))
         })
         .await?;

      let now = TimeStamp::now_as_millis()?.timestamp();

      let eth_call = EthCall {
         timestamp: now,
         result,
      };

      self.write(|ctx| {
         cache_insert_capped(
            &mut ctx.eth_calls,
            (chain.id(), tx),
            eth_call.clone(),
         );
      });

      Ok(eth_call)
   }

   /// Get the latest block
   ///
   /// If the block is not found in cache, it will be retrieved from the blockchain
   pub async fn get_latest_block(&self) -> Result<Block, anyhow::Error> {
      let chain = self.chain();
      let block_time = chain.block_time_millis();
      let now = TimeStamp::now_as_millis()?.timestamp();
      let block = self.read(|ctx| ctx.latest_block.get(&chain.id()).cloned());

      if let Some(block) = block {
         let block_timestamp_ms = block.timestamp * 1000u64;
         if cache_elapsed_fresh(now, block_timestamp_ms, block_time) {
            return Ok(block);
         }
      }

      let z_client = self.get_client_manager();
      let block = z_client
         .request(chain.id(), |client| async move {
            client.get_block(BlockId::latest()).await.map_err(|e| anyhow!("{:?}", e))
         })
         .await?;

      if let Some(block) = block {
         let block = Block::new(block.header.number, block.header.timestamp);
         self.write(|ctx| {
            ctx.latest_block.insert(chain.id(), block.clone());
         });
         return Ok(block);
      }

      Err(anyhow!("No block found"))
   }

   /// Get a block by number or tag
   ///
   /// If the block is not found in cache, it will be retrieved from the blockchain.
   /// `hydrated` selects full transactions (`true`) or hashes only (`false`).
   ///
   /// Only requests with a stable numeric key are cached: an explicit number,
   /// `latest` (resolved via the cached tip) and `earliest` (genesis). Tags that
   /// can move independently of the tip (`safe` / `finalized` / `pending`) bypass
   /// the cache so a stale entry is never served for them.
   pub async fn get_block_by_number(
      &self,
      block_id: BlockId,
      hydrated: bool,
   ) -> Result<Option<RpcBlock>, anyhow::Error> {
      let chain = self.chain();

      let cache_number = if let Some(number) = block_id.as_u64() {
         Some(number)
      } else if block_id.is_latest() {
         Some(self.get_latest_block().await?.number)
      } else if block_id.is_earliest() {
         Some(0)
      } else {
         None
      };

      if let Some(number) = cache_number {
         let cached = self.read(|ctx| ctx.blocks_by_number.get(&(chain.id(), number)).cloned());
         if let Some(cached) = cached {
            let now = TimeStamp::now_as_millis()?.timestamp();
            if cached.hydrated == hydrated
               && cache_elapsed_fresh(now, cached.timestamp, chain.block_time_millis())
            {
               return Ok(Some(cached.block));
            }
         }
      }

      let z_client = self.get_client_manager();
      let block = z_client
         .request(chain.id(), move |client| async move {
            let req = client.get_block(block_id);
            if hydrated {
               req.full().await.map_err(|e| anyhow!("{:?}", e))
            } else {
               req.await.map_err(|e| anyhow!("{:?}", e))
            }
         })
         .await?;

      let Some(block) = block else {
         return Ok(None);
      };

      if let Some(number) = cache_number {
         let now = TimeStamp::now_as_millis()?.timestamp();
         self.write(|ctx| {
            cache_insert_capped(
               &mut ctx.blocks_by_number,
               (chain.id(), number),
               CachedBlock {
                  timestamp: now,
                  hydrated,
                  block: block.clone(),
               },
            );
         });
      }

      Ok(Some(block))
   }

   /// Get a block by its hash
   ///
   /// If the block is not found in cache, it will be retrieved from the blockchain.
   /// `hydrated` selects full transactions (`true`) or hashes only (`false`); the
   /// cache stores one variant per hash and refetches when the requested one differs.
   pub async fn get_block_by_hash(
      &self,
      hash: FixedBytes<32>,
      hydrated: bool,
   ) -> Result<Option<RpcBlock>, anyhow::Error> {
      let chain = self.chain();
      let block_time = chain.block_time_millis();
      let now = TimeStamp::now_as_millis()?.timestamp();
      let cached = self.read(|ctx| ctx.blocks_by_hash.get(&(chain.id(), hash)).cloned());

      if let Some(cached) = cached {
         if cached.hydrated == hydrated && cache_elapsed_fresh(now, cached.timestamp, block_time) {
            return Ok(Some(cached.block));
         }
      }

      let z_client = self.get_client_manager();
      let block = z_client
         .request(chain.id(), move |client| async move {
            let req = client.get_block_by_hash(hash);
            if hydrated {
               req.full().await.map_err(|e| anyhow!("{:?}", e))
            } else {
               req.await.map_err(|e| anyhow!("{:?}", e))
            }
         })
         .await?;

      let Some(block) = block else {
         return Ok(None);
      };

      let now = TimeStamp::now_as_millis()?.timestamp();

      self.write(|ctx| {
         cache_insert_capped(
            &mut ctx.blocks_by_hash,
            (chain.id(), hash),
            CachedBlock {
               timestamp: now,
               hydrated,
               block: block.clone(),
            },
         );
      });

      Ok(Some(block))
   }

   /// Get the transaction count (nonce) for the given address
   ///
   /// If the count is not found in cache, it will be retrieved from the blockchain
   pub async fn get_transaction_count(&self, address: Address) -> Result<u64, anyhow::Error> {
      let chain = self.chain();
      let block_time = chain.block_time_millis();
      let now = TimeStamp::now_as_millis()?.timestamp();
      let cached = self.read(|ctx| ctx.tx_counts.get(&(chain.id(), address)).cloned());

      if let Some(cached) = cached {
         if cache_elapsed_fresh(now, cached.timestamp, block_time) {
            return Ok(cached.count);
         }
      }

      let z_client = self.get_client_manager();
      let count = z_client
         .request(chain.id(), move |client| async move {
            client.get_transaction_count(address).await.map_err(|e| anyhow!("{:?}", e))
         })
         .await?;

      let now = TimeStamp::now_as_millis()?.timestamp();

      self.write(|ctx| {
         cache_insert_capped(
            &mut ctx.tx_counts,
            (chain.id(), address),
            TransactionCount {
               timestamp: now,
               count,
            },
         );
      });

      Ok(count)
   }

   pub fn server_port(&self) -> u16 {
      self.read(|ctx| ctx.server_port)
   }
}

pub struct ZeusContext {
   /// Client manager that handles almost all the RPC calls in Zeus
   pub client_manager: ClientManager,

   /// The current selected chain from the GUI
   pub chain: ChainId,

   /// True if the privacy mode is enabled (Railgun)
   pub privacy_mode: bool,

   /// Railgun resync attempts
   pub railgun_resync_attempts: HashMap<u64, u8>,

   /// Railgun provider mapped by chain
   pub railgun_provider: HashMap<u64, RailgunProvider<RpcClient>>,

   /// The current selected wallet from the GUI (no key material).
   pub current_wallet: WalletInfo,

   /// Cached `WalletInfo` for quickly accessing & cloning any wallet
   /// without its private key.
   pub wallet_info_cache: WalletInfoCache,

   /// Loaded Vault
   pub vault: Arc<Mutex<Vault>>,

   /// Frequently updated wallet app state (contacts, balances, portfolios, txs, …).
   ///
   /// Sealed separately in `wallet_state.data` with [`Vault`]'s `wallet_state_key`.
   pub wallet_state: WalletState,

   /// Transaction history (`tx_history.db`), sealed with [`Vault`]'s `wallet_state_key`.
   pub tx_db: TxDBHandle,

   /// Security settings (persisted, sealed in `security.data` with the vault's
   /// `wallet_state_key`). Holds the Argon2 params of the current vault.
   pub security: SecuritySettings,

   /// True while the UI is locked by auto-lock. Not persisted; false at startup.
   pub locked: bool,

   /// Wall-clock ms of the last user input, for the auto-lock timer.
   pub last_activity_ms: u64,

   /// Wall-clock ms of the last "auto-lock is not configured" toast.
   pub last_autolock_notice_ms: u64,

   /// False until the first "auto-lock is not configured" toast has fired, so
   /// the first one comes `first_delay` after unlock and the rest `repeat`.
   pub autolock_notice_shown: bool,

   /// True if a vault exists in the data directory
   pub vault_exists: bool,

   /// True if the vault is unlocked,
   /// only then the Zeus UI unlocks
   pub vault_unlocked: bool,

   /// Fast address names (wallets, contacts, well-known + ERC-7730/Sourcify).
   ///
   /// Token names stay in [`Self::currency_db`]. ENS names live in [`Self::ens_cache`] — they can
   /// expire, so they are deliberately not persisted here.
   pub address_book: AddressBookHandle,

   /// ENS names resolved this session. In memory only — see [`EnsCache`].
   pub ens_cache: EnsCache,

   /// Holds all ERC20 tokens
   pub currency_db: CurrencyDB,

   /// Holds the NFTs the user tracks, plus cached collection metadata
   pub nft_db: NftDB,

   /// Pool manager used for the Uniswap UI
   /// and price manager
   pub pool_manager: PoolManagerHandle,

   /// Calculate the $USD price of an ERC20 token
   /// based purely on the on-chain pool data
   /// no 3rd party APIs
   pub price_manager: PriceManagerHandle,

   /// State flags for the UI that showup on the top right corner
   pub data_syncing: bool,
   pub on_startup_syncing: bool,
   pub save_vault_in_progress: bool,
   pub save_wallet_state_in_progress: bool,

   /// State sync flag per chain
   pub state_sync: HashMap<u64, bool>,

   /// Cached base fees for each chain
   pub base_fee: HashMap<u64, BaseFee>,

   // Cached data for the wallet connector
   pub latest_block: HashMap<u64, Block>,
   pub eth_calls: HashMap<(u64, TransactionRequest), EthCall>,
   pub estimate_gas: HashMap<(u64, TransactionRequest), EstimateGas>,
   pub codes: HashMap<(u64, u64, Address), Bytes>,
   pub storage: HashMap<(u64, u64, Address, U256), U256>,
   pub transactions: HashMap<(u64, FixedBytes<32>), Transaction>,
   pub receipts: HashMap<(u64, FixedBytes<32>), TransactionReceipt>,
   pub blocks_by_hash: HashMap<(u64, FixedBytes<32>), CachedBlock>,
   pub blocks_by_number: HashMap<(u64, u64), CachedBlock>,
   pub tx_counts: HashMap<(u64, Address), TransactionCount>,

   /// Cached priority fees for each chain
   pub priority_fee: PriorityFee,

   /// Currently connected dapps
   pub connected_dapps: ConnectedDapps,

   /// Currently delegated wallets
   pub delegated_wallets: DelegatedWallets,

   /// Local connector HTTP port (default [`crate::server::SERVER_PORT`];
   /// may change at bind time if that port is taken).
   pub server_port: u16,

   /// True if the local server that communicates with the wallet connector (browser extension) is running
   pub server_running: bool,

   /// Last time checked for available RPCs
   pub last_checked_for_available_rpcs: HashMap<u64, u64>,

   /// Last time checked for malfunctioning RPCs
   pub last_checked_for_malfunction: HashMap<u64, u64>,

   /// Last time we detected a malfunctioning RPC
   pub last_detected_malfunction: HashMap<u64, u64>,

   /// Mapped available RPCs for each chain
   ///
   /// - `Key`: chain_id
   ///
   /// - `Value`: true if we have at least one working & enabled RPC
   pub available_rpcs: HashMap<u64, bool>,

   /// Last UNIX ms timestamp we checked if a railgun provider
   /// is syncing for the given chain
   pub railgun_provider_sync_last_check: HashMap<u64, u64>,

   /// Disabled Chains
   pub disabled_chains: DisabledChains,

   /// Railgun status for the services UI
   pub railgun_status: RailgunStatus,

   /// Railgun configuration
   pub railgun_config: RailgunConfig,

   /// Misc persisted settings (token icons, Sourcify names, …)
   pub misc_config: MiscConfig,
}

/// `write_private` only tightens mode on the next save. Existing `0644` files
/// from older builds stay world-readable until then — fix them at startup.
fn tighten_existing_secret_files() {
   let paths = [
      Vault::dir().ok(),
      WalletState::dir().ok(),
      TxDBHandle::dir().ok(),
      CurrencyDB::dir().ok(),
      AddressBookHandle::dir().ok(),
      pool_data_dir().ok(),
      ClientManager::dir().ok(),
      bundler_url_dir().ok(),
      PriceManagerHandle::dir().ok(),
      persisted::file_path(PersistedFile::Security).ok(),
      persisted::file_path(PersistedFile::Connector).ok(),
   ];

   for path in paths.into_iter().flatten() {
      if !path.exists() {
         continue;
      }
      if let Err(e) = crate::utils::restrict_file_to_owner(&path) {
         tracing::warn!("Failed to restrict {}: {e}", path.display());
      }
   }
}

impl ZeusContext {
   pub fn new() -> Self {
      tighten_existing_secret_files();

      let client_manager = ClientManager::default();

      let currency_db = CurrencyDB::default();

      let nft_db = NftDB::default();

      let vault_exists = Vault::exists().is_ok_and(|p| p);

      let pool_manager = PoolManagerHandle::default();

      let price_manager = PriceManagerHandle::new();

      let delegated_wallets = DelegatedWallets::new();

      let disabled_chains = match DisabledChains::load_from_file() {
         Ok(chains) => chains,
         Err(e) => {
            tracing::error!("Failed to load disabled chains: {:?}", e);
            DisabledChains::default()
         }
      };

      let railgun_config = match RailgunConfig::load_from_file() {
         Ok(config) => config,
         Err(e) => {
            tracing::error!("Failed to load Railgun config: {:?}", e);
            RailgunConfig::default()
         }
      };

      let misc_config = match MiscConfig::load_from_file() {
         Ok(config) => config,
         Err(e) => {
            tracing::error!("Failed to load misc config: {:?}", e);
            MiscConfig::default()
         }
      };

      let priority_fee = PriorityFee::default();

      Self {
         client_manager,
         chain: ChainId::new(1).unwrap(),
         privacy_mode: false,
         railgun_resync_attempts: HashMap::new(),
         railgun_provider: HashMap::new(),
         current_wallet: WalletInfo::default(),
         wallet_info_cache: WalletInfoCache::default(),
         vault: Arc::new(Mutex::new(Vault::default())),
         wallet_state: WalletState::default(),
         tx_db: TxDBHandle::new(),
         security: SecuritySettings::default(),
         locked: false,
         last_activity_ms: 0,
         last_autolock_notice_ms: 0,
         autolock_notice_shown: false,
         save_vault_in_progress: false,
         save_wallet_state_in_progress: false,
         vault_exists,
         vault_unlocked: false,
         address_book: AddressBookHandle::default(),
         ens_cache: EnsCache::new(),
         currency_db,
         nft_db,
         pool_manager,
         price_manager,
         data_syncing: false,
         on_startup_syncing: false,
         state_sync: HashMap::with_capacity(SUPPORTED_CHAINS.len()),
         base_fee: HashMap::with_capacity(SUPPORTED_CHAINS.len()),
         latest_block: HashMap::with_capacity(SUPPORTED_CHAINS.len()),
         eth_calls: HashMap::with_capacity(CONNECTOR_CACHE_CAP),
         estimate_gas: HashMap::with_capacity(CONNECTOR_CACHE_CAP),
         codes: HashMap::with_capacity(CONNECTOR_CACHE_CAP),
         storage: HashMap::with_capacity(CONNECTOR_CACHE_CAP),
         transactions: HashMap::with_capacity(CONNECTOR_CACHE_CAP),
         receipts: HashMap::with_capacity(CONNECTOR_CACHE_CAP),
         blocks_by_hash: HashMap::with_capacity(CONNECTOR_CACHE_CAP),
         blocks_by_number: HashMap::with_capacity(CONNECTOR_CACHE_CAP),
         tx_counts: HashMap::with_capacity(CONNECTOR_CACHE_CAP),
         priority_fee,
         connected_dapps: ConnectedDapps::default(),
         delegated_wallets,
         server_port: SERVER_PORT,
         server_running: false,
         last_checked_for_available_rpcs: HashMap::new(),
         last_checked_for_malfunction: HashMap::new(),
         last_detected_malfunction: HashMap::new(),
         railgun_provider_sync_last_check: HashMap::new(),
         available_rpcs: HashMap::new(),
         disabled_chains,
         railgun_status: RailgunStatus::new(),
         railgun_config,
         misc_config,
      }
   }

   pub fn railgun_is_supported(&self, chain: ChainId) -> bool {
      railgun_supported(chain)
   }

   pub fn railgun_status(&self) -> &RailgunStatus {
      &self.railgun_status
   }

   /// Shared access to the vault without cloning its contents.
   pub fn read_vault<R>(&self, reader: impl FnOnce(&Vault) -> R) -> R {
      reader(&self.vault.lock().unwrap())
   }

   /// Mutable access to the vault.
   pub fn write_vault<R>(&self, writer: impl FnOnce(&mut Vault) -> R) -> R {
      writer(&mut self.vault.lock().unwrap())
   }

   /// Shared access to wallet app state.
   pub fn read_wallet_state<R>(
      &self,
      reader: impl FnOnce(&crate::core::WalletStateInner) -> R,
   ) -> R {
      self.wallet_state.read(reader)
   }

   /// Mutable access to wallet app state.
   pub fn write_wallet_state<R>(
      &self,
      writer: impl FnOnce(&mut crate::core::WalletStateInner) -> R,
   ) -> R {
      self.wallet_state.write(writer)
   }

   pub fn get_eth_balance(&self, chain: u64, owner: Address) -> NumericValue {
      self.wallet_state.read(|ws| ws.balance_manager.get_eth_balance(chain, owner))
   }

   pub fn get_token_balance(&self, chain: u64, owner: Address, token: Address) -> NumericValue {
      self
         .wallet_state
         .read(|ws| ws.balance_manager.get_token_balance(chain, owner, token))
   }

   /// How many of this NFT the wallet holds, or `None` when the balance manager has never been asked.
   ///
   /// A row must read `None` as "no claim" and `Some(0)` as `NOT OWNED`, which is why this is not the
   /// `NumericValue` the token getters return: for a token the two cases can be the same thing, for an
   /// NFT they cannot.
   pub fn get_nft_balance(
      &self,
      chain: u64,
      owner: Address,
      collection: Address,
      token_id: U256,
   ) -> Option<u64> {
      self
         .wallet_state
         .read(|ws| ws.balance_manager.get_nft_balance(chain, owner, collection, token_id))
   }

   pub fn get_base_fee(&self, chain: u64) -> Option<BaseFee> {
      self.base_fee.get(&chain).cloned()
   }

   pub fn get_currency_value_for_amount(&self, amount: f64, currency: &Currency) -> NumericValue {
      let price = self.get_currency_price(currency);
      NumericValue::value(amount, price.f64())
   }

   pub fn get_token_value_for_amount(&self, amount: f64, token: &ERC20Token) -> NumericValue {
      let price = self.get_token_price(token);
      NumericValue::value(amount, price.f64())
   }

   pub fn get_currency_balance(
      &self,
      chain: u64,
      owner: Address,
      currency: &Currency,
   ) -> NumericValue {
      if currency.is_native() {
         self.get_eth_balance(chain, owner)
      } else {
         let token = currency.erc20_opt().unwrap();
         self.get_token_balance(chain, owner, token.address)
      }
   }

   /// Get the USD price of a currency
   pub fn get_currency_price(&self, currency: &Currency) -> NumericValue {
      if currency.is_native() {
         let wrapped_token = ERC20Token::wrapped_native_token(currency.chain_id());
         self.get_token_price(&wrapped_token)
      } else {
         let token = currency.erc20_opt().unwrap();
         self.get_token_price(token)
      }
   }

   pub fn get_currency_value_for_owner(
      &self,
      chain: u64,
      owner: Address,
      currency: &Currency,
   ) -> NumericValue {
      let price = self.get_currency_price(currency);
      let balance = self.get_currency_balance(chain, owner, currency);
      NumericValue::value(balance.f64(), price.f64())
   }

   fn get_token_price(&self, token: &ERC20Token) -> NumericValue {
      self.price_manager.get_token_price(token).unwrap_or_default()
   }

   /// Return the name of this address if its known.
   ///
   /// Checks the address book, then the session ENS cache, then token metadata. No list scans.
   /// The zero address is never named (ERC-7730 placeholders must not label burns).
   ///
   /// The book comes first on purpose: a wallet, contact or contract label the user set (or that
   /// ERC-7730 gave us) outranks an ENS name resolved for the same address.
   pub fn get_address_name(&self, chain: u64, address: Address) -> Option<Arc<str>> {
      if address.is_zero() {
         return None;
      }

      if let Some(name) = self.address_book.get(chain, address) {
         return Some(name);
      }

      if let Some(name) = self.ens_cache.get(chain, address) {
         return Some(name);
      }

      if let Some(name) = self.currency_db.get_token_name(chain, address) {
         return Some(name);
      }

      // An NFT collection names itself: on-chain metadata, cached by the time a token of it was
      // discovered or added, so this needs no registry or Sourcify round trip. Last, because it is
      // self-reported and a book or ENS name outranks it. The confirmation window's "Contract
      // interaction" row reads this for the collection being called.
      let name = self.nft_db.get_collection(chain, address)?.name?;
      let name = name.trim();

      if name.is_empty() {
         return None;
      }

      Some(Arc::from(name))
   }

   /// Get the wallet info for the given zk address
   pub fn get_wallet_info_by_zk_address(&self, address: &str) -> Option<WalletInfo> {
      for wallet in self.wallet_info_cache.ordered_slice() {
         if wallet.zk_address_ref() == address {
            return Some(wallet.clone());
         }
      }
      None
   }

   /// Get the wallet name for the given address
   pub fn get_wallet_name(&self, address: Address) -> Option<String> {
      let wallet = self.wallet_info_cache.get(&address);
      wallet.map(|wallet| wallet.name().to_string())
   }

   pub fn current_wallet_info(&self) -> WalletInfo {
      let w = self.wallet_info_cache.get(&self.current_wallet.address).cloned();
      w.expect("Current Wallet Info should be in cache")
   }

   /// Get the wallet with the given address
   pub fn get_wallet(&self, address: Address) -> Option<Wallet> {
      self.read_vault(|vault| {
         vault
            .all_wallets()
            .into_iter()
            .find(|wallet| wallet.address() == address)
            .cloned()
      })
   }

   /// Clone the current wallet (including key material) from the vault.
   pub fn get_current_wallet(&self) -> Wallet {
      let w = self.get_wallet(self.current_wallet.address);
      w.expect("Current Wallet should be in cache")
   }

   /// Get all wallets info by this order:
   /// - Master wallet
   /// - Children wallets
   /// - Imported wallets
   pub fn all_wallets_info_ordered(&self) -> &[WalletInfo] {
      &self.wallet_info_cache.ordered_slice()
   }

   /// Is this wallet selected as the current wallet
   pub fn is_current_wallet(&self, address: Address) -> bool {
      self.current_wallet.address == address
   }

   /// Get a contact by it's zk address
   pub fn get_contact_by_zk_address(&self, zk_address: &str) -> Option<Contact> {
      let zk_address = zk_address.to_lowercase();
      self
         .wallet_state
         .read(|ws| ws.contacts.iter().find(|c| c.zk_address.to_lowercase() == zk_address).cloned())
   }

   /// Get a contact by it's address
   pub fn get_contact_by_address(&self, evm_address: &str) -> Option<Contact> {
      let evm_address = evm_address.to_lowercase();
      self.wallet_state.read(|ws| {
         ws.contacts
            .iter()
            .find(|c| c.evm_address.to_lowercase() == evm_address)
            .cloned()
      })
   }

   pub fn connected_dapps(&self) -> Vec<String> {
      self.connected_dapps.connected_dapps()
   }

   /// The account an app was connected with, if it has connected before.
   pub fn dapp_account(&self, origin: &str) -> Option<Address> {
      self.wallet_state.read(|ws| ws.dapp_accounts.get(origin))
   }

   pub fn connect_dapp(&mut self, dapp: String) {
      self.connected_dapps.connect_dapp(dapp);
   }

   pub fn disconnect_dapp(&mut self, dapp: &str) {
      self.connected_dapps.disconnect_dapp(dapp);
   }

   pub fn disconnect_all_dapps(&mut self) {
      self.connected_dapps.disconnect_all();
   }

   /// Get the total value for the given owner across all of its wallets and chains
   pub fn get_total_value(&self, owner: Address, include_testnets: bool) -> WalletValue {
      let mut total_public = 0.0;
      let mut total_private = 0.0;

      for chain in ChainId::supported_chains() {
         if !include_testnets && chain.is_testnet() {
            continue;
         }

         let portfolio = self.wallet_state.read(|ws| ws.portfolio_db.get(chain.id(), owner));
         total_public += portfolio.public_value().f64();
         total_private += portfolio.private_value().f64();
      }

      let owner_value = WalletValue {
         public: NumericValue::from_f64(total_public),
         private: NumericValue::from_f64(total_private),
      };

      owner_value
   }

   /// Disable the given chain
   ///
   /// It wont show up in the UI anymore and wont be used for RPC calls
   pub fn disable_chain(&mut self, chain: u64) {
      self.disabled_chains.disable(chain);
   }

   /// Enable the given chain
   ///
   /// It will show up in the UI again and will be used for RPC calls
   pub fn enable_chain(&mut self, chain: u64) {
      self.disabled_chains.enable(chain);
   }

   pub fn is_chain_disabled(&self, chain: u64) -> bool {
      self.disabled_chains.is_disabled(chain)
   }

   /// Check the address book to see if we have ever requested a name for this address
   ///
   /// # Returns
   /// `true` if we have ever requested a name for this address
   pub fn address_name_requested(&self, chain: u64, address: Address) -> bool {
      if address.is_zero() {
         return true;
      }
      let first = self.address_book.mark_pending(chain, address);
      !first
   }

   /// Check if we have any enabled and working RPCs for the given chain
   ///
   /// Returns true if we have at least one enabled and working RPC
   pub fn check_for_available_rpcs(&mut self, now_millis: u64, chain: u64, threshold: u64) -> bool {
      let last_checked = self.last_checked_for_available_rpcs.get(&chain).cloned().unwrap_or(0);

      let should_check = now_millis.saturating_sub(last_checked) > threshold;

      if should_check {
         let ok = self.client_manager.rpc_available(chain);
         self.last_checked_for_available_rpcs.insert(chain, now_millis);
         self.available_rpcs.insert(chain, ok);

         return ok;
      }

      self.available_rpcs.get(&chain).cloned().unwrap_or(false)
   }

   /// Check if any enabled, already-tested RPC is not fully functional.
   ///
   /// Returns true at most once per `toast_duration_ms` while the problem lasts, so the
   /// caller can enqueue a toast without stacking a new one every frame.
   pub fn check_for_malfunction(
      &mut self,
      toast_duration_ms: u64,
      now_millis: u64,
      chain: u64,
      threshold: u64,
   ) -> bool {
      if self.state_sync.get(&chain).copied().unwrap_or(false) {
         return false;
      }

      let last_checked = self.last_checked_for_malfunction.get(&chain).copied().unwrap_or(0);
      if now_millis.saturating_sub(last_checked) <= threshold {
         return false;
      }

      self.last_checked_for_malfunction.insert(chain, now_millis);

      if self.client_manager.rpcs_fully_functional(chain) {
         return false;
      }

      let last_detected = self.last_detected_malfunction.get(&chain).copied().unwrap_or(0);
      if now_millis.saturating_sub(last_detected) <= toast_duration_ms {
         return false;
      }

      self.last_detected_malfunction.insert(chain, now_millis);
      true
   }

   /// Whether an "auto-lock is not configured" toast is due.
   ///
   /// Fires at most once per `repeat_ms`, and the first one only after
   /// `first_delay_ms` from the seeded timestamp (both are set at unlock).
   /// Returns false once the user has made an explicit choice.
   pub fn should_warn_autolock(
      &mut self,
      now_millis: u64,
      first_delay_ms: u64,
      repeat_ms: u64,
   ) -> bool {
      if self.security.autolock_changed || self.locked {
         return false;
      }

      let delay = if self.autolock_notice_shown {
         repeat_ms
      } else {
         first_delay_ms
      };

      if now_millis.saturating_sub(self.last_autolock_notice_ms) < delay {
         return false;
      }

      self.last_autolock_notice_ms = now_millis;
      self.autolock_notice_shown = true;
      true
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

#[cfg(test)]
mod tests {
   use super::*;
   use std::str::FromStr;
   use zeus_eth::{
      alloy_primitives::{U256, utils::format_units},
      alloy_provider::Provider,
      alloy_rpc_types::BlockId,
      types::SUPPORTED_CHAINS,
   };

   #[tokio::test]
   #[should_panic]
   async fn test_must_panic_if_no_mev_protect_client() {
      let ctx = ZeusCtx::new();
      let _r = ctx.get_mev_protect_client(1).await.unwrap();
   }

   /// the "auto-lock is not configured" nudge comes once ~30 s after
   /// unlock, then at most hourly, and never once the user made a choice.
   #[test]
   fn autolock_notice_cadence() {
      const FIRST: u64 = 30_000;
      const REPEAT: u64 = 3_600_000;

      let ctx = ZeusCtx::new();
      ctx.write(|ctx| {
         ctx.security.autolock_changed = false;
         ctx.locked = false;
         ctx.last_autolock_notice_ms = 1_000;
         ctx.autolock_notice_shown = false;
      });

      let warn = |now: u64| ctx.write(|ctx| ctx.should_warn_autolock(now, FIRST, REPEAT));

      assert!(
         !warn(1_000 + FIRST - 1),
         "too early for the first notice"
      );
      assert!(warn(1_000 + FIRST), "first notice is due");
      assert!(
         !warn(1_000 + FIRST + REPEAT - 1),
         "within the repeat interval"
      );
      assert!(warn(1_000 + FIRST + REPEAT), "repeat is due");

      ctx.write(|ctx| ctx.security.autolock_changed = true);
      assert!(
         !warn(1_000 + 100 * REPEAT),
         "an explicit choice stops it"
      );

      ctx.write(|ctx| {
         ctx.security.autolock_changed = false;
         ctx.locked = true;
      });
      assert!(!warn(1_000 + 100 * REPEAT), "locked never warns");
   }

   /// The unlock guard refuses while locked and passes while unlocked.
   #[test]
   fn ensure_unlocked_refuses_while_locked() {
      let ctx = ZeusCtx::new();

      ctx.write(|ctx| ctx.locked = false);
      assert!(ctx.ensure_unlocked().is_ok());

      ctx.write(|ctx| ctx.locked = true);
      let err = ctx.ensure_unlocked().unwrap_err();
      assert!(
         err.to_string().contains("locked"),
         "unexpected error: {err}"
      );
   }

   /// A collection names itself, and the confirmation window's "Contract interaction" row reads this to
   /// label the contract being called. The name is cached by discovery, so it has to come out with no
   /// registry or Sourcify round trip — and a curated label still outranks it.
   #[test]
   fn get_address_name_resolves_a_cached_nft_collection() {
      use zeus_eth::nft::{NftCollection, NftStandard};

      let ctx = ZeusCtx::new();
      let collection = Address::from([0x3e; 20]);

      assert_eq!(ctx.get_address_name(1, collection), None);

      ctx.write(|ctx| {
         ctx.nft_db.insert_collection(
            1,
            NftCollection {
               chain_id: 1,
               address: collection,
               standard: NftStandard::Erc721,
               name: Some("Zeus Test Collection".to_string()),
               symbol: Some("ZTC".to_string()),
            },
         );
      });

      assert_eq!(
         ctx.get_address_name(1, collection).as_deref(),
         Some("Zeus Test Collection")
      );

      // The same address on another chain, where nothing is cached.
      assert_eq!(ctx.get_address_name(10, collection), None);

      // A curated address-book label outranks a contract's self-reported name.
      ctx.address_book().insert_contract(1, collection, "Curated");
      assert_eq!(
         ctx.get_address_name(1, collection).as_deref(),
         Some("Curated")
      );

      // A collection whose `name()` returned nothing names nothing.
      let unnamed = Address::from([0x4f; 20]);
      ctx.write(|ctx| {
         ctx.nft_db.insert_collection(
            1,
            NftCollection {
               chain_id: 1,
               address: unnamed,
               standard: NftStandard::Erc721,
               name: None,
               symbol: None,
            },
         );
      });

      assert_eq!(ctx.get_address_name(1, unnamed), None);
   }

   #[tokio::test]
   async fn test_base_fee() {
      let ctx = ZeusCtx::new();

      let client = ctx.get_client(1).await.unwrap();
      let block = client.get_block(BlockId::latest()).await.unwrap().unwrap();
      let base_fee = block.header.base_fee_per_gas.unwrap();
      let fee = format_units(base_fee, "gwei").unwrap();
      println!("Ethereum base fee: {}", fee);

      let client = ctx.get_client(10).await.unwrap();
      let gas_price = client.get_gas_price().await.unwrap();
      let fee = format_units(gas_price, "gwei").unwrap();
      println!("Optimism base fee: {}", fee);

      let client = ctx.get_client(56).await.unwrap();
      let gas_price = client.get_gas_price().await.unwrap();
      let fee = format_units(gas_price, "gwei").unwrap();
      println!("BSC base fee: {}", fee);

      let client = ctx.get_client(42161).await.unwrap();
      let gas_price = client.get_gas_price().await.unwrap();
      let fee = format_units(gas_price, "gwei").unwrap();
      println!("Arbitrum base fee: {}", fee);
   }

   #[tokio::test]
   async fn test_extract_delegated_address() {
      let ctx = ZeusCtx::new();

      let chain = 1;
      let account = Address::from_str("0x67d3FA6a5CF45D85F697A497b3270A06415E5BfE").unwrap();
      let delegated_address =
         Address::from_str("0x63c0c19a282a1B52b07dD5a65b58948A07DAE32B").unwrap();

      let client = ctx.get_client(chain).await.unwrap();
      let code = client.get_code_at(account).await.unwrap();
      eprintln!("Code {}", code);
      eprintln!("Code length: {}", code.len());

      let addr_slice = &code[3..];
      let address = Address::from_slice(&addr_slice);
      eprintln!("Address: {}", address);

      assert_eq!(address, delegated_address);
   }

   #[tokio::test]
   async fn test_priority_fee_suggestion() {
      let ctx = ZeusCtx::new();

      for chain in SUPPORTED_CHAINS {
         let client = ctx.get_client(chain).await.unwrap();
         let fee = client.get_max_priority_fee_per_gas().await.unwrap();
         let fee = format_units(U256::from(fee), "gwei").unwrap();
         println!("Suggested Fee on {}: {}", chain, fee)
      }
   }
}
