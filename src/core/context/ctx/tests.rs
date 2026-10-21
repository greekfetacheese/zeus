use super::wallet::should_lookup_address_name;
use super::*;
use crate::utils::TimeStamp;
use std::str::FromStr;
use std::time::Duration;
use zeus_eth::{
   alloy_primitives::{U256, utils::format_units},
   alloy_provider::Provider,
   alloy_rpc_types::BlockId,
   types::SUPPORTED_CHAINS,
   utils::ens::ENS_CHAIN,
};

#[tokio::test]
#[should_panic]
async fn test_must_panic_if_no_mev_protect_client() {
   let ctx = ZeusCtx::new(crate::core::platform::runtime::test_handle());
   let _r = ctx.get_mev_protect_client(1).await.unwrap();
}

/// the "auto-lock is not configured" nudge comes once ~30 s after
/// unlock, then at most hourly, and never once the user made a choice.
#[test]
fn autolock_notice_cadence() {
   const FIRST: u64 = 30_000;
   const REPEAT: u64 = 3_600_000;

   let ctx = ZeusCtx::new(crate::core::platform::runtime::test_handle());
   ctx.write(|ctx| {
      ctx.security.autolock_changed = false;
      ctx.locked = false;
      ctx.last_autolock_notice_ms = 1_000;
      ctx.autolock_notice_shown = false;
   });

   let warn = |now: u64| ctx.write(|ctx| ctx.should_warn_autolock(now, FIRST, REPEAT));

   assert!(
      !warn(1_000 + FIRST - 1),
      "too early for the first notice"
   );
   assert!(warn(1_000 + FIRST), "first notice is due");
   assert!(
      !warn(1_000 + FIRST + REPEAT - 1),
      "within the repeat interval"
   );
   assert!(warn(1_000 + FIRST + REPEAT), "repeat is due");

   ctx.write(|ctx| ctx.security.autolock_changed = true);
   assert!(
      !warn(1_000 + 100 * REPEAT),
      "an explicit choice stops it"
   );

   ctx.write(|ctx| {
      ctx.security.autolock_changed = false;
      ctx.locked = true;
   });
   assert!(!warn(1_000 + 100 * REPEAT), "locked never warns");
}

/// The unlock guard refuses while locked and passes while unlocked.
#[test]
fn ensure_unlocked_refuses_while_locked() {
   let ctx = ZeusCtx::new(crate::core::platform::runtime::test_handle());

   ctx.write(|ctx| ctx.locked = false);
   assert!(ctx.ensure_unlocked().is_ok());

   ctx.write(|ctx| ctx.locked = true);
   let err = ctx.ensure_unlocked().unwrap_err();
   assert!(
      err.to_string().contains("locked"),
      "unexpected error: {err}"
   );
}

/// A collection names itself, and the confirmation window's "Contract interaction" row reads this to
/// label the contract being called. The name is cached by discovery, so it has to come out with no
/// registry or Sourcify round trip — and a curated label still outranks it.
#[test]
fn get_address_name_resolves_a_cached_nft_collection() {
   use zeus_eth::nft::{NftCollection, NftStandard};

   let ctx = ZeusCtx::new(crate::core::platform::runtime::test_handle());
   let collection = Address::from([0x3e; 20]);

   assert_eq!(ctx.get_address_name(1, collection), None);

   ctx.write(|ctx| {
      ctx.nft_db.insert_collection(
         1,
         NftCollection {
            chain_id: 1,
            address: collection,
            standard: NftStandard::Erc721,
            name: Some("Zeus Test Collection".to_string()),
            symbol: Some("ZTC".to_string()),
         },
      );
   });

   assert_eq!(
      ctx.get_address_name(1, collection).as_deref(),
      Some("Zeus Test Collection")
   );

   // The same address on another chain, where nothing is cached.
   assert_eq!(ctx.get_address_name(10, collection), None);

   // A curated address-book label outranks a contract's self-reported name.
   ctx.address_book().insert_contract(1, collection, "Curated");
   assert_eq!(
      ctx.get_address_name(1, collection).as_deref(),
      Some("Curated")
   );

   // A collection whose `name()` returned nothing names nothing.
   let unnamed = Address::from([0x4f; 20]);
   ctx.write(|ctx| {
      ctx.nft_db.insert_collection(
         1,
         NftCollection {
            chain_id: 1,
            address: unnamed,
            standard: NftStandard::Erc721,
            name: None,
            symbol: None,
         },
      );
   });

   assert_eq!(ctx.get_address_name(1, unnamed), None);
}

