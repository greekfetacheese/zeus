//! zeus-waku-broadcaster
//!
//! Rust implementation of Railgun's Waku-based broadcaster client.
//! See RAILGUN_ROADMAP.md (Phase 1) for the full plan.
//!
//! This module will provide:
//! - Waku connection & pub/sub for Railgun-specific topics
//! - Fee quote discovery, validation, and caching
//! - Encrypted transact request/response handling
//! - Broadcaster selection

pub mod fees;
pub mod client;
pub mod transact;
pub mod models;

pub use models::*;


pub use fees::{
   BroadcasterFeeCache, CachedTokenFee, SelectedBroadcaster, find_best_broadcaster,
   find_broadcasters_for_token,
};
pub use transact::BroadcasterTransaction;

/// Chain identifier used across Railgun (type + id, e.g. Ethereum mainnet = {0, 1}).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Chain {
   #[serde(rename = "type")]
   pub type_: u8,
   pub id: u64,
}

impl Chain {
   pub const ETHEREUM_MAINNET: Self = Self { type_: 0, id: 1 };
   pub const POLYGON_MAINNET: Self = Self { type_: 0, id: 137 };
   // Add more as needed
}

/// Common token addresses for fee quotes (lowercase).
pub mod tokens {
   pub const USDC_ETHEREUM: &str = "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48";
   pub const WETH_ETHEREUM: &str = "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2";
   pub const USDT_ETHEREUM: &str = "0xdac17f958d2ee523a2206206994597c13d831ec7";
   pub const DAI_ETHEREUM: &str = "0x6b175474e8000fa2a0b9c0d0e0a0b9c0d0e0a0b9c0"; // placeholder, use real if needed
}


/// Options passed at start (port of BroadcasterOptions + BroadcasterConfig).
#[derive(Debug, Clone, Default)]
pub struct BroadcasterOptions {
   pub trusted_fee_signer: String, // or Vec for multiple
   pub pub_sub_topic: Option<String>,
   pub fee_expiration_timeout: Option<u64>,
   pub peer_discovery_timeout: Option<u64>,
   pub additional_direct_peers: Vec<String>,
   pub use_dns_discovery: Option<bool>,
   pub broadcaster_version_range: Option<BroadcasterVersionRange>,
   // ... more (poi lists, cluster/shard, etc.)
}

#[derive(Debug, Clone)]
pub struct BroadcasterVersionRange {
   pub min_version: String,
   pub max_version: String,
}

#[derive(Debug, Clone, Copy)]
pub enum BroadcasterConnectionStatus {
   Searching,
   Connected,
   Error,
   // ...
}

// SelectedBroadcaster and fee selection now live in fees::best_broadcaster (re-exported above)

// TODO: move more types to proper modules as we implement.
pub mod placeholder {
   // Placeholder types that will be expanded from TS models
}
