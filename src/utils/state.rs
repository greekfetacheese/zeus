use crate::core::ctx::railgun_dir;
use crate::core::{WalletPortfolio, ZeusCtx, types::BaseFee};
use crate::utils::{RT, malloc_trim};
use anyhow::anyhow;
use tracing::{debug, error, info, warn};

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;
use zeus_eth::{
   alloy_primitives::U256,
   alloy_provider::Provider,
   alloy_rpc_types::BlockId,
   types::{ChainId, SUPPORTED_CHAINS},
   utils::{NumericValue, block::calculate_next_block_base_fee},
};
use zeus_railgun::{Groth16Prover, PrefetchReport, RailgunSigner};

const MALLOC_TRIM_INTERVAL: u64 = 300;
const MEASURE_RPCS_INTERVAL: u64 = 200;
const WALLET_STATE_INTERVAL: u64 = 600;
const FEE_INTERVAL: u64 = 60;
const RAILGUN_SYNC_INTERVAL: u64 = 60;

pub async fn test_and_measure_rpcs(ctx: ZeusCtx) {
   let client = ctx.get_zeus_client();

   let mut tasks = Vec::new();
   let semaphore = Arc::new(Semaphore::new(5));

   let time = std::time::Instant::now();
   for chain in SUPPORTED_CHAINS {
      if ctx.is_chain_disabled(chain) {
         continue;
      }

      let rpcs = client.get_rpcs(chain);

      for (_url, rpc) in rpcs {
         let client = client.clone();
         let semaphore = semaphore.clone();

         if rpc.should_run_check() && rpc.is_enabled() {
            let ctx = ctx.clone();

            let task = RT.spawn(async move {
               let _permit = semaphore.acquire().await.unwrap();
               client.run_check_for(ctx, rpc).await;
            });
            tasks.push(task);
         } else if rpc.is_enabled() {
            let task = RT.spawn(async move {
               let _permit = semaphore.acquire().await.unwrap();
               client.run_latency_check_for(rpc).await;
            });
            tasks.push(task);
         }
      }
   }

   for task in tasks {
      let _ = task.await;
   }

   client.sort_by_fastest();

   info!(
      "RPC checks took {} secs",
      time.elapsed().as_secs_f32()
   );
}

/// Do a full state sync for the given chain
pub async fn sync_state(ctx: ZeusCtx, chain: u64) {
   if ctx.is_chain_disabled(chain) {
      return;
   }

   if ctx.is_chain_syncing(chain) {
      return;
   }

   ctx.set_chain_syncing(chain, true);

   let time = Instant::now();

   let z_client = ctx.get_zeus_client();
   let available_rpcs = z_client.rpc_available(chain);

   if !available_rpcs {
      warn!(
         "No RPCs available for chain {}, skipping state sync",
         chain
      );
      return;
   }

   update_token_balances(ctx.clone(), chain, false).await;
   update_token_prices(ctx.clone(), chain, false).await;

   let wallets = ctx.get_all_wallets_info();
   let addresses = wallets.iter().map(|w| w.address).collect::<Vec<_>>();

   for addr in &addresses {
      ctx.update_public_data(chain, *addr);
   }

   if let Err(e) = ctx.register_railgun_signers(chain, false).await {
      error!("Error registering Railgun signers: {:?}", e);
   }

   if let Err(e) = ctx.sync_railgun(chain, false).await {
      error!("Error syncing Railgun: {:?}", e);
   }

   for addr in addresses {
      ctx.update_private_data(chain, addr).await;
   }

   if let Err(e) = update_priority_fee(ctx.clone(), chain).await {
      warn!(
         "Error updating priority fee for chain {}: {:?}",
         chain, e
      );
   }

   if let Err(e) = get_base_fee(ctx.clone(), chain).await {
      warn!("Error updating base fee: {:?}", e);
   }

   check_delegated_status(ctx.clone(), chain).await;

   ctx.save_price_manager();
   ctx.save_pool_manager();

   insert_missing_portfolios(ctx.clone(), chain);

   ctx.set_chain_syncing(chain, false);

   info!(
      "Synced state for chain {} in {} ms",
      chain,
      time.elapsed().as_millis()
   );
}

