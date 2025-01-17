use tokio::runtime::Runtime;

use zeus_eth::alloy_provider::{ ProviderBuilder, RootProvider };
use alloy_transport_http::{ reqwest::Url, Client, Http };

use std::path::PathBuf;
use lazy_static::lazy_static;

use crate::core::data::db::*;

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

/// Load any required data on startup
pub fn on_startup() -> Result<(), anyhow::Error> {
    if let Ok(db) = ZeusDB::load_from_file() {
        let mut old_db = ZEUS_DB.write().unwrap();
        *old_db = db;
    } else {
        let mut db = ZEUS_DB.write().unwrap();
        db.load_default_currencies()?;
    }

    Ok(())
}

pub fn get_http_client(url: &str) -> Result<HttpClient, anyhow::Error> {
    let url = Url::parse(url)?;
    let client = ProviderBuilder::new().on_http(url);

    Ok(client)
}