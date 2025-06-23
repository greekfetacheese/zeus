use crate::core::{BaseFee, ZeusCtx, context::providers::client_test, utils::*};
use anyhow::{anyhow, bail};
use std::time::{Duration, Instant};
use zeus_eth::alloy_rpc_types::BlockId;
use zeus_eth::amm::DexKind;
use zeus_eth::{
   alloy_primitives::{U256, utils::format_units},
   alloy_provider::Provider,
   currency::ERC20Token,
   types::{ChainId, SUPPORTED_CHAINS},
   utils::NumericValue,
   utils::block::calculate_next_block_base_fee,
   utils::client,
};

const MEASURE_RPCS_INTERVAL: u64 = 60;
const POOL_MANAGER_INTERVAL: u64 = 600;
const PORTFOLIO_INTERVAL: u64 = 60;
const BALANCE_INTERVAL: u64 = 600;
const PRIORITY_FEE_INTERVAL: u64 = 90;
const BASE_FEE_INTERVAL: u64 = 180;

pub async fn on_startup(ctx: ZeusCtx) {
   ctx.write(|ctx| {
      ctx.on_startup_syncing = true;
   });

   let ctx2 = ctx.clone();
   RT.spawn(async move {
      let time = std::time::Instant::now();
      test_rpcs(ctx2).await;
      tracing::info!(
         "Testing RPCs took {} ms",
         time.elapsed().as_millis()
      );
   });

   let ctx2 = ctx.clone();
   RT.spawn(async move {
      let time = std::time::Instant::now();
      measure_rpcs(ctx2).await;
      tracing::info!(
         "Measuring RPCs took {} ms",
         time.elapsed().as_millis()
      );
   });


   for chain in SUPPORTED_CHAINS {
      let ctx2 = ctx.clone();
      RT.spawn(async move {
         match update_priority_fee(ctx2, chain).await {
            Ok(_) => {}
            Err(e) => tracing::error!("Error updating priority fee: {:?}", e),
         }
      });
   }

   let balance_manager = ctx.balance_manager();

   let eth_balance_fut = balance_manager.update_eth_balance_across_wallets_and_chains(ctx.clone());
   let token_balance_fut =
      balance_manager.update_tokens_balance_across_wallets_and_chains(ctx.clone());

   resync_pools(ctx.clone()).await;

   if !ctx.pools_need_resync() {
      update_pool_manager(ctx.clone()).await;
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
      ctx_clone.save_portfolio_db();
      ctx_clone.write(|ctx| {
         ctx.on_startup_syncing = false;
      });
      tracing::info!("Updated portfolio value");
   });

   // Update the base fee for all chains

   for chain in SUPPORTED_CHAINS {
      let ctx = ctx.clone();
      RT.spawn(async move {
         match get_base_fee(ctx.clone(), chain).await {
            Ok(_) => tracing::debug!("Updated base fee for chain: {}", chain),
            Err(e) => tracing::error!("Error updating base fee: {:?}", e),
         }
      });
   }

   let ctx_clone = ctx.clone();
   RT.spawn(async move {
      update_pool_manager_interval(ctx_clone).await;
   });

   let ctx_clone = ctx.clone();
   RT.spawn_blocking(move || {
      calculate_portfolio_value_interval(ctx_clone);
   });

   let ctx_clone = ctx.clone();
   RT.spawn(async move {
      balance_update_interval(ctx_clone).await;
   });

   let ctx_clone = ctx.clone();
   RT.spawn(async move {
      update_base_fee_interval(ctx_clone).await;
   });

   let ctx_clone = ctx.clone();
   RT.spawn(async move {
      update_priority_fee_interval(ctx_clone).await;
   });

   let ctx_clone = ctx.clone();
   RT.spawn(async move {
      measure_rpcs_interval(ctx_clone).await;
   });

   // Sync v4 pools and base pools
   ctx.write(|ctx| {
      ctx.data_syncing = true;
   });

   let mut tasks = Vec::new();
   for chain in SUPPORTED_CHAINS {
      let ctx_clone = ctx.clone();
      let tokens = ERC20Token::base_tokens(chain);
      let task = RT.spawn(async move {
         match eth::sync_pools_for_tokens(ctx_clone.clone(), chain, tokens, true).await {
            Ok(_) => {}
            Err(e) => tracing::error!("Error syncing V4 pools: {:?}", e),
         }

         let client = match ctx_clone.get_client(chain).await {
            Ok(client) => client,
            Err(e) => {
               tracing::error!(
                  "Error getting client for chain {}: {:?}",
                  chain,
                  e
               );
               return;
            }
         };

         let manager = ctx_clone.pool_manager();
         let v4_pools = manager.get_v4_pools_for_chain(chain);

         match manager
            .update_state_for_pools(client, chain, v4_pools)
            .await
         {
            Ok(_) => {}
            Err(e) => tracing::error!(
               "Error updating pool manager for chain {}: {:?}",
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

   let manager = ctx.pool_manager();
   manager.cleanup_v4_pools();

   ctx.write(|ctx| {
      ctx.data_syncing = false;
   });

   RT.spawn_blocking(move || {
      ctx.save_all();
   });
}

pub fn calculate_portfolio_value_interval(ctx: ZeusCtx) {
   let mut time_passed = Instant::now();

   loop {
      if time_passed.elapsed().as_secs() > PORTFOLIO_INTERVAL {
         for chain in SUPPORTED_CHAINS {
            let portfolios = ctx.read(|ctx| ctx.portfolio_db.get_all(chain));
            for portfolio in &portfolios {
               ctx.calculate_portfolio_value(chain, portfolio.owner);
            }
         }
         ctx.save_portfolio_db();
         time_passed = Instant::now();
      }
      std::thread::sleep(Duration::from_secs(1));
   }
}

pub async fn update_pool_manager_interval(ctx: ZeusCtx) {
   let mut time_passed = Instant::now();

   loop {
      if time_passed.elapsed().as_secs() > POOL_MANAGER_INTERVAL {
         update_pool_manager(ctx.clone()).await;
         time_passed = Instant::now();
      }
      tokio::time::sleep(Duration::from_secs(1)).await;
   }
}

/// Update the pool manager state for all chains
async fn update_pool_manager(ctx: ZeusCtx) {
   let mut tasks = Vec::new();
   for chain in SUPPORTED_CHAINS {
      let ctx = ctx.clone();
      let task = RT.spawn(async move {
         let client = match ctx.get_client(chain).await {
            Ok(client) => client,
            Err(e) => {
               tracing::error!(
                  "Error getting client for chain {}: {:?}",
                  chain,
                  e
               );
               return;
            }
         };

         let pool_manager = ctx.pool_manager();

         let tokens = ctx.get_all_tokens_from_portfolios(chain);
         let mut currencies = Vec::new();
         for token in tokens {
            if token.is_weth() || token.is_wbnb() {
               continue;
            }

            currencies.push(Currency::from(token));
         }

         if currencies.is_empty() {
            match pool_manager
               .update_base_token_prices(client.clone(), chain)
               .await
            {
               Ok(_) => tracing::info!("Updated base token prices for chain: {}", chain),
               Err(e) => tracing::error!(
                  "Error updating base token prices for chain {}: {:?}",
                  chain,
                  e
               ),
            }
         }

         match pool_manager
            .update_for_currencies(client, chain, currencies)
            .await
         {
            Ok(_) => tracing::info!("Updated pool manager for chain: {}", chain),
            Err(e) => tracing::error!(
               "Error updating pool manager for chain {}: {:?}",
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

   RT.spawn_blocking(move || {
      let _ = ctx.save_pool_manager();
   });
}

async fn balance_update_interval(ctx: ZeusCtx) {
   let mut time_passed = Instant::now();

   loop {
      if time_passed.elapsed().as_secs() > BALANCE_INTERVAL {
         let manager = ctx.balance_manager();
         manager
            .update_eth_balance_across_wallets_and_chains(ctx.clone())
            .await;
         manager
            .update_tokens_balance_across_wallets_and_chains(ctx.clone())
            .await;
         time_passed = Instant::now();
      }
      tokio::time::sleep(Duration::from_secs(1)).await;
   }
}

pub async fn get_base_fee(ctx: ZeusCtx, chain: u64) -> Result<BaseFee, anyhow::Error> {
   let client = ctx.get_client(chain).await?;
   let chain = ChainId::new(chain)?;

   if chain.is_ethereum() {
      let block = client
         .get_block(BlockId::latest())
         .await?
         .ok_or(anyhow!("Latest block not found"))?;
      let base_fee = block.header.base_fee_per_gas.unwrap_or_default();
      let next_base_fee = calculate_next_block_base_fee(block);
      ctx.update_base_fee(chain.id(), base_fee, next_base_fee);
      Ok(BaseFee::new(base_fee, next_base_fee))
   } else {
      let gas_price = client.get_gas_price().await?;
      let fee: u64 = gas_price.try_into()?;
      ctx.update_base_fee(chain.id(), fee, fee);
      Ok(BaseFee::new(fee, fee))
   }
}

pub async fn update_base_fee_interval(ctx: ZeusCtx) {
   let mut time_passed = Instant::now();

   loop {
      if time_passed.elapsed().as_secs() > BASE_FEE_INTERVAL {
         for chain in SUPPORTED_CHAINS {
            match get_base_fee(ctx.clone(), chain).await {
               Ok(_) => tracing::info!("Updated base fee for chain: {}", chain),
               Err(e) => tracing::error!("Error updating base fee: {:?}", e),
            }
         }
         time_passed = Instant::now();
      }
      tokio::time::sleep(Duration::from_secs(1)).await;
   }
}

pub async fn update_priority_fee(ctx: ZeusCtx, chain: u64) -> Result<(), anyhow::Error> {
   let client = ctx.get_client(chain).await?;
   let chain = ChainId::new(chain)?;
   if chain.is_ethereum() || chain.is_optimism() || chain.is_base() {
      let fee = client.get_max_priority_fee_per_gas().await?;
      let fee_str = format_units(U256::from(fee), "gwei")?;
      let fee_value = NumericValue::parse_to_gwei(&fee_str);
      if fee_value.formatted() == "0" {
         bail!(
            "Rpc returned bad data, Fee (Wei) {} For Chain: {}",
            fee,
            chain.id()
         );
      }
      ctx.update_priority_fee(chain.id(), fee_value);
   }
   Ok(())
}

pub async fn update_priority_fee_interval(ctx: ZeusCtx) {
   let mut time_passed = Instant::now();

   loop {
      if time_passed.elapsed().as_secs() > PRIORITY_FEE_INTERVAL {
         for chain in SUPPORTED_CHAINS {
            match update_priority_fee(ctx.clone(), chain).await {
               Ok(_) => tracing::debug!("Updated priority fee for chain: {}", chain),
               Err(e) => tracing::error!("Error updating priority fee: {:?}", e),
            }
         }
         time_passed = Instant::now();
      }
      tokio::time::sleep(Duration::from_secs(1)).await;
   }
}

pub async fn test_rpcs(ctx: ZeusCtx) {
   let providers = ctx.rpc_providers();

   let mut tasks = Vec::new();
   for chain in SUPPORTED_CHAINS {
      let rpcs = providers.get_all(chain);

      for rpc in &rpcs {
         if !rpc.enabled {
            continue;
         }
         let rpc = rpc.clone();
         let ctx = ctx.clone();
         let task = RT.spawn(async move {
            match client_test(rpc.clone()).await {
               Ok(archive) => {
                  ctx.write(|ctx| {
                     if let Some(rpc) = ctx.providers.rpc_mut(chain, rpc.url.clone()) {
                        rpc.working = true;
                        rpc.archive_node = archive;
                     }
                  });
               }
               Err(e) => {
                  tracing::error!("Error testing RPC {} {:?}", rpc.url, e);
                  ctx.write(|ctx| {
                     if let Some(rpc) = ctx.providers.rpc_mut(chain, rpc.url.clone()) {
                        rpc.working = false;
                     }
                  });
               }
            }
         });
         tasks.push(task);
      }
   }

   for task in tasks {
      let _ = task.await;
   }
}

pub async fn measure_rpcs(ctx: ZeusCtx) {
   let providers = ctx.rpc_providers();

   let mut tasks = Vec::new();
   for chain in SUPPORTED_CHAINS {
      let rpcs = providers.get_all(chain);

      for rpc in rpcs {
         if !rpc.enabled {
            continue;
         }
         let rpc = rpc.clone();
         let ctx = ctx.clone();
         let task = RT.spawn(async move {
            let retry = client::retry_layer(2, 400, 600);
            let throttle = client::throttle_layer(5);
            let client = match client::get_client(&rpc.url, retry, throttle).await {
               Ok(client) => client,
               Err(e) => {
                  tracing::error!("Error getting client using {} {}", rpc.url, e);
                  return;
               }
            };

            let time = Instant::now();
            match client.get_block_number().await {
               Ok(_) => {
                  let latency = time.elapsed();
                  ctx.write(|ctx| {
                     if let Some(rpc) = ctx.providers.rpc_mut(chain, rpc.url.clone()) {
                        rpc.latency = Some(latency);
                     }
                  });
               }
               Err(e) => {
                  tracing::error!("Error testing RPC: {} {}", rpc.url, e);
                  ctx.write(|ctx| {
                     if let Some(rpc) = ctx.providers.rpc_mut(chain, rpc.url.clone()) {
                        rpc.working = false;
                     }
                  });
               }
            }
         });
         tasks.push(task);
      }
   }
   for task in tasks {
      match task.await {
         Ok(_) => {}
         Err(e) => tracing::error!("Error testing RPC: {:?}", e),
      }
   }
}

pub async fn measure_rpcs_interval(ctx: ZeusCtx) {
   let mut time_passed = Instant::now();

   loop {
      if time_passed.elapsed().as_secs() > MEASURE_RPCS_INTERVAL {
         measure_rpcs(ctx.clone()).await;
         time_passed = Instant::now();
      }
      tokio::time::sleep(Duration::from_secs(1)).await;
   }
}

/// eg. WETH/USDT etc..
/// Also sync v4 pools
pub async fn sync_basic_pools(ctx: ZeusCtx, chain: u64) -> Result<(), anyhow::Error> {
   let tokens = ERC20Token::base_tokens(chain);
   eth::sync_pools_for_tokens(ctx, chain, tokens, false).await
}

/// If needed re-sync pools for all tokens across all chains
pub async fn resync_pools(ctx: ZeusCtx) {
   let need_resync = ctx.pools_need_resync();

   if !need_resync {
      tracing::info!("No need to resync pools");
      return;
   }

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

         let client = match ctx.get_client(chain).await {
            Ok(client) => client,
            Err(e) => {
               tracing::error!(
                  "Error getting client for chain {}: {:?}",
                  chain,
                  e
               );
               return;
            }
         };

         let dexes = DexKind::main_dexes(chain);
         let pool_manager = ctx.pool_manager();

         match pool_manager
            .sync_pools_for_tokens(client.clone(), chain, tokens, dexes, false)
            .await
         {
            Ok(_) => {}
            Err(e) => tracing::error!(
               "Failed to sync pools for chain_id {} {}",
               chain,
               e
            ),
         }

         match pool_manager.update(client, chain).await {
            Ok(_) => {}
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

      match ctx.save_pool_manager() {
         Ok(_) => tracing::info!("Pool data saved"),
         Err(e) => tracing::error!("Error saving pool data: {:?}", e),
      }
   });
}

#[cfg(test)]
mod tests {
   use super::*;

   #[tokio::test]
   async fn test_basic_pools() {
      let ctx = ZeusCtx::new();
      let chain = ChainId::new(1).unwrap();
      sync_basic_pools(ctx.clone(), chain.id()).await.unwrap();
   }
}
