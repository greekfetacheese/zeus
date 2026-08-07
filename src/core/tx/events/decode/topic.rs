//! Per-log decode dispatch.
//!
//! Matches the previous `TransactionAnalysis::new` order so behavior stays equivalent.
//! New protocol families should add a branch here (or a topic0 table entry) and keep
//! params in `events::kinds`.

use super::{DecodeCtx, DecodeOutcome};
use crate::core::tx::events::{
   BridgeParams, DecodedEvent, PermitParams, ShieldParams, SwapParams, TokenApproveParams,
   TransferParams, UniswapPositionParams, UnshieldParams, UnwrapWETHParams, WrapETHParams,
};
use zeus_eth::alloy_primitives::Log;

/// Decode a single log into zero or more [`DecodedEvent`]s.
///
/// Order mirrors the historical ladder in `analysis.rs` and is load-bearing for
/// overlapping decode attempts (e.g. wrap before generic transfer).
pub async fn decode_log(dctx: &DecodeCtx, log: &Log) -> DecodeOutcome {
   // Wrap ETH (WETH Deposit)
   if let Ok(params) = WrapETHParams::from_log(dctx.ctx.clone(), dctx.chain, log) {
      return DecodeOutcome::One {
         event: DecodedEvent::WrapETH(params),
         counts_as_known: true,
      };
   }

   // Unwrap WETH (WETH Withdrawal)
   if let Ok(params) = UnwrapWETHParams::from_log(dctx.ctx.clone(), dctx.chain, log) {
      return DecodeOutcome::One {
         event: DecodedEvent::UnwrapWETH(params),
         counts_as_known: true,
      };
   }

   // ERC-20 / native transfer (native path rarely hits from a log)
   if let Ok(params) = TransferParams::new(
      dctx.ctx.clone(),
      dctx.chain,
      dctx.sender,
      dctx.interact_to,
      dctx.call_data.clone(),
      dctx.value,
      log,
   )
   .await
   {
      let counts_as_known = params.is_erc20_transfer();
      return DecodeOutcome::One {
         event: DecodedEvent::Transfer(params),
         counts_as_known,
      };
   }

   // Railgun Shield (may emit multiple commitments)
   if let Ok(params) = ShieldParams::from_log(dctx.ctx.clone(), dctx.chain, log).await {
      let events = params.into_iter().map(DecodedEvent::Shield).collect::<Vec<_>>();
      return DecodeOutcome::Many {
         events,
         counts_as_known: true,
      };
   }

   // Railgun Unshield
   if let Ok(params) = UnshieldParams::from_log(dctx.ctx.clone(), dctx.chain, log).await {
      return DecodeOutcome::One {
         event: DecodedEvent::Unshield(params),
         counts_as_known: true,
      };
   }

   // ERC-20 Approval
   if let Ok(params) = TokenApproveParams::from_log(dctx.ctx.clone(), dctx.chain, log).await {
      return DecodeOutcome::One {
         event: DecodedEvent::TokenApprove(params),
         counts_as_known: true,
      };
   }

   // Permit2 Permit / Approval
   if let Ok(params) = PermitParams::from_log(dctx.ctx.clone(), dctx.chain, log).await {
      return DecodeOutcome::One {
         event: DecodedEvent::Permit(params),
         counts_as_known: true,
      };
   }

   // Across bridge deposit
   if let Ok(params) = BridgeParams::from_log(dctx.ctx.clone(), dctx.chain, log).await {
      return DecodeOutcome::One {
         event: DecodedEvent::Bridge(params),
         counts_as_known: true,
      };
   }

   // Uniswap swaps (V2 → V3 → V4)
   if let Ok(params) =
      SwapParams::from_uniswap_v2(dctx.ctx.clone(), dctx.chain, dctx.sender, log).await
   {
      return DecodeOutcome::One {
         event: DecodedEvent::SwapToken(params),
         counts_as_known: true,
      };
   }

   if let Ok(params) =
      SwapParams::from_uniswap_v3(dctx.ctx.clone(), dctx.chain, dctx.sender, log).await
   {
      return DecodeOutcome::One {
         event: DecodedEvent::SwapToken(params),
         counts_as_known: true,
      };
   }

   if let Ok(params) =
      SwapParams::from_uniswap_v4(dctx.ctx.clone(), dctx.chain, dctx.sender, log).await
   {
      return DecodeOutcome::One {
         event: DecodedEvent::SwapToken(params),
         counts_as_known: true,
      };
   }

   // Uniswap V3 collect fees (single-log). Add/decrease liquidity are phase 2.
   if let Ok(params) = UniswapPositionParams::collect_fees_for_v3_from_log(
      dctx.ctx.clone(),
      dctx.chain,
      dctx.sender,
      log,
   )
   .await
   {
      return DecodeOutcome::One {
         event: DecodedEvent::UniswapPositionOperation(params),
         counts_as_known: true,
      };
   }

   DecodeOutcome::None
}
