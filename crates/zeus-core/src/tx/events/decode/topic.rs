//! Per-log decode dispatch.
//!
//! Matches the previous `TransactionAnalysis::new` order so behavior stays equivalent.
//! New protocol families should add a branch here (or a topic0 table entry) and keep
//! params in `events::kinds`.

use super::{DecodeCtx, DecodeOutcome};
use crate::tx::events::{
   BridgeParams, DecodedEvent, NftApproveParams, NftTransferParams, PermitParams, ShieldParams,
   SwapParams, TokenApproveParams, TransferParams, UniswapPositionParams, UnshieldParams,
   UnwrapWETHParams, WrapETHParams,
};
use zeus_eth::{abi::eip7708, alloy_primitives::Log};

/// The native transfer an [EIP-7708](https://eips.ethereum.org/EIPS/eip-7708) log describes, if
/// this log is one.
fn eth_transfer(dctx: &DecodeCtx, log: &Log) -> Option<TransferParams> {
   if !eip7708::is_eth_transfer_log(log) {
      return None;
   }

   let (from, to, value) = eip7708::decode_eth_transfer_log(log).ok()?;
   TransferParams::from_eth_transfer_log(dctx.ctx.clone(), dctx.chain, from, to, value).ok()
}

/// Decode a single log into zero or more [`DecodedEvent`]s.
///
/// Order mirrors the historical ladder in `analysis.rs` and is load-bearing for
/// overlapping decode attempts (e.g. wrap before generic transfer).
pub async fn decode_log(dctx: &DecodeCtx, log: &Log) -> DecodeOutcome {
   // ETH transfer log (EIP-7708), which comes first because its topic0 is ERC-20's `Transfer`
   // signature: the fungible attempt below would look the system address up as a token, and the
   // native one would describe the transaction's own value move instead of what the log says.
   //
   // It counts as a decoded log — the events window reports how many of the transaction's logs were
   // made sense of, and this one is rendered below with the rest. `TransactionAnalysis` keeps it out
   // of the *intent* count, which is a different question (see `decoded_events`).
   if let Some(params) = eth_transfer(dctx, log) {
      return DecodeOutcome::One {
         event: DecodedEvent::Transfer(params),
         counts_as_known: true,
      };
   }

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

   // NFT transfer / mint / burn.
   //
   // Deliberately *before* the fungible attempt below, which is the opposite of what the plan assumed.
   // `TransferParams::new` is not a fungible decoder: it accepts an ERC-721 `Transfer` too and reports
   // `is_erc20_transfer() == false` for it, so placing this after it means this branch is never reached
   // for an ERC-721 — the transaction decodes as a `Transfer` that is neither ERC-20 nor native, and
   // renders as nothing. Placing it here is safe because this decoder is strict: an ERC-721 `Transfer`
   // needs the token id as a fourth topic, which an ERC-20 `Transfer` never has, and the ERC-1155 events
   // have topic0s of their own. `an_erc20_transfer_is_not_an_nft_transfer` pins exactly that.
   if let Ok(params) = NftTransferParams::from_log(dctx.chain, log) {
      let events = params.into_iter().map(DecodedEvent::NftTransfer).collect::<Vec<_>>();

      return DecodeOutcome::Many {
         events,
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

   // NFT approval — per token or collection-wide.
   //
   // Before the ERC-20 attempt for the same reason the NFT transfer sits before the fungible one:
   // an ERC-721 per-token `Approval` shares the ERC-20 `Approval` topic0 and is separated only by
   // its fourth topic. Asking the ERC-20 decoder first would leave it to decide — and it is the one
   // that accepts the 3-topic shape, so the narrower read has to come first.
   if let Ok(params) = NftApproveParams::from_log(dctx.ctx.clone(), dctx.chain, log).await {
      return DecodeOutcome::One {
         event: DecodedEvent::NftApprove(params),
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

#[cfg(test)]
mod tests {
   use super::*;
   use crate::ZeusCtx;
   use zeus_eth::{
      abi::eip7708::{EIP7708, ETH_TRANSFER_LOG_ADDRESS},
      alloy_primitives::{Address, Bytes, Log, U256, address},
      alloy_sol_types::SolEvent,
   };

   const CHAIN: u64 = 11155111;
   const SENDER: Address = address!("A88D2cb271E3dB59CF8CD4c84Ea7d83eEF1c94b0");
   const RECIPIENT: Address = address!("80Dbe0E147739CE637E8f6f87a912b43Aee17543");

   /// What a Sepolia ETH transfer log reports: 0.002713297724087723 ETH.
   const LOGGED_VALUE: u64 = 2_713_297_724_087_723;

   fn eth_transfer_log() -> Log {
      Log {
         address: ETH_TRANSFER_LOG_ADDRESS,
         data: EIP7708::Transfer {
            from: SENDER,
            to: RECIPIENT,
            value: U256::from(LOGGED_VALUE),
         }
         .encode_log_data(),
      }
   }

   /// The log reaches the ladder ahead of the fungible attempt (they share `topics[0]`), and what it
   /// describes is what it says — not the transaction's own value move, which is all the native
   /// decoder could have reported.
   #[tokio::test]
   async fn an_eth_transfer_log_decodes_from_the_log_not_the_transaction() {
      let dctx = DecodeCtx::new(
         ZeusCtx::new(crate::platform::runtime::test_handle()),
         CHAIN,
         SENDER,
         RECIPIENT,
         Bytes::new(),
         U256::from(1), // the transaction moved 1 wei; the log says otherwise
      );

      let DecodeOutcome::One {
         event,
         counts_as_known,
      } = decode_log(&dctx, &eth_transfer_log()).await
      else {
         panic!("the ETH transfer log was not claimed");
      };

      let params = event.as_transfer().expect("a transfer event");
      assert!(params.is_native_transfer());
      assert_eq!(params.sender, SENDER);
      assert_eq!(params.recipient, RECIPIENT);
      assert_eq!(params.amount.wei(), U256::from(LOGGED_VALUE));
      assert!(
         counts_as_known,
         "the log was understood, so the window counts it"
      );
   }

   /// The events window reports "decoded N out of M" from these two numbers, and the ETH transfer
   /// log was the case that read "0 out of 1" while being listed below: it is one of the
   /// transaction's logs and it was decoded, so N is 1. The analysis's *intent* count stays 0 —
   /// that is the separate question, and what keeps a plain ETH send headlined as a transfer.
   #[tokio::test]
   async fn a_decoded_eth_transfer_is_one_of_one() {
      use crate::tx::TransactionAnalysis;
      use crate::tx::events::decode::decode_transaction;

      let logs = vec![eth_transfer_log()];
      let ctx = ZeusCtx::new(crate::platform::runtime::test_handle());
      let dctx = DecodeCtx::new(
         ctx.clone(),
         CHAIN,
         SENDER,
         RECIPIENT,
         Bytes::new(),
         U256::from(1),
      );

      let (events, known_events) = decode_transaction(&dctx, &logs, Vec::new()).await;
      let analysis = TransactionAnalysis::new(
         ctx,
         CHAIN,
         SENDER,
         RECIPIENT,
         Some(false),
         Bytes::new(),
         U256::from(1),
         logs,
         21_000,
         U256::ZERO,
         U256::ZERO,
         Vec::new(),
      )
      .await
      .expect("analysis");

      assert_eq!(events.len(), 1);
      assert_eq!(known_events, 1, "the numerator the window shows");
      assert_eq!(
         analysis.total_events(),
         1,
         "one log is the denominator"
      );
      assert_eq!(analysis.decoded_events(), 0, "still not intent");
      assert!(analysis.is_native_transfer());
   }
}
