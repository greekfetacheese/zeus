//! Infer the primary user-facing intent from a decoded transaction.
//!
//! Decode produces hop-level / atomic [`DecodedEvent`]s; this module ranks and
//! composes them into a single main event for notifications, history, and confirm UI.

use super::analysis::TransactionAnalysis;
use super::events::{DecodedEvent, SwapParams, TransferParams};
use crate::core::{ZeusCtx, types::Dapp};
use zeus_eth::{
   alloy_primitives::U256,
   currency::{Currency, NativeCurrency},
   utils::NumericValue,
};

impl TransactionAnalysis {
   /// Try to infer the main event from the analysis.
   ///
   /// If [`Self::set_main_event`] was used, that override wins.
   pub fn infer_main_event(&self, ctx: ZeusCtx, chain: u64) -> DecodedEvent {
      if let Some(event) = self.main_event_override() {
         return event;
      }

      // Priority table (high → low). Keep protocol-agnostic kinds so new
      // bridges/swaps plug in without rewriting ranking.
      if self.shield_len() == 1 {
         return DecodedEvent::Shield(self.shields()[0].clone());
      }

      if self.unshield_len() == 1 {
         return DecodedEvent::Unshield(self.unshields()[0].clone());
      }

      // ETH Transfer (no logs / empty calldata path)
      if self.is_native_transfer() {
         return compose_native_transfer(self, ctx, chain);
      }

      // Simple single-event txs
      if self.decoded_events() == 1 && self.erc20_transfers_len() == 1 {
         return DecodedEvent::Transfer(self.erc20_transfers()[0].clone());
      }

      if self.decoded_events() == 1 && self.token_approvals_len() == 1 {
         return DecodedEvent::TokenApprove(self.token_approvals()[0].clone());
      }

      if self.decoded_events() == 1 && self.permits_len() == 1 {
         return DecodedEvent::Permit(self.permits()[0].clone());
      }

      if self.decoded_events() == 1 && self.eth_wraps_len() == 1 {
         return DecodedEvent::WrapETH(self.eth_wraps()[0].clone());
      }

      if self.decoded_events() == 1 && self.weth_unwraps_len() == 1 {
         return DecodedEvent::UnwrapWETH(self.weth_unwraps()[0].clone());
      }

      if self.decoded_events() == 1 && self.positions_ops_len() == 1 {
         return DecodedEvent::UniswapPositionOperation(self.positions_ops()[0].clone());
      }

      if self.bridges_len() == 1 {
         return DecodedEvent::Bridge(self.bridges()[0].clone());
      }

      if self.eoa_delegates_len() == 1 {
         return DecodedEvent::EOADelegate(self.eoa_delegates()[0].clone());
      }

      // Single swap hop
      if self.swaps_len() == 1 {
         return DecodedEvent::SwapToken(compose_single_swap(self));
      }

      // Multi-hop swap composition
      if self.swaps_len() > 1 {
         if let Some(params) = compose_multi_hop_swap(self) {
            return DecodedEvent::SwapToken(params);
         }
      }

      DecodedEvent::Other
   }

   /// Stored override from [`Self::set_main_event`], if any.
   fn main_event_override(&self) -> Option<DecodedEvent> {
      self.main_event_opt().cloned()
   }
}

fn compose_native_transfer(analysis: &TransactionAnalysis, ctx: ZeusCtx, chain: u64) -> DecodedEvent {
   let native: Currency = NativeCurrency::from(chain).into();
   let amount = NumericValue::format_wei(analysis.value, native.decimals());
   let amount_usd = ctx.get_currency_value_for_amount(amount.f64(), &native);

   DecodedEvent::Transfer(TransferParams {
      currency: native,
      amount,
      amount_usd: Some(amount_usd),
      real_amount_sent: None,
      real_amount_sent_usd: None,
      sender: analysis.sender,
      recipient: analysis.interact_to,
   })
}

fn compose_single_swap(analysis: &TransactionAnalysis) -> SwapParams {
   let erc20_transfers = analysis.erc20_transfers();
   let mut params = analysis.swaps()[0].clone();

   apply_eth_weth_abstraction(analysis, &mut params);

   if params.output_currency.is_erc20() {
      enrich_swap_received_from_transfers(analysis, &mut params, &erc20_transfers);
   }

   params
}

/// Fold multiple pool hops into one user-facing swap (first in → last out).
fn compose_multi_hop_swap(analysis: &TransactionAnalysis) -> Option<SwapParams> {
   let swaps = analysis.swaps();
   let swaps_len = swaps.len();
   if swaps_len < 2 {
      return None;
   }

   let erc20_transfers = analysis.erc20_transfers();
   let mut params = SwapParams {
      dapp: Dapp::Uniswap,
      sender: analysis.sender,
      ..Default::default()
   };

   for (i, swap) in swaps.iter().enumerate() {
      let is_first = i == 0;
      let is_last = i == swaps_len - 1;

      if is_first {
         let mut input = swap.input_currency.clone();
         if input.is_native_wrapped() && analysis.value > U256::ZERO {
            input = NativeCurrency::from(analysis.chain).into();
         }
         params.input_currency = input;
         params.amount_in = swap.amount_in.clone();
         params.amount_in_usd = swap.amount_in_usd.clone();
      }

      if is_last {
         let mut output = swap.output_currency.clone();
         if output.is_native_wrapped() && analysis.weth_unwraps_len() == 1 {
            output = NativeCurrency::from(analysis.chain).into();
         }
         params.output_currency = output;

         if swap.output_currency.is_erc20() {
            for transfer in erc20_transfers.iter() {
               if transfer.currency.address() == swap.output_currency.address() {
                  // Preserve historical condition (including operator precedence).
                  if !transfer.recipient == analysis.sender {
                     continue;
                  }
                  params.received = transfer.amount.clone();
                  params.received_usd = transfer.amount_usd.clone();
                  params.recipient = Some(transfer.recipient);
                  break;
               }
            }
         } else {
            params.received = swap.received.clone();
            params.received_usd = swap.received_usd.clone();
         }
      }
   }

   Some(params)
}

fn apply_eth_weth_abstraction(analysis: &TransactionAnalysis, params: &mut SwapParams) {
   if params.input_currency.is_native_wrapped() {
      if analysis.value > U256::ZERO {
         params.input_currency = NativeCurrency::from(analysis.chain).into();
      }
   }

   if params.output_currency.is_native_wrapped() && analysis.weth_unwraps_len() == 1 {
      params.output_currency = NativeCurrency::from(analysis.chain).into();
   }
}

fn enrich_swap_received_from_transfers(
   analysis: &TransactionAnalysis,
   params: &mut SwapParams,
   erc20_transfers: &[TransferParams],
) {
   for transfer in erc20_transfers.iter() {
      if transfer.currency.address() == params.output_currency.address() {
         // Preserve historical condition (including operator precedence).
         if !transfer.recipient == analysis.sender {
            continue;
         }
         params.received = transfer.amount.clone();
         params.received_usd = transfer.amount_usd.clone();
         params.recipient = Some(transfer.recipient);
         break;
      }
   }
}
