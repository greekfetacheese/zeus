//! Railgun Broadcaster Fee Message models (port from @railgun-community/shared-models + handle-fees-message.ts)
//!
//! Wire format over Waku (content topic /railgun/v2/{type}-{id}-fees/json):
//!   The raw payload (base64 in sidecar) is JSON:
//!   {
//!     "data": "<hex-encoded JSON of BroadcasterFeeMessageData>",
//!     "signature": "<hex signature over the data hex>"
//!   }

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// The inner data structure sent by broadcasters.
/// This is what you get after:
///   base64(payload) -> utf8 -> {data, signature} -> hex(data) -> utf8 -> JSON
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BroadcasterFeeMessageData {
   /// Map of token address (lowercase) -> fee per unit gas (as hex string, e.g. "0x1234")
   pub fees: HashMap<String, String>,

   /// Unix timestamp (ms) when this fee quote expires
   #[serde(rename = "feeExpiration")]
   pub fee_expiration: u64,

   /// Unique ID for this fee quote set
   #[serde(rename = "feesID")]
   pub fees_id: String,

   /// The broadcaster's Railgun address (0zk...)
   #[serde(rename = "railgunAddress")]
   pub railgun_address: String,

   /// Optional identifier (used for naming the broadcaster)
   pub identifier: Option<String>,

   /// How many wallets this broadcaster claims to support simultaneously
   #[serde(rename = "availableWallets")]
   pub available_wallets: u32,

   /// Broadcaster software version (e.g. "8.1.0")
   pub version: String,

   /// RelayAdapt contract address (for private swaps)
   #[serde(rename = "relayAdapt")]
   pub relay_adapt: String,

   /// POI list keys this broadcaster requires
   #[serde(rename = "requiredPOIListKeys")]
   pub required_poi_list_keys: Vec<String>,

   /// Reliability score reported by broadcaster (0.0 - 1.0)
   pub reliability: Option<f64>,
}

/// The outer signed wrapper that actually travels over Waku.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedBroadcasterFeeMessage {
   /// hex-encoded JSON of BroadcasterFeeMessageData
   pub data: String,
   /// hex signature (produced with the broadcaster's viewing private key)
   pub signature: String,
}

impl SignedBroadcasterFeeMessage {
   /// Parse a raw Waku payload (already base64-decoded bytes from sidecar)
   /// into the signed wrapper.
   pub fn from_waku_payload(bytes: &[u8]) -> anyhow::Result<Self> {
      let json_str = std::str::from_utf8(bytes)?;
      let wrapper: SignedBroadcasterFeeMessage = serde_json::from_str(json_str)?;
      Ok(wrapper)
   }

   /// Decode the inner `data` hex field into the actual fee message data.
   pub fn parse_inner_data(&self) -> anyhow::Result<BroadcasterFeeMessageData> {
      let hex_data = self.data.trim_start_matches("0x");
      let bytes = hex::decode(hex_data)?;
      let json_str = std::str::from_utf8(&bytes)?;
      let fee_data: BroadcasterFeeMessageData = serde_json::from_str(json_str)?;
      Ok(fee_data)
   }
}

/// Result of processing one fee message (for the cache / selection logic).
#[derive(Debug, Clone)]
pub struct ProcessedFeeMessage {
   pub data: BroadcasterFeeMessageData,
   pub signature: String,
   pub received_at: u64, // unix ms from Waku or local time
   pub verified: bool,   // set after signature check
}
