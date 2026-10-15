//! NFT decoding at the app level: raw logs → `DecodedEvent` → the main event.
//!
//! The params themselves are unit-tested where they live (`core::tx::events::kinds::nft`, and the ABI
//! encoders in `zeus-eth`). What can only be proven here is the **wiring between them**: that the NFT
//! branch takes the shared `Transfer` topic *before* the fungible attempt — which would otherwise
//! accept an ERC-721 `Transfer` as a non-ERC-20 transfer and swallow the NFT — that an ERC-1155 batch
//! expands through `DecodeOutcome::Many`, and that a transaction moving an NFT ends up *described* as
//! one — title, notification and history all come from the main event, so a gap between the decoder and
//! the ranking is invisible until a user sees "Unknown Interaction" on their own transaction.
//!
//! These run against a configured endpoint (`test_ctx`), because the fungible side of the ladder
//! resolves token metadata — exactly like the swap and bridge regressions.

#[cfg(test)]
mod tests {
   use crate::core::{
      TransactionAnalysis, ZeusCtx,
      tx::events::{
         DecodedEvent,
         decode::{DecodeCtx, decode_transaction},
      },
   };
   use crate::tests::test_ctx;
   use alloy_sol_types::{SolCall, SolEvent, SolValue};
   use zeus_eth::{
      abi::{
         erc20::IERC20,
         erc721::IERC721,
         erc1155::{IERC1155, IERC5216},
         protocols::uniswap::v4::{
            IPoolManager, IPositionManager, PoolKey,
            actions::{
               DecreaseLiquidityParams, ExactInputSingleParams, MintPositionParams,
               SettleAllParams, SettlePairParams, TakePairParams, TakeParams,
            },
         },
      },
      alloy_primitives::{
         Address, B256, Bytes, I256, Log, LogData, U256, address,
         aliases::{I24, U24},
         keccak256,
      },
      amm::uniswap::v4::Actions,
      nft::NftStandard,
      utils::{NumericValue, address_book},
   };

   const CHAIN: u64 = 1;
   const BAYC: Address = address!("BC4CA0EdA7647A8aB7C2061c2E118A18a936f13D");
   const WETH: Address = address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2");
   const USDC: Address = address!("A0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48");
   const OPERATOR: Address = address!("f39fd6e51aad88f6f4ce6ab8827279cfffb92266");
   const SENDER: Address = address!("46efbaedc92067e6d60e84ed6395099723252496");
   const RECIPIENT: Address = address!("000000000000000000000000000000000000dEaD");

   fn log(emitter: Address, data: LogData) -> Log {
      Log {
         address: emitter,
         data,
      }
   }

   /// An ERC-20 `Transfer`: one of the two shapes that share the topic0, value in the data.
   fn erc20_transfer() -> Log {
      log(
         WETH,
         IERC20::Transfer {
            from: SENDER,
            to: RECIPIENT,
            value: U256::from(1_000_000_000_000_000_000u64),
         }
         .encode_log_data(),
      )
   }

   /// The call that produced it. The ladder decides "native transfer" by an empty calldata, so an
   /// ERC-20 transfer is only recognised as one when the call is present — as it always is on chain.
   fn erc20_calldata() -> Bytes {
      IERC20::transferCall {
         recipient: RECIPIENT,
         amount: U256::from(1_000_000_000_000_000_000u64),
      }
      .abi_encode()
      .into()
   }

   /// An ERC-721 `Transfer`: the token id is indexed, so it lands in the topics.
   fn erc721_transfer(token_id: u64) -> Log {
      log(
         BAYC,
         IERC721::Transfer {
            from: SENDER,
            to: RECIPIENT,
            tokenId: U256::from(token_id),
         }
         .encode_log_data(),
      )
   }

   fn erc1155_single(id: u64, amount: u64) -> Log {
      log(
         BAYC,
         IERC1155::TransferSingle {
            operator: OPERATOR,
            from: SENDER,
            to: RECIPIENT,
            id: U256::from(id),
            value: U256::from(amount),
         }
         .encode_log_data(),
      )
   }

   /// An ERC-20 `Approval` — `Approval(address,address,uint256)` with the value in the data, which is
   /// the *same* topic0 an ERC-721 per-token `Approval` uses.
   fn erc20_approval() -> Log {
      log(
         WETH,
         IERC20::Approval {
            owner: SENDER,
            spender: OPERATOR,
            value: U256::from(1_000_000_000_000_000_000u64),
         }
         .encode_log_data(),
      )
   }

