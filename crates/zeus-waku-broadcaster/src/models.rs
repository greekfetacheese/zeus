//! Data models (port of models/ from TS broadcaster client + shared-models concepts).

pub mod fee_message;
pub use fee_message::{BroadcasterFeeMessageData, ProcessedFeeMessage, SignedBroadcasterFeeMessage};

#[derive(Debug, Clone, Default)]
pub struct BroadcasterConfig {
    // statics from TS: feeExpirationTimeout, version ranges, etc.
}

pub mod constants {
    pub const WAKU_RAILGUN_DEFAULT_PUBSUB: &str = "/waku/2/rs/5/1";
    // ... more from constants.ts
}
