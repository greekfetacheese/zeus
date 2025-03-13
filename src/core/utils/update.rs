use crate::core::{ZeusCtx, utils::*};
use alloy_network::primitives::BlockTransactionsKind;
use anyhow::anyhow;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use zeus_eth::alloy_rpc_types::BlockId;
use zeus_eth::currency::NativeCurrency;
use zeus_eth::{
   alloy_primitives::{Address, U256, utils::format_units},
   alloy_provider::Provider,
   currency::ERC20Token,
   types::{ChainId, SUPPORTED_CHAINS},
   utils::NumericValue,
   utils::batch_request::get_erc20_balance,
   utils::block::calculate_next_block_base_fee,
   utils::client::get_http_client,
};

/// on startup update the necceary data
pub async fn on_startup(ctx: ZeusCtx) {
   measure_rpcs(ctx.clone()).await;
   resync_pools(ctx.clone()).await;
   update_pool_manager(ctx.clone()).await;

   if let Err(e) = update_eth_balance(ctx.clone()).await {
      tracing::error!("Error updating eth balance: {:?}", e);
   }

   if let Err(e) = update_token_balance(ctx.clone()).await {
      tracing::error!("Error updating token balance: {:?}", e);
   }

   portfolio_update(ctx.clone());

   for chain in SUPPORTED_CHAINS {
      match update_base_fee(ctx.clone(), chain).await {
         Ok(_) => tracing::info!("Updated base fee for chain: {}", chain),
         Err(e) => tracing::error!("Error updating base fee: {:?}", e),
      }
   }

   for chain in SUPPORTED_CHAINS {
      match update_priority_fee(ctx.clone(), chain).await {
         Ok(_) => tracing::info!("Updated priority fee for chain: {}", chain),
         Err(e) => tracing::error!("Error updating priority fee: {:?}", e),
      }
   }

   let ctx_clone = ctx.clone();
   RT.spawn(async move {
      update_pool_manager_interval(ctx_clone).await;
   });

   let ctx_clone = ctx.clone();
   RT.spawn(async move {
      portfolio_update_interval(ctx_clone);
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
}

pub fn portfolio_update_interval(ctx: ZeusCtx) {
   const INTERVAL: u64 = 300;
   let mut time_passed = Instant::now();

   loop {
      if time_passed.elapsed().as_secs() > INTERVAL {
         portfolio_update(ctx.clone());
         time_passed = Instant::now();
      }
      std::thread::sleep(Duration::from_secs(1));
   }
}

pub fn portfolio_update(ctx: ZeusCtx) {
   let wallets = ctx.profile().wallets;
   for chain in SUPPORTED_CHAINS {
      for wallet in &wallets {
         let owner = wallet.key.inner().address();
         ctx.update_portfolio_value(chain, owner);
      }
   }

   match ctx.save_portfolio_db() {
      Ok(_) => tracing::info!("PortfolioDB saved"),
      Err(e) => tracing::error!("Error saving PortfolioDB: {:?}", e),
   }
}

pub async fn update_pool_manager_interval(ctx: ZeusCtx) {
   const INTERVAL: u64 = 600;
   let mut time_passed = Instant::now();

   loop {
      if time_passed.elapsed().as_secs() > INTERVAL {
         update_pool_manager(ctx.clone()).await;
         time_passed = Instant::now();
      }
      tokio::time::sleep(Duration::from_secs(1)).await;
   }
}

/// Update the pool manager state for all chains
pub async fn update_pool_manager(ctx: ZeusCtx) {
   let pool_manager = ctx.pool_manager();

   // update the base token prices
   for chain in SUPPORTED_CHAINS {
      let client = ctx.get_client_with_id(chain).unwrap();
      match pool_manager.update_base_token_prices(client, chain).await {
         Ok(_) => tracing::info!("Updated base token prices for chain: {}", chain),
         Err(e) => tracing::error!(
            "Error updating base token prices for chain {}: {:?}",
            chain,
            e
         ),
      }
   }

   for chain in SUPPORTED_CHAINS {
      let client = ctx.get_client_with_id(chain).unwrap();

      match pool_manager.update_and_clean(client.clone(), chain).await {
         Ok(_) => tracing::info!("Updated price manager for chain: {}", chain),
         Err(e) => tracing::error!("Error updating price manager for chain {}: {:?}", chain, e),
      }
   }
   match ctx.save_pool_data() {
      Ok(_) => tracing::info!("Pool data saved"),
      Err(e) => tracing::error!("Error saving pool data: {:?}", e),
   }
}

/// Update the eth balance for all wallets across all chains
pub async fn update_eth_balance(ctx: ZeusCtx) -> Result<(), anyhow::Error> {
   let wallets = ctx.profile().wallets;

   for chain in SUPPORTED_CHAINS {
      let client = ctx.get_client_with_id(chain).unwrap();

      for wallet in &wallets {
         let owner = wallet.key.inner().address();
         let balance = client.get_balance(owner).await?;
         let native = NativeCurrency::from_chain_id(chain)?;
         ctx.write(|ctx| {
            ctx.balance_db
               .insert_eth_balance(chain, owner, balance, &native);
         })
      }
   }

   match ctx.save_balance_db() {
      Ok(_) => tracing::info!("BalanceDB saved"),
      Err(e) => tracing::error!("Error saving DB: {:?}", e),
   }

   Ok(())
}

/// Update the token balance for all wallets across all chains
pub async fn update_token_balance(ctx: ZeusCtx) -> Result<(), anyhow::Error> {
   let wallets = ctx.profile().wallets;

   for chain in SUPPORTED_CHAINS {
      let client = ctx.get_client_with_id(chain).unwrap();

      for wallet in &wallets {
         let owner = wallet.key.inner().address();
         let portfolio = ctx.get_portfolio(chain, owner);
         let tokens = portfolio.erc20_tokens();

         if tokens.is_empty() {
            continue;
         }

         let token_map: HashMap<Address, &ERC20Token> = tokens.iter().map(|token| (token.address, token)).collect();

         let tokens_addr = tokens.iter().map(|t| t.address).collect::<Vec<_>>();

         let mut token_with_balance = Vec::new();
         for token_addr in tokens_addr.chunks(100) {
            let balances = get_erc20_balance(client.clone(), None, owner, token_addr.to_vec()).await?;
            for balance in balances {
               token_with_balance.push((balance.token, balance.balance));
            }
         }

         // Match each balance with its corresponding ERC20Token
         for (token_address, balance) in token_with_balance {
            if let Some(token) = token_map.get(&token_address) {
               ctx.write(|ctx| {
                  ctx.balance_db
                     .insert_token_balance(chain, owner, balance, *token);
               });
            } else {
               tracing::warn!("No matching token found for address: {:?}", token_address);
            }
         }
      }
   }
   Ok(())
}

async fn balance_update_interval(ctx: ZeusCtx) {
   const INTERVAL: u64 = 600;
   let mut time_passed = Instant::now();

   loop {
      if time_passed.elapsed().as_secs() > INTERVAL {
         if let Err(e) = update_eth_balance(ctx.clone()).await {
            tracing::error!("Error updating eth balance: {:?}", e);
         }
         if let Err(e) = update_token_balance(ctx.clone()).await {
            tracing::error!("Error updating token balance: {:?}", e);
         }
         time_passed = Instant::now();
      }
      tokio::time::sleep(Duration::from_secs(1)).await;
   }
}

pub async fn update_base_fee(ctx: ZeusCtx, chain: u64) -> Result<(), anyhow::Error> {
   let client = ctx.get_client_with_id(chain)?;
   let chain = ChainId::new(chain)?;

   if chain.is_ethereum() {
      let block = client
         .get_block(BlockId::latest(), BlockTransactionsKind::Full)
         .await?
         .ok_or(anyhow!("Latest block not found"))?;
      let base_fee = block.header.base_fee_per_gas.unwrap_or_default();
      let next_base_fee = calculate_next_block_base_fee(block);
      ctx.update_base_fee(chain.id(), base_fee, next_base_fee);
   } else {
      let gas_price = client.get_gas_price().await?;
      let fee: u64 = gas_price.try_into()?;
      ctx.update_base_fee(chain.id(), fee, fee);
   }
   Ok(())
}

pub async fn update_base_fee_interval(ctx: ZeusCtx) {
   const INTERVAL: u64 = 600;
   let mut time_passed = Instant::now();

   loop {
      if time_passed.elapsed().as_secs() > INTERVAL {
         for chain in SUPPORTED_CHAINS {
            match update_base_fee(ctx.clone(), chain).await {
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
   let client = ctx.get_client_with_id(chain)?;
   let chain = ChainId::new(chain)?;
   if chain.is_ethereum() || chain.is_optimism() || chain.is_base() {
      let fee = client.get_max_priority_fee_per_gas().await?;
      let fee = format_units(U256::from(fee), "gwei")?;
      ctx.update_priority_fee(chain.id(), NumericValue::gwei_to_wei(&fee));
   }
   Ok(())
}

pub async fn update_priority_fee_interval(ctx: ZeusCtx) {
   const INTERVAL: u64 = 300;
   let mut time_passed = Instant::now();

   loop {
      if time_passed.elapsed().as_secs() > INTERVAL {
         for chain in SUPPORTED_CHAINS {
            match update_priority_fee(ctx.clone(), chain).await {
               Ok(_) => tracing::info!("Updated priority fee for chain: {}", chain),
               Err(e) => tracing::error!("Error updating priority fee: {:?}", e),
            }
         }
         time_passed = Instant::now();
      }
      tokio::time::sleep(Duration::from_secs(1)).await;
   }
}

pub async fn measure_rpcs(ctx: ZeusCtx) {
   let providers = ctx.rpc_providers();

   for chain in SUPPORTED_CHAINS {
      let rpcs = providers.get_all(chain);

      for rpc in rpcs {
         let rpc = rpc.clone();
         let ctx = ctx.clone();
         RT.spawn(async move {
            let client = get_http_client(&rpc.url).unwrap();
            let time = Instant::now();
            let _ = client.get_block_number().await.unwrap();
            let latency = time.elapsed();
            ctx.write(|ctx| {
               if let Some(rpc) = ctx.providers.rpc_mut(chain, rpc.url.clone()) {
                  rpc.latency = Some(latency);
               }
            });
         });
      }
   }
}

pub async fn measure_rpcs_interval(ctx: ZeusCtx) {
   const INTERVAL: u64 = 300;
   let mut time_passed = Instant::now();

   loop {
      if time_passed.elapsed().as_secs() > INTERVAL {
         measure_rpcs(ctx.clone()).await;
         time_passed = Instant::now();
      }
      tokio::time::sleep(Duration::from_secs(1)).await;
   }
}

/// If needed re-sync pools for all tokens across all chains
pub async fn resync_pools(ctx: ZeusCtx) {
   let need_resync = ctx.pools_need_resync();

   if need_resync {
      ctx.write(|ctx| {
         ctx.data_syncing = true;
      });

      tracing::info!("Resyncing pools");

      for chain in SUPPORTED_CHAINS {
         let tokens = ctx.get_all_erc20_tokens(chain);
         for token in tokens {
            // add a small delay to avoid rate limiting
            tokio::time::sleep(Duration::from_millis(500)).await;
            match eth::sync_pools_for_token(ctx.clone(), token.clone(), true, true).await {
               Ok(_) => {}
               Err(e) => {
                  tracing::error!("Failed to sync pools for token {}: {}", token.symbol, e);
               }
            }
         }
      }

      ctx.write(|ctx| {
         ctx.data_syncing = false;
      });

      match ctx.save_pool_data() {
         Ok(_) => tracing::info!("PoolData saved"),
         Err(e) => tracing::error!("Error saving PoolData: {:?}", e),
      }
   } else {
      tracing::info!("No need to resync pools");
   }
}


/* 
#[allow(dead_code)]
async fn sync_pools_if_needed(ctx: ZeusCtx) {
   // let pools_synced = ctx.read(|ctx| ctx.db.pool_data_synced);
   let pools_synced = false;

   if !pools_synced {
      tracing::info!("Syncing pools for the first time");
      ctx.write(|ctx| {
         ctx.pool_data_syncing = true;
      });

      let chains = SUPPORTED_CHAINS.to_vec();
      let res = sync_pools(ctx.clone(), chains).await;
      if res.is_ok() {
         /*
         ctx.write(|ctx| {
            ctx.db.pool_data_synced = true;
         });
         */

         ctx.write(|ctx| {
            ctx.pool_data_syncing = false;
         });
         match ctx.save_pool_data() {
            Ok(_) => tracing::info!("PoolData saved"),
            Err(e) => tracing::error!("Error saving DB: {:?}", e),
         }
      } else {
         ctx.write(|ctx| {
            ctx.pool_data_syncing = false;
         });
         tracing::error!("Error syncing pools: {:?}", res.err().unwrap());
      }
   }
}
   */

/// Sync all the V2 & V3 pools for all the tokens
pub async fn sync_pools(ctx: ZeusCtx, chains: Vec<u64>) -> Result<(), anyhow::Error> {
   const MAX_RETRY: usize = 5;

   let mut total_v2 = 0;
   let mut total_v3 = 0;
   let time = Instant::now();

   for chain in chains {
      let currencies = ctx.get_currencies(chain);

      for currency in &*currencies {
         if currency.is_native() {
            continue;
         }

         let token = currency.erc20().unwrap();
         let ctx = ctx.clone();

         let mut retry = 0;
         let mut v2_pools = None;
         let mut v3_pools = None;

         while v2_pools.is_none() && retry < MAX_RETRY {
            match eth::get_v2_pools_for_token(ctx.clone(), token.clone()).await {
               Ok(pools) => {
                  total_v2 += pools.len();
                  v2_pools = Some(pools);
               }
               Err(e) => tracing::error!("Error getting v2 pools: {:?}", e),
            }
            retry += 1;
            std::thread::sleep(Duration::from_secs(1));
         }

         retry = 0;
         while v3_pools.is_none() && retry < MAX_RETRY {
            match eth::get_v3_pools_for_token(ctx.clone(), token.clone()).await {
               Ok(pools) => {
                  total_v3 += pools.len();
                  v3_pools = Some(pools);
               }
               Err(e) => tracing::error!("Error getting v3 pools: {:?}", e),
            }
            retry += 1;
            std::thread::sleep(Duration::from_secs(1));
         }

         if let Some(v2_pools) = v2_pools {
            tracing::info!("Got {} v2 pools for: {}", v2_pools.len(), token.symbol);
         }

         if let Some(v3_pools) = v3_pools {
            tracing::info!("Got {} v3 pools for: {}", v3_pools.len(), token.symbol);
         }
      }
   }

   ctx.save_pool_data()?;

   tracing::info!("Finished syncing pools");
   tracing::info!("Total V2 pools: {}", total_v2);
   tracing::info!("Total V3 pools: {}", total_v3);
   tracing::info!("Time taken: {:?} secs", time.elapsed().as_secs_f32());

   Ok(())
}
