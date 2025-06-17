use crate::core::{BaseFee, ZeusCtx, context::providers::client_test, utils::*};
use anyhow::{anyhow, bail};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use zeus_eth::alloy_rpc_types::BlockId;
use zeus_eth::amm::DexKind;
use zeus_eth::currency::NativeCurrency;
use zeus_eth::{
   alloy_primitives::{Address, U256, utils::format_units},
   alloy_provider::Provider,
   currency::ERC20Token,
   types::{ChainId, SUPPORTED_CHAINS},
   utils::NumericValue,
   utils::batch::get_erc20_balances,
   utils::block::calculate_next_block_base_fee,
   utils::client,
};

const MEASURE_RPCS_INTERVAL: u64 = 60;
const POOL_MANAGER_INTERVAL: u64 = 600;
const PORTFOLIO_INTERVAL: u64 = 60;
const BALANCE_INTERVAL: u64 = 120;
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

   let eth_fut = update_eth_balance(ctx.clone());
   let token_fut = update_token_balance(ctx.clone());

   resync_pools(ctx.clone()).await;

   if !ctx.pools_need_resync() {
      update_pool_manager(ctx.clone()).await;
   }

   match eth_fut.await {
      Ok(_) => tracing::info!("Updated ETH balances"),
      Err(e) => tracing::error!("Error updating ETH balance: {:?}", e),
   }

   match token_fut.await {
      Ok(_) => tracing::info!("Updated token balances"),
      Err(e) => tracing::error!("Error updating token balance: {:?}", e),
   }

   let ctx_clone = ctx.clone();
   RT.spawn(async move {
      while !ctx_clone.logged_in() {
         tokio::time::sleep(Duration::from_millis(100)).await;
      }

      let wallets = ctx_clone.wallets_info();
      for chain in SUPPORTED_CHAINS {
         for wallet in &wallets {
            ctx_clone.calculate_portfolio_value(chain, wallet.address);
         }
      }
      ctx_clone.save_portfolio_db();
      ctx_clone.write(|ctx| {
         ctx.on_startup_syncing = false;
      });
      tracing::info!("Updated portfolio value");
   });

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

   /*
   // Sync v4 pools and base pools
   ctx.write(|ctx| {
      ctx.data_syncing = true;
   });
   let mut tasks = Vec::new();
   for chain in SUPPORTED_CHAINS {
      let ctx_clone = ctx.clone();
      let tokens = ERC20Token::base_tokens(chain);
      let task = RT.spawn(async move {
         match eth::sync_pools_for_tokens(ctx_clone, chain, tokens, true).await {
            Ok(_) => {}
            Err(e) => tracing::error!("Error syncing V4 pools: {:?}", e),
         }
      });
      tasks.push(task);
   }

   for task in tasks {
      task.await.unwrap();
   }

   ctx.write(|ctx| {
      ctx.data_syncing = false;
   });

   RT.spawn_blocking(move || {
      ctx.save_pool_manager().unwrap();
   });
   */
}

pub fn calculate_portfolio_value_interval(ctx: ZeusCtx) {
   let mut time_passed = Instant::now();

   loop {
      if time_passed.elapsed().as_secs() > PORTFOLIO_INTERVAL {
         let wallets = ctx.wallets_info();
         for chain in SUPPORTED_CHAINS {
            for wallet in &wallets {
               ctx.calculate_portfolio_value(chain, wallet.address);
            }
         }
         ctx.save_portfolio_db();
         time_passed = Instant::now();
      }
      std::thread::sleep(Duration::from_secs(1));
   }
}

