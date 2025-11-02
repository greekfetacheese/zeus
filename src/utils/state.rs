use crate::core::{BaseFee, ZeusCtx, context::Portfolio};
use crate::utils::RT;
use anyhow::anyhow;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};
use zeus_eth::{
   alloy_primitives::U256,
   alloy_provider::Provider,
   alloy_rpc_types::BlockId,
   currency::{Currency, ERC20Token},
   types::{ChainId, SUPPORTED_CHAINS},
   utils::{NumericValue, block::calculate_next_block_base_fee},
};

use tokio::sync::Semaphore;

const MEASURE_RPCS_INTERVAL: u64 = 200;
const WALLET_STATE_INTERVAL: u64 = 600;
const FEE_INTERVAL: u64 = 60;

pub async fn test_and_measure_rpcs(ctx: ZeusCtx) {
   let client = ctx.get_zeus_client();

   let mut tasks = Vec::new();
   let semaphore = Arc::new(Semaphore::new(5));

   let time = std::time::Instant::now();
   for chain in SUPPORTED_CHAINS {
      let rpcs = client.read(|rpcs| rpcs.get(&chain).unwrap_or(&vec![]).clone());

      for rpc in &rpcs {
         let client = client.clone();
         let semaphore = semaphore.clone();

         if rpc.should_run_check() {
            let rpc = rpc.clone();
            let ctx = ctx.clone();

            let task = RT.spawn(async move {
               let _permit = semaphore.acquire().await.unwrap();
               client.run_check_for(ctx, rpc).await;
            });
            tasks.push(task);
         } else {
            let rpc = rpc.clone();
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

   RT.spawn_blocking(move || {
      ctx.save_zeus_client();
   });

   tracing::info!(
      "RPC checks took {} secs",
      time.elapsed().as_secs_f32()
   );
}

pub async fn on_startup(ctx: ZeusCtx) {
   ctx.write(|ctx| {
      ctx.on_startup_syncing = true;
   });

   for chain in SUPPORTED_CHAINS {
      let ctx2 = ctx.clone();
      RT.spawn(async move {
         match update_priority_fee(ctx2, chain).await {
            Ok(_) => {
               tracing::trace!("Updated priority fee for chain: {}", chain)
            }
            Err(e) => tracing::error!("Error updating priority fee: {:?}", e),
         }
      });
   }

   let balance_manager = ctx.balance_manager();

   let eth_balance_fut = balance_manager.update_eth_balance_across_wallets_and_chains(ctx.clone());
   let token_balance_fut =
      balance_manager.update_tokens_balance_across_wallets_and_chains(ctx.clone());

   if ctx.pools_need_resync() {
      resync_pools(ctx.clone()).await;
   } else {
      update_token_prices(ctx.clone()).await;
   }

   eth_balance_fut.await;
   token_balance_fut.await;

   // Calculate the portfolio value for all chains
   let ctx_clone = ctx.clone();
   RT.spawn_blocking(move || {
      for chain in SUPPORTED_CHAINS {
         let ctx_clone = ctx_clone.clone();
         let portfolios = ctx_clone.read(|ctx| ctx.portfolio_db.get_all(chain));
         for portfolio in &portfolios {
            ctx_clone.calculate_portfolio_value(chain, portfolio.owner);
         }
      }
      ctx_clone.write(|ctx| {
         ctx.on_startup_syncing = false;
      });
   });

   // Update the base fee for all chains
   for chain in SUPPORTED_CHAINS {
      let ctx = ctx.clone();
      RT.spawn(async move {
         match get_base_fee(ctx.clone(), chain).await {
            Ok(_) => {
               tracing::trace!("Updated base fee for chain: {}", chain)
            }
            Err(e) => tracing::error!("Error updating base fee: {:?}", e),
         }
      });
   }

   let ctx_clone = ctx.clone();
   RT.spawn(async move {
      check_smart_account_status(ctx_clone).await;
   });

   let ctx_clone = ctx.clone();
   RT.spawn_blocking(move || {
      insert_missing_portfolios(ctx_clone);
   });

   let ctx_clone = ctx.clone();
   RT.spawn(async move {
      state_update_interval(ctx_clone).await;
   });

   RT.spawn_blocking(move || {
      ctx.save_all();
   });
}

fn insert_missing_portfolios(ctx: ZeusCtx) {
   while !ctx.vault_unlocked() {
      std::thread::sleep(Duration::from_millis(100));
   }

   let wallets = ctx.get_all_wallets_info();
   for chain in SUPPORTED_CHAINS {
      for wallet in &wallets {
         let has_portfolio = ctx.has_portfolio(chain, wallet.address);
         let balance = ctx.get_eth_balance(chain, wallet.address);
         if !balance.is_zero() && !has_portfolio {
            let portfolio = Portfolio::new(wallet.address, chain);
            ctx.write(|ctx| {
               ctx.portfolio_db.insert_portfolio(chain, wallet.address, portfolio);
            });
         }
      }
   }

   for chain in SUPPORTED_CHAINS {
      let portfolios = ctx.read(|ctx| ctx.portfolio_db.get_all(chain));
      for portfolio in &portfolios {
         ctx.calculate_portfolio_value(chain, portfolio.owner);
      }
   }
}

/// Check the smart account status for all wallets across all chains
async fn check_smart_account_status(ctx: ZeusCtx) {
   let accounts = ctx.get_all_wallets_info();
   let mut tasks = Vec::new();

   for chain in SUPPORTED_CHAINS {
      let ctx = ctx.clone();
      let accounts = accounts.clone();

      let task = RT.spawn(async move {
         for account in &accounts {
            if ctx.should_check_smart_account_status(chain, account.address) {
               match ctx.check_smart_account_status(chain, account.address).await {
                  Ok(_) => {}
                  Err(e) => tracing::error!("Error checking smart account status: {:?}", e),
               }
            }
         }
      });
      tasks.push(task);
   }

   for task in tasks {
      let _ = task.await;
   }
}

/// Update the token prices across all chains
async fn update_token_prices(ctx: ZeusCtx) {
   let mut tasks = Vec::new();
   for chain in SUPPORTED_CHAINS {
      let ctx = ctx.clone();
      let task = RT.spawn(async move {
         let price_manager = ctx.price_manager();
         let pool_manager = ctx.pool_manager();

         let portfolio_tokens = ctx.get_all_tokens_from_portfolios(chain);
         let mut inserted = HashSet::new();
         let mut tokens = Vec::new();

         for token in portfolio_tokens {
            if token.is_base() || inserted.contains(&token.address) {
               continue;
            }

            inserted.insert(token.address);
            tokens.push(token);
         }

         match price_manager.update_base_token_prices(ctx.clone(), chain).await {
            Ok(_) => tracing::trace!("Updated base token prices for chain: {}", chain),
            Err(e) => tracing::error!(
               "Error updating base token prices for chain {}: {:?}",
               chain,
               e
            ),
         }

         match price_manager.calculate_prices(ctx, chain, pool_manager, tokens).await {
            Ok(_) => tracing::trace!("Updated token prices for chain: {}", chain),
            Err(e) => tracing::error!(
               "Error updating token prices for chain {}: {:?}",
               chain,
               e
            ),
         }
      });
      tasks.push(task);
   }

   for task in tasks {
      let _ = task.await;
   }
}

async fn state_update_interval(ctx: ZeusCtx) {
   let mut wallet_state_passed = Instant::now();
   let mut fee_time_passed = Instant::now();
   let mut rpc_measure_time_passed = Instant::now();

   loop {
      if wallet_state_passed.elapsed().as_secs() > WALLET_STATE_INTERVAL {
         let manager = ctx.balance_manager();
         manager.update_eth_balance_across_wallets_and_chains(ctx.clone()).await;
         manager.update_tokens_balance_across_wallets_and_chains(ctx.clone()).await;
         update_token_prices(ctx.clone()).await;

         for chain in SUPPORTED_CHAINS {
            let portfolios = ctx.read(|ctx| ctx.portfolio_db.get_all(chain));
            for portfolio in &portfolios {
               ctx.calculate_portfolio_value(chain, portfolio.owner);
            }
         }

         check_smart_account_status(ctx.clone()).await;
         ctx.save_smart_accounts();
         ctx.save_portfolio_db();
         ctx.save_price_manager();
         ctx.save_balance_manager();

         wallet_state_passed = Instant::now();
      }

      if fee_time_passed.elapsed().as_secs() > FEE_INTERVAL {
         for chain in SUPPORTED_CHAINS {
            match update_priority_fee(ctx.clone(), chain).await {
               Ok(_) => {
                  tracing::trace!("Updated priority fee for chain: {}", chain)
               }
               Err(e) => tracing::error!(
                  "Error updating priority fee for chain {}: {:?}",
                  chain,
                  e
               ),
            }

            match get_base_fee(ctx.clone(), chain).await {
               Ok(_) => {
                  tracing::trace!("Updated base fee for chain: {}", chain)
               }
               Err(e) => tracing::error!("Error updating base fee: {:?}", e),
            }
         }
         fee_time_passed = Instant::now();
      }

      if rpc_measure_time_passed.elapsed().as_secs() > MEASURE_RPCS_INTERVAL {
         let z_client = ctx.get_zeus_client();
         z_client.run_latency_checks().await;
         rpc_measure_time_passed = Instant::now();
      }

      tokio::time::sleep(Duration::from_secs(1)).await;
   }
}

pub async fn get_base_fee(ctx: ZeusCtx, chain: u64) -> Result<BaseFee, anyhow::Error> {
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
   ctx.update_base_fee(chain.id(), fee, fee);
   Ok(BaseFee::new(fee, fee))
}

pub async fn update_priority_fee(ctx: ZeusCtx, chain: u64) -> Result<(), anyhow::Error> {
   let z_client = ctx.get_zeus_client();
   let chain = ChainId::new(chain)?;
   if chain.is_ethereum() || chain.is_optimism() || chain.is_base() {
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

      ctx.update_priority_fee(chain.id(), fee_value);
   }
   Ok(())
}

/// If needed re-sync pools for all tokens across all chains
pub async fn resync_pools(ctx: ZeusCtx) {
   ctx.write(|ctx| {
      ctx.data_syncing = true;
   });

   tracing::info!("Resyncing pools");

   for chain in SUPPORTED_CHAINS {
      let ctx = ctx.clone();
      RT.spawn(async move {
         let mut tokens = ctx.get_all_tokens_from_portfolios(chain);
         let base_tokens = ERC20Token::base_tokens(chain);
         tokens.extend(base_tokens);

         let pool_manager = ctx.pool_manager();

         match pool_manager.sync_pools_for_tokens(ctx.clone(), chain, tokens).await {
            Ok(_) => {}
            Err(e) => tracing::error!(
               "Failed to sync pools for chain_id {} {}",
               chain,
               e
            ),
         }

         let portfolio_tokens = ctx.get_all_tokens_from_portfolios(chain);
         let mut currencies = Vec::new();
         let mut tokens = Vec::new();
         let mut inserted = HashSet::new();

         for token in &portfolio_tokens {
            if token.is_base() || inserted.contains(&token.address) {
               continue;
            }

            inserted.insert(token.address);
            currencies.push(Currency::from(token.clone()));
            tokens.push(token.clone());
         }

         match pool_manager.update_for_currencies(ctx.clone(), chain, currencies).await {
            Ok(_) => {
               tracing::trace!("Updated price manager for chain: {}", chain)
            }
            Err(e) => tracing::error!(
               "Error updating price manager for chain {}: {:?}",
               chain,
               e
            ),
         }
      });
   }

   ctx.write(|ctx| {
      ctx.data_syncing = false;
   });

   RT.spawn_blocking(move || {
      for chain in SUPPORTED_CHAINS {
         let portfolios = ctx.read(|ctx| ctx.portfolio_db.get_all(chain));
         for portfolio in &portfolios {
            ctx.calculate_portfolio_value(chain, portfolio.owner);
         }
      }
      ctx.save_portfolio_db();
      ctx.save_pool_manager();
      ctx.save_price_manager();
   });
}