   fn erc20_approve_calldata() -> Bytes {
      IERC20::approveCall {
         spender: OPERATOR,
         amount: U256::from(1_000_000_000_000_000_000u64),
      }
      .abi_encode()
      .into()
   }

   /// An ERC-721 per-token `Approval`: the token id is indexed, so it is a **fourth** topic and the
   /// data stays empty. That is the only thing separating it from the ERC-20 shape above.
   fn erc721_approval(token_id: u64, approved: Address) -> Log {
      log(
         BAYC,
         IERC721::Approval {
            owner: SENDER,
            approved,
            tokenId: U256::from(token_id),
         }
         .encode_log_data(),
      )
   }

   /// `ApprovalForAll` — byte-identical between ERC-721 and ERC-1155.
   fn approval_for_all(approved: bool) -> Log {
      log(
         BAYC,
         IERC721::ApprovalForAll {
            owner: SENDER,
            operator: OPERATOR,
            approved,
         }
         .encode_log_data(),
      )
   }

   /// An ERC-5216 allowance: its own topic0, and the id is *not* indexed.
   fn erc5216_approval(id: u64, amount: u64) -> Log {
      log(
         BAYC,
         IERC5216::Approval {
            account: SENDER,
            operator: OPERATOR,
            id: U256::from(id),
            amount: U256::from(amount),
         }
         .encode_log_data(),
      )
   }

   /// An approval is always produced *by* a call, so these have to carry the calldata that made them.
   /// With empty calldata the ladder reads the transaction as a native transfer before it ever reaches
   /// the approval branches — a combination that cannot occur on chain, since no call means no
   /// `Approval` log, but one that would make these tests prove nothing.
   fn erc721_approve_calldata(token_id: u64, to: Address) -> Bytes {
      IERC721::approveCall {
         to,
         tokenId: U256::from(token_id),
      }
      .abi_encode()
      .into()
   }

   fn set_approval_for_all_calldata(approved: bool) -> Bytes {
      IERC721::setApprovalForAllCall {
         operator: OPERATOR,
         approved,
      }
      .abi_encode()
      .into()
   }

   fn erc5216_approve_calldata(id: u64, amount: u64) -> Bytes {
      IERC5216::approveCall {
         operator: OPERATOR,
         id: U256::from(id),
         amount: U256::from(amount),
      }
      .abi_encode()
      .into()
   }

   /// The V4 pool the position ops below run against: native ETH + USDC, the fee tier and spacing
   /// the position tests use, no hooks. `address(0)` sorts first, as V4 requires.
   fn v4_pool_key() -> PoolKey {
      PoolKey {
         currency0: Address::ZERO,
         currency1: USDC,
         fee: U24::from(500u32),
         tickSpacing: I24::try_from(10).unwrap(),
         hooks: Address::ZERO,
      }
   }

   /// The pool id V4 derives from the PoolKey — `keccak256(abi.encode(key))`, the same expression
   /// as `UniswapV4Pool::id()`. A decrease names only the position NFT, so this is what leads the
   /// decoder back to the pool's currencies.
   fn v4_pool_id() -> B256 {
      keccak256(
         (
            Address::ZERO,
            USDC,
            U24::from(500u32),
            I24::try_from(10).unwrap(),
            Address::ZERO,
         )
            .abi_encode(),
      )
   }

   fn v4_pool_manager() -> Address {
      address_book::uniswap_v4_pool_manager(CHAIN).unwrap()
   }

   fn v4_position_manager() -> Address {
      address_book::uniswap_v4_nft_position_manager(CHAIN).unwrap()
   }

   /// The core `ModifyLiquidity` event the pool manager emits on every liquidity change: positive
   /// delta for an add, negative for a removal.
   fn v4_modify_liquidity_log(liquidity_delta: i64) -> Log {
      log(
         v4_pool_manager(),
         IPoolManager::ModifyLiquidity {
            id: v4_pool_id(),
            sender: v4_position_manager(),
            tickLower: I24::try_from(-887_220).unwrap(),
            tickUpper: I24::try_from(887_220).unwrap(),
            liquidityDelta: I256::try_from(liquidity_delta as i128).unwrap(),
            salt: B256::ZERO,
         }
         .encode_log_data(),
      )
   }

