use crate::core::ZeusCtx;

use zeus_eth::{
   abi::permit::{Permit2::allowanceReturn, allowance},
   alloy_primitives::{Address, Signature, U256},
   alloy_signer::Signer,
   currency::{Currency, ERC20Token, NativeCurrency},
   utils::{
      NumericValue, SecureSigner, address_book, generate_permit2_json_value, parse_typed_data,
   },
};

use anyhow::anyhow;
use chrono::{DateTime, Utc};
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::cmp::Ordering;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::runtime::Runtime;

lazy_static! {
   pub static ref RT: Runtime = Runtime::new().unwrap();
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

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimeStamp {
   Seconds(u64),
   Millis(u64),
}

impl Default for TimeStamp {
   fn default() -> Self {
      TimeStamp::Seconds(0)
   }
}

impl TimeStamp {
   pub fn now_as_secs() -> Self {
      TimeStamp::Seconds(SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs())
   }

   pub fn now_as_millis() -> Self {
      TimeStamp::Millis(SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64)
   }

   pub fn add(self, seconds: u64) -> Self {
      match self {
         TimeStamp::Seconds(s) => TimeStamp::Seconds(s + seconds),
         TimeStamp::Millis(m) => TimeStamp::Millis(m + seconds * 1000),
      }
   }

   pub fn sub(self, seconds: u64) -> Self {
      match self {
         TimeStamp::Seconds(s) => TimeStamp::Seconds(s - seconds),
         TimeStamp::Millis(m) => TimeStamp::Millis(m - seconds * 1000),
      }
   }

   pub fn timestamp(&self) -> u64 {
      match self {
         TimeStamp::Seconds(seconds) => *seconds,
         TimeStamp::Millis(millis) => *millis,
      }
   }

   pub fn cmp(&self, other: &Self) -> Ordering {
      match (self, other) {
         (TimeStamp::Seconds(a), TimeStamp::Seconds(b)) => a.cmp(b),
         (TimeStamp::Millis(a), TimeStamp::Millis(b)) => a.cmp(b),
         _ => Ordering::Equal,
      }
   }

   pub fn to_relative(&self) -> String {
      timestamp_to_relative_time(&self)
   }

   pub fn to_date_string(&self) -> String {
      let dt_opt = match self {
         TimeStamp::Seconds(seconds) => DateTime::<Utc>::from_timestamp_secs(*seconds as i64),
         TimeStamp::Millis(millis) => DateTime::<Utc>::from_timestamp_millis(*millis as i64),
      };

      if let Some(dt) = dt_opt {
         dt.format("%Y-%m-%d %H:%M:%S %Z").to_string()
      } else {
         format!("Invalid timestamp: {}", self.timestamp())
      }
   }

   pub fn to_date(&self) -> DateTime<Utc> {
      let opt = match self {
         TimeStamp::Seconds(seconds) => DateTime::<Utc>::from_timestamp_secs(*seconds as i64),
         TimeStamp::Millis(millis) => DateTime::<Utc>::from_timestamp_millis(*millis as i64),
      };

      match opt {
         Some(dt) => dt,
         None => DateTime::<Utc>::default(),
      }
   }
}

/// Convert a timestamp to relative time
///
/// Eg. X time ago, or in X time
fn timestamp_to_relative_time(timestamp: &TimeStamp) -> String {
   let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();

   let (now, timestamp) = match timestamp {
      TimeStamp::Seconds(seconds) => (now.as_secs(), *seconds),
      TimeStamp::Millis(millis) => (now.as_millis() as u64, *millis),
   };

   let elapsed_opt = if now > timestamp {
      Some(now - timestamp)
   } else {
      None
   };

   let future_time_opt = if timestamp > now {
      Some(timestamp - now)
   } else {
      None
   };

   if let Some(elapsed) = elapsed_opt {
      if elapsed < 60 {
         return format!("{} seconds ago", elapsed);
      } else if elapsed < 3600 {
         return format!("{} minutes ago", elapsed / 60);
      } else if elapsed < 86400 {
         return format!("{} hours ago", elapsed / 3600);
      } else if elapsed < 604800 {
         return format!("{} days ago", elapsed / 86400);
      } else if elapsed < 2419200 {
         return format!("{} weeks ago", elapsed / 604800);
      } else if elapsed < 29030400 {
         return format!("{} months ago", elapsed / 2419200);
      } else if elapsed < 31536000 {
         return format!("{} years ago", elapsed / 29030400);
      }
   }

   if let Some(future_time) = future_time_opt {
      if future_time < 60 {
         return format!("in {} seconds", future_time);
      } else if future_time < 3600 {
         return format!("in {} minutes", future_time / 60);
      } else if future_time < 86400 {
         return format!("in {} hours", future_time / 3600);
      } else if future_time < 604800 {
         return format!("in {} days", future_time / 86400);
      } else if future_time < 2419200 {
         return format!("in {} weeks", future_time / 604800);
      } else if future_time < 29030400 {
         return format!("in {} months", future_time / 2419200);
      } else if future_time < 31536000 {
         return format!("in {} years", future_time / 29030400);
      }
   }

   format!("Invalid timestamp")
}

/// Info for a token approval through the Permit2 contract
#[derive(Clone)]
pub struct Permit2Details {
   /// The allowance details from the Permit2 contract for a token
   pub allowance: allowanceReturn,

   /// Whether the Permit2 contract needs to be approved to spend the token
   ///
   /// Usually we do this approval one-time with unlimited allowance
   pub permit2_needs_approval: bool,

   /// Whether we need to sign again an approval
   pub needs_new_signature: bool,

   /// When this approval should expire
   pub expiration: U256,

   /// When this signature should expire
   pub sig_deadline: U256,

   /// The message to be signed
   pub msg: Option<Value>,
}

impl Permit2Details {
   pub async fn new(
      ctx: ZeusCtx,
      chain: u64,
      token: &ERC20Token,
      amount: U256,
      owner: Address,
      spender: Address,
   ) -> Result<Self, anyhow::Error> {
      let permit2 = address_book::permit2_contract(chain)?;
      let client = ctx.get_zeus_client();

      let data_fut = client.request(chain, |client| async move {
         let data = allowance(client, permit2, owner, token.address, spender).await?;
         Ok(data)
      });

      let allowance_fut = client.request(chain, |client| async move {
         token.allowance(client, owner, permit2).await
      });

      let (data, allowance) = tokio::try_join!(data_fut, allowance_fut)?;

      let permit2_contract_need_approval = allowance < amount;

      let current_time = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();

      let expired = u64::try_from(data.expiration)? < current_time;
      let needs_new_signature = U256::from(data.amount) < amount || expired;

      #[cfg(feature = "dev")]
      {
         tracing::info!("AllowanceReturn {:?}", data);
         tracing::info!("Permit2 Expired: {}", expired);
         tracing::info!(
            "Permit2 Contract Needs Approval: {}",
            permit2_contract_need_approval
         );
         tracing::info!(
            "Permit2 Needs New Signature: {}",
            needs_new_signature
         );
      }

      let expiration = U256::from(current_time + 30 * 24 * 60 * 60); // 30 days
      let sig_deadline = U256::from(current_time + 30 * 60); // 30 minutes

      let value = if needs_new_signature {
         let v = generate_permit2_json_value(
            chain,
            token.address,
            spender,
            amount,
            permit2,
            expiration,
            sig_deadline,
            data.nonce,
         );
         Some(v)
      } else {
         None
      };

      Ok(Self {
         allowance: data,
         permit2_needs_approval: permit2_contract_need_approval,
         needs_new_signature,
         expiration,
         sig_deadline,
         msg: value,
      })
   }

   pub async fn sign(&self, signer: &SecureSigner) -> Result<Signature, anyhow::Error> {
      let typed = if let Some(msg) = &self.msg {
         parse_typed_data(msg.clone())?
      } else {
         return Err(anyhow!("No message to sign"));
      };

      let signature = signer.to_signer().sign_dynamic_typed_data(&typed).await?;
      Ok(signature)
   }
}
