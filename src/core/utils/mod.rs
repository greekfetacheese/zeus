use tokio::runtime::Runtime;

use zeus_eth::alloy_provider::{ ProviderBuilder, RootProvider };
use alloy_transport_http::{ reqwest::Url, Client, Http };

use std::path::PathBuf;
use lazy_static::lazy_static;


pub mod tracing;

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