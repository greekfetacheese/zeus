use tokio::runtime::Runtime;

use zeus_eth::alloy_provider::{ ProviderBuilder, RootProvider };
use alloy_transport_http::{ reqwest::Url, Client, Http };

use std::path::PathBuf;
use lazy_static::lazy_static;
use super::ZeusCtx;
use zeus_eth::prelude::{ SUPPORTED_CHAINS, ETH, BASE, BSC, OPTIMISM, ARBITRUM, DexKind, UniswapV2Pool, UniswapV3Pool };

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

pub fn get_http_client(url: &str) -> Result<HttpClient, anyhow::Error> {
    let url = Url::parse(url)?;
    let client = ProviderBuilder::new().on_http(url);

    Ok(client)
}

/// Sync all the V2 & V3 pools for all the tokens
pub async fn sync_pools(ctx: ZeusCtx) -> Result<(), anyhow::Error> {
    const MAX_RETRY: usize = 5;
    for chain in SUPPORTED_CHAINS.iter() {
        let ctx = ctx.clone();
        let currencies = ctx.get_currencies(*chain);

        for currency in &*currencies {
            if currency.is_native() {
                continue;
            }

            let mut retry = 0;
            let mut v2_pools = None;
            let mut v3_pools = None;
            let token = currency.erc20().unwrap();

            while v2_pools.is_none() && retry < MAX_RETRY {
                v2_pools = fetch::get_v2_pools_for_token(ctx.clone(), token.clone()).await.ok();
                retry += 1;
                std::thread::sleep(std::time::Duration::from_secs(1));
            }

            retry = 0;
            while v3_pools.is_none() && retry < MAX_RETRY {
                v3_pools = fetch::get_v3_pools_for_token(ctx.clone(), token.clone()).await.ok();
                retry += 1;
                std::thread::sleep(std::time::Duration::from_secs(1));
            }

            if let Some(v2_pools) = v2_pools {
                tracing::info!("Got {} v2 pools for: {}", v2_pools.len(), token.symbol);
                ctx.add_v2_pools(v2_pools);
            } else {
                tracing::error!("Failed to get v2 pools for: {}", token.symbol);
            }

            if let Some(v3_pools) = v3_pools {
                tracing::info!("Got {} v3 pools for: {}", v3_pools.len(), token.symbol);
                ctx.add_v3_pools(v3_pools);
            } else {
                tracing::error!("Failed to get v3 pools for: {}", token.symbol);
            }

            ctx.read(|ctx| {
                ctx.db.save_to_file().expect("Failed to save db");
            });
        }
    }



    Ok(())
}
