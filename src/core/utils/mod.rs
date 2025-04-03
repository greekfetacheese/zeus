use lazy_static::lazy_static;
use std::path::PathBuf;
use tokio::runtime::Runtime;
use zeus_eth::{
   alloy_primitives::{U256, utils::format_units},
   currency::ERC20Token,
   utils::NumericValue,
};

use super::ZeusCtx;

pub mod eth;
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

/// Estimate the gas cost for a transaction
///
/// Returns (cost_in_wei, cost_in_usd)
pub fn estimate_gas_cost(ctx: ZeusCtx, chain: u64, gas_used: u64, priority_fee: U256) -> (U256, NumericValue) {
   let base_fee = ctx.get_base_fee(chain).unwrap_or_default().next;
   let total_fee = priority_fee + U256::from(base_fee);

   // native currency price
   let wrapped_token = ERC20Token::wrapped_native_token(chain);
   let price = ctx.get_token_price(&wrapped_token).unwrap_or_default();

   let cost_in_wei = total_fee * U256::from(gas_used);
   let cost = format_units(cost_in_wei, wrapped_token.decimals).unwrap_or_default();
   let cost: f64 = cost.parse().unwrap_or_default();

   let cost_in_usd = NumericValue::value(cost, price.f64());

   (cost_in_wei, cost_in_usd)
}