   /// The position NFT's own `Transfer` the position manager emits on a mint, from the zero address.
   fn v4_position_mint_transfer(token_id: u64) -> Log {
      log(
         v4_position_manager(),
         IERC721::Transfer {
            from: Address::ZERO,
            to: SENDER,
            tokenId: U256::from(token_id),
         }
         .encode_log_data(),
      )
   }

   /// The position NFT's burn `Transfer`, to the zero address.
   fn v4_position_burn_transfer(token_id: u64) -> Log {
      log(
         v4_position_manager(),
         IERC721::Transfer {
            from: SENDER,
            to: Address::ZERO,
            tokenId: U256::from(token_id),
         }
         .encode_log_data(),
      )
   }

   /// A currency `Transfer` with the emitter chosen, so a leg can be attached to USDC rather than
   /// the WETH the existing helpers use.
   fn erc20_transfer_of(token: Address, from: Address, to: Address, value: u64) -> Log {
      log(
         token,
         IERC20::Transfer {
            from,
            to,
            value: U256::from(value),
         }
         .encode_log_data(),
      )
   }

   /// A `modifyLiquidities` call: `(bytes actions, bytes[] params)` as the unlock data of the
   /// position manager call.
   fn modify_liquidities_call(actions: &[u8], params: Vec<Bytes>) -> Bytes {
      let unlock_data: Bytes = (Bytes::from(actions.to_vec()), params).abi_encode().into();

      IPositionManager::modifyLiquiditiesCall {
         unlockData: unlock_data,
         deadline: U256::from(u64::MAX),
      }
      .abi_encode()
      .into()
   }

   /// Decode `logs` exactly the way a transaction does, and build the analysis the confirmation window
   /// and the tx history are rendered from.
   ///
   /// `interact_to` is the transaction's target and `call_data` its calldata: the fungible decoder
   /// decides what the transaction is from them, so they have to match the logs.
   async fn analyse(
      ctx: &ZeusCtx,
      interact_to: Address,
      call_data: Bytes,
      logs: Vec<Log>,
   ) -> (Vec<DecodedEvent>, TransactionAnalysis) {
      let dctx = DecodeCtx::new(
         ctx.clone(),
         CHAIN,
         SENDER,
         interact_to,
         call_data.clone(),
         U256::ZERO,
      );

      let (events, known_events) = decode_transaction(&dctx, &logs, Vec::new()).await;

      let analysis = TransactionAnalysis::new(
         ctx.clone(),
         CHAIN,
         SENDER,
         interact_to,
         Some(true),
         call_data,
         U256::ZERO,
         logs,
         21_000,
         U256::ZERO,
         U256::ZERO,
         Vec::new(),
      )
      .await
      .expect("analysis");

      assert_eq!(
         known_events,
         analysis.decoded_events(),
         "the count reported by the decoder and the analysis must agree"
      );

      (events, analysis)
   }

   /// The one that matters most. ERC-20 and ERC-721 `Transfer` share topic0, so if the fungible decoder
   /// ever became lenient — or the NFT branch were placed before it — every ERC-20 transfer in the app
   /// would be reported as an NFT. Asserted through the real ladder, then through the main event, which
   /// is what the user actually reads.
   #[tokio::test]
   #[ignore = "live chain: needs a working RPC and this machine's data/ profile — see tests/mod.rs"]
   async fn an_erc20_transfer_stays_an_erc20_transfer() {
      let ctx = test_ctx(CHAIN);
      let (events, analysis) = analyse(
         &ctx,
         WETH,
         erc20_calldata(),
         vec![erc20_transfer()],
      )
      .await;

      assert_eq!(events.len(), 1);
      assert!(
         events[0].is_erc20_transfer(),
         "decoded as {}",
         events[0].name()
      );
      assert!(!events[0].is_nft_transfer());

      let main_event = analysis.infer_main_event(ctx.clone(), CHAIN);
      assert!(
         main_event.is_erc20_transfer(),
         "ranked as {}",
         main_event.name()
      );
      assert!(!main_event.is_nft_transfer());
   }

