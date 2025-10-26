use super::ZeusCtx;
use anyhow::anyhow;
use lazy_static::lazy_static;
use serde_json::Value;
use tokio::runtime::Runtime;
use zeus_eth::{
   alloy_dyn_abi::{Eip712Domain, Eip712Types, Resolver, TypedData},
   alloy_primitives::U256,
   currency::{Currency, NativeCurrency},
   utils::NumericValue,
};

pub mod eth;
pub mod sign;
pub mod trace;
pub mod tx;
pub mod update;

lazy_static! {
   pub static ref RT: Runtime = Runtime::new().unwrap();
}

pub fn parse_typed_data(json: Value) -> Result<TypedData, anyhow::Error> {
   let domain: Eip712Domain = serde_json::from_value(json["domain"].clone())?;
   let types: Eip712Types = serde_json::from_value(json["types"].clone())?;
   let resolver = Resolver::from(&types);
   let primary_type =
      json["primaryType"].as_str().ok_or(anyhow!("Missing primaryType"))?.to_string();

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
   let native = NativeCurrency::from(chain);
   let price = ctx.get_currency_price(&Currency::from(native.clone()));

   let cost_in_wei = total_fee * U256::from(gas_used);
   let cost = NumericValue::format_wei(cost_in_wei, native.decimals);

   let cost_in_usd = NumericValue::value(cost.f64(), price.f64());

   (cost, cost_in_usd)
}


pub fn truncate_symbol_or_name(string: &str, max_chars: usize) -> String {
   if string.chars().count() > max_chars {
      // Take the first `max_chars` characters and collect them into a new String
      let truncated: String = string.chars().take(max_chars).collect();
      format!("{}...", truncated)
   } else {
      string.to_string()
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