pub async fn on_startup(ctx: ZeusCtx) {
   ctx.write(|ctx| {
      ctx.on_startup_syncing = true;
   });

   cleanup_orphaned_wallet_data(ctx.clone());

   let mut tasks = Vec::new();

   for chain in SUPPORTED_CHAINS {
      let semaphore = Arc::new(Semaphore::new(2));
      let ctx = ctx.clone();

      let task = RT.spawn(async move {
         let _ = semaphore.acquire().await.unwrap();
         sync_state(ctx, chain).await;
      });

      tasks.push(task);
   }

   // Prefetch all transact circuit artifacts (01x01 ..= 05x05) into the
   // on-disk cache so first prove / merge does not hit the network cold.
   RT.spawn(async move {
      match prefetch_railgun_circuits().await {
         Ok(report) => {
            info!(
               "Railgun circuit prefetch: {} ready ({} embedded, {} disk, {} downloaded), {} failed",
               report.ok_count(),
               report.embedded.len(),
               report.already_cached.len(),
               report.downloaded.len(),
               report.failed_count()
            );
            for (name, err) in &report.failed {
               warn!("Circuit prefetch failed for {}: {}", name, err);
            }
         }
         Err(e) => error!("Railgun circuit prefetch error: {:?}", e),
      }
   });

   for task in tasks {
      let _ = task.await;
   }

   ctx.write(|ctx| {
      ctx.on_startup_syncing = false;
   });

   malloc_trim();

   let ctx_clone = ctx.clone();
   RT.spawn(async move {
      state_update_interval(ctx_clone).await;
   });

   RT.spawn(async move {
      malloc_trim_interval().await;
   });
}

/// Remove BalanceManager / PortfolioDB / TxDB / ApprovalManager entries for
/// addresses that are no longer present as wallets in the vault.
pub fn cleanup_orphaned_wallet_data(ctx: ZeusCtx) {
   let wallets: HashSet<_> = ctx.get_all_wallets_info().into_iter().map(|w| w.address).collect();

   let (
      eth_removed,
      token_removed,
      portfolio_removed,
      tx_removed,
      approval_token_removed,
      approval_permit_removed,
   ) = ctx.write_wallet_state(|ws| {
      let (eth_removed, token_removed) = ws.balance_manager.retain_wallets(&wallets);
      let portfolio_removed = ws.portfolio_db.retain_wallets(&wallets);
      let tx_removed = ws.tx_db.retain_wallets(&wallets);
      let (approval_token_removed, approval_permit_removed) =
         ws.approval_manager.retain_wallets(&wallets);
      (
         eth_removed,
         token_removed,
         portfolio_removed,
         tx_removed,
         approval_token_removed,
         approval_permit_removed,
      )
   });

   let total = eth_removed
      + token_removed
      + portfolio_removed
      + tx_removed
      + approval_token_removed
      + approval_permit_removed;
   if total > 0 {
      info!(
         "Cleaned orphaned wallet data: {} eth balances, {} token balances, {} portfolios, {} tx histories, {} token approvals, {} permits",
         eth_removed,
         token_removed,
         portfolio_removed,
         tx_removed,
         approval_token_removed,
         approval_permit_removed
      );
   } else {
      debug!("No orphaned wallet data to clean");
   }

   RT.spawn(async move {
      let wallets = ctx.read_vault(|vault| vault.clone_all_wallets());
      let mut keep_signers = Vec::new();

      for wallet in wallets {
         if let Ok(seed) = wallet.seed() {
            let signer = RailgunSigner::from_seed(&seed, 0, 1).expect("invalid seed");
            keep_signers.push(signer);
         }
      }

      let timeout = Duration::from_secs(120);
      let start = Instant::now();

      for chain in ChainId::supported_chains() {
         if !ctx.railgun_is_supported(chain) {
            continue;
         }

         loop {
            if Instant::now().duration_since(start) > timeout {
               error!(
                  "Timed out waiting for Railgun provider on chain {}",
                  chain.id()
               );
               break;
            }

            let ready = ctx.read(|ctx| ctx.railgun_provider.get(&chain.id()).is_some());

            if !ready {
               tokio::time::sleep(Duration::from_millis(500)).await;
               continue;
            }

            let provider = match ctx.get_railgun_provider(chain.id(), false).await {
               Ok(provider) => provider,
               Err(e) => {
                  error!("Error getting Railgun provider: {:?}", e);
                  tokio::time::sleep(Duration::from_millis(500)).await;
                  continue;
               }
            };

            match provider.cleanup_orphaned_accounts(&keep_signers).await {
               Ok(removed) => {
                  info!(
                     "Cleaned {} orphaned Railgun account(s) on chain {}",
                     removed,
                     chain.id()
                  );
               }
               Err(e) => {
                  error!(
                     "Error cleaning orphaned Railgun accounts: {:?}",
                     e
                  );
               }
            }

            break;
         }
      }
   });
}

