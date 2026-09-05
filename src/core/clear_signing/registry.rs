use super::descriptor::{self, Descriptor};
use super::registry_pins::{
   MAX_REGISTRY_JSON_BYTES, RegistryPinError, is_allowed_registry_path, is_registry_file_pinned,
   verify_registry_pin,
};
use crate::core::persisted::{
   CLEAR_SIGNING_INDEX_CALLDATA, CLEAR_SIGNING_INDEX_EIP712, PersistedTree, tree_dir,
};
use crate::embedded::clear_signing as embedded_registry;
use crate::utils::write_private;
use anyhow::Context;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;
use zeus_eth::alloy_primitives::{Address, B256, keccak256};

const REGISTRY_BASE: &str =
   "https://raw.githubusercontent.com/ethereum/clear-signing-erc7730-registry/master";
const EIP712_INDEX_PATH: &str = CLEAR_SIGNING_INDEX_EIP712;
const CALLDATA_INDEX_PATH: &str = CLEAR_SIGNING_INDEX_CALLDATA;
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);
const INDEX_CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const MAX_INCLUDE_DEPTH: usize = 3;

fn http_client() -> &'static reqwest::Client {
   static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
   CLIENT.get_or_init(|| {
      reqwest::Client::builder()
         .user_agent("zeus-wallet")
         .timeout(FETCH_TIMEOUT)
         .build()
         .unwrap_or_else(|_| reqwest::Client::new())
   })
}

fn cache_dir() -> Result<PathBuf, anyhow::Error> {
   tree_dir(PersistedTree::ClearSigning)
}

fn cached_index_path(name: &str) -> Result<PathBuf, anyhow::Error> {
   Ok(cache_dir()?.join(name))
}

fn cached_index_is_fresh(path: &Path) -> bool {
   let Ok(meta) = std::fs::metadata(path) else {
      return false;
   };
   let Ok(modified) = meta.modified() else {
      return false;
   };
   modified.elapsed().map(|age| age < INDEX_CACHE_TTL).unwrap_or(false)
}

fn cached_file_path(registry_path: &str) -> Result<PathBuf, anyhow::Error> {
   let hash = keccak256(registry_path.as_bytes());
   Ok(cache_dir()?.join(format!("{hash:x}.json")))
}

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

fn read_pinned_json(path: &Path, registry_path: &str) -> Option<Value> {
   let bytes = std::fs::read(path).ok()?;
   match verify_registry_pin(registry_path, &bytes) {
      Ok(()) => serde_json::from_slice(&bytes).ok(),
      Err(RegistryPinError::Mismatch { .. } | RegistryPinError::TooLarge { .. }) => {
         let _ = std::fs::remove_file(path);
         None
      }
      Err(_) => None,
   }
}

async fn fetch_registry_bytes(registry_path: &str) -> Result<Vec<u8>, anyhow::Error> {
   let url = format!("{REGISTRY_BASE}/{registry_path}");
   let bytes = http_client()
      .get(&url)
      .send()
      .await
      .context("registry fetch")?
      .error_for_status()
      .context("registry status")?
      .bytes()
      .await?;
   if bytes.len() > MAX_REGISTRY_JSON_BYTES {
      return Err(
         RegistryPinError::TooLarge {
            path: registry_path.to_string(),
            size: bytes.len(),
         }
         .into(),
      );
   }
   Ok(bytes.to_vec())
}

fn cache_registry_file(path: &Path, bytes: &[u8], label: &str) {
   if let Err(e) = write_private(path, bytes) {
      tracing::warn!("Failed to cache ERC-7730 {label}: {e}");
   }
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

   if let Ok(path) = cached_file_path(registry_path) {
      if let Some(v) = read_pinned_json(&path, registry_path) {
         return Ok(v);
      }
   }

   let bytes = fetch_registry_bytes(registry_path).await?;
   verify_registry_pin(registry_path, &bytes)?;
   let value: Value = serde_json::from_slice(&bytes)?;

   if let Ok(path) = cached_file_path(registry_path) {
      cache_registry_file(&path, &bytes, registry_path);
   }

   Ok(value)
}

pub async fn prefetch_index() {
   match fetch_named_index(EIP712_INDEX_PATH, true).await {
      Ok(_) => tracing::info!("ERC-7730 EIP-712 index ready"),
      Err(e) => tracing::warn!("ERC-7730 EIP-712 index prefetch failed: {e}"),
   }
   match fetch_named_index(CALLDATA_INDEX_PATH, true).await {
      Ok(_) => tracing::info!("ERC-7730 calldata index ready"),
      Err(e) => tracing::warn!("ERC-7730 calldata index prefetch failed: {e}"),
   }
}

async fn fetch_index(force_network: bool) -> Result<Value, anyhow::Error> {
   fetch_named_index(EIP712_INDEX_PATH, force_network).await
}

async fn fetch_calldata_index(force_network: bool) -> Result<Value, anyhow::Error> {
   fetch_named_index(CALLDATA_INDEX_PATH, force_network).await
}

async fn fetch_named_index(name: &str, force_network: bool) -> Result<Value, anyhow::Error> {
   ensure_pinned_path(name)?;

   if let Some(v) = read_embedded_json(name)? {
      return Ok(v);
   }

   if let Ok(path) = cached_index_path(name) {
      let fresh = cached_index_is_fresh(&path);
      if fresh || !force_network {
         if let Some(v) = read_pinned_json(&path, name) {
            return Ok(v);
         }
      }
   }

   let bytes = match fetch_registry_bytes(name).await {
      Ok(bytes) => bytes,
      Err(e) => {
         if let Ok(path) = cached_index_path(name) {
            if let Some(v) = read_pinned_json(&path, name) {
               tracing::warn!("ERC-7730 index {name} fetch failed ({e}); using pinned cache");
               return Ok(v);
            }
         }
         return Err(e);
      }
   };

   if let Err(e) = verify_registry_pin(name, &bytes) {
      if let Ok(path) = cached_index_path(name) {
         if let Some(v) = read_pinned_json(&path, name) {
            tracing::warn!("ERC-7730 index {name} pin mismatch ({e}); using pinned cache");
            return Ok(v);
         }
      }
      return Err(e.into());
   }
   
   let value: Value = serde_json::from_slice(&bytes)?;
   if let Ok(path) = cached_index_path(name) {
      cache_registry_file(&path, &bytes, name);
   }
   Ok(value)
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
   let index = fetch_calldata_index(false).await.ok()?;
   let path = lookup_calldata_index_path(&index, chain, to)?;
   let merged = load_merged(&path).await.ok()?;
   let descriptor = descriptor::parse_descriptor(&merged).ok()?;
   Some((path, descriptor))
}

/// Best-effort human name from ERC-7730 metadata (calldata index, then EIP-712 index).
pub async fn resolve_contract_label(chain: u64, address: Address) -> Option<String> {
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
   let index = fetch_calldata_index(false).await.ok()?;
   lookup_calldata_index_path(&index, chain, address)
}

async fn lookup_any_eip712_path(chain: u64, address: Address) -> Option<String> {
   let index = fetch_index(false).await.ok()?;
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
   let index = fetch_index(false).await.ok()?;
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
