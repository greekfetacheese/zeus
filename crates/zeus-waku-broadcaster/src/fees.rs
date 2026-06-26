//! Fee quote handling, caching, message parsing, signature verification.
//! Direct port of fees/ + handle-fees-message etc from the TS client.

pub struct BroadcasterFeeCache;

impl BroadcasterFeeCache {
    pub fn init(_poi_keys: Vec<String>) {
        // TODO
    }

    pub fn reset_cache(_chain: crate::Chain) {
        // TODO
    }

    // update_fees_for_broadcaster, find best, etc.
}
