use super::ZeusCtx;
use alloy_dyn_abi::{Eip712Domain, Eip712Types, Resolver, TypedData};
use anyhow::anyhow;
use lazy_static::lazy_static;
use serde_json::Value;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH, Duration};
use tokio::runtime::Runtime;
use zeus_eth::{
   alloy_primitives::{U256, utils::format_units},
   currency::{Currency, NativeCurrency},
   utils::NumericValue,
};

pub mod action;
pub mod eth;
pub mod sign;
pub mod trace;
pub mod tx;
pub mod update;

lazy_static! {
   pub static ref RT: Runtime = Runtime::new().unwrap();
}

const POOL_DATA_FILE: &str = "pool_data.json";

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
   let dir = data_dir()?.join(POOL_DATA_FILE);
   Ok(dir)
}

pub fn parse_typed_data(json: Value) -> Result<TypedData, anyhow::Error> {
   let domain: Eip712Domain = serde_json::from_value(json["domain"].clone())?;
   let types: Eip712Types = serde_json::from_value(json["types"].clone())?;
   let resolver = Resolver::from(&types);
   let primary_type = json["primaryType"]
      .as_str()
      .ok_or(anyhow!("Missing primaryType"))?
      .to_string();

   let message = json["message"].clone();

   Ok(TypedData {
      domain,
      resolver,
      primary_type,
      message,
   })
}

/// Estimate the cost for a transaction
///
/// Returns (cost_in_wei, cost_in_usd)
pub fn estimate_tx_cost(
   ctx: ZeusCtx,
   chain: u64,
   gas_used: u64,
   priority_fee: U256,
) -> (NumericValue, NumericValue) {
   let base_fee = ctx.get_base_fee(chain).unwrap_or_default().next;
   let total_fee = priority_fee + U256::from(base_fee);

   // native currency price
   let native = NativeCurrency::from_chain_id(chain).unwrap();
   let price = ctx.get_currency_price(&Currency::from(native.clone()));

   let cost_in_wei = total_fee * U256::from(gas_used);
   let cost = format_units(cost_in_wei, native.decimals).unwrap_or_default();
   let cost: f64 = cost.parse().unwrap_or_default();

   let cost_in_usd = NumericValue::value(cost, price.f64());
   let cost_in_wei = NumericValue::format_wei(cost_in_wei, 18);

   (cost_in_wei, cost_in_usd)
}

pub fn format_expiry(timestamp: u64) -> String {
   let now = SystemTime::now();
   let expiry_time = UNIX_EPOCH + Duration::from_secs(timestamp);

   match expiry_time.duration_since(now) {
      Ok(duration) => {
         let secs = duration.as_secs();
         if secs < 60 {
            format!("{} seconds", secs)
         } else if secs < 3600 {
            format!("{} minutes", secs / 60)
         } else if secs < 86400 {
            format!("{} hours", secs / 3600)
         } else if secs < 2_592_000 {
            // ~30 days
            format!("{} days", secs / 86400)
         } else {
            let months = secs / 2_592_000;
            format!("{} months", months)
         }
      }
      Err(_) => "Expired".to_string(),
   }
}

pub fn truncate_address(address: String) -> String {
   format!("{}...{}", &address[..6], &address[36..])
}

pub fn truncate_hash(hash: String) -> String {
   if hash.len() > 38 {
      format!("{}...{}", &hash[..6], &hash[hash.len() - 4..])
   } else {
      hash
   }
}
