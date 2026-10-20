use super::descriptor::{self, Descriptor};
use super::registry_pins::{
   RegistryPinError, is_allowed_registry_path, is_registry_file_pinned, verify_registry_pin,
};
use crate::embedded::clear_signing as embedded_registry;
use serde_json::Value;
use zeus_eth::alloy_primitives::{Address, B256};

const EIP712_INDEX_PATH: &str = "index.eip712.json";
const CALLDATA_INDEX_PATH: &str = "index.calldata.json";
const MAX_INCLUDE_DEPTH: usize = 3;

fn ensure_pinned_path(registry_path: &str) -> Result<(), anyhow::Error> {
   if !is_allowed_registry_path(registry_path) {
      anyhow::bail!("refusing registry path {registry_path}");
   }
   if !is_registry_file_pinned(registry_path) {
      return Err(
         RegistryPinError::Unpinned {
            path: registry_path.to_string(),
         }
         .into(),
      );
   }
   Ok(())
}

fn read_embedded_json(registry_path: &str) -> Result<Option<Value>, anyhow::Error> {
   let Some(bytes) = embedded_registry::get(registry_path) else {
      return Ok(None);
   };
   verify_registry_pin(registry_path, bytes)?;
   Ok(Some(serde_json::from_slice(bytes)?))
}

async fn get_json(registry_path: &str) -> Result<Value, anyhow::Error> {
   ensure_pinned_path(registry_path)?;

   if let Some(v) = read_embedded_json(registry_path)? {
      return Ok(v);
   }

   Err(anyhow::anyhow!(
      "{registry_path} is not in the embedded registry"
   ))
}

/// Warm the ERC-7730 indexes so the first unknown typed-data sign does not wait on them.
pub async fn prefetch_index() {
   match fetch_named_index(EIP712_INDEX_PATH).await {
      Ok(_) => tracing::info!("ERC-7730 EIP-712 index ready"),
      Err(e) => tracing::warn!("ERC-7730 EIP-712 index prefetch failed: {e}"),
   }
   match fetch_named_index(CALLDATA_INDEX_PATH).await {
      Ok(_) => tracing::info!("ERC-7730 calldata index ready"),
      Err(e) => tracing::warn!("ERC-7730 calldata index prefetch failed: {e}"),
   }
}

async fn fetch_index() -> Result<Value, anyhow::Error> {
   fetch_named_index(EIP712_INDEX_PATH).await
}

async fn fetch_calldata_index() -> Result<Value, anyhow::Error> {
   fetch_named_index(CALLDATA_INDEX_PATH).await
}

async fn fetch_named_index(name: &str) -> Result<Value, anyhow::Error> {
   ensure_pinned_path(name)?;

   if let Some(v) = read_embedded_json(name)? {
      return Ok(v);
   }

   Err(anyhow::anyhow!(
      "{name} is not in the embedded registry"
   ))
}

pub fn lookup_index_path(
   index: &Value,
   chain: u64,
   verifying: Address,
   type_hash: B256,
) -> Option<String> {
   let keys = caip10_keys(chain, verifying);

   let want = format!("{type_hash:#x}");
   for key in keys {
      let Some(entry) = index.get(&key) else {
         continue;
      };
      let Some(map) = entry.as_object() else {
         continue;
      };
      for (_primary, arr) in map {
         let Some(arr) = arr.as_array() else {
            continue;
         };
         for item in arr {
            let hashes = item
               .get("encodeTypeHashes")
               .and_then(|v| v.as_array())
               .cloned()
               .unwrap_or_default();
            let hit = hashes
               .iter()
               .any(|h| h.as_str().map(|s| s.eq_ignore_ascii_case(&want)).unwrap_or(false));
            if hit {
               if let Some(path) = item.get("path").and_then(|v| v.as_str()) {
                  return Some(path.to_string());
               }
            }
         }
      }
   }
   None
}

fn caip10_keys(chain: u64, address: Address) -> [String; 2] {
   [
      format!("eip155:{chain}:{}", format!("{address:#x}")),
      format!("eip155:{chain}:{}", address.to_checksum(None)),
   ]
}

pub fn lookup_calldata_index_path(index: &Value, chain: u64, to: Address) -> Option<String> {
   for key in caip10_keys(chain, to) {
      if let Some(path) = index.get(&key).and_then(|v| v.as_str()) {
         return Some(path.to_string());
      }
   }
   None
}

pub async fn resolve_calldata_descriptor(chain: u64, to: Address) -> Option<(String, Descriptor)> {
   let index = fetch_calldata_index().await.ok()?;
   let path = lookup_calldata_index_path(&index, chain, to)?;
   let merged = load_merged(&path).await.ok()?;
   let descriptor = descriptor::parse_descriptor(&merged).ok()?;
   Some((path, descriptor))
}

/// Best-effort human name from ERC-7730 metadata (calldata index, then EIP-712 index).
pub async fn resolve_contract_label(chain: u64, address: Address) -> Option<String> {
   // Hyperliquid (and similar) register EIP-712 verifyingContract as 0x0.
   // That must not become a display name for the zero address in tx events.
   if address.is_zero() {
      return None;
   }
   if let Some(path) = lookup_calldata_path(chain, address).await {
      if let Some(name) = label_from_descriptor_path(&path).await {
         return Some(name);
      }
   }
   if let Some(path) = lookup_any_eip712_path(chain, address).await {
      if let Some(name) = label_from_descriptor_path(&path).await {
         return Some(name);
      }
   }
   None
}