   /// A mint is not a transfer between two accounts. The ERC-20 standard's own shape for creating
   /// tokens is a `Transfer` from the zero address, and taken at face value it is decoded, ranked as the
   /// main event if it is the only log (`main_event` prefers a single transfer) and read by the user as
   /// «Transfer … from 0x0000…0000». The tokens arriving are the receipt's balance-diff business.
   #[tokio::test]
   #[ignore = "live chain: needs a working RPC and this machine's data/ profile — see tests/mod.rs"]
   async fn a_mint_log_is_not_a_transfer() {
      let ctx = test_ctx(CHAIN);
      let mint = log(
         WETH,
         IERC20::Transfer {
            from: Address::ZERO,
            to: RECIPIENT,
            value: U256::from(1_000_000_000_000_000_000u64),
         }
         .encode_log_data(),
      );

      let (events, analysis) = analyse(&ctx, WETH, erc20_calldata(), vec![mint]).await;

      assert!(
         !events.iter().any(|event| event.is_erc20_transfer()),
         "a mint is not a transfer, but the ladder decoded {:?}",
         events.iter().map(|event| event.name()).collect::<Vec<_>>()
      );

      let main_event = analysis.infer_main_event(ctx.clone(), CHAIN);
      assert!(
         !main_event.is_erc20_transfer(),
         "a lone mint must not become the headline: ranked as {}",
         main_event.name()
      );
   }

   /// An ERC-721 transfer decodes with its id and its standard, and becomes the main event — which is
   /// what decides whether the user sees an NFT transfer or "Unknown Interaction".
   #[tokio::test]
   #[ignore = "live chain: needs a working RPC and this machine's data/ profile — see tests/mod.rs"]
   async fn an_erc721_transfer_decodes_and_is_what_the_transaction_is_described_as() {
      let ctx = test_ctx(CHAIN);
      let (events, analysis) = analyse(&ctx, BAYC, Bytes::new(), vec![erc721_transfer(1)]).await;

      assert_eq!(events.len(), 1);
      assert!(
         events[0].is_nft_transfer(),
         "decoded as {}",
         events[0].name()
      );

      let params = events[0].nft_transfer_params();
      assert_eq!(params.collection, BAYC);
      assert_eq!(params.token_id, Some(U256::from(1)));
      assert_eq!(params.standard, NftStandard::Erc721);
      assert_eq!(
         params.amount,
         U256::from(1),
         "an ERC-721 moves exactly one"
      );
      assert_eq!((params.from, params.to), (SENDER, RECIPIENT));

      let main_event = analysis.infer_main_event(ctx.clone(), CHAIN);
      assert_eq!(main_event.name(), "NFT Transfer");
   }

   /// A batch is one log carrying several transfers, so the ladder has to expand it — one event per id,
   /// each with its own amount, and all of them the main event's siblings.
   #[tokio::test]
   #[ignore = "live chain: needs a working RPC and this machine's data/ profile — see tests/mod.rs"]
   async fn an_erc1155_batch_expands_to_one_decoded_event_per_id() {
      let ctx = test_ctx(CHAIN);
      let (events, analysis) = analyse(
         &ctx,
         BAYC,
         Bytes::new(),
         vec![erc1155_single(7, 3), erc1155_single(9, 5)],
      )
      .await;

      assert_eq!(events.len(), 2, "one event per log here");
      assert!(events.iter().all(|event| event.is_nft_transfer()));

      let ids: Vec<U256> = events
         .iter()
         .map(|event| event.nft_transfer_params().token_id.unwrap())
         .collect();
      assert_eq!(ids, vec![U256::from(7), U256::from(9)]);

      assert_eq!(
         analysis.nft_transfers_len(),
         2,
         "the analysis carries them for the window that lists them"
      );
   }

   /// A transaction moving an NFT *and* paying out WETH (a marketplace sale, say) keeps both decoded:
   /// the ladder does not let one kind swallow the other, and the ranking has to pick a single main
   /// event without losing what it did not pick.
   #[tokio::test]
   #[ignore = "live chain: needs a working RPC and this machine's data/ profile — see tests/mod.rs"]
   async fn an_nft_transfer_and_an_erc20_transfer_in_one_transaction_are_both_decoded() {
      let ctx = test_ctx(CHAIN);
      let (events, analysis) = analyse(
         &ctx,
         WETH,
         erc20_calldata(),
         vec![erc721_transfer(1), erc20_transfer()],
      )
      .await;

      assert_eq!(events.len(), 2);
      assert_eq!(analysis.nft_transfers_len(), 1);
      assert_eq!(analysis.erc20_transfers_len(), 1);
   }

