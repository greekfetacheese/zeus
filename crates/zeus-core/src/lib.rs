pub mod clear_signing;
pub mod context;
pub mod data_export;
pub mod data_import;
pub mod embedded;
pub mod persisted;
pub mod platform;
pub mod signature;
pub mod simulate;
pub mod tx;
pub mod types;
pub mod ui;
pub mod urls;
pub mod vault;
pub mod wallet;
pub mod wallet_state;
pub mod wallet_state_key;

pub use context::*;
pub use platform::{
   TimeStamp, malloc_trim, restrict_dir_to_owner, restrict_file_to_owner, truncate_address,
   truncate_hash, write_private, write_private_atomic, write_private_from_reader,
};
pub use signature::{msg::SignMsgType, sign::sign_message};
pub use tx::{
   analysis::*, approval_diff::*, approve::*, balance_diff::*, events::*, finalize::*, rich::*,
   send::*, send_calls::*,
};
pub use vault::*;
pub use wallet::*;
pub use wallet_state::{WalletState, WalletStateInner, WalletStateKey};

/// Whether this crate was compiled with its own `dev` feature.
///
/// Core gates its dev-only logging and diagnostics on `cfg(feature = "dev")` (its own flag, not the
/// binary's). A root `dev = []` that forgets to forward `zeus-core/dev` is invisible to the type
/// checker: the binary keeps its `dev` feature while every one of core's
/// `#[cfg(feature = "dev")]` blocks goes dark. Exposing what core actually compiled to lets the
/// binary assert the two flags agree.
pub const DEV_FEATURE_ENABLED: bool = cfg!(feature = "dev");

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
