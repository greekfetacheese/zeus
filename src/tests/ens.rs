#[cfg(test)]
mod tests {
   use crate::core::ZeusCtx;
   use std::sync::Arc;
   use zeus_eth::alloy_primitives::{Address, address};
   use zeus_eth::utils::{client::RpcClient, ens, interoperable_name};

   const VITALIK: Address = address!("d8dA6BF26964aF9D7eEd9e03E53415D37aA96045");
   const NO_PRIMARY_NAME: Address = address!("000000000000000000000000000000000000dEaD");

   /// A configured mainnet RPC that can actually answer an ENS call, plus a
   /// connected client: `(url, client)`.
   ///
   /// The default RPC list contains endpoints that answer `eth_chainId` but refuse
   /// `eth_call` ("rpc method is not whitelisted"). The app filters those out during
   /// startup measurement, so a test has to find a usable one the same way — by
   /// making the call.
   async fn usable_mainnet_rpc(ctx: &ZeusCtx) -> (Arc<str>, RpcClient) {
      let z_client = ctx.get_zeus_client();

      for (url, rpc) in z_client.get_rpcs(ens::ENS_CHAIN) {
         let Ok(client) = ctx.connect_to_rpc(&rpc).await else {
            continue;
         };

         if ens::resolve_name(&client, "vitalik.eth").await.is_ok() {
            return (url, client);
         }
      }

      panic!("no configured mainnet rpc can resolve ENS names");
   }

   async fn mainnet_client(ctx: &ZeusCtx) -> RpcClient {
      usable_mainnet_rpc(ctx).await.1
   }

   /// A context whose usable mainnet RPC is marked as measured, so the
   /// `ZeusCtx::get_client` call inside `lookup_address_name` can pick it. The app
   /// does this measurement at startup.
   async fn ctx_with_measured_mainnet_rpc() -> ZeusCtx {
      let ctx = ZeusCtx::new();
      let z_client = ctx.get_zeus_client();
      let (url, _client) = usable_mainnet_rpc(&ctx).await;

      let mut rpcs = z_client.get_rpcs(ens::ENS_CHAIN);
      let mut rpc = rpcs.remove(&url).expect("rpc is configured");

      // Builtin RPCs are seeded *disabled*, and `get_best_rpc` only considers
      // enabled, working endpoints — the app enables/measures them at startup.
      rpc.enabled = true;
      rpc.check.working = true;
      rpc.check.fully_functional = true;
      z_client.add_rpc(ens::ENS_CHAIN, rpc);

      ctx
   }

   #[tokio::test]
   async fn test_resolve_ens_name() {
      let ctx = ZeusCtx::new();
      let client = mainnet_client(&ctx).await;

      assert_eq!(
         ens::resolve_name(&client, "vitalik.eth").await.unwrap(),
         Some(VITALIK)
      );

      // Our normalizer folds case and trims, so a name `alloy-ens` would hash raw
      // (and "resolve" to the zero address) still lands on the same address.
      assert_eq!(
         ens::resolve_name(&client, "  Vitalik.ETH  ").await.unwrap(),
         Some(VITALIK)
      );
      assert_eq!(
         ens::resolve_name(&client, "VITALIK.ETH").await.unwrap(),
         Some(VITALIK)
      );
   }

   #[tokio::test]
   async fn test_ens_name_without_address_record_is_none() {
      let ctx = ZeusCtx::new();
      let client = mainnet_client(&ctx).await;

      // "ethereum.eth" has a resolver but no `addr` record. alloy-ens reports that as
      // a *successful* lookup of the zero address, which must never become a recipient.
      assert_eq!(
         ens::resolve_name(&client, "ethereum.eth").await.unwrap(),
         None
      );
   }

   #[tokio::test]
   async fn test_unknown_and_invalid_ens_names() {
      let ctx = ZeusCtx::new();
      let client = mainnet_client(&ctx).await;

      // No such name (resolver not found).
      assert!(
         ens::resolve_name(&client, "not-registered-zzz-999.eth")
            .await
            .unwrap_or(None)
            .is_none()
      );

      // Names the normalizer refuses are never hashed, so no request is made at all.
      for invalid in [
         "",
         "vitalik..eth",
         "-vitalik.eth",
         "vitalik.eth.",
         "vitalik eth",
         "vital\u{456}k.eth",
         "\u{1F4A9}.eth",
      ] {
         assert_eq!(
            ens::resolve_name(&client, invalid).await.unwrap(),
            None,
            "{invalid:?} must not resolve"
         );
      }
   }

   #[tokio::test]
   async fn test_offchain_names_are_refused() {
      let ctx = ZeusCtx::new();
      let client = mainnet_client(&ctx).await;

      // ERC-3668 name that only an HTTP gateway can answer. With alloy-ens's default
      // `shared_http_ccip_read_client` this returns an address; with our refusing
      // gateway it has to fail. If this ever starts succeeding, a request left the
      // RPC client.
      assert!(
         ens::resolve_name(&client, "1.offchainexample.eth").await.is_err(),
         "offchain resolution must stay refused"
      );
   }

