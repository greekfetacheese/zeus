//! End-to-end tests that talk to live chains, from this machine's own `data/` profile.
//!
//! They are `#[ignore]`d on purpose, for the same reason as the live suites in `zeus-eth` (see
//! `crates/zeus-eth/src/test_utils.rs`): they read the real profile in `data/` — `unlock_ctx` below
//! unlocks the vault with the development credentials and asserts an enabled, working RPC per chain
//! — so whether they pass says more about the machine and the moment than about the code. Public
//! endpoints are unreliable: several block non-browser user agents, `eth_getLogs` is unusable on
//! most of them, and they rot.
//!
//! A plain `cargo test` is meant to be a self-contained signal, so these run on purpose instead:
//!
//! ```sh
//! cargo test -- --ignored
//! ```
//!
//! What they are still good for: exercising a real swap, bridge, ENS or NFT flow end to end against
//! a chain, and catching the wiring that a unit test cannot see.

mod bridge;
mod connector;
mod dev_feature;
mod embedded;
mod ens;
mod main_event;
mod nft;
mod permit2_revoke;
mod swap;

#[cfg(test)]
pub fn unlock_ctx() -> crate::core::ZeusCtx {
   use crate::core::Vault;
   use crate::core::WalletState;
   use crate::core::ZeusCtx;
   use ncrypt_me::{Credentials, secure_types::SecureString};

   let ctx = ZeusCtx::new(crate::runtime::test_handle());

   let credentials = Credentials::new(
      SecureString::from("dev"),
      SecureString::from("dev"),
      SecureString::from("dev"),
   );

   let mut vault = Vault::default();
   vault.set_credentials(credentials);

   let data = vault.decrypt(None).unwrap();
   vault.load(data).unwrap();

   let key = vault.wallet_state_key().unwrap();

   let (state, _) = WalletState::load_or_migrate(&key, None).unwrap();

   ctx.set_vault(vault);
   ctx.set_wallet_state(state);
   ctx.load_tx_db();
   ctx.build_wallet_info_cache();
   ctx.load_currency_db();
   ctx.load_nft_db();
   ctx.load_pool_manager();
   ctx.load_client_manager();
   ctx.load_price_manager();

   ctx
}

/// A test context with a **usable** endpoint for `chain`.
///
/// `unlock_ctx` loads this machine's `data/` — including its RPC list — and the builtin endpoints it
/// seeds are left disabled and unmeasured, while `Rpc::get_best_rpc` needs one that is enabled *and*
/// working. Mark one usable, preferring a builtin so a test never depends on an endpoint that was
/// added by hand. Shared by the swap and bridge regressions.
#[cfg(test)]
pub fn test_ctx(chain: u64) -> crate::core::ZeusCtx {
   let ctx = unlock_ctx();
   let client = ctx.get_client_manager();
   let rpcs = client.get_rpcs(chain);

   assert!(
      !rpcs.is_empty(),
      "no endpoint for chain {chain}: open the app on that chain once, or add one in settings"
   );

   if !rpcs.values().any(|r| r.enabled && r.check.working) {
      let mut rpc = rpcs
         .values()
         .find(|r| r.default)
         .or_else(|| rpcs.values().next())
         .cloned()
         .unwrap();
      rpc.enabled = true;
      rpc.check.working = true;
      client.add_rpc(chain, rpc);
   }

   ctx
}