   /// The approval half of the shared-topic0 trap. An ERC-20 `Approval` and an ERC-721 per-token
   /// `Approval` are the same signature, and only the topic count separates them — so the ladder has
   /// to keep the ERC-20 reading for a 3-topic log, exactly as it does for `Transfer`.
   #[tokio::test]
   #[ignore = "live chain: needs a working RPC and this machine's data/ profile — see tests/mod.rs"]
   async fn an_erc20_approval_stays_an_erc20_approval() {
      let ctx = test_ctx(CHAIN);
      let (events, analysis) = analyse(
         &ctx,
         WETH,
         erc20_approve_calldata(),
         vec![erc20_approval()],
      )
      .await;

      assert_eq!(events.len(), 1);
      assert!(
         events[0].is_token_approval(),
         "decoded as {}",
         events[0].name()
      );
      assert!(
         !events[0].is_nft_approval(),
         "an ERC-20 approval must not be read as an NFT one"
      );
      assert_eq!(analysis.nft_approvals_len(), 0);

      let main_event = analysis.infer_main_event(ctx.clone(), CHAIN);
      assert!(main_event.is_token_approval());
   }

   /// An ERC-721 per-token `Approval` — the 4-topic shape — decodes as an NFT approval, is *not*
   /// mistaken for the ERC-20 one it shares a topic0 with, and becomes the transaction's description.
   #[tokio::test]
   #[ignore = "live chain: needs a working RPC and this machine's data/ profile — see tests/mod.rs"]
   async fn an_erc721_approval_is_an_nft_approval_and_not_a_token_approval() {
      let ctx = test_ctx(CHAIN);
      let (events, analysis) = analyse(
         &ctx,
         BAYC,
         erc721_approve_calldata(1071, OPERATOR),
         vec![erc721_approval(1071, OPERATOR)],
      )
      .await;

      assert_eq!(events.len(), 1);
      assert!(
         events[0].is_nft_approval(),
         "decoded as {}",
         events[0].name()
      );
      assert!(!events[0].is_token_approval());
      assert!(!events[0].is_nft_transfer());

      let params = events[0].nft_approve_params();
      assert_eq!(params.collection, BAYC);
      assert_eq!(params.token_id, Some(U256::from(1071)));
      assert_eq!(
         (params.owner, params.operator),
         (SENDER, OPERATOR)
      );
      assert!(!params.is_collection_wide());

      let main_event = analysis.infer_main_event(ctx.clone(), CHAIN);
      assert_eq!(main_event.name(), "NFT Approval");
      assert!(main_event.is_nft_approval());
   }

   /// A cleared ERC-721 approval is the same event with the zero address, and must rank as the main
   /// event all the same — a revocation is the thing users most need to see.
   #[tokio::test]
   #[ignore = "live chain: needs a working RPC and this machine's data/ profile — see tests/mod.rs"]
   async fn a_revoked_erc721_approval_is_still_the_main_event() {
      let ctx = test_ctx(CHAIN);
      let (events, analysis) = analyse(
         &ctx,
         BAYC,
         erc721_approve_calldata(1071, Address::ZERO),
         vec![erc721_approval(1071, Address::ZERO)],
      )
      .await;

      assert!(events[0].nft_approve_params().is_revoke());

      let main_event = analysis.infer_main_event(ctx.clone(), CHAIN);
      assert_eq!(main_event.name(), "Revoke NFT Approval");
   }

   /// `ApprovalForAll` goes down the NFT-approval path, not the transfer one: it is collection-wide and
   /// carries no token id, and the ranking must still pick it up.
   #[tokio::test]
   #[ignore = "live chain: needs a working RPC and this machine's data/ profile — see tests/mod.rs"]
   async fn an_approval_for_all_is_an_nft_approval_that_covers_the_collection() {
      let ctx = test_ctx(CHAIN);
      let (events, analysis) = analyse(
         &ctx,
         BAYC,
         set_approval_for_all_calldata(true),
         vec![approval_for_all(true)],
      )
      .await;

      assert_eq!(events.len(), 1);
      assert!(
         events[0].is_nft_approval(),
         "decoded as {}",
         events[0].name()
      );
      assert!(!events[0].is_nft_transfer());

      let params = events[0].nft_approve_params();
      assert!(params.is_collection_wide());
      assert_eq!(params.token_id, None);
      assert_eq!(params.approved, Some(true));
      assert_eq!(
         (params.owner, params.operator),
         (SENDER, OPERATOR)
      );

      let main_event = analysis.infer_main_event(ctx.clone(), CHAIN);
      assert_eq!(main_event.name(), "NFT Approval");
   }

