use crate::core::{
   ZeusCtx,
   utils::{RT, eth, wallet_value},
};
use std::time::{Duration, Instant};
use zeus_eth::{alloy_provider::Provider, types::SUPPORTED_CHAINS, utils::batch_request::get_erc20_balance};

/// Update the necceary data
pub async fn update(ctx: ZeusCtx) {

   update_price_manager(ctx.clone()).await;

   if let Err(e) = update_eth_balance(ctx.clone()).await {
      tracing::error!("Error updating eth balance: {:?}", e);
   }

   if let Err(e) = update_token_balance(ctx.clone()).await {
      tracing::error!("Error updating token balance: {:?}", e);
   }

   portfolio_update(ctx.clone());

   let pools_synced = ctx.read(|ctx| ctx.db.pool_data_synced);

   if !pools_synced {
      tracing::info!("Syncing pools for the first time");
      ctx.write(|ctx| {
         ctx.pool_data_syncing = true;
      });

      let chains = SUPPORTED_CHAINS.to_vec();
      let res = sync_pools(ctx.clone(), chains).await;
      if res.is_ok() {
         ctx.write(|ctx| {
            ctx.db.pool_data_synced = true;
         });
         ctx.write(|ctx| {
            ctx.pool_data_syncing = false;
         });
         match ctx.save_db() {
            Ok(_) => tracing::info!("DB saved"),
            Err(e) => tracing::error!("Error saving DB: {:?}", e),
         }
      } else {
         ctx.write(|ctx| {
            ctx.pool_data_syncing = false;
         });
         tracing::error!("Error syncing pools: {:?}", res.err().unwrap());
      }

      let ctx_clone = ctx.clone();
      RT.spawn(async move {
         update_price_manager_interval(ctx_clone).await;
      });

      let ctx_clone = ctx.clone();
      RT.spawn(async move {
         portfolio_update_interval(ctx_clone);
      });

      let ctx_clone = ctx.clone();
      RT.spawn(async move {
         balance_update_interval(ctx_clone).await;
      });
   }
}

pub fn portfolio_update_interval(ctx: ZeusCtx) {
   const INTERVAL: u64 = 60;
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
         let _ = wallet_value(ctx.clone(), chain, owner);
      }
   }

   tracing::info!("Calculated Portfolio Value");
   match ctx.save_db() {
      Ok(_) => tracing::info!("DB saved"),
      Err(e) => tracing::error!("Error saving DB: {:?}", e),
   }
}

pub async fn update_price_manager_interval(ctx: ZeusCtx) {
   const INTERVAL: u64 = 300;
   let mut time_passed = Instant::now();

   loop {
      if time_passed.elapsed().as_secs() > INTERVAL {
         update_price_manager(ctx.clone()).await;
         time_passed = Instant::now();
      }
      tokio::time::sleep(Duration::from_secs(1)).await;
   }
}

pub async fn update_price_manager(ctx: ZeusCtx) {
   let pool_manager = ctx.pool_manager();

   let wallets = ctx.profile().wallets;
   for chain in SUPPORTED_CHAINS {
      let client = ctx.get_client_with_id(chain).unwrap();

      for wallet in &wallets {
         let owner = wallet.key.inner().address();
         let portfolio = ctx.get_portfolio(chain, owner).unwrap_or_default();
         if portfolio.currencies().is_empty() {
            continue;
         }
         let tokens = portfolio.erc20_tokens();

         match pool_manager
            .update_minimal(client.clone(), chain, tokens)
            .await
         {
            Ok(_) => tracing::info!(
               "Updated price manager for owner {}, chain: {}",
               owner,
               chain
            ),
            Err(e) => tracing::error!("Error updating price manager: {:?}", e),
         }
      }
   }
   match ctx.save_pool_data() {
      Ok(_) => tracing::info!("Pool data saved"),
      Err(e) => tracing::error!("Error saving pool data: {:?}", e),
   }
}

/// Update the eth balance for all wallets
pub async fn update_eth_balance(ctx: ZeusCtx) -> Result<(), anyhow::Error> {
   let wallets = ctx.profile().wallets;

   for chain in SUPPORTED_CHAINS {
      let client = ctx.get_client_with_id(chain).unwrap();

      for wallet in &wallets {
         let owner = wallet.key.inner().address();
         let balance = client.get_balance(owner).await?;
         ctx.write(|ctx| {
            ctx.db.insert_eth_balance(chain, owner, balance);
         })
      }
   }
   Ok(())
}

/// Update the token balance for all wallets
pub async fn update_token_balance(ctx: ZeusCtx) -> Result<(), anyhow::Error> {
   let wallets = ctx.profile().wallets;

   for chain in SUPPORTED_CHAINS {
      let client = ctx.get_client_with_id(chain).unwrap();

      for wallet in &wallets {
         let owner = wallet.key.inner().address();
         let portfolio = ctx.get_portfolio(chain, owner).unwrap_or_default();
         let tokens = portfolio.erc20_tokens();

         if portfolio.currencies.is_empty() || tokens.is_empty() {
            continue;
         }

         let tokens = tokens.iter().map(|t| t.address).collect::<Vec<_>>();

         for token in tokens.chunks(100) {
            let balances = get_erc20_balance(client.clone(), None, owner, token.to_vec()).await?;
            for balance in balances {
               ctx.write(|ctx| {
                  ctx.db
                     .insert_token_balance(chain, owner, balance.token, balance.balance);
               });
            }
         }
      }
   }
   Ok(())
}

async fn balance_update_interval(ctx: ZeusCtx) {
   const INTERVAL: u64 = 300;
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