/// The bug this pins: the accessor claims an unnamed address and reports that a request is due,
/// and the spawned worker then has to agree to run. When the worker re-consumed that claim with
/// its own `mark_pending`, every request died on arrival — the reverse lookup never ran at all.
#[test]
fn ens_reverse_request_survives_the_shared_claim() {
   let ctx = ZeusCtx::new(crate::core::platform::runtime::test_handle());
   let (chain, address) = (ENS_CHAIN, Address::repeat_byte(0x60));

   // The frame path asks: the address is unnamed and unclaimed, so a request is due — and this
   // consult has just claimed the address for the session.
   assert!(
      !ctx.read(|ctx| ctx.address_name_requested(chain, address)),
      "a request is due for an unnamed address"
   );

   // The worker's guard, asked right after the request was spawned.
   assert!(
      should_lookup_address_name(
         &ctx.address_book(),
         &ctx.ens_cache(),
         chain,
         address
      ),
      "the worker must proceed for an address whose request was just spawned"
   );
}

/// The accessor is one-consumption: a second consult — another row in the same frame, or the
/// next frame — must not report a second request for the same address.
#[test]
fn ens_reverse_claim_is_consumed_once() {
   let ctx = ZeusCtx::new(crate::core::platform::runtime::test_handle());
   let (chain, address) = (ENS_CHAIN, Address::repeat_byte(0x61));

   let requested = |ctx: &ZeusCtx| ctx.read(|ctx| ctx.address_name_requested(chain, address));

   assert!(
      !requested(&ctx),
      "the first consult is due a request"
   );
   assert!(requested(&ctx), "the second consult is not");
   assert!(requested(&ctx), "nor any later one");
}

/// A resolved, unexpired name is never requested — not on a second consult, not on any later
/// frame of the session.
#[test]
fn ens_reverse_live_name_is_never_requested() {
   let ctx = ZeusCtx::new(crate::core::platform::runtime::test_handle());
   let (chain, address) = (ENS_CHAIN, Address::repeat_byte(0x62));

   // A label with no onchain expiry, as the forward picker stores one.
   assert!(ctx.ens_cache().insert(chain, address, "alice.eth", 0));

   let requested = |ctx: &ZeusCtx| ctx.read(|ctx| ctx.address_name_requested(chain, address));

   assert!(requested(&ctx));
   assert!(requested(&ctx));

   // And the worker's guard agrees there is nothing to look up.
   assert!(!should_lookup_address_name(
      &ctx.address_book(),
      &ctx.ens_cache(),
      chain,
      address
   ));
}

/// A lapsed binding makes the address requestable exactly once more — the lapse is the token,
/// not a licence to re-resolve on every frame.
#[test]
fn ens_reverse_lapsed_binding_is_requested_once_more() {
   let ctx = ZeusCtx::new(crate::core::platform::runtime::test_handle());
   let (chain, address) = (ENS_CHAIN, Address::repeat_byte(0x63));

   // Live for another second, then lapsed, with the state built directly — no chain is touched.
   let takeover_at = TimeStamp::now_as_secs().unwrap().timestamp() + 1;
   assert!(ctx.ens_cache().insert(chain, address, "lapsing.eth", takeover_at));

   // While the binding is live the address is named, so no request is due.
   assert!(ctx.read(|ctx| ctx.address_name_requested(chain, address)));

   std::thread::sleep(Duration::from_millis(1_500));

   assert_eq!(ctx.get_address_name(chain, address), None);

   let requested = |ctx: &ZeusCtx| ctx.read(|ctx| ctx.address_name_requested(chain, address));

   // The lapse is consumed: one re-resolution becomes due, and the worker's guard lets it
   // through ...
   assert!(
      !requested(&ctx),
      "a lapsed binding must be resolved once more"
   );
   assert!(should_lookup_address_name(
      &ctx.address_book(),
      &ctx.ens_cache(),
      chain,
      address
   ));
   // ... and the re-resolution is then claimed like any other.
   assert!(requested(&ctx), "but only once");
}

