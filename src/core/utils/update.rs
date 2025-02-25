use std::time::{Duration, Instant};
use crate::core::{utils::{RT, eth}, ZeusCtx};
use zeus_eth::types::SUPPORTED_CHAINS;


/// Update the necceary data
pub async fn update(ctx: ZeusCtx) {
    RT.spawn(async move {
        update_price_manager(ctx.clone()).await;
    });
}

pub async fn update_price_manager(ctx: ZeusCtx) {
    const INTERVAL: u64 = 600;

    let mut time_passed = Instant::now();

    loop {
        if time_passed.elapsed().as_secs() > INTERVAL {
            let pool_manager = ctx.pool_manager();

            for chain in SUPPORTED_CHAINS {
                let client = ctx.get_client_with_id(chain).unwrap();
                let res = pool_manager.update(client.clone(), chain).await;
                if let Err(e) = res {
                    tracing::error!("Error updating pool manager: {:?}", e);
                }
            }
            time_passed = Instant::now();

            ctx.save_pool_data().unwrap();
            tracing::info!("Pool State Manager updated");
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}




/// Sync all the V2 & V3 pools for all the tokens
pub async fn sync_pools(ctx: ZeusCtx, chains: Vec<u64>) -> Result<(), anyhow::Error> {
    const MAX_RETRY: usize = 5;

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
                        v3_pools = Some(pools);
                    }
                    Err(e) => tracing::error!("Error getting v3 pools: {:?}", e),
                }
                retry += 1;
                std::thread::sleep(Duration::from_secs(1));
            }

            if let Some(v2_pools) = v2_pools {
                tracing::info!("Got {} v2 pools for: {}", v2_pools.len(), token.symbol);
                ctx.add_v2_pools(v2_pools);
            }

            if let Some(v3_pools) = v3_pools {
                tracing::info!("Got {} v3 pools for: {}", v3_pools.len(), token.symbol);
                ctx.add_v3_pools(v3_pools);
            }
        }
    }

    ctx.save_pool_data()?;

    Ok(())
}