   /// An ERC-5216 allowance has a topic0 of its own, so it cannot be confused with either shape above.
   #[tokio::test]
   #[ignore = "live chain: needs a working RPC and this machine's data/ profile — see tests/mod.rs"]
   async fn an_erc5216_allowance_decodes_with_its_id_and_amount() {
      let ctx = test_ctx(CHAIN);
      let (events, analysis) = analyse(
         &ctx,
         BAYC,
         erc5216_approve_calldata(7, 5),
         vec![erc5216_approval(7, 5)],
      )
      .await;

      assert_eq!(events.len(), 1);
      assert!(events[0].is_nft_approval());

      let params = events[0].nft_approve_params();
      assert_eq!(params.standard, Some(NftStandard::Erc1155));
      assert_eq!(params.token_id, Some(U256::from(7)));
      assert_eq!(params.amount, Some(U256::from(5)));
      assert_eq!(
         (params.owner, params.operator),
         (SENDER, OPERATOR)
      );

      assert!(analysis.infer_main_event(ctx.clone(), CHAIN).is_nft_approval());
   }

   /// A V4 mint is one `modifyLiquidities` batch: `MINT_POSITION`, `SETTLE_PAIR`, `TAKE_PAIR`.
   /// The pool manager's `ModifyLiquidity` event is what proves the pool moved; without a decoder
   /// for it the transaction is headlined by the position NFT's own mint `Transfer` ("NFT Mint") —
   /// the receipt token, not the liquidity change the user asked for.
   #[tokio::test]
   #[ignore = "live chain: needs a working RPC and this machine's data/ profile — see tests/mod.rs"]
   async fn a_v4_position_mint_reads_as_add_liquidity() {
      let ctx = test_ctx(CHAIN);

      let mint = MintPositionParams {
         poolKey: v4_pool_key(),
         tickLower: I24::try_from(-887_220).unwrap(),
         tickUpper: I24::try_from(887_220).unwrap(),
         liquidity: U256::from(1_000_000u64),
         amount0Max: u128::MAX,
         amount1Max: u128::MAX,
         owner: SENDER,
         hookData: Bytes::default(),
      };

      let call_data = modify_liquidities_call(
         &[0x02, 0x0d, 0x11],
         vec![
            Actions::MINT_POSITION(mint).abi_encode(),
            Actions::SETTLE_PAIR(SettlePairParams {
               currency0: Address::ZERO,
               currency1: USDC,
            })
            .abi_encode(),
            Actions::TAKE_PAIR(TakePairParams {
               currency0: Address::ZERO,
               currency1: USDC,
               recipient: SENDER,
            })
            .abi_encode(),
         ],
      );

      let position_manager = v4_position_manager();
      let logs = vec![
         v4_modify_liquidity_log(1_000_000),
         v4_position_mint_transfer(1234),
         erc20_transfer_of(USDC, SENDER, position_manager, 5_000_000),
      ];

      let (events, analysis) = analyse(&ctx, position_manager, call_data, logs).await;

      assert!(
         events.iter().any(|event| event.is_nft_transfer()),
         "the position NFT's mint is still one of the decoded events"
      );

      let main_event = analysis.infer_main_event(ctx.clone(), CHAIN);
      assert!(
         main_event.is_uniswap_position_op(),
         "ranked as {}",
         main_event.name()
      );
      assert!(!main_event.is_nft_transfer());

      let params = main_event.uniswap_position_params();
      assert!(params.op_is_add_liquidity());
      assert_eq!(params.currency0.address(), Address::ZERO);
      assert_eq!(params.currency1.address(), USDC);
      assert_eq!(
         params.amount1,
         NumericValue::parse_to_wei("5", 6),
         "the USDC leg is the transfer into the position manager"
      );
      assert_eq!(
         params.amount0,
         NumericValue::parse_to_wei("0", 18),
         "the harness sends no call value, so the native leg is zero"
      );
      assert_eq!(
         params.min_amount0, None,
         "a mint's amount0Max is a cap, not a minimum"
      );
      assert_eq!(params.min_amount1, None);
   }