/// An address that resolves to nothing is looked up once, not once per frame.
#[test]
fn ens_reverse_unnamed_address_is_not_re_requested() {
   let ctx = ZeusCtx::new(crate::core::platform::runtime::test_handle());
   let (chain, address) = (ENS_CHAIN, Address::repeat_byte(0x64));

   let requested = |ctx: &ZeusCtx| ctx.read(|ctx| ctx.address_name_requested(chain, address));

   assert!(!requested(&ctx));

   // The worker ran and found no name to store.
   assert!(ctx.get_address_name(chain, address).is_none());

   assert!(
      requested(&ctx),
      "an unnamed address is not asked for again"
   );
}

#[tokio::test]
#[ignore = "live chain: needs a working public RPC on four chains — a manual fee read"]
async fn test_base_fee() {
   let ctx = ZeusCtx::new(crate::core::platform::runtime::test_handle());

   let client = ctx.get_client(1).await.unwrap();
   let block = client.get_block(BlockId::latest()).await.unwrap().unwrap();
   let base_fee = block.header.base_fee_per_gas.unwrap();
   let fee = format_units(base_fee, "gwei").unwrap();
   println!("Ethereum base fee: {}", fee);

   let client = ctx.get_client(10).await.unwrap();
   let gas_price = client.get_gas_price().await.unwrap();
   let fee = format_units(gas_price, "gwei").unwrap();
   println!("Optimism base fee: {}", fee);

   let client = ctx.get_client(56).await.unwrap();
   let gas_price = client.get_gas_price().await.unwrap();
   let fee = format_units(gas_price, "gwei").unwrap();
   println!("BSC base fee: {}", fee);

   let client = ctx.get_client(42161).await.unwrap();
   let gas_price = client.get_gas_price().await.unwrap();
   let fee = format_units(gas_price, "gwei").unwrap();
   println!("Arbitrum base fee: {}", fee);
}

#[tokio::test]
#[ignore = "live chain: needs a working public RPC, and pins one mainnet account's delegation"]
async fn test_extract_delegated_address() {
   let ctx = ZeusCtx::new(crate::core::platform::runtime::test_handle());

   let chain = 1;
   let account = Address::from_str("0x67d3FA6a5CF45D85F697A497b3270A06415E5BfE").unwrap();
   let delegated_address = Address::from_str("0x63c0c19a282a1B52b07dD5a65b58948A07DAE32B").unwrap();

   let client = ctx.get_client(chain).await.unwrap();
   let code = client.get_code_at(account).await.unwrap();
   eprintln!("Code {}", code);
   eprintln!("Code length: {}", code.len());

   let addr_slice = &code[3..];
   let address = Address::from_slice(addr_slice);
   eprintln!("Address: {}", address);

   assert_eq!(address, delegated_address);
}

#[tokio::test]
#[ignore = "live chain: needs a working public RPC on every supported chain"]
async fn test_priority_fee_suggestion() {
   let ctx = ZeusCtx::new(crate::core::platform::runtime::test_handle());

   for chain in SUPPORTED_CHAINS {
      let client = ctx.get_client(chain).await.unwrap();
      let fee = client.get_max_priority_fee_per_gas().await.unwrap();
      let fee = format_units(U256::from(fee), "gwei").unwrap();
      println!("Suggested Fee on {}: {}", chain, fee)
   }
}