   #[tokio::test]
   async fn test_reverse_ens_lookup() {
      let ctx = ZeusCtx::new();
      let client = mainnet_client(&ctx).await;

      assert_eq!(
         ens::lookup_name(&client, &VITALIK).await.unwrap().as_deref(),
         Some("vitalik.eth")
      );

      // The common case: an address with no primary name is `None`, not an error.
      assert_eq!(
         ens::lookup_name(&client, &NO_PRIMARY_NAME).await.unwrap(),
         None
      );
   }

   #[tokio::test]
   async fn test_lookup_address_name_stores_ens_name() {
      let ctx = ctx_with_measured_mainnet_rpc().await;

      assert!(ctx.lookup_address_name(ens::ENS_CHAIN, VITALIK).await);
      assert_eq!(
         ctx.get_address_name(ens::ENS_CHAIN, VITALIK).as_deref(),
         Some("vitalik.eth")
      );

      // Cached: an address that already has a name is never looked up again.
      assert!(!ctx.lookup_address_name(ens::ENS_CHAIN, VITALIK).await);

      // No primary name means nothing is stored, and no error is raised.
      assert!(!ctx.lookup_address_name(ens::ENS_CHAIN, NO_PRIMARY_NAME).await);
      assert!(ctx.get_address_name(ens::ENS_CHAIN, NO_PRIMARY_NAME).is_none());

      // A name the user set explicitly (wallet / contact) wins over ENS: an address
      // that already has a name is never looked up, so a reverse name can never
      // replace it.
      let owned = address!("1111111111111111111111111111111111111111");
      ctx.address_book().insert_identity(owned, "My Wallet");

      assert!(
         !ctx.lookup_address_name(ens::ENS_CHAIN, owned).await,
         "an explicitly named address must not be looked up"
      );
      assert_eq!(
         ctx.get_address_name(ens::ENS_CHAIN, owned).as_deref(),
         Some("My Wallet")
      );
   }

   #[tokio::test]
   async fn test_resolve_chain_labels() {
      let ctx = ZeusCtx::new();
      let client = mainnet_client(&ctx).await;

      // The live `on.eth` registry. Resolution has to go through the Universal Resolver:
      // the `on.eth` resolver is wildcard-only (ENSIP-10) and reverts on a direct call.
      assert_eq!(
         ens::resolve_chain_label(&client, "ethereum").await.unwrap(),
         Some(1)
      );
      assert_eq!(
         ens::resolve_chain_label(&client, "optimism").await.unwrap(),
         Some(10)
      );
      // An alias: `op.on.eth` and `optimism.on.eth` are the same chain.
      assert_eq!(
         ens::resolve_chain_label(&client, "op").await.unwrap(),
         Some(10)
      );
      assert_eq!(
         ens::resolve_chain_label(&client, "base").await.unwrap(),
         Some(8453)
      );
      assert_eq!(
         ens::resolve_chain_label(&client, "arbitrum").await.unwrap(),
         Some(42161)
      );

      // Labels are normalized like names.
      assert_eq!(
         ens::resolve_chain_label(&client, "  BASE  ").await.unwrap(),
         Some(8453)
      );

      // Not registered under `on.eth`.
      assert_eq!(
         ens::resolve_chain_label(&client, "polygon").await.unwrap(),
         None
      );
      assert_eq!(
         ens::resolve_chain_label(&client, "not-a-real-chain").await.unwrap(),
         None
      );

      // A label, not a name: `base.on.eth` must not become `base.on.eth.on.eth`.
      assert_eq!(
         ens::resolve_chain_label(&client, "base.on.eth").await.unwrap(),
         None
      );

      // Labels the normalizer refuses are never hashed, so no request is made at all.
      for invalid in ["", "-base", "base-", "b\u{456}se", "base eth"] {
         assert_eq!(
            ens::resolve_chain_label(&client, invalid).await.unwrap(),
            None,
            "{invalid:?} must not resolve"
         );
      }
   }

   #[tokio::test]
   async fn test_resolve_name_for_chain() {
      let ctx = ZeusCtx::new();
      let client = mainnet_client(&ctx).await;

      // Chain 1 uses coin type 60 — the same record the plain path reads.
      assert_eq!(
         ens::resolve_name_for_chain(&client, "vitalik.eth", ens::ENS_CHAIN)
            .await
            .unwrap()
            .map(|chain_address| chain_address.address),
         Some(VITALIK)
      );

      // A name with a record explicitly set for Base.
      let base = ens::resolve_name_for_chain(&client, "jefflau.eth", 8453)
         .await
         .unwrap()
         .expect("jefflau.eth has an explicit Base record");
      assert!(!base.from_default_evm_record);

      // The point of the whole feature: a name with no Base record and no default-EVM
      // record yields nothing — *not* mainnet's address. The mainnet record must never
      // leak across chains.
      assert_eq!(
         ens::resolve_name_for_chain(&client, "vitalik.eth", 8453).await.unwrap(),
         None
      );

      // A name whose only EVM record is the ENSIP-19 default one still resolves, flagged.
      let default = ens::resolve_name_for_chain(&client, "brantly.eth", 8453)
         .await
         .unwrap()
         .expect("brantly.eth has a default EVM chain record");
      assert!(default.from_default_evm_record);

      // Chain ids ENSIP-11 reserves for nothing: no request is made.
      assert_eq!(
         ens::resolve_name_for_chain(&client, "vitalik.eth", 0x8000_0000).await.unwrap(),
         None
      );
   }