   /// A removal is the mirror: a negative-delta `ModifyLiquidity` plus a decrease action. Without
   /// the decode a burned position is headlined by the NFT's burn `Transfer` ("NFT Burn") and a
   /// partial removal by "Unknown Interaction" — both hide the tokens coming back.
   #[tokio::test]
   #[ignore = "live chain: needs a working RPC and this machine's data/ profile — see tests/mod.rs"]
   async fn a_v4_position_burn_reads_as_remove_liquidity() {
      let ctx = test_ctx(CHAIN);

      let decrease = DecreaseLiquidityParams {
         tokenId: U256::from(4321),
         liquidity: U256::from(500_000u64),
         amount0Min: 1_000_000_000_000_000,
         amount1Min: 2_400_000,
         hookData: Bytes::default(),
      };

      let call_data = modify_liquidities_call(
         &[0x01, 0x0c, 0x0e],
         vec![
            Actions::DECREASE_LIQUIDITY(decrease).abi_encode(),
            Actions::SETTLE_ALL(SettleAllParams {
               currency: USDC,
               maxAmount: U256::MAX,
            })
            .abi_encode(),
            Actions::TAKE(TakeParams {
               currency: USDC,
               recipient: SENDER,
               amount: U256::MAX,
            })
            .abi_encode(),
         ],
      );

      let position_manager = v4_position_manager();
      let logs = vec![
         v4_modify_liquidity_log(-500_000),
         v4_position_burn_transfer(4321),
         erc20_transfer_of(USDC, position_manager, SENDER, 2_500_000),
      ];

      let (_events, analysis) = analyse(&ctx, position_manager, call_data, logs).await;

      let main_event = analysis.infer_main_event(ctx.clone(), CHAIN);
      assert!(
         main_event.is_uniswap_position_op(),
         "ranked as {}",
         main_event.name()
      );
      assert!(!main_event.is_nft_transfer());

      let params = main_event.uniswap_position_params();
      assert!(params.op_is_decrease_liquidity());
      assert_eq!(params.currency0.address(), Address::ZERO);
      assert_eq!(params.currency1.address(), USDC);
      assert_eq!(
         params.amount1,
         NumericValue::parse_to_wei("2.5", 6),
         "the USDC leg is the payout back to the sender"
      );
      assert_eq!(
         params.min_amount0,
         Some(NumericValue::parse_to_wei("0.001", 18))
      );
      assert_eq!(
         params.min_amount1,
         Some(NumericValue::parse_to_wei("2.4", 6))
      );
   }

   /// Swap-family actions are not a position operation: however the receipt looks — here a pool
   /// manager `ModifyLiquidity` log and a payment into the position manager — nothing is decoded
   /// as an add or a removal, because no action adds or removes liquidity.
   #[tokio::test]
   #[ignore = "live chain: needs a working RPC and this machine's data/ profile — see tests/mod.rs"]
   async fn a_v4_swap_only_does_not_read_as_a_position_op() {
      let ctx = test_ctx(CHAIN);

      let swap = ExactInputSingleParams {
         poolKey: v4_pool_key(),
         zeroForOne: false,
         amountIn: 1_000_000,
         amountOutMinimum: 0,
         hookData: Bytes::default(),
      };

      let call_data = modify_liquidities_call(
         &[0x06, 0x0d, 0x11],
         vec![
            Actions::SWAP_EXACT_IN_SINGLE(swap).abi_encode(),
            Actions::SETTLE_PAIR(SettlePairParams {
               currency0: Address::ZERO,
               currency1: USDC,
            })
            .abi_encode(),
            Actions::TAKE_PAIR(TakePairParams {
               currency0: Address::ZERO,
               currency1: USDC,
               recipient: SENDER,
            })
            .abi_encode(),
         ],
      );

      let position_manager = v4_position_manager();
      let logs = vec![
         v4_modify_liquidity_log(1_000_000),
         erc20_transfer_of(USDC, SENDER, position_manager, 5_000_000),
      ];

      let (events, analysis) = analyse(&ctx, position_manager, call_data, logs).await;

      assert!(
         !events.iter().any(|event| event.is_uniswap_position_op()),
         "swap actions decoded as {:?}",
         events.iter().map(|event| event.name()).collect::<Vec<_>>()
      );

      let main_event = analysis.infer_main_event(ctx.clone(), CHAIN);
      assert!(!main_event.is_uniswap_position_op());
   }
}
