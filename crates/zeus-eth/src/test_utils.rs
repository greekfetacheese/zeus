//! Helpers for tests that talk to a live chain.
//!
//! Those tests are `#[ignore]`d on purpose. Public endpoints are unreliable: several block
//! non-browser user agents, `eth_getLogs` is unusable on most of them, and they rot — the
//! endpoint these tests used to hardcode is dead, which is why they were failing.
//!
//! Point them at a keyed endpoint with `ZEUS_ETH_RPC`:
//!
//! ```text
//! ZEUS_ETH_RPC=<keyed url> cargo test -p zeus-eth --lib -- --ignored
//! ```

use url::Url;

/// Environment variable holding an RPC endpoint that serves `eth_call`.
pub const RPC_ENV: &str = "ZEUS_ETH_RPC";

/// Convenience fallback. Expect it to be rate-limited, blocked, or gone; do not depend on it.
const PUBLIC_FALLBACK: &str = "https://ethereum-rpc.publicnode.com";

/// The endpoint to run live tests against: `ZEUS_ETH_RPC` when set, else [`PUBLIC_FALLBACK`].
pub fn rpc_url() -> Url {
   let raw = std::env::var(RPC_ENV).unwrap_or_else(|_| PUBLIC_FALLBACK.to_string());
   Url::parse(&raw).expect("RPC url must parse")
}
