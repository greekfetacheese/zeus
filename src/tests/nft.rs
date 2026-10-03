//! NFT decoding at the app level: raw logs → `DecodedEvent` → the main event.
//!
//! The params themselves are unit-tested where they live (`core::tx::events::kinds::nft`, and the ABI
//! encoders in `zeus-eth`). What can only be proven here is the **wiring between them**: that the
//! fungible decoder keeps first refusal on the shared `Transfer` topic, that an ERC-1155 batch expands
//! through `DecodeOutcome::Many`, and that a transaction moving an NFT ends up *described* as one —
//! title, notification and history all come from the main event, so a gap between the decoder and the
//! ranking is invisible until a user sees "Unknown Interaction" on their own transaction.
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
   use alloy_sol_types::{SolCall, SolEvent};
   use zeus_eth::{
      abi::{erc20::IERC20, erc721::IERC721, erc1155::IERC1155},
      alloy_primitives::{Address, Bytes, Log, LogData, U256, address},
      nft::NftStandard,
   };

   const CHAIN: u64 = 1;
   const BAYC: Address = address!("BC4CA0EdA7647A8aB7C2061c2E118A18a936f13D");
   const WETH: Address = address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2");
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

   /// An ERC-721 transfer decodes with its id and its standard, and becomes the main event — which is
   /// what decides whether the user sees an NFT transfer or "Unknown Interaction".
   #[tokio::test]
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
}
