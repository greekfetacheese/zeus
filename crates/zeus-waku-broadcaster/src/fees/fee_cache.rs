//! BroadcasterFeeCache - Rust port of the TS BroadcasterFeeCache
//!
//! Stores and queries fee quotes received from Railgun broadcasters over Waku.
//! We primarily rely on historical Store queries for now (live will be added later).
//!
//! Key concepts from TS:
//! - Per-chain, per-token, per-broadcaster (railgun 0zk address) cache
//! - Expiration checks
//! - Version filtering
//! - findBestBroadcaster logic lives in a separate search module (see best_broadcaster.rs)

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::models::fee_message::BroadcasterFeeMessageData;

/// A single cached fee for a token from a broadcaster.
#[derive(Debug, Clone)]
pub struct CachedTokenFee {
   pub fee_per_unit_gas: String, // hex string, e.g. "0x1234"
   pub expiration: u64,          // unix ms
   pub railgun_address: String,
   pub identifier: Option<String>,
   pub fees_id: String, // the feesID for transact
   pub version: String,
   pub received_at: u64, // when we stored it (ms)
}

/// Internal cache structure
/// forNetwork -> forToken (lowercase) -> forBroadcaster (0zk addr) -> forIdentifier
type Cache = HashMap<String, HashMap<String, HashMap<String, HashMap<String, CachedTokenFee>>>>;

#[derive(Clone)]
pub struct BroadcasterFeeCache {
   cache: Cache,
   last_fee_received_at: Option<u64>,
}

impl BroadcasterFeeCache {
   pub fn new() -> Self {
      Self {
         cache: HashMap::new(),
         last_fee_received_at: None,
      }
   }

   /// Add fees from a received BroadcasterFeeMessageData.
   /// Mirrors TS addTokenFees().
   pub fn add_token_fees(&mut self, chain: &crate::Chain, data: &BroadcasterFeeMessageData) {
      let network_name = network_name_for_chain(chain);

      // Basic expiration check
      let now = current_ms();
      if data.fee_expiration < now {
         tracing::debug!(
            "Fee expired for broadcaster {}",
            data.railgun_address
         );
         return;
      }

      // Basic version check (very loose for now)
      if data.version.is_empty() {
         return;
      }

      let broadcaster = data.railgun_address.to_lowercase();
      let identifier = data.identifier.clone().unwrap_or_else(|| "default".to_string());

      // Structure for Phase 1B:
      // network -> token (lowercase) -> broadcaster (0zk) -> identifier -> CachedTokenFee
      let net_entry = self.cache.entry(network_name).or_insert_with(HashMap::new);

      for (token, fee_hex) in &data.fees {
         let token_lc = token.to_lowercase();

         let token_entry = net_entry.entry(token_lc.clone()).or_insert_with(HashMap::new);
         let broad_entry = token_entry.entry(broadcaster.clone()).or_insert_with(HashMap::new);

         let cached = CachedTokenFee {
            fee_per_unit_gas: fee_hex.clone(),
            expiration: data.fee_expiration,
            railgun_address: data.railgun_address.clone(),
            identifier: data.identifier.clone(),
            fees_id: data.fees_id.clone(),
            version: data.version.clone(),
            received_at: now,
         };

         broad_entry.insert(identifier.clone(), cached);
      }

      self.last_fee_received_at = Some(now);
   }

   pub fn fees_for_chain(
      &self,
      chain: &crate::Chain,
   ) -> Option<&HashMap<String, HashMap<String, HashMap<String, CachedTokenFee>>>> {
      let name = network_name_for_chain(chain);
      self.cache.get(&name)
   }

   pub fn fees_for_token(
      &self,
      chain: &crate::Chain,
      token: &str,
   ) -> Option<&HashMap<String, HashMap<String, CachedTokenFee>>> {
      self.fees_for_chain(chain)?.get(&token.to_lowercase())
   }

   pub fn last_received_at(&self) -> Option<u64> {
      self.last_fee_received_at
   }

   pub fn clear_for_chain(&mut self, chain: &crate::Chain) {
      let name = network_name_for_chain(chain);
      self.cache.remove(&name);
   }

   pub fn clear_expired_fees(&mut self, chain: &crate::Chain) -> usize {
      let now = current_ms();
      let name = network_name_for_chain(chain);
      let mut fees_removed = 0;

      if let Some(cached_fees) = self.cache.get_mut(&name) {
         for (_token, token_fees) in cached_fees {
            for (_broadcaster, broadcaster_fees) in token_fees {
               let expired_ids: Vec<String> = broadcaster_fees
                  .iter()
                  .filter(|(_, fee)| fee.expiration <= now)
                  .map(|(fee_id, _)| fee_id.clone())
                  .collect();

               fees_removed += expired_ids.len();
               for fee_id in &expired_ids {
                  broadcaster_fees.remove(fee_id);
               }
            }
         }
      }

      fees_removed
   }
}

impl Default for BroadcasterFeeCache {
   fn default() -> Self {
      Self::new()
   }
}

fn network_name_for_chain(chain: &crate::Chain) -> String {
   // Simple for now - matches how TS uses network.name
   match (chain.type_, chain.id) {
      (0, 1) => "Ethereum".to_string(),
      (0, 137) => "Polygon".to_string(),
      (0, 42161) => "Arbitrum".to_string(),
      _ => format!("chain-{}-{}", chain.type_, chain.id),
   }
}

fn current_ms() -> u64 {
   SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64
}

/// Helper: check if a fee is still usable
pub fn fee_is_usable(fee: &CachedTokenFee) -> bool {
   let now = current_ms();
   fee.expiration > now
}
