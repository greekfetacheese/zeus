use crate::core::ZeusContext;

use zeus_eth::{
   alloy_primitives::U256,
   currency::{Currency, NativeCurrency},
   utils::NumericValue,
};

use chrono::{DateTime, Utc};
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::runtime::Runtime;

use crate::core::ctx::{railgun_db_file, railgun_dir};
use zeus_eth::utils::client::RpcClient;
use zeus_railgun::{
   ChainConfig, Groth16Prover, RailgunDbKey, RailgunProvider, RedbDatabase, RootVerifier,
   RpcSyncer, SnapshotLoader, SubsquidSyncer, UtxoIndexer, UtxoSyncer,
};

use anyhow::anyhow;

lazy_static! {
   pub static ref RT: Runtime = Runtime::new().unwrap();
}

pub async fn create_railgun_provider(
   client: RpcClient,
   chain: u64,
   db_key: RailgunDbKey,
) -> Result<RailgunProvider<RpcClient>, anyhow::Error> {
   let time = std::time::Instant::now();

   let db_file = railgun_db_file(chain)?;
   let railgun_dir = railgun_dir()?;

   let snapshot_loader = SnapshotLoader::new(railgun_dir.clone());
   let chain_config = match ChainConfig::from_chain_id(chain) {
      Some(chain_config) => chain_config,
      None => {
         return Err(anyhow!("Chain {} not supported", chain));
      }
   };

   let utxo_verifier = RootVerifier::new(client.clone(), chain_config.railgun_smart_wallet);
   let rpc_syncer = RpcSyncer::new(
      client.clone(),
      chain,
      chain_config.railgun_smart_wallet,
   )
   .with_snapshot_loader(snapshot_loader.clone());

   let subsquid_syncer: Option<Arc<dyn UtxoSyncer>> = Some(Arc::new(
      SubsquidSyncer::new(&chain_config.subsquid_endpoint, chain)
         .with_snapshot_loader(snapshot_loader),
   ));

   let db = RedbDatabase::new(db_file, db_key)?;
   let utxo_indexer = UtxoIndexer::new(
      Arc::new(db),
      Arc::new(rpc_syncer),
      subsquid_syncer,
      Arc::new(utxo_verifier),
   )
   .await?;

   let prover = Groth16Prover::new(Some(railgun_dir))
      .with_embedded_circuits(crate::embedded::railgun::embedded_circuits());

   let railgun_provider = RailgunProvider::new(
      chain_config,
      client.clone(),
      utxo_indexer,
      prover,
      None,
   )
   .await?;

   tracing::info!("Railgun provider created for chain {} in {}ms", chain, time.elapsed().as_millis());

   Ok(railgun_provider)
}

/// Estimate the cost for a transaction
///
/// Returns (cost_in_wei, cost_in_usd)
pub fn estimate_tx_cost(
   ctx: &mut ZeusContext,
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
