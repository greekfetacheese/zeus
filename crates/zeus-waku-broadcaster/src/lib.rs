//! zeus-waku-broadcaster
//!
//! Rust implementation of Railgun's Waku-based broadcaster client.
//! See RAILGUN_ROADMAP.md (Phase 1) for the full plan.
//!
//! Architecture (Option A):
//! - JS sidecar is dumb Waku pipe only.
//! - Rust owns fee cache, selection, encryption, transact logic.
//!
//! Current status: Fee reception + selection complete. Moving to transact layer.

pub mod client;
pub mod encryption;
pub mod fees;
pub mod models;
pub mod transact;

pub use client::{WakuSidecarClient, PeerInfo};
pub use encryption::{
   aes_gcm_decrypt, aes_gcm_encrypt, encrypt_transact_payload, generate_response_key,
};
pub use fees::{
   BroadcasterFeeCache, CachedTokenFee, SelectedBroadcaster, find_best_broadcaster,
   find_broadcasters_for_token,
};
pub use models::*;
pub use transact::BroadcasterTransaction;

pub fn default_topic() -> String {
   "/railgun/v2/default/json".to_string()
}

pub fn fees_topic(chain: Chain) -> String {
   format!("/railgun/v2/${}-${}-fees/json", chain.type_, chain.id)
}

pub fn transact_topic(chain: Chain) -> String {
   format!("/railgun/v2/${}-${}-transact/json", chain.type_, chain.id)
}

pub fn trasnact_responce_topic(chain: Chain) -> String {
   format!("/railgun/v2/${}-${}-transact-response/json", chain.type_, chain.id)
}

pub fn metrics_topic() -> String {
   format!("/railgun/v2/metrics/json")
}

pub fn encrypted_topic(topic: &str) -> String {
   format!("/railgun/v2/encrypted-${topic}/json")
}

/// Chain identifier used across Railgun (type + id).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Chain {
   #[serde(rename = "type")]
   pub type_: u8,
   pub id: u64,
}

impl Chain {
   pub const ETHEREUM_MAINNET: Self = Self { type_: 0, id: 1 };
   pub const POLYGON_MAINNET: Self = Self { type_: 0, id: 137 };
}

/// Common token addresses (for convenience in examples/tests).
pub mod tokens {
   pub const USDC_ETHEREUM: &str = "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48";
   pub const WETH_ETHEREUM: &str = "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2";
   pub const USDT_ETHEREUM: &str = "0xdac17f958d2ee523a2206206994597c13d831ec7";
}

/// Version range for broadcaster compatibility.
#[derive(Debug, Clone)]
pub struct BroadcasterVersionRange {
   pub min_version: String,
   pub max_version: String,
}
