//! Log decode pipeline: topic0 dispatch + multi-log phase.

mod topic;

use super::{DecodedEvent, EOADelegateParams, UniswapPositionParams};
use crate::core::ZeusCtx;
use alloy_eips::eip7702::SignedAuthorization;
use zeus_eth::alloy_primitives::{Address, Bytes, Log, U256};

/// Shared context for all log decoders.
#[derive(Clone)]
pub struct DecodeCtx {
   pub ctx: ZeusCtx,
   pub chain: u64,
   pub sender: Address,
   pub interact_to: Address,
   pub call_data: Bytes,
   pub value: U256,
}

impl DecodeCtx {
   pub fn new(
      ctx: ZeusCtx,
      chain: u64,
      sender: Address,
      interact_to: Address,
      call_data: Bytes,
      value: U256,
   ) -> Self {
      Self {
         ctx,
         chain,
         sender,
         interact_to,
         call_data,
         value,
      }
   }
}

/// Result of attempting to decode one log via the topic0 table.
#[derive(Debug)]
pub enum DecodeOutcome {
   /// No decoder claimed this log.
   None,
   /// Single decoded event.
   One {
      event: DecodedEvent,
      /// Whether this contributes to `known_events`.
      counts_as_known: bool,
   },
   /// Multiple events from one log (e.g. Railgun Shield commitments).
   Many {
      events: Vec<DecodedEvent>,
      counts_as_known: bool,
   },
}

/// Run the full decode pipeline over auth list + logs.
pub async fn decode_transaction(
   dctx: &DecodeCtx,
   logs: &[Log],
   auth_list: Vec<SignedAuthorization>,
) -> (Vec<DecodedEvent>, usize) {
   let mut decoded_events = Vec::new();
   let mut known_events = 0usize;

   for auth in auth_list {
      let params = EOADelegateParams::new(dctx.chain, dctx.sender, auth);
      decoded_events.push(DecodedEvent::EOADelegate(params));
   }

   // Phase 1: single-log topic0 / ordered dispatch
   for log in logs {
      match topic::decode_log(dctx, log).await {
         DecodeOutcome::None => {}
         DecodeOutcome::One {
            event,
            counts_as_known,
         } => {
            if let DecodedEvent::Transfer(ref p) = event {
               if p.is_erc20_transfer() && counts_as_known {
                  known_events += 1;
               }
            } else if counts_as_known {
               known_events += 1;
            }
            decoded_events.push(event);
         }
         DecodeOutcome::Many {
            events,
            counts_as_known,
         } => {
            if counts_as_known {
               known_events += 1;
            }
            decoded_events.extend(events);
         }
      }
   }

   // Phase 2: multi-log Uniswap position ops (add/decrease liquidity).
   // Previous code scanned the full log slice from inside the per-log loop;
   // run once here so we don't depend on loop order.
   if let Ok(params) = UniswapPositionParams::add_liquidity_for_v3_from_logs(
      dctx.ctx.clone(),
      dctx.chain,
      dctx.sender,
      logs,
   )
   .await
   {
      decoded_events.push(DecodedEvent::UniswapPositionOperation(params));
      known_events += 1;
   }

   if let Ok(params) = UniswapPositionParams::decrease_liquidity_for_v3_from_logs(
      dctx.ctx.clone(),
      dctx.chain,
      dctx.sender,
      logs,
   )
   .await
   {
      decoded_events.push(DecodedEvent::UniswapPositionOperation(params));
      known_events += 1;
   }

   (decoded_events, known_events)
}
