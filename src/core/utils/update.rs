use crate::core::{
   BaseFee, ZeusCtx,
   context::{Portfolio, price_manager::TOKEN_PRICE_UPDATE_INTERVAL},
   utils::*,
};
use anyhow::{anyhow, bail};
use std::collections::HashSet;
use std::time::{Duration, Instant};
use zeus_eth::{
   alloy_primitives::{U256, utils::format_units},
   alloy_provider::Provider,
   alloy_rpc_types::BlockId,
   amm::uniswap::DexKind,
   currency::ERC20Token,
   types::{ChainId, SUPPORTED_CHAINS},
   utils::{NumericValue, block::calculate_next_block_base_fee},
};

const MEASURE_RPCS_INTERVAL: u64 = 120;
const PORTFOLIO_INTERVAL: u64 = 600;
const BALANCE_INTERVAL: u64 = 600;
const PRIORITY_FEE_INTERVAL: u64 = 90;
const BASE_FEE_INTERVAL: u64 = 180;

pub async fn test_and_measure_rpcs(ctx: ZeusCtx) {
   let client = ctx.get_zeus_client();

   let mut tasks = Vec::new();

   let time = std::time::Instant::now();
   for chain in SUPPORTED_CHAINS {
      let rpcs = client.read(|rpcs| rpcs.get(&chain).unwrap_or(&vec![]).clone());

      for rpc in &rpcs {
         let client = client.clone();

         if rpc.should_run_check() {
            let rpc = rpc.clone();
            let ctx = ctx.clone();

            let task = RT.spawn(async move {
               client.run_check_for(ctx, rpc).await;
            });
            tasks.push(task);
         } else {
            let rpc = rpc.clone();
            let task = RT.spawn(async move {
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
            Ok(_) => {}
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
   RT.spawn_blocking(move || {
      insert_missing_portfolios(ctx_clone);
   });

   let ctx_clone = ctx.clone();
   RT.spawn(async move {
      update_token_prices_interval(ctx_clone).await;
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

   // Sync V4 pools if needed
   let sync_v4 = ctx.pool_manager().do_we_sync_v4_pools();

   if sync_v4 {
      ctx.write(|ctx| {
         ctx.dex_syncing = true;
      });

      let mut tasks = Vec::new();
      for chain in SUPPORTED_CHAINS {
         /*
         let ignore_chains = ctx.pool_manager().ignore_chains();
         if ignore_chains.contains(&chain) {
            continue;
         }
          */

         tracing::info!("Syncing V4 pools for chain {}", chain);

         let ctx_clone = ctx.clone();

         let task = RT.spawn(async move {
            let manager = ctx_clone.pool_manager();
            let dex = DexKind::UniswapV4;

            match manager.sync_pools(ctx_clone.clone(), chain.into(), dex, None).await {
               Ok(_) => {}
               Err(e) => tracing::error!("Error syncing pools: {:?}", e),
            }

            // let v4_pools = manager.get_v4_pools_for_chain(chain);

            /*
            match manager.update_state_for_pools(ctx_clone, chain, v4_pools).await {
               Ok(_) => {}
               Err(e) => tracing::error!(
                  "Error updating pool manager for chain {}: {:?}",
                  chain,
                  e
               ),
            }
            */
         });
         tasks.push(task);
      }

      for task in tasks {
         let _ = task.await;
      }

      let manager = ctx.pool_manager();
      manager.remove_v4_pools_with_no_base_token();
      manager.remove_v4_pools_with_high_fee();
      // manager.remove_v4_pools_with_no_liquidity();

      ctx.write(|ctx| {
         ctx.dex_syncing = false;
      });
   }

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
   ctx.save_portfolio_db();
   tracing::info!("Updated portfolio value");
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

pub async fn update_token_prices_interval(ctx: ZeusCtx) {
   let mut time_passed = Instant::now();

   loop {
      if time_passed.elapsed().as_secs() > TOKEN_PRICE_UPDATE_INTERVAL {
         update_token_prices(ctx.clone()).await;
         time_passed = Instant::now();
      }
      tokio::time::sleep(Duration::from_secs(1)).await;
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
            Ok(_) => tracing::info!("Updated base token prices for chain: {}", chain),
            Err(e) => tracing::error!(
               "Error updating base token prices for chain {}: {:?}",
               chain,
               e
            ),
         }

         match price_manager.calculate_prices(ctx, chain, pool_manager, tokens).await {
            Ok(_) => tracing::info!("Updated token prices for chain: {}", chain),
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

   RT.spawn_blocking(move || {
      ctx.save_price_manager();
   });
}

async fn balance_update_interval(ctx: ZeusCtx) {
   let mut time_passed = Instant::now();

   loop {
      if time_passed.elapsed().as_secs() > BALANCE_INTERVAL {
         let manager = ctx.balance_manager();
         manager.update_eth_balance_across_wallets_and_chains(ctx.clone()).await;
         manager.update_tokens_balance_across_wallets_and_chains(ctx.clone()).await;
         time_passed = Instant::now();
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
         bail!("Latest block not found");
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
   let z_client = ctx.get_zeus_client();
   let chain = ChainId::new(chain)?;
   if chain.is_ethereum() || chain.is_optimism() || chain.is_base() {
      let fee = z_client
         .request(chain.id(), |client| async move {
            client.get_max_priority_fee_per_gas().await.map_err(|e| anyhow!("{:?}", e))
         })
         .await?;

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

pub async fn measure_rpcs_interval(ctx: ZeusCtx) {
   let mut time_passed = Instant::now();

   loop {
      if time_passed.elapsed().as_secs() > MEASURE_RPCS_INTERVAL {
         let z_client = ctx.get_zeus_client();
         z_client.run_latency_checks().await;
         time_passed = Instant::now();
      }
      tokio::time::sleep(Duration::from_secs(1)).await;
   }
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

         let dexes = DexKind::main_dexes(chain);
         let pool_manager = ctx.pool_manager();

         match pool_manager
            .sync_pools_for_tokens(ctx.clone(), chain, tokens, dexes, false)
            .await
         {
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
      ctx.save_pool_manager();
      ctx.save_price_manager();
   });
}