fn insert_missing_portfolios(ctx: ZeusCtx, chain: u64) {
   if ctx.is_chain_disabled(chain) {
      return;
   }

   while !ctx.vault_unlocked() {
      std::thread::sleep(Duration::from_millis(100));
   }

   let wallets = ctx.get_all_wallets_info();

   for wallet in &wallets {
      let has_portfolio = ctx.has_portfolio(chain, wallet.address);
      let balance = ctx.get_eth_balance(chain, wallet.address);
      if !balance.is_zero() && !has_portfolio {
         let portfolio = WalletPortfolio::new(wallet.address, chain);
         ctx.write_wallet_state(|ws| {
            ws.portfolio_db.insert_portfolio(chain, wallet.address, portfolio);
         });
      }
   }

   let portfolios = ctx.read_wallet_state(|ws| ws.portfolio_db.get_all(chain));
   for portfolio in &portfolios {
      ctx.update_public_data(chain, portfolio.owner());
   }
}

/// Check the delegated status for all wallets for the given chain
async fn check_delegated_status(ctx: ZeusCtx, chain: u64) {
   let accounts = ctx.get_all_wallets_info();

   if ctx.is_chain_disabled(chain) {
      return;
   }

   let ctx = ctx.clone();
   let accounts = accounts.clone();

   for account in &accounts {
      if ctx.should_check_delegated_wallet_status(chain, account.address) {
         match ctx.check_delegated_wallet_status(chain, account.address).await {
            Ok(_) => {
               #[cfg(feature = "debug")]
               debug!(
                  "Checked delegated wallet status for {}",
                  account.address
               )
            }
            Err(e) => error!("Error checking delegated wallet status: {:?}", e),
         }
      }
   }
}

// TODO: Improve the efficiency of the batch calls, right now is not worth doing
// TODO: a full sync
/// Update the token balances for all the wallet portfolios for the given chain
///
/// - Arguments:
///    - ctx: The Zeus context
///    - chain: The chain ID
///    - update_for_all: If true, update the balances for all the ERC20 tokens known to Zeus
pub async fn update_token_balances(ctx: ZeusCtx, chain: u64, update_for_all: bool) {
   if ctx.is_chain_disabled(chain) {
      return;
   }

   let balance_manager = ctx.balance_manager();
   let wallets_info = ctx.get_all_wallets_info();
   let wallets = wallets_info.iter().map(|w| w.address).collect::<Vec<_>>();

   let portfolio_tokens = ctx.get_all_tokens_from_portfolios(chain);

   let mut inserted = HashSet::new();
   let mut tokens = Vec::new();

   if update_for_all {
      let currencies = ctx.get_currencies(chain);

      for curr in &currencies {
         let token = curr.to_erc20().into_owned();

         if inserted.contains(&token.address) {
            continue;
         }

         inserted.insert(token.address);
         tokens.push(token);
      }
   }

   for token in portfolio_tokens {
      if inserted.contains(&token.address) {
         continue;
      }

      inserted.insert(token.address);
      tokens.push(token);
   }

   if let Err(e) = balance_manager
      .update_eth_balance(ctx.clone(), chain, wallets.clone(), false)
      .await
   {
      error!("Error updating eth balance: {:?}", e);
   }

   for wallet in wallets {
      if let Err(e) = balance_manager
         .update_tokens_balance(ctx.clone(), chain, wallet, tokens.clone(), false)
         .await
      {
         error!("Error updating tokens balance: {:?}", e);
      }
   }
}

