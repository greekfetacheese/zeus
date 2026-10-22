//! Main-event ranking tests that need a live chain.
//!
//! These call [`test_ctx`](crate::tests::test_ctx), which unlocks this machine's real `data/`
//! profile with the development credentials, so they live with the binary's other live suites
//! rather than in `zeus-core` (whose tests run with the crate directory as cwd and have no profile).

#[cfg(test)]
mod tests {
   use crate::core::tx::analysis::TransactionAnalysis;
   use crate::core::tx::events::{DecodedEvent, NftTransferParams, PositionOperation};
   use crate::tests::test_ctx;
   use zeus_eth::{
      alloy_primitives::{Address, U256},
      nft::NftStandard,
   };

   fn analysis(sender: Address, events: Vec<DecodedEvent>) -> TransactionAnalysis {
      let mut analysis = TransactionAnalysis::dummy_swap();
      analysis.chain = 1;
      analysis.sender = sender;
      analysis.decoded_events = events;
      analysis.remove_main_event();
      analysis
   }

   /// One position op of the given kind, built from the shared dummy.
   fn position_op(operation: PositionOperation) -> DecodedEvent {
      let DecodedEvent::UniswapPositionOperation(mut params) =
         DecodedEvent::dummy_uniswap_position_operation()
      else {
         unreachable!("the dummy is a position operation")
      };

      params.position_operation = operation;
      DecodedEvent::UniswapPositionOperation(params)
   }

   /// The position NFT the position manager moves when it creates or closes a position.
   fn position_nft(is_mint: bool, is_burn: bool) -> DecodedEvent {
      DecodedEvent::NftTransfer(NftTransferParams {
         chain: 1,
         standard: NftStandard::Erc721,
         collection: Address::from([0xbc; 20]),
         token_id: Some(U256::from(1)),
         amount: U256::from(1),
         from: Address::ZERO,
         to: Address::ZERO,
         is_mint,
         is_burn,
      })
   }

   /// A Uniswap V3 mint arrives as the position manager's `Transfer` of the position NFT **and**
   /// `IncreaseLiquidity`. That is one intent, and the one worth showing is the liquidity change —
   /// ranking it as the NFT is what the confirm window, the history row and the notification would
   /// all have claimed.
   ///
   /// The NFT is a *mint* because that is the shape a position is created in, and `is_mint` is why the
   /// wrong answer read "NFT Mint" rather than "NFT Transfer".
   #[test]
   fn a_v3_mint_ranks_as_the_position_op_not_as_the_nft_it_mints() {
      let analysis = analysis(
         Address::ZERO,
         vec![
            position_nft(true, false),
            position_op(PositionOperation::AddLiquidity),
         ],
      );

      let main_event = analysis.infer_main_event(test_ctx(1), 1);

      assert!(
         main_event.is_uniswap_position_op(),
         "ranked as {}",
         main_event.name()
      );
      assert_eq!(main_event.name(), "Add Liquidity");
   }

   /// A Uniswap V3 removal is one call that decodes to **two** position ops: the position manager
   /// emits `DecreaseLiquidity` and the `Collect` carrying the amounts actually returned, and the
   /// per-log pass turns that `Collect` into an op of its own. Requiring exactly one op therefore
   /// sent every removal past the position rung to the position NFT's own `Transfer` — burned when
   /// the position is closed — so a transaction whose whole point was returning two tokens to the
   /// user was headlined "NFT Burn".
   #[test]
   fn a_v3_removal_ranks_as_the_position_op_not_as_the_nft_it_burns() {
      let analysis = analysis(
         Address::ZERO,
         vec![
            position_nft(false, true),
            position_op(PositionOperation::DecreaseLiquidity),
            position_op(PositionOperation::CollectFees),
         ],
      );

      let main_event = analysis.infer_main_event(test_ctx(1), 1);

      assert!(
         main_event.is_uniswap_position_op(),
         "ranked as {}",
         main_event.name()
      );
      assert_eq!(main_event.name(), "Remove Liquidity");
   }

   /// The same two ops without a burn: a partial removal leaves the position NFT alone, so there was
   /// nothing for the rungs below to claim and the removal came out as "Unknown Interaction".
   #[test]
   fn a_partial_v3_removal_ranks_as_the_position_op() {
      let analysis = analysis(
         Address::ZERO,
         vec![
            position_op(PositionOperation::DecreaseLiquidity),
            position_op(PositionOperation::CollectFees),
         ],
      );

      let main_event = analysis.infer_main_event(test_ctx(1), 1);

      assert_eq!(main_event.name(), "Remove Liquidity");
   }

   /// Collecting fees from a position is an intent of its own — the rule must not need a change of
   /// liquidity to be present before it names the operation.
   #[test]
   fn a_lone_collect_fees_ranks_as_collect_fees() {
      let analysis = analysis(
         Address::ZERO,
         vec![position_op(PositionOperation::CollectFees)],
      );

      let main_event = analysis.infer_main_event(test_ctx(1), 1);

      assert_eq!(main_event.name(), "Collect Fees");
   }
}
