pub mod bip32;
pub mod context;
pub mod transaction;
pub mod tx_analysis;
pub mod user;
pub mod utils;

pub use context::*;
pub use transaction::*;
pub use tx_analysis::TransactionAnalysis;
pub use user::wallet::*;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Dapp {
   Across,
   Uniswap,
}

impl Dapp {
   pub fn is_across(&self) -> bool {
      matches!(self, Self::Across)
   }

   pub fn is_uniswap(&self) -> bool {
      matches!(self, Self::Uniswap)
   }
}

mod serde_hashmap {
   use serde::{Deserialize, Deserializer, Serialize, Serializer, de::DeserializeOwned};
   use std::collections::HashMap;

   pub fn serialize<S, K, V>(map: &HashMap<K, V>, serializer: S) -> Result<S::Ok, S::Error>
   where
      S: Serializer,
      K: Serialize,
      V: Serialize,
   {
      let stringified_map: HashMap<String, &V> =
         map.iter().map(|(k, v)| (serde_json::to_string(k).unwrap(), v)).collect();
      stringified_map.serialize(serializer)
   }

   pub fn deserialize<'de, D, K, V>(deserializer: D) -> Result<HashMap<K, V>, D::Error>
   where
      D: Deserializer<'de>,
      K: DeserializeOwned + std::cmp::Eq + std::hash::Hash,
      V: DeserializeOwned,
   {
      let stringified_map: HashMap<String, V> = HashMap::deserialize(deserializer)?;
      stringified_map
         .into_iter()
         .map(|(k, v)| {
            let key = serde_json::from_str(&k).map_err(serde::de::Error::custom)?;
            Ok((key, v))
         })
         .collect()
   }
}
