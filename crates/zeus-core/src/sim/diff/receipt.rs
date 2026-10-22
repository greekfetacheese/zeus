//! Diffs for a transaction that has already been mined, read from its receipt's logs.

use super::before::fetch_before_state;
use super::probe::combine_diffs;
use super::state::{AfterState, NftRequest, known_approvals, split_approval_pairs};
use super::{approvals_from_state, resolve_raw_diffs};
use crate::ZeusCtx;
use crate::sim::approval_diff::{ApprovalDiff, collect_approval_candidates};
use crate::sim::balance_diff::{BalanceDiff, collect_token_candidates};
use anyhow::anyhow;
use std::sync::Arc;
use tokio::sync::Semaphore;
use zeus_eth::alloy_contract::private::Provider;
use zeus_eth::alloy_primitives::{Address, Bytes, Log, U256};
use zeus_eth::alloy_rpc_types::BlockId;

/// Signer diffs from `balanceOf` / `allowance` at `tx_block - 1` vs `tx_block`.
///
/// Log values are still ignored. Candidates come from receipt logs the same
/// way sim does. Other txs in the same block for this signer can land in the
/// delta — unusual for a wallet send.
pub async fn diffs_from_receipt(
   ctx: ZeusCtx,
   chain: u64,
   from: Address,
   interact_to: Address,
   call_data: &Bytes,
   logs: &[Log],
   tx_block: u64,
   native_after: U256,
) -> Result<(BalanceDiff, ApprovalDiff), anyhow::Error> {
   if tx_block == 0 {
      return Err(anyhow!("no tx block for receipt diffs"));
   }

   let parent = BlockId::number(tx_block - 1);
   let mined = BlockId::number(tx_block);

   let portfolio = ctx.get_portfolio(chain, from);
   let (known_erc20, known_permit2) = known_approvals(&ctx, chain, from);

   let tokens = collect_token_candidates(
      portfolio.tokens().iter().map(|t| t.address),
      interact_to,
      logs.iter().map(|log| log.address),
   );
   let candidates = collect_approval_candidates(
      from,
      interact_to,
      call_data,
      logs,
      known_erc20,
      known_permit2,
   );
   let (erc20_pairs, permit2_pairs) = split_approval_pairs(&candidates);

   let nft = NftRequest::collect(&ctx, chain, from, interact_to, call_data, logs);

   let client = ctx.get_client_manager();
   let semaphore = Arc::new(Semaphore::new(client.concurrency()));
   let native_before_fut = client.request(chain, |client| async move {
      client.get_balance(from).block_id(parent).await.map_err(|e| anyhow!("{:?}", e))
   });

   let before_fut = fetch_before_state(
      ctx.clone(),
      chain,
      from,
      parent,
      tokens.clone(),
      erc20_pairs.clone(),
      permit2_pairs.clone(),
      nft.clone(),
      semaphore.clone(),
   );
   let after_fut = fetch_before_state(
      ctx.clone(),
      chain,
      from,
      mined,
      tokens.clone(),
      erc20_pairs,
      permit2_pairs,
      nft.clone(),
      semaphore.clone(),
   );

   let (native_before, before, after) = tokio::join!(native_before_fut, before_fut, after_fut);
   let native_before = native_before?;

   let after_approvals = approvals_from_state(&candidates, &after);
   let after_state = AfterState {
      tokens: after.tokens,
      approvals: after_approvals,
      // Both blocks are read with the same request, so the mined block's NFT maps are exactly what
      // the fork path would have measured on the EVM — no second reading path to keep in step.
      nft: after.nft,
   };

   let raw = combine_diffs(&tokens, &candidates, &nft, before, after_state);

   Ok(resolve_raw_diffs(ctx, chain, native_before, native_after, raw).await)
}
