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

pub mod models;
pub mod waku;
pub mod fees;
pub mod transact;
pub mod sidecar;

pub use models::*;

// Re-exports for convenience (will grow)
pub use fees::BroadcasterFeeCache;
pub use transact::BroadcasterTransaction;

/// Chain identifier used across Railgun (type + id, e.g. Ethereum mainnet = {0, 1}).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Chain {
    pub type_: u8,
    pub id: u64,
}

impl Chain {
    pub const ETHEREUM_MAINNET: Self = Self { type_: 0, id: 1 };
    pub const POLYGON_MAINNET: Self = Self { type_: 0, id: 137 };
    // Add more as needed
}

/// High-level client mirroring the TS WakuBroadcasterClient.
pub struct WakuBroadcasterClient;

impl WakuBroadcasterClient {
    /// Start the client, connect to Waku, begin listening for fees on the given chain.
    pub async fn start(
        _chain: Chain,
        _options: BroadcasterOptions,
        _status_callback: impl Fn(BroadcasterConnectionStatus) + Send + Sync + 'static,
    ) -> anyhow::Result<()> {
        // TODO Phase 1: init waku core, set observers, poll historical, start status poller
        todo!("WakuBroadcasterClient::start - see roadmap")
    }

    /// Find the best (lowest fee) broadcaster currently cached for a token on this chain.
    pub fn find_best_broadcaster(
        _chain: Chain,
        _token_address: &str,
        _use_relay_adapt: bool,
    ) -> Option<SelectedBroadcaster> {
        // TODO: query BroadcasterFeeCache
        None
    }

    // TODO: stop, set_chain, get peer counts, etc.
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

#[derive(Debug, Clone)]
pub struct SelectedBroadcaster {
    pub railgun_address: String,
    pub fees_id: String,
    // fee rates per token, etc.
}

// TODO: move to proper modules as we implement.
pub mod placeholder {
    // Placeholder types that will be expanded from TS models
}
