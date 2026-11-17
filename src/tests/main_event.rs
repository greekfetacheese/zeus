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

   /// A position NFT is the receipt, not the intent: a V3 mint arrives as that NFT's `Transfer` **and**
   /// `IncreaseLiquidity`, a removal as two position ops plus the NFT's burn, a partial removal as the
   /// two ops alone, and a lone `Collect` as an op of its own. Ranking any of them as the NFT is what
   /// the confirm window, the history row and the notification would all have claimed — "NFT Mint",
   /// "NFT Burn" or "Unknown Interaction" instead of the liquidity change the user asked for.
   #[test]
   fn position_ops_rank_above_the_nft_they_mint_or_burn() {
      // Case: a_v3_mint_ranks_as_the_position_op_not_as_the_nft_it_mints
      {
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

      // Case: a_v3_removal_ranks_as_the_position_op_not_as_the_nft_it_burns
      {
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

      // Case: a_partial_v3_removal_ranks_as_the_position_op
      {
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

      // Case: a_lone_collect_fees_ranks_as_collect_fees
      {
         let analysis = analysis(
            Address::ZERO,
            vec![position_op(PositionOperation::CollectFees)],
         );

         let main_event = analysis.infer_main_event(test_ctx(1), 1);

         assert_eq!(main_event.name(), "Collect Fees");
      }
   }
}