/// Update the token prices from all the wallet portfolios for the given chain
///
/// - Arguments:
///    - ctx: The Zeus context
///    - chain: The chain ID
///    - update_for_all: If true, update the prices for all the ERC20 tokens known to Zeus
pub async fn update_token_prices(ctx: ZeusCtx, chain: u64, update_for_all: bool) {
   if ctx.is_chain_disabled(chain) {
      return;
   }

   let price_manager = ctx.price_manager();
   let pool_manager = ctx.pool_manager();

   let portfolio_tokens = ctx.get_all_tokens_from_portfolios(chain);
   let mut inserted = HashSet::new();
   let mut tokens = Vec::new();

   if update_for_all {
      let currencies = ctx.get_currencies(chain);

      for curr in &currencies {
         let token = curr.to_erc20().into_owned();

         if token.is_base() || inserted.contains(&token.address) {
            continue;
         }

         inserted.insert(token.address);
         tokens.push(token);
      }
   }

   for token in portfolio_tokens {
      if token.is_base() || inserted.contains(&token.address) {
         continue;
      }

      inserted.insert(token.address);
      tokens.push(token);
   }

   if let Err(e) = price_manager.update_base_token_prices(ctx.clone(), chain).await {
      error!(
         "Error updating base token prices for chain {}: {:?}",
         chain, e
      );
   }

   if let Err(e) = price_manager.calculate_prices(ctx, chain, pool_manager, tokens).await {
      error!(
         "Error updating token prices for chain {}: {:?}",
         chain, e
      );
   }
}

async fn malloc_trim_interval() {
   let mut malloc_trim_passed = Instant::now();

   loop {
      if malloc_trim_passed.elapsed().as_secs() > MALLOC_TRIM_INTERVAL {
         malloc_trim();
         malloc_trim_passed = Instant::now();
      }

      tokio::time::sleep(Duration::from_secs(10)).await;
   }
}

async fn state_update_interval(ctx: ZeusCtx) {
   let mut wallet_state_passed = Instant::now();
   let mut fee_time_passed = Instant::now();
   let mut rpc_measure_time_passed = Instant::now();
   let mut railgun_sync_time_passed = Instant::now();

   loop {
      if wallet_state_passed.elapsed().as_secs() > WALLET_STATE_INTERVAL {
         let manager = ctx.balance_manager();
         manager.update_eth_balance_across_wallets_and_chains(ctx.clone()).await;
         manager.update_tokens_balance_across_wallets_and_chains(ctx.clone()).await;

         for chain in SUPPORTED_CHAINS {
            if ctx.is_chain_disabled(chain) {
               continue;
            }

            update_token_prices(ctx.clone(), chain, false).await;

            let portfolios = ctx.read_wallet_state(|ws| ws.portfolio_db.get_all(chain));
            for portfolio in &portfolios {
               ctx.update_public_data(chain, portfolio.owner());
            }

            check_delegated_status(ctx.clone(), chain).await;
         }

         ctx.save_price_manager();
         wallet_state_passed = Instant::now();
      }

      if fee_time_passed.elapsed().as_secs() > FEE_INTERVAL {
         for chain in SUPPORTED_CHAINS {
            if ctx.is_chain_disabled(chain) {
               continue;
            }

            if let Err(e) = update_priority_fee(ctx.clone(), chain).await {
               warn!(
                  "Error updating priority fee for chain {}: {:?}",
                  chain, e
               );
            }

            if let Err(e) = get_base_fee(ctx.clone(), chain).await {
               error!("Error updating base fee: {:?}", e);
            }
         }
         fee_time_passed = Instant::now();
      }

      if railgun_sync_time_passed.elapsed().as_secs() > RAILGUN_SYNC_INTERVAL {
         let ctx_clone = ctx.clone();
         RT.spawn(async move {
            for chain in SUPPORTED_CHAINS {
               let error = ctx_clone.read(|ctx| ctx.railgun_status.sync_error(chain));

               // Only do a resync if we detect an invalid root error
               let is_invalid_root = error.map(|e| e.contains("Invalid root")).unwrap_or(false);

               if is_invalid_root {
                  match ctx_clone.resync_railgun(chain).await {
                     Ok(_) => {
                        info!(
                           "Railgun resynced to valid root for chain {}",
                           chain
                        );
                     }
                     Err(e) => error!("Error syncing Railgun: {:?}", e),
                  }
               } else {
                  if let Err(e) = ctx_clone.register_railgun_signers(chain, false).await {
                     error!("Error registering Railgun signers: {:?}", e);
                  }

                  if let Err(e) = ctx_clone.sync_railgun(chain, false).await {
                     error!("Error syncing Railgun: {:?}", e);
                  }
               }
            }
         });
         railgun_sync_time_passed = Instant::now();
      }

      if rpc_measure_time_passed.elapsed().as_secs() > MEASURE_RPCS_INTERVAL {
         let z_client = ctx.get_zeus_client();
         z_client.run_latency_checks(ctx.clone()).await;
         rpc_measure_time_passed = Instant::now();
      }

      tokio::time::sleep(Duration::from_secs(1)).await;
   }
}

