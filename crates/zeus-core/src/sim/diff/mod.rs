//! Measure signer balance / approval diffs from a simulated tx.
//!
//! Before-state is taken at the fork `block_id` (native from the pre-sim EVM,
//! ERC-20 / Permit2 via RPC). After-state is probed on the post-sim EVM —
//! amounts still come from `balanceOf` / `allowance`, not logs.

use crate::ZeusCtx;
use crate::sim::approval_diff::{
   ApprovalCandidate, ApprovalChange, ApprovalDiff, ApprovalKind, NftApprovalCandidate,
   NftApprovalChange, NftApprovalValue,
};
use crate::sim::balance_diff::{BalanceDiff, NftBalanceChange, native_change, token_change};
use crate::sim::{
   AccountPrefetch, ForkPrefetch, ForkSim, ForkSimRequest, StoragePrefetch, eip7702_implementation,
   pinned_head, simulate_on_fork_with,
};
use alloy_eips::eip7702::SignedAuthorization;
use anyhow::anyhow;
use state::{BeforeState, nft_standard};
use std::collections::HashMap;
use zeus_eth::alloy_contract::private::Provider;
use zeus_eth::{
   alloy_primitives::{Address, Bytes, Log, U256},
   alloy_rpc_types::BlockId,
   types::ChainId,
};

mod before;
mod fork;
mod probe;
mod receipt;
mod state;

#[cfg(test)]
mod tests;

pub use fork::{DiffProbe, MeasuredDiffs};
pub use receipt::diffs_from_receipt;

pub struct SimulatedTx {
   pub sim_res: zeus_eth::revm_utils::ExecutionResult,
   pub logs: Vec<Log>,
   pub balance_before: U256,
   pub balance_after: U256,
   pub contract_interact: bool,
   pub balance_diff: BalanceDiff,
   pub approval_diff: ApprovalDiff,
   /// The block the fork was pinned to (see [`ForkSim::block_number`]).
   pub block_number: u64,
}

pub(super) struct TokenWei {
   pub(super) token: Address,
   pub(super) before: U256,
   pub(super) after: U256,
}

pub(super) struct ApprovalWei {
   pub(super) cand: ApprovalCandidate,
   pub(super) before: U256,
   pub(super) after: U256,
   pub(super) expiration_before: Option<u64>,
   pub(super) expiration_after: Option<u64>,
}

/// An NFT approval delta before the standard is known.
///
/// The values are already final: what is missing is only which standard the row's collection is,
/// which needs the catalog and so cannot happen while the EVMs are still in scope.
pub(super) struct NftApprovalWei {
   pub(super) cand: NftApprovalCandidate,
   pub(super) before: NftApprovalValue,
   pub(super) after: NftApprovalValue,
}

/// Wei-level deltas. Resolve with [`resolve_raw_diffs`] after dropping the EVMs.
pub struct RawSimDiffs {
   pub(super) tokens: Vec<TokenWei>,
   pub(super) approvals: Vec<ApprovalWei>,
   /// NFT ownership changes, already in their final shape: an NFT row has no decimals and no price,
   /// so there is nothing left to resolve.
   pub(super) nft_balances: Vec<NftBalanceChange>,
   pub(super) nft_approvals: Vec<NftApprovalWei>,
}

pub async fn resolve_raw_diffs(
   ctx: ZeusCtx,
   chain: u64,
   native_before: U256,
   native_after: U256,
   raw: RawSimDiffs,
) -> (BalanceDiff, ApprovalDiff) {
   let mut token_changes = Vec::new();
   for delta in raw.tokens {
      let Ok(token) = ctx.get_token(chain, delta.token).await else {
         continue;
      };

      let price = ctx.get_token_price(&token);

      if let Some(change) = token_change(token, price, delta.before, delta.after) {
         token_changes.push(change);
      }
   }

   let mut approval_changes = Vec::new();
   for delta in raw.approvals {
      let Ok(token) = ctx.get_token(chain, delta.cand.token).await else {
         continue;
      };

      let price = ctx.get_token_price(&token);

      if let Some(change) = ApprovalChange::from_wei(
         delta.cand.kind,
         token,
         delta.cand.spender,
         delta.before,
         delta.after,
         price,
         delta.expiration_before,
         delta.expiration_after,
      ) {
         approval_changes.push(change);
      }
   }

   let mut nft_changes = Vec::new();
   for delta in raw.nft_approvals {
      let standard = nft_standard(&ctx, chain, &delta.cand);

      if let Some(change) = NftApprovalChange::from_state(
         delta.cand.collection,
         delta.cand.operator,
         delta.cand.target,
         standard,
         delta.before,
         delta.after,
      ) {
         nft_changes.push(change);
      }
   }

   let eth_price = ctx.get_eth_price(chain);

   (
      BalanceDiff {
         native: native_change(chain, eth_price, native_before, native_after),
         tokens: token_changes,
         // Nothing to resolve: an NFT row is final the moment it is measured — no decimals, no
         // price, and the standard came in with the candidate.
         nfts: raw.nft_balances,
      },
      ApprovalDiff {
         changes: approval_changes,
         nft_changes,
      },
   )
}

