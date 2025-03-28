pub mod context;
pub mod user;
pub mod utils;

pub use context::*;
pub use user::{Account, wallet::Wallet};



mod serde_hashmap {
    use serde::{Deserialize, Deserializer, Serialize, Serializer, de::DeserializeOwned};
    use std::collections::HashMap;
 
    pub fn serialize<S, K, V>(map: &HashMap<K, V>, serializer: S) -> Result<S::Ok, S::Error>
    where
       S: Serializer,
       K: Serialize,
       V: Serialize,
    {
       let stringified_map: HashMap<String, &V> = map
          .iter()
          .map(|(k, v)| (serde_json::to_string(k).unwrap(), v))
          .collect();
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