/// Update the portfolio state for the given chain and wallet
pub async fn update_portfolio_state(
   ctx: ZeusCtx,
   chain: u64,
   owner: Address,
) -> Result<(), anyhow::Error> {
   let pool_manager = ctx.pool_manager();
   let client = ctx.get_client(chain).await?;
   let tokens = ctx.get_portfolio(chain, owner).erc20_tokens();
   let dex_kinds = DexKind::main_dexes(chain);

   update_eth_balance(ctx.clone()).await?;
   update_tokens_balance_for_chain(ctx.clone(), chain, owner, tokens.clone()).await?;

   // check if any pool data is missing
   for token in &tokens {
      let currency = token.clone().into();
      let pools = pool_manager.get_pools_that_have_currency(&currency);

      if pools.is_empty() {
         let _ = pool_manager
            .sync_pools_for_tokens(
               client.clone(),
               chain,
               vec![token.clone()],
               dex_kinds.clone(),
               false,
            )
            .await;
      }
   }

   pool_manager.update(client, chain).await?;
   RT.spawn_blocking(move || {
      ctx.calculate_portfolio_value(chain, owner);
      ctx.save_all();
   });

   Ok(())
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
pub async fn update_pool_manager(ctx: ZeusCtx) {
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

         /* 
         let tokens = ctx.get_all_erc20_tokens(chain);
         let mut currencies = Vec::new();
         for token in tokens {
            // skip this step for now and just update all pools

            if token.is_weth() || token.is_wbnb() {
               continue;
            }

            currencies.push(Currency::from(token));
         }
         */

         match pool_manager.update(client, chain).await {
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

/// Update the eth balance for all wallets across all chains
pub async fn update_eth_balance(ctx: ZeusCtx) -> Result<(), anyhow::Error> {
   while !ctx.logged_in() {
      tokio::time::sleep(Duration::from_millis(100)).await;
   }

   let wallets = ctx.wallets_info();

   let mut tasks = Vec::new();
   for chain in SUPPORTED_CHAINS {
      let wallets = wallets.clone();
      let ctx = ctx.clone();

      let task = RT.spawn(async move {
         let client = ctx.get_client(chain).await.unwrap();

         for wallet in &wallets {
            let balance = match client.get_balance(wallet.address).await {
               Ok(balance) => balance,
               Err(e) => {
                  tracing::error!(
                     "Error getting balance for chain {}: {:?}",
                     chain,
                     e
                  );
                  continue;
               }
            };
            let native = NativeCurrency::from_chain_id(chain).unwrap();

            ctx.write(|ctx| {
               ctx.balance_db
                  .insert_eth_balance(chain, wallet.address, balance, &native);
            })
         }
      });
      tasks.push(task);
   }

   for task in tasks {
      if let Err(e) = task.await {
         tracing::error!("Error updating ETH balance: {:?}", e);
      }
   }

   RT.spawn_blocking(move || {
      ctx.save_balance_db();
   });

   Ok(())
}

/// Update the balance of tokens for the given chain and owner
pub async fn update_tokens_balance_for_chain(
   ctx: ZeusCtx,
   chain: u64,
   owner: Address,
   tokens: Vec<ERC20Token>,
) -> Result<(), anyhow::Error> {
   let token_map: HashMap<Address, &ERC20Token> =
      tokens.iter().map(|token| (token.address, token)).collect();
   let tokens_addr = tokens.iter().map(|t| t.address).collect::<Vec<_>>();
   let client = ctx.get_client(chain).await?;

   let mut token_with_balance = Vec::new();
   for token_addr in tokens_addr.chunks(100) {
      let balances = get_erc20_balances(client.clone(), None, owner, token_addr.to_vec()).await?;
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
         tracing::warn!(
            "No matching token found for address: {:?}",
            token_address
         );
      }
   }
   Ok(())
}

/// Update the token balance for all wallets across all chains
pub async fn update_token_balance(ctx: ZeusCtx) -> Result<(), anyhow::Error> {
      while !ctx.logged_in() {
      tokio::time::sleep(Duration::from_millis(100)).await;
   }

   let wallets = ctx.wallets_info();

   let mut tasks = Vec::new();
   for chain in SUPPORTED_CHAINS {
      let wallets = wallets.clone();
      let ctx = ctx.clone();
      let task = RT.spawn(async move {
         for wallet in &wallets {
            let portfolio = ctx.get_portfolio(chain, wallet.address);
            let tokens = portfolio.erc20_tokens();

            if tokens.is_empty() {
               continue;
            }

            match update_tokens_balance_for_chain(ctx.clone(), chain, wallet.address, tokens).await
            {
               Ok(_) => {}
               Err(e) => {
                  tracing::error!(
                     "Error updating token balance for chain {}: {:?}",
                     chain,
                     e
                  );
               }
            }
         }
      });
      tasks.push(task);
   }

   for task in tasks {
      if let Err(e) = task.await {
         tracing::error!("Error updating token balance: {:?}", e);
      }
   }
   Ok(())
}

async fn balance_update_interval(ctx: ZeusCtx) {
   let mut time_passed = Instant::now();

   loop {
      if time_passed.elapsed().as_secs() > BALANCE_INTERVAL {
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
         let mut tokens = ctx.get_all_erc20_tokens(chain);
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
            .sync_pools_for_tokens(
               client.clone(),
               chain,
               tokens.clone(),
               dexes,
               false,
            )
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
      let wallets = ctx.wallets_info();
      for chain in SUPPORTED_CHAINS {
         for wallet in &wallets {
            ctx.calculate_portfolio_value(chain, wallet.address);
         }
      }
      ctx.save_portfolio_db();

      match ctx.save_pool_manager() {
         Ok(_) => tracing::info!("Pool data saved for"),
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