fn approvals_from_state(
   candidates: &[ApprovalCandidate],
   state: &BeforeState,
) -> HashMap<(ApprovalKind, Address, Address), (U256, Option<u64>)> {
   let mut out = HashMap::new();
   for cand in candidates {
      match cand.kind {
         ApprovalKind::Erc20 => {
            if let Some(&amount) = state.erc20.get(&(cand.token, cand.spender)) {
               out.insert(
                  (cand.kind, cand.token, cand.spender),
                  (amount, None),
               );
            }
         }
         ApprovalKind::Permit2 => {
            if let Some(&(amount, expiration)) = state.permit2.get(&(cand.token, cand.spender)) {
               out.insert(
                  (cand.kind, cand.token, cand.spender),
                  (amount, Some(expiration)),
               );
            }
         }
      }
   }
   out
}

/// Fork, simulate, and attach signer balance / approval diffs.
pub async fn simulate_and_diff(
   ctx: ZeusCtx,
   chain: ChainId,
   from: Address,
   interact_to: Address,
   call_data: Bytes,
   value: U256,
   authorization_list: Vec<SignedAuthorization>,
) -> Result<SimulatedTx, anyhow::Error> {
   let client = ctx.get_client_manager();

   let (block, block_id) = pinned_head(ctx.clone(), chain, BlockId::latest()).await?;

   let probe = DiffProbe::start(
      ctx.clone(),
      chain,
      block_id,
      from,
      interact_to,
      call_data.clone(),
   );

   let bytecode = client
      .request(chain.id(), |client| async move {
         client.get_code_at(interact_to).await.map_err(|e| anyhow!("{:?}", e))
      })
      .await?;

   let interact_prefetch = if bytecode.is_empty() {
      AccountPrefetch::eoa(interact_to)
   } else {
      AccountPrefetch::contract(interact_to)
   };

   let mut accounts = vec![
      AccountPrefetch::eoa(from),
      interact_prefetch,
      AccountPrefetch::eoa(block.header.beneficiary),
   ];

   for auth in &authorization_list {
      accounts.push(AccountPrefetch::contract(auth.address));
   }

   if let Some(implementation) = eip7702_implementation(&bytecode) {
      accounts.push(AccountPrefetch::contract(implementation));
   }

   let portfolio = ctx.get_portfolio(chain.id(), from);
   for token in portfolio.tokens() {
      accounts.push(AccountPrefetch::contract(token.address));
   }

   let (sim, measured) = simulate_on_fork_with(
      ctx.clone(),
      chain,
      ForkPrefetch::new(block, accounts, StoragePrefetch::None),
      ForkSimRequest {
         from,
         interact_to,
         call_data: call_data.clone(),
         value,
         gas_limit: None,
         authorization_list,
      },
      |evm, sim| probe.measure(ctx.clone(), chain, block_id, &sim.logs, evm),
   )
   .await?;

   let ForkSim {
      sim_res,
      logs,
      balance_before,
      balance_after,
      block_number,
   } = sim;

   let (balance_diff, approval_diff) =
      measured.resolve(ctx, chain, balance_before, balance_after).await?;

   Ok(SimulatedTx {
      sim_res,
      logs,
      balance_before,
      balance_after,
      contract_interact: !bytecode.is_empty(),
      balance_diff,
      approval_diff,
      block_number,
   })
}
