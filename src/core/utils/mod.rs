use tokio::runtime::Runtime;

use zeus_eth::alloy_provider::{ ProviderBuilder, RootProvider };
use alloy_transport_http::{ reqwest::Url, Client, Http };

use std::path::PathBuf;
use lazy_static::lazy_static;
use super::ZeusCtx;
use zeus_eth::{SUPPORTED_CHAINS, defi};


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


pub async fn sync_token_usd_prices(ctx: ZeusCtx) -> Result<(), anyhow::Error> {
    for chain in SUPPORTED_CHAINS.iter() {
        let ctx = ctx.clone();
        let currencies = ctx.get_currencies(*chain);

        for currency in &*currencies {
            if currency.is_native() {
                continue;
            }

            let token = currency.erc20().unwrap();
            let price = fetch::get_token_price(ctx.clone(), token.clone()).await?;
            tracing::info!("Price for {} on chain {} is {}", token.symbol, chain, price);
            ctx.write(|ctx| {
                ctx.db.insert_price(*chain, token.address, price);
            });
        }
    }
    ctx.read(|ctx| {
        ctx.db.save_to_file().unwrap();
    });

    tracing::info!("Finished syncing token prices");
    Ok(())
}