   #[tokio::test]
   async fn test_resolve_interoperable_names() {
      let ctx = ZeusCtx::new();
      let client = mainnet_client(&ctx).await;

      // Label form: `on.eth` supplies the chain, the name supplies the address.
      let resolved = interoperable_name::resolve(&client, "jefflau.eth@base")
         .await
         .unwrap()
         .expect("jefflau.eth has a Base record");
      assert_eq!(resolved.chain_id, 8453);
      assert_eq!(resolved.name.as_deref(), Some("jefflau.eth"));
      assert!(!resolved.from_default_evm_record);

      // The CAIP-2 form of the same identity resolves identically.
      assert_eq!(
         interoperable_name::resolve(&client, "jefflau.eth@eip155:8453").await.unwrap(),
         Some(resolved)
      );

      // A raw address with a CAIP-2 chain part costs no lookup at all, and the checksum is
      // checked against the ERC-7930 fields.
      let raw = interoperable_name::resolve(
         &client,
         "0xFe89cc7aBB2C4183683ab71653C4cdc9B02D44b7@eip155:1#80B12379",
      )
      .await
      .unwrap()
      .expect("a raw address needs no lookup");
      assert_eq!(
         raw.address,
         address!("Fe89cc7aBB2C4183683ab71653C4cdc9B02D44b7")
      );
      assert_eq!(raw.chain_id, 1);
      assert_eq!(raw.name, None);

      // The `ethereum` label form of the same raw address, checksum included.
      assert_eq!(
         interoperable_name::resolve(
            &client,
            "0xFe89cc7aBB2C4183683ab71653C4cdc9B02D44b7@ethereum#80B12379",
         )
         .await
         .unwrap(),
         Some(raw)
      );

      // A checksum that does not match is refused, not silently accepted.
      assert!(matches!(
         interoperable_name::resolve(
            &client,
            "0xFe89cc7aBB2C4183683ab71653C4cdc9B02D44b7@eip155:1#DEADBEEF",
         )
         .await,
         Err(interoperable_name::InteroperableNameError::ChecksumMismatch { .. })
      ));

      // An unknown chain label is "nothing to offer" rather than an error.
      assert!(
         interoperable_name::resolve(&client, "vitalik.eth@polygon")
            .await
            .unwrap()
            .is_none()
      );

      // A name with no address for that chain is also "nothing to offer".
      assert!(
         interoperable_name::resolve(&client, "vitalik.eth@base")
            .await
            .unwrap()
            .is_none()
      );

      // Not an Interoperable Name at all.
      assert!(interoperable_name::resolve(&client, "vitalik.eth").await.is_err());
   }

   /// A chain-specific name has to survive into the display paths, which only ever see
   /// `(chain, address)`. `jefflau.eth@base` resolves to an address whose *primary* name is
   /// `jeff.eth`, so a label re-derived from the address is a different name — and for most
   /// chain-specific names there is no primary name at all, which is how the recipient used to end
   /// up displayed as a truncated address.
   #[tokio::test]
   async fn test_remember_resolved_name_beats_the_reverse_lookup() {
      let ctx = ctx_with_measured_mainnet_rpc().await;
      let client = usable_mainnet_rpc(&ctx).await.1;

      let resolved = interoperable_name::resolve(&client, "jefflau.eth@base")
         .await
         .unwrap()
         .expect("jefflau.eth has a Base record");
      let (address, chain) = (resolved.address, resolved.chain_id);
      assert_eq!(chain, 8453);

      // Nothing is known about the address until the name we resolved is remembered.
      assert_eq!(ctx.get_address_name(chain, address), None);

      assert!(ctx.remember_resolved_name(chain, address, "jefflau.eth"));
      assert_eq!(
         ctx.get_address_name(chain, address).as_deref(),
         Some("jefflau.eth")
      );

      // An existing name is never overwritten, and once one exists the reverse lookup is skipped
      // outright — so it cannot replace the name the user actually entered with `jeff.eth`.
      assert!(!ctx.remember_resolved_name(chain, address, "something.else"));
      assert!(!ctx.lookup_address_name(chain, address).await);
      assert_eq!(
         ctx.get_address_name(chain, address).as_deref(),
         Some("jefflau.eth")
      );

      // It stays chain-specific: resolving for Base must not label the address on mainnet.
      assert_eq!(ctx.get_address_name(1, address), None);
   }
}
