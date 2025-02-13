use tokio::runtime::Runtime;

use zeus_eth::alloy_provider::{ ProviderBuilder, RootProvider };
use alloy_transport_http::{ reqwest::Url, Client, Http };

use std::path::PathBuf;
use lazy_static::lazy_static;
use super::ZeusCtx;
use zeus_eth::types::*;
use zeus_eth::currency::erc20::ERC20Token;

pub mod trace;

lazy_static! {
    pub static ref RT: Runtime = Runtime::new().unwrap();
}

pub type HttpClient = RootProvider<Http<Client>>;

pub mod fetch;

/// Zeus data directory
pub fn data_dir() -> Result<PathBuf, anyhow::Error> {
    let dir = std::env::current_dir()?.join("data");

    if !dir.exists() {
        std::fs::create_dir_all(dir.clone())?;
    }

    Ok(dir)
}


/// Pool data directory
pub fn pool_data_dir() -> Result<PathBuf, anyhow::Error> {
    let dir = data_dir()?.join("pool_data.json");
    Ok(dir)
}

pub fn get_http_client(url: &str) -> Result<HttpClient, anyhow::Error> {
    let url = Url::parse(url)?;
    let client = ProviderBuilder::new().on_http(url);

    Ok(client)
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
                match fetch::get_v2_pools_for_token(ctx.clone(), token.clone()).await {
                    Ok(pools) => v2_pools = Some(pools),
                    Err(e) => tracing::error!("Error getting v2 pools: {:?}", e)
                };
                retry += 1;
                std::thread::sleep(std::time::Duration::from_secs(1));
            }

            retry = 0;
            while v3_pools.is_none() && retry < MAX_RETRY {
                match fetch::get_v3_pools_for_token(ctx.clone(), token.clone()).await {
                    Ok(pools) => v3_pools = Some(pools),
                    Err(e) => tracing::error!("Error getting v3 pools: {:?}", e)
                };
                retry += 1;
                std::thread::sleep(std::time::Duration::from_secs(1));
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

pub async fn update(ctx: ZeusCtx) {
    const INTERVAL: u64 = 600;
    update_price_manager(ctx.clone()).await;

    let mut time_passed = std::time::Instant::now();

    loop {
        if time_passed.elapsed().as_secs() > INTERVAL {
            update_price_manager(ctx.clone()).await;
            time_passed = std::time::Instant::now();
        }

        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}

pub async fn update_price_manager(ctx: ZeusCtx) {
    let pool_manager = ctx.pool_manager();

    for chain in SUPPORTED_CHAINS {
        let client = ctx.get_client_with_id(chain).unwrap();

        let base_tokens = ERC20Token::base_tokens(chain);
        let res = pool_manager.update_base_token_prices(client.clone(), base_tokens).await;
        if let Err(e) = res {
            tracing::error!("Error updating base token prices: {:?}", e);
        }
    }
    ctx.save_pool_data().unwrap();
    tracing::info!("Pool State Manager updated");
}