async fn lookup_calldata_path(chain: u64, address: Address) -> Option<String> {
   let index = fetch_calldata_index().await.ok()?;
   lookup_calldata_index_path(&index, chain, address)
}

async fn lookup_any_eip712_path(chain: u64, address: Address) -> Option<String> {
   let index = fetch_index().await.ok()?;
   lookup_any_eip712_index_path(&index, chain, address)
}

fn lookup_any_eip712_index_path(index: &Value, chain: u64, address: Address) -> Option<String> {
   for key in caip10_keys(chain, address) {
      let Some(entry) = index.get(&key) else {
         continue;
      };
      let Some(map) = entry.as_object() else {
         continue;
      };
      for arr in map.values() {
         let Some(arr) = arr.as_array() else {
            continue;
         };
         for item in arr {
            if let Some(path) = item.get("path").and_then(|v| v.as_str()) {
               return Some(path.to_string());
            }
         }
      }
   }
   None
}

async fn label_from_descriptor_path(path: &str) -> Option<String> {
   let merged = load_merged(path).await.ok()?;
   let meta = merged.get("metadata")?;
   let contract_name = meta.get("contractName").and_then(|v| v.as_str()).filter(|s| !s.is_empty());
   let owner = meta.get("owner").and_then(|v| v.as_str()).filter(|s| !s.is_empty());
   match (contract_name, owner) {
      (Some(c), _) => Some(c.to_string()),
      (None, Some(o)) => Some(o.to_string()),
      _ => None,
   }
}

pub async fn resolve_eip712_descriptor(
   chain: u64,
   verifying: Address,
   type_hash: B256,
) -> Option<(String, Descriptor)> {
   let index = fetch_index().await.ok()?;
   let path = lookup_index_path(&index, chain, verifying, type_hash)?;
   let merged = load_merged(&path).await.ok()?;
   let descriptor = descriptor::parse_descriptor(&merged).ok()?;
   Some((path, descriptor))
}

async fn load_merged(registry_path: &str) -> Result<Value, anyhow::Error> {
   load_merged_depth(registry_path, 0).await
}

async fn load_merged_depth(registry_path: &str, depth: usize) -> Result<Value, anyhow::Error> {
   if depth > MAX_INCLUDE_DEPTH {
      return Err(anyhow::anyhow!("include depth exceeded"));
   }
   let doc = get_json(registry_path).await?;
   if let Some(inc) = doc.get("includes").and_then(|v| v.as_str()) {
      let inc_path = resolve_include(registry_path, inc);
      let included = Box::pin(load_merged_depth(&inc_path, depth + 1)).await?;
      return Ok(descriptor::merge_descriptor_json(included, doc));
   }
   Ok(doc)
}

fn resolve_include(parent: &str, inc: &str) -> String {
   if inc.starts_with("http://") || inc.starts_with("https://") {
      return inc.to_string();
   }
   let parent_dir = parent.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
   let mut parts: Vec<&str> = parent_dir.split('/').filter(|s| !s.is_empty()).collect();
   for seg in inc.split('/') {
      match seg {
         "." => {}
         ".." => {
            parts.pop();
         }
         other => parts.push(other),
      }
   }
   parts.join("/")
}

/// Test helper: parse a merged JSON value without hitting the network.
#[cfg(test)]
pub fn parse_merged_json(value: Value) -> Result<Descriptor, anyhow::Error> {
   descriptor::parse_descriptor(&value)
}

#[cfg(test)]
pub fn index_lookup_for_tests(
   index: &Value,
   chain: u64,
   verifying: Address,
   type_hash: B256,
) -> Option<String> {
   lookup_index_path(index, chain, verifying, type_hash)
}

#[cfg(test)]
pub fn resolve_include_for_tests(parent: &str, inc: &str) -> String {
   resolve_include(parent, inc)
}

#[cfg(test)]
pub fn calldata_index_lookup_for_tests(index: &Value, chain: u64, to: Address) -> Option<String> {
   lookup_calldata_index_path(index, chain, to)
}

#[cfg(test)]
mod tests {
   use super::*;

   /// Descriptors are served only from the embedded, pin-verified pack.
   ///
   /// The embedded index resolves with no opt-in and no network; a path without a pin is refused
   /// before it can be read from anywhere; and an allowed path the pack does not carry fails without
   /// naming a URL, because there is no remote registry left to reach.
   #[tokio::test]
   async fn descriptors_are_served_only_from_the_embedded_pinned_pack() {
      assert!(
         get_json(EIP712_INDEX_PATH).await.is_ok(),
         "the embedded index must be served without the opt-in"
      );

      let err = get_json("registry/not-a-pinned-file.json")
         .await
         .expect_err("a path without a pin must be refused");
      assert!(
         matches!(
            err.downcast_ref::<RegistryPinError>(),
            Some(RegistryPinError::Unpinned { .. })
         ),
         "{err}"
      );

      let err = get_json("ercs/absent-from-the-embedded-pack.json")
         .await
         .expect_err("an allowed path the pack does not carry must error");
      let message = err.to_string();
      assert!(!message.contains("http"), "{message}");
      assert!(!message.contains("://"), "{message}");
   }
}
