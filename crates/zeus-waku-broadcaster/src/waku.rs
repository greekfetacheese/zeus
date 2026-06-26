//! Waku networking layer.
//! Will contain: core init, observers, topics, peer discovery, healthcheck.
//! Uses waku-bindings (or equivalent).

pub mod topics {
    use crate::Chain;

    pub fn fees_topic(chain: Chain) -> String {
        format!("/railgun/v2/{}-{}-fees/json", chain.type_, chain.id)
    }

    pub fn transact_topic(chain: Chain) -> String {
        format!("/railgun/v2/{}-{}-transact/json", chain.type_, chain.id)
    }

    pub fn transact_response_topic(chain: Chain) -> String {
        format!("/railgun/v2/{}-{}-transact-response/json", chain.type_, chain.id)
    }
}

// TODO: WakuCore, Observers, peer discovery, event handling
// Example future:
// use waku_bindings::...;
// pub async fn init_waku(...) { ... }
