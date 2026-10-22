//! Probe a fork simulation in three steps, so the EVM never has to cross an await.

use super::before::fetch_before_state;
use super::probe::{combine_diffs, measure_approval_after, measure_nft_after, measure_token_after};
use super::resolve_raw_diffs;
use super::state::{
   AfterState, BeforeState, NftRequest, known_approvals, not_in, split_approval_pairs,
};
use crate::ZeusCtx;
use crate::sim::approval_diff::{ApprovalCandidate, ApprovalDiff, collect_approval_candidates};
use crate::sim::balance_diff::{BalanceDiff, collect_token_candidates};
use anyhow::anyhow;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Semaphore;
use zeus_eth::alloy_primitives::{Address, Bytes, Log, U256};
use zeus_eth::alloy_rpc_types::BlockId;
use zeus_eth::revm_utils::{Database, Evm2};
use zeus_eth::types::ChainId;
use zeus_eth::utils::address_book;

/// Signer diffs a fork simulation can produce, probed in three steps.
///
/// [`DiffProbe::start`] runs before simulating so the "before" RPC overlaps the EVM
/// work; [`DiffProbe::measure`] runs while the EVM is in scope, and has to be sync
/// because the EVM cannot be held across an `await` (it is not `Send`);
/// [`MeasuredDiffs::resolve`] turns the measurements into diffs once it is gone.
///
/// Splitting it this way is what lets a caller with its own fork
/// (`unshield_via_paymaster`, which forges its ephemeral smart account's EIP-7702
/// delegation) produce diffs without duplicating any of this.
pub struct DiffProbe {
   from: Address,
   interact_to: Address,
   call_data: Bytes,
   tokens_pre: Vec<Address>,
   erc20_pre: Vec<(Address, Address)>,
   permit2_pre: Vec<(Address, Address)>,
   nft_pre: NftRequest,
   known_erc20: Vec<(Address, Address)>,
   known_permit2: Vec<(Address, Address)>,
   permit2: Option<Address>,
   before_handle: Option<tokio::task::JoinHandle<BeforeState>>,
   semaphore: Arc<Semaphore>,
}

impl DiffProbe {
   /// Collect the candidates `call_data` implies and spawn the "before" fetch.
   pub fn start(
      ctx: ZeusCtx,
      chain: ChainId,
      block_id: BlockId,
      from: Address,
      interact_to: Address,
      call_data: Bytes,
   ) -> Self {
      let (known_erc20, known_permit2) = known_approvals(&ctx, chain.id(), from);
      let permit2 = address_book::permit2_contract(chain.id()).ok();
      let concurrency = ctx.get_client_manager().concurrency();
      let semaphore = Arc::new(Semaphore::new(concurrency));

      let portfolio = ctx.get_portfolio(chain.id(), from);

      let tokens_pre = collect_token_candidates(
         portfolio.tokens().iter().map(|t| t.address),
         interact_to,
         std::iter::empty(),
      );

      let candidates_pre = collect_approval_candidates(
         from,
         interact_to,
         &call_data,
         &[],
         known_erc20.clone(),
         known_permit2.clone(),
      );

      let (erc20_pre, permit2_pre) = split_approval_pairs(&candidates_pre);

      // No logs yet: they only exist once the simulation has run, and this pass is what overlaps it.
      let nft_pre = NftRequest::collect(
         &ctx,
         chain.id(),
         from,
         interact_to,
         &call_data,
         &[],
      );

      #[cfg(feature = "dev")]
      {
         tracing::info!("Permit2 Pre {:?}", permit2_pre);
         tracing::info!("ERC20 Pre {:?}", erc20_pre);
         tracing::info!("NFT Pre {:?}", nft_pre.balances);
      }

      let before_handle =
         if BeforeState::is_empty_request(&tokens_pre, &erc20_pre, &permit2_pre, &nft_pre) {
            None
         } else {
            Some(tokio::spawn(fetch_before_state(
               ctx,
               chain.id(),
               from,
               block_id,
               tokens_pre.clone(),
               erc20_pre.clone(),
               permit2_pre.clone(),
               nft_pre.clone(),
               semaphore.clone(),
            )))
         };

      Self {
         from,
         interact_to,
         call_data,
         tokens_pre,
         erc20_pre,
         permit2_pre,
         nft_pre,
         known_erc20,
         known_permit2,
         permit2,
         before_handle,
         semaphore,
      }
   }