pub async fn get_base_fee(ctx: ZeusCtx, chain: u64) -> Result<BaseFee, anyhow::Error> {
   if ctx.is_chain_disabled(chain) {
      return Ok(BaseFee::new(0, 0));
   }

   let z_client = ctx.get_zeus_client();
   let chain = ChainId::new(chain)?;

   if chain.is_ethereum() {
      let block = z_client
         .request(chain.id(), |client| async move {
            client.get_block(BlockId::latest()).await.map_err(|e| anyhow!("{:?}", e))
         })
         .await?;

      if let Some(block) = block {
         let base_fee = block.header.base_fee_per_gas.unwrap_or_default();
         let next_base_fee = calculate_next_block_base_fee(block);
         ctx.update_base_fee(chain.id(), base_fee, next_base_fee);

         return Ok(BaseFee::new(base_fee, next_base_fee));
      } else {
         return Err(anyhow!("Latest block not found"));
      }
   }

   let gas_price = z_client
      .request(chain.id(), |client| async move {
         client.get_gas_price().await.map_err(|e| anyhow!("{:?}", e))
      })
      .await?;

   let fee: u64 = gas_price.try_into()?;

   #[cfg(feature = "debug")]
   let fee_gwei = NumericValue::format_to_gwei(U256::from(fee));

   #[cfg(feature = "debug")]
   debug!(
      "Base fee for chain {} is {}",
      chain.id(),
      fee_gwei.formatted()
   );

   ctx.update_base_fee(chain.id(), fee, fee);
   Ok(BaseFee::new(fee, fee))
}

pub async fn update_priority_fee(ctx: ZeusCtx, chain: u64) -> Result<(), anyhow::Error> {
   if ctx.is_chain_disabled(chain) {
      return Ok(());
   }

   let z_client = ctx.get_zeus_client();
   let chain = ChainId::new(chain)?;
   if chain.supports_type_2_tx() {
      let fee = z_client
         .request(chain.id(), |client| async move {
            client.get_max_priority_fee_per_gas().await.map_err(|e| anyhow!("{:?}", e))
         })
         .await?;

      let fee_value = NumericValue::format_to_gwei(U256::from(fee));

      if fee_value.is_zero() {
         return Err(anyhow!(
            "Rpc returned bad data, Fee (Wei) {} For Chain: {}",
            fee,
            chain.id()
         ));
      }

      #[cfg(feature = "debug")]
      debug!(
         "Priority fee for chain {} is {}",
         chain.id(),
         fee_value.formatted()
      );

      ctx.update_priority_fee(chain.id(), fee_value);
   }
   Ok(())
}

/// Prefetch pack circuits (`railgun/01x01` ..= `05x05`) into the Zeus
/// railgun data directory. Skips circuits already complete on disk.
async fn prefetch_railgun_circuits() -> Result<PrefetchReport, anyhow::Error> {
   let dir = railgun_dir()?;
   let prover = Groth16Prover::new(Some(dir))
      .with_embedded_circuits(crate::embedded::railgun::embedded_circuits());
   Ok(prover.prefetch_artifacts().await?)
}
