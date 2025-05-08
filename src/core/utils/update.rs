use crate::core::{BaseFee, ZeusCtx, utils::*};
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

const MEASURE_RPCS_INTERVAL: u64 = 100;
const POOL_MANAGER_INTERVAL: u64 = 600;
const PORTFOLIO_INTERVAL: u64 = 60;
const BALANCE_INTERVAL: u64 = 120;
const PRIORITY_FEE_INTERVAL: u64 = 60;
const BASE_FEE_INTERVAL: u64 = 180;

/// on startup update the necceary data
pub async fn on_startup(ctx: ZeusCtx) {
   let time = std::time::Instant::now();
   measure_rpcs(ctx.clone()).await;
   tracing::info!(
      "Measuring RPCs took {} ms",
      time.elapsed().as_millis()
   );

   for chain in SUPPORTED_CHAINS {
      let ctx_clone = ctx.clone();
      RT.spawn(async move {
         match update_priority_fee(ctx_clone, chain).await {
            Ok(_) => tracing::info!("Updated priority fee for chain: {}", chain),
            Err(e) => tracing::error!("Error updating priority fee: {:?}", e),
         }
      });
   }

   let resynced = resync_pools(ctx.clone()).await;

   if !resynced {
      update_pool_manager(ctx.clone());
   }

   let eth_fut = update_eth_balance(ctx.clone());
   let token_fut = update_token_balance(ctx.clone());

   match eth_fut.await {
      Ok(_) => tracing::info!("Updated ETH balances"),
      Err(e) => tracing::error!("Error updating ETH balance: {:?}", e),
   }

   match token_fut.await {
      Ok(_) => tracing::info!("Updated token balances"),
      Err(e) => tracing::error!("Error updating token balance: {:?}", e),
   }

   let ctx_clone = ctx.clone();
   RT.spawn_blocking(move || {
      portfolio_update(ctx_clone.clone());
      tracing::info!("Updated portfolio value");
   });

   for chain in SUPPORTED_CHAINS {
      let ctx = ctx.clone();
      RT.spawn(async move {
         match get_base_fee(ctx.clone(), chain).await {
            Ok(_) => tracing::info!("Updated base fee for chain: {}", chain),
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
   let mut time_passed = Instant::now();

   loop {
      if time_passed.elapsed().as_secs() > PORTFOLIO_INTERVAL {
         portfolio_update(ctx.clone());
         time_passed = Instant::now();
      }
      std::thread::sleep(Duration::from_secs(1));
   }
}

/// Update the portfolio value for all wallets across all chains
pub fn portfolio_update(ctx: ZeusCtx) {
   let wallets = ctx.wallets_info();
   for chain in SUPPORTED_CHAINS {
      for wallet in &wallets {
         ctx.update_portfolio_value(chain, wallet.address);
      }
   }

   ctx.save_portfolio_db();
}

/// Update the portofolio state for the given chain and wallet
pub async fn update_portfolio_state(
   ctx: ZeusCtx,
   chain: u64,
   owner: Address,
) -> Result<(), anyhow::Error> {
   let pool_manager = ctx.pool_manager();
   let client = ctx.get_client_with_id(chain).unwrap();
   let tokens = ctx.get_portfolio(chain, owner).erc20_tokens();
   let dex_kinds = DexKind::main_dexes(chain);

   update_eth_balance(ctx.clone()).await?;
   update_tokens_balance_for_chain(ctx.clone(), chain, owner, tokens.clone()).await?;

   // check if any pool data is missing
   for token in &tokens {
      let currency = token.clone().into();
      let pools = pool_manager.get_pools_from_currency(&currency);

      if pools.is_empty() {
         let _ = pool_manager
            .sync_pools_for_tokens(client.clone(), vec![token.clone()], dex_kinds.clone())
            .await;
      }
   }

   pool_manager.update(client, chain).await?;
   RT.spawn_blocking(move || {
      ctx.update_portfolio_value(chain, owner);
      ctx.save_all();
   });

   Ok(())
}

pub async fn update_pool_manager_interval(ctx: ZeusCtx) {
   let mut time_passed = Instant::now();

   loop {
      if time_passed.elapsed().as_secs() > POOL_MANAGER_INTERVAL {
         update_pool_manager(ctx.clone());
         time_passed = Instant::now();
      }
      tokio::time::sleep(Duration::from_secs(1)).await;
   }
}

/// Update the pool manager state for all chains
pub fn update_pool_manager(ctx: ZeusCtx) {
   for chain in SUPPORTED_CHAINS {
      let client = ctx.get_client_with_id(chain).unwrap();
      let pool_manager = ctx.pool_manager();

      let ctx_clone = ctx.clone();
      RT.spawn(async move {
         match pool_manager.update(client, chain).await {
            Ok(_) => tracing::info!("Updated price manager for chain: {}", chain),
            Err(e) => tracing::error!(
               "Error updating price manager for chain {}: {:?}",
               chain,
               e
            ),
         }

         RT.spawn_blocking(move || match ctx_clone.save_pool_manager() {
            Ok(_) => tracing::info!("Pool data saved for chain: {}", chain),
            Err(e) => tracing::error!(
               "Error saving pool data for chain {}: {:?}",
               chain,
               e
            ),
         });
      });
   }
}

/// Update the eth balance for all wallets across all chains
pub async fn update_eth_balance(ctx: ZeusCtx) -> Result<(), anyhow::Error> {
   let wallets = ctx.wallets_info();

   let mut tasks = Vec::new();
   for chain in SUPPORTED_CHAINS {
      let wallets = wallets.clone();
      let ctx = ctx.clone();

      let task = RT.spawn(async move {
         let client = ctx.get_client_with_id(chain).unwrap();

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

/// Update the token's balance for the given chain and owner
pub async fn update_tokens_balance_for_chain(
   ctx: ZeusCtx,
   chain: u64,
   owner: Address,
   tokens: Vec<ERC20Token>,
) -> Result<(), anyhow::Error> {
   let token_map: HashMap<Address, &ERC20Token> =
      tokens.iter().map(|token| (token.address, token)).collect();
   let tokens_addr = tokens.iter().map(|t| t.address).collect::<Vec<_>>();
   let client = ctx.get_client_with_id(chain).unwrap();

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
   let client = ctx.get_client_with_id(chain)?;
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
   let client = ctx.get_client_with_id(chain)?;
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

pub async fn measure_rpcs(ctx: ZeusCtx) {
   let providers = ctx.rpc_providers();

   let mut tasks = Vec::new();
   for chain in SUPPORTED_CHAINS {
      let rpcs = providers.get_all(chain);

      for rpc in rpcs {
         let rpc = rpc.clone();
         let ctx = ctx.clone();
         let task = RT.spawn(async move {
            let retry = client::retry_layer(2, 400, 600);
            let throttle = client::throttle_layer(5);
            let client = client::get_http_client(&rpc.url, retry, throttle).unwrap();
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

/// If needed re-sync pools for all tokens across all chains
pub async fn resync_pools(ctx: ZeusCtx) -> bool {
   let need_resync = ctx.pools_need_resync();
   let pool_manager = ctx.pool_manager();

   if need_resync {
      ctx.write(|ctx| {
         ctx.data_syncing = true;
      });

      tracing::info!("Resyncing pools");

      for chain in SUPPORTED_CHAINS {
         let tokens = ctx.get_all_erc20_tokens(chain);
         let client = ctx.get_client_with_id(chain).unwrap();
         for token in tokens {
            match eth::sync_pools_for_token(ctx.clone(), token.clone(), true, true).await {
               Ok(_) => {}
               Err(e) => {
                  tracing::error!(
                     "Failed to sync pools for token {}: {}",
                     token.symbol,
                     e
                  );
               }
            }
         }
         match pool_manager.update(client, chain).await {
            Ok(_) => tracing::info!("Updated price manager for chain: {}", chain),
            Err(e) => tracing::error!(
               "Error updating price manager for chain {}: {:?}",
               chain,
               e
            ),
         }
      }

      ctx.write(|ctx| {
         ctx.data_syncing = false;
      });

      RT.spawn_blocking(move || {
         portfolio_update(ctx.clone());

         match ctx.save_pool_manager() {
            Ok(_) => tracing::info!("Pool data saved for"),
            Err(e) => tracing::error!("Error saving pool data: {:?}", e),
         }
      });

      // resynced
      true
   } else {
      tracing::info!("No need to resync pools");
      false
   }
}