   /// Measure what the post-simulation EVM knows. Spawns the extras fetch.
   pub fn measure<DB: Database>(
      self,
      ctx: ZeusCtx,
      chain: ChainId,
      block_id: BlockId,
      logs: &[Log],
      evm: &mut Evm2<DB>,
   ) -> MeasuredDiffs {
      let portfolio = ctx.get_portfolio(chain.id(), self.from);

      let tokens = collect_token_candidates(
         portfolio.tokens().iter().map(|t| t.address),
         self.interact_to,
         logs.iter().map(|log| log.address),
      );

      let candidates = collect_approval_candidates(
         self.from,
         self.interact_to,
         &self.call_data,
         logs,
         self.known_erc20,
         self.known_permit2,
      );

      let nft = NftRequest::collect(
         &ctx,
         chain.id(),
         self.from,
         self.interact_to,
         &self.call_data,
         logs,
      );

      let (erc20_pairs, permit2_pairs) = split_approval_pairs(&candidates);

      let extra_tokens = not_in(&tokens, &self.tokens_pre);
      let extra_erc20 = not_in(&erc20_pairs, &self.erc20_pre);
      let extra_permit2 = not_in(&permit2_pairs, &self.permit2_pre);
      let extra_nft = nft.extras(&self.nft_pre);

      let extras_handle = if BeforeState::is_empty_request(
         &extra_tokens,
         &extra_erc20,
         &extra_permit2,
         &extra_nft,
      ) {
         None
      } else {
         Some(tokio::spawn(fetch_before_state(
            ctx,
            chain.id(),
            self.from,
            block_id,
            extra_tokens,
            extra_erc20,
            extra_permit2,
            extra_nft,
            self.semaphore.clone(),
         )))
      };

      let time = Instant::now();
      let after_tokens = measure_token_after(self.from, &tokens, evm);
      let after_approvals = measure_approval_after(self.from, &candidates, self.permit2, evm);
      let after_nft = measure_nft_after(self.from, &nft, evm);

      tracing::info!(
         "measure_after_diffs took {} ms",
         time.elapsed().as_millis()
      );

      MeasuredDiffs {
         before_handle: self.before_handle,
         tokens,
         candidates,
         nft,
         after: AfterState {
            tokens: after_tokens,
            approvals: after_approvals,
            nft: after_nft,
         },
         extras_handle,
      }
   }
}

/// A [`DiffProbe`] measured against a finished simulation.
pub struct MeasuredDiffs {
   before_handle: Option<tokio::task::JoinHandle<BeforeState>>,
   tokens: Vec<Address>,
   candidates: Vec<ApprovalCandidate>,
   nft: NftRequest,
   after: AfterState,
   extras_handle: Option<tokio::task::JoinHandle<BeforeState>>,
}

impl MeasuredDiffs {
   /// Await the "before" fetches and turn them into signer diffs.
   pub async fn resolve(
      self,
      ctx: ZeusCtx,
      chain: ChainId,
      native_before: U256,
      native_after: U256,
   ) -> Result<(BalanceDiff, ApprovalDiff), anyhow::Error> {
      let mut before = BeforeState::empty();

      if let Some(handle) = self.before_handle {
         let pre = handle.await.map_err(|e| anyhow!("before-state: {e}"))?;
         before.merge(pre);
      }

      if let Some(handle) = self.extras_handle {
         let extra = handle.await.map_err(|e| anyhow!("before-state extras: {e}"))?;
         before.merge(extra);
      }

      let raw = combine_diffs(
         &self.tokens,
         &self.candidates,
         &self.nft,
         before,
         self.after,
      );

      Ok(resolve_raw_diffs(ctx, chain.id(), native_before, native_after, raw).await)
   }
}
