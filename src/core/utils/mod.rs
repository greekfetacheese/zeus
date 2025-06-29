use super::ZeusCtx;
use alloy_dyn_abi::{Eip712Domain, Eip712Types, Resolver, TypedData};
use anyhow::anyhow;
use lazy_static::lazy_static;
use serde_json::Value;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::runtime::Runtime;
use zeus_eth::{
   alloy_primitives::{Address, U160, U256, aliases::U48},
   alloy_provider::Provider,
   alloy_network::Network,
   currency::{Currency, ERC20Token, NativeCurrency},
   utils::{address, NumericValue, generate_permit2_batch_value},
   abi::permit::{self, Permit2::{PermitBatch, PermitDetails}},
};

use egui_theme::ThemeKind;


pub mod eth;
pub mod sign;
pub mod trace;
pub mod tx;
pub mod update;

lazy_static! {
   pub static ref RT: Runtime = Runtime::new().unwrap();
}

const THEME_FILE: &str = "theme.json";
const POOL_DATA_FULL: &str = "pool_data_full.json";
const POOL_DATA_FILE: &str = "pool_data.json";

/// Zeus data directory
pub fn data_dir() -> Result<PathBuf, anyhow::Error> {
   let dir = std::env::current_dir()?.join("data");

   if !dir.exists() {
      std::fs::create_dir_all(dir.clone())?;
   }

   Ok(dir)
}

pub fn theme_kind_dir() -> Result<PathBuf, anyhow::Error> {
   let dir = data_dir()?.join(THEME_FILE);
   Ok(dir)
}

pub fn load_theme_kind() -> Result<ThemeKind, anyhow::Error> {
   let dir = theme_kind_dir()?;
   let theme_kind_str = std::fs::read_to_string(dir)?;
   let theme_kind = serde_json::from_str(&theme_kind_str)?;
   Ok(theme_kind)
}

/// Pool data directory
pub fn pool_data_dir() -> Result<PathBuf, anyhow::Error> {
   let dir = data_dir()?.join(POOL_DATA_FILE);
   Ok(dir)
}

pub fn pool_data_full_dir() -> Result<PathBuf, anyhow::Error> {
   let dir = data_dir()?.join(POOL_DATA_FULL);
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
   let native = NativeCurrency::from(chain);
   let price = ctx.get_currency_price(&Currency::from(native.clone()));

   let cost_in_wei = total_fee * U256::from(gas_used);
   let cost = NumericValue::format_wei(cost_in_wei, native.decimals);

   let cost_in_usd = NumericValue::value(cost.f64(), price.f64());

   (cost, cost_in_usd)
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


/// In X days from now in UNIX time
pub fn get_unix_time_in_days(days: u64) -> Result<u64, anyhow::Error> {
   let now = std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)?
      .as_secs();

   Ok(now + 86400 * days)
}

/// In X minutes from now in UNIX time
pub fn get_unix_time_in_minutes(minutes: u64) -> Result<u64, anyhow::Error> {
   let now = std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)?
      .as_secs();

   Ok(now + 60 * minutes)
}



#[derive(Debug, Clone)]
pub struct Permit2BatchApproval {
   /// The permit batch struct to be abi encoded
   pub permit_batch: PermitBatch,
   /// The message value to be signed
   pub msg_value: Value,
   /// Tokens to approve
   pub tokens: Vec<ERC20Token>,
   /// Amounts to approve
   pub amounts: Vec<NumericValue>,
}

#[derive(Debug, Clone)]
pub struct TokenAmounts {
   pub token: ERC20Token,
   pub amount: NumericValue,
}

/// See if the given tokens need Permit2 approval and return the details
///
/// ## Arguments
///
/// * `tokens` - The tokens to check
/// * `owner` - The owner address
/// * `spender` - The spender address
/// * `amount` - The amount to approve
/// * `expiration` - The expiration time
/// * `sig_deadline` - The signature deadline
///
/// ## Returns
///
/// Returns `None` if no approval is needed
///
/// Returns `Some(Permit2BatchApproval)` if approval is needed
pub async fn get_permit2_batch_approval<P, N>(
   client: P,
   chain_id: u64,
   tokens: Vec<TokenAmounts>,
   owner: Address,
   spender: Address,
   expiration: U256,
   sig_deadline: U256,
) -> Result<Option<Permit2BatchApproval>, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let mut futures = Vec::new();
   let permit2_address = address::permit2_contract(chain_id)?;

   for token in &tokens {
      let allowance = permit::allowance(
         client.clone(),
         permit2_address,
         owner,
         token.token.address,
         spender,
      );
      futures.push(allowance);
   }

   let allowances = futures::future::join_all(futures).await;

   let allowances = allowances
      .into_iter()
      .zip(tokens.into_iter())
      .collect::<Vec<_>>();

   let current_time = std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)?
      .as_secs();

   let mut permit_details = Vec::new();

   let mut tokens_to_approve = Vec::new();
   let mut amounts_to_approve = Vec::new();
   for (allowance, token) in allowances {
      let allowance = allowance?;

      let expired = u64::try_from(allowance.expiration)? < current_time;
      let needs_permit2 = U256::from(allowance.amount) < token.amount.wei2() || expired;

      if needs_permit2 {
         tokens_to_approve.push(token.token.clone());
         amounts_to_approve.push(token.amount.clone());

         permit_details.push(PermitDetails {
            token: token.token.address,
            amount: U160::from(token.amount.wei2()),
            expiration: U48::from(expiration),
            nonce: allowance.nonce,
         });
      }
   }

   if !permit_details.is_empty() {
      let permit_batch = PermitBatch {
         details: permit_details.clone(),
         spender,
         sigDeadline: sig_deadline,
      };

      let msg_value = generate_permit2_batch_value(
         chain_id,
         permit_details,
         spender,
         permit2_address,
         sig_deadline,
      );

      return Ok(Some(Permit2BatchApproval {
         permit_batch,
         msg_value,
         tokens: tokens_to_approve,
         amounts: amounts_to_approve,
      }));
   } else {
      return Ok(None);
   }
}



pub fn generate_eip2612_permit_msg(
    token_name: String,
    token_address: Address,
    chain_id: u64,
    owner: Address,
    spender: Address,
    value: U256,
    nonce: U256,
    deadline: U256,
) -> Value {
    serde_json::json!({
      "types": {
        "EIP712Domain": [
          {"name": "name", "type": "string"},
          {"name": "version", "type": "string"},
          {"name": "chainId", "type": "uint256"},
          {"name": "verifyingContract", "type": "address"}
        ],
        "Permit": [
          {"name": "owner", "type": "address"},
          {"name": "spender", "type": "address"},
          {"name": "value", "type": "uint256"},
          {"name": "nonce", "type": "uint256"},
          {"name": "deadline", "type": "uint256"}
        ]
      },
      "primaryType": "Permit",
      "domain": {
        "name": token_name,
        "version": "1",
        "chainId": chain_id.to_string(),
        "verifyingContract": token_address.to_string()
      },
      "message": {
        "owner": owner.to_string(),
        "spender": spender.to_string(),
        "value": value.to_string(),
        "nonce": nonce.to_string(),
        "deadline": deadline.to_string()
      }
    })
}