//! Measure signer balance / approval diffs from a simulated tx.
//!
//! ERC-20 before-state comes from ZeusCtx / the approval manager. Permit2
//! before-state is batched over Multicall3 (allowances can expire). After-state
//! is probed on the post-sim EVM — amounts still come from `balanceOf` /
//! `allowance`, not logs.

use super::approval_diff::{
   ApprovalCandidate, ApprovalChange, ApprovalDiff, ApprovalKind, collect_approval_candidates,
};
use super::balance_diff::{BalanceDiff, collect_token_candidates, native_change, token_change};
use crate::core::ZeusCtx;
use crate::utils::simulate::{AccountPrefetch, fetch_accounts_info, simulate_transaction};
use alloy_eips::eip7702::SignedAuthorization;
use anyhow::anyhow;
use std::collections::HashMap;
use std::time::Instant;
use zeus_eth::{
   alloy_contract::private::Provider,
   alloy_primitives::{Address, Bytes, Log, U256},
   alloy_rpc_types::BlockId,
   revm_utils::{
      Database, Evm2, ForkFactory, Host, new_evm,
      simulate::{erc20_allowance, erc20_balance, permit2_allowance},
   },
   types::ChainId,
   utils::{address_book, batch},
};

/// Max `(token, spender)` pairs per Multicall3 aggregate so the eth_call stays under gas limits.
const PERMIT2_PAIR_BATCH: usize = 20;

pub struct SimulatedTx {
   pub sim_res: zeus_eth::revm_utils::ExecutionResult,
   pub logs: Vec<Log>,
   pub balance_after: U256,
   pub contract_interact: bool,
   pub balance_diff: BalanceDiff,
   pub approval_diff: ApprovalDiff,
}

struct TokenWei {
   token: Address,
   before: U256,
   after: U256,
}

struct ApprovalWei {
   cand: ApprovalCandidate,
   before: U256,
   after: U256,
   expiration_after: Option<u64>,
}

/// Wei-level deltas. Resolve with [`resolve_raw_diffs`] after dropping the EVMs.
pub struct RawSimDiffs {
   tokens: Vec<TokenWei>,
   approvals: Vec<ApprovalWei>,
}

struct BeforeState {
   tokens: HashMap<Address, U256>,
   erc20: HashMap<(Address, Address), U256>,
   permit2: HashMap<(Address, Address), (U256, u64)>,
}

impl BeforeState {
   fn merge(&mut self, other: Self) {
      self.tokens.extend(other.tokens);
      self.erc20.extend(other.erc20);
      self.permit2.extend(other.permit2);
   }
}

fn split_approval_pairs(
   candidates: &[ApprovalCandidate],
) -> (Vec<(Address, Address)>, Vec<(Address, Address)>) {
   let mut erc20 = Vec::new();
   let mut permit2 = Vec::new();
   for cand in candidates {
      match cand.kind {
         ApprovalKind::Erc20 => erc20.push((cand.token, cand.spender)),
         ApprovalKind::Permit2 => permit2.push((cand.token, cand.spender)),
      }
   }
   (erc20, permit2)
}

fn not_in<T: Copy + PartialEq>(full: &[T], pre: &[T]) -> Vec<T> {
   full.iter().copied().filter(|item| !pre.contains(item)).collect()
}

fn local_before_state(
   ctx: &ZeusCtx,
   chain: u64,
   from: Address,
   tokens: &[Address],
   erc20_pairs: &[(Address, Address)],
) -> BeforeState {
   let manager = ctx.approval_manager();
   let tokens = tokens
      .iter()
      .map(|&token| {
         (
            token,
            ctx.get_token_balance(chain, from, token).wei(),
         )
      })
      .collect();
   let erc20 = erc20_pairs
      .iter()
      .map(|&(token, spender)| {
         let amount = manager
            .get_token_approval(chain, from, token, spender)
            .map(|p| p.amount.wei())
            .unwrap_or(U256::ZERO);
         ((token, spender), amount)
      })
      .collect();
   BeforeState {
      tokens,
      erc20,
      permit2: HashMap::new(),
   }
}

fn known_approvals(
   ctx: &ZeusCtx,
   chain: u64,
   owner: Address,
) -> (Vec<(Address, Address)>, Vec<(Address, Address)>) {
   let known_erc20 = ctx
      .approval_manager()
      .get_token_approvals(chain, owner)
      .into_iter()
      .map(|p| (p.token.address, p.spender))
      .collect();
   let known_permit2 = ctx
      .approval_manager()
      .get_permits(chain, owner)
      .into_iter()
      .map(|p| (p.token.address(), p.spender))
      .collect();
   (known_erc20, known_permit2)
}

fn measure_token_after<DB: Database>(
   from: Address,
   tokens: &[Address],
   after_evm: &mut Evm2<DB>,
) -> HashMap<Address, U256> {
   let mut out = HashMap::new();
   for &token_addr in tokens {
      if let Ok(after) = erc20_balance(after_evm, token_addr, from) {
         out.insert(token_addr, after);
      }
   }
   out
}

fn measure_approval_after<DB: Database>(
   from: Address,
   candidates: &[ApprovalCandidate],
   permit2: Option<Address>,
   after_evm: &mut Evm2<DB>,
) -> HashMap<(ApprovalKind, Address, Address), (U256, Option<u64>)> {
   let mut out = HashMap::new();
   for cand in candidates {
      match cand.kind {
         ApprovalKind::Erc20 => {
            if let Ok(after) = erc20_allowance(after_evm, cand.token, from, cand.spender) {
               out.insert(
                  (cand.kind, cand.token, cand.spender),
                  (after, None),
               );
            }
         }
         ApprovalKind::Permit2 => {
            let Some(permit2) = permit2 else {
               continue;
            };
            if let Ok((after, expiration)) =
               permit2_allowance(after_evm, permit2, from, cand.token, cand.spender)
            {
               out.insert(
                  (cand.kind, cand.token, cand.spender),
                  (after, Some(expiration)),
               );
            }
         }
      }
   }
   out
}

fn combine_diffs(
   tokens: &[Address],
   candidates: &[ApprovalCandidate],
   before: BeforeState,
   after_tokens: HashMap<Address, U256>,
   after_approvals: HashMap<(ApprovalKind, Address, Address), (U256, Option<u64>)>,
) -> RawSimDiffs {
   let mut token_deltas = Vec::new();
   for &token in tokens {
      let Some(&token_before) = before.tokens.get(&token) else {
         continue;
      };
      let Some(&token_after) = after_tokens.get(&token) else {
         continue;
      };
      if token_before != token_after {
         token_deltas.push(TokenWei {
            token,
            before: token_before,
            after: token_after,
         });
      }
   }

   let mut approval_deltas = Vec::new();
   for cand in candidates {
      let Some(&(allow_after, expiration_after)) =
         after_approvals.get(&(cand.kind, cand.token, cand.spender))
      else {
         continue;
      };
      let allow_before = match cand.kind {
         ApprovalKind::Erc20 => before.erc20.get(&(cand.token, cand.spender)).copied(),
         ApprovalKind::Permit2 => before.permit2.get(&(cand.token, cand.spender)).map(|(a, _)| *a),
      };
      let Some(allow_before) = allow_before else {
         continue;
      };
      if allow_before != allow_after {
         approval_deltas.push(ApprovalWei {
            cand: *cand,
            before: allow_before,
            after: allow_after,
            expiration_after,
         });
      }
   }

   RawSimDiffs {
      tokens: token_deltas,
      approvals: approval_deltas,
   }
}

async fn fetch_permit2_before(
   ctx: ZeusCtx,
   chain: u64,
   from: Address,
   block_id: BlockId,
   pairs: Vec<(Address, Address)>,
) -> HashMap<(Address, Address), (U256, u64)> {
   if pairs.is_empty() {
      return HashMap::new();
   }
   let Some(permit2) = address_book::permit2_contract(chain).ok() else {
      return HashMap::new();
   };

   let client = ctx.get_zeus_client();

   let time = Instant::now();

   let map = match client
      .request(chain, |client| {
         let pairs = pairs.clone();
         async move {
            let mut out = Vec::new();
            for chunk in pairs.chunks(PERMIT2_PAIR_BATCH) {
               let rows = batch::get_permit2_allowances(
                  client.clone(),
                  permit2,
                  from,
                  chunk.to_vec(),
                  Some(block_id),
               )
               .await?;
               out.extend(rows);
            }
            Ok(out)
         }
      })
      .await
   {
      Ok(rows) => rows
         .into_iter()
         .map(|(token, spender, amount, expiration)| ((token, spender), (amount, expiration)))
         .collect(),
      Err(e) => {
         tracing::warn!("Multicall3 Permit2 allowances failed: {:?}", e);
         HashMap::new()
      }
   };

   tracing::info!(
      "fetch_permit2_before took {} ms",
      time.elapsed().as_millis()
   );

   map
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
      if let Some(change) = token_change(token, delta.before, delta.after) {
         token_changes.push(change);
      }
   }

   let mut approval_changes = Vec::new();
   for delta in raw.approvals {
      let Ok(token) = ctx.get_token(chain, delta.cand.token).await else {
         continue;
      };
      if let Some(change) = ApprovalChange::from_wei(
         delta.cand.kind,
         token,
         delta.cand.spender,
         delta.before,
         delta.after,
         delta.expiration_after,
      ) {
         approval_changes.push(change);
      }
   }

   (
      BalanceDiff {
         native: native_change(chain, native_before, native_after),
         tokens: token_changes,
      },
      ApprovalDiff {
         changes: approval_changes,
      },
   )
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
   balance_before: U256,
) -> Result<SimulatedTx, anyhow::Error> {
   let client = ctx.get_zeus_client();

   let block = client
      .request(chain.id(), |client| async move {
         client.get_block(BlockId::latest()).await.map_err(|e| anyhow!("{:?}", e))
      })
      .await?;

   let block = block.ok_or_else(|| anyhow!("No block found, this is usally a provider issue"))?;
   let block_id = BlockId::number(block.header.number);

   let portfolio = ctx.get_portfolio(chain.id(), from);
   let (known_erc20, known_permit2) = known_approvals(&ctx, chain.id(), from);
   let permit2 = address_book::permit2_contract(chain.id()).ok();

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

   #[cfg(feature = "dev")]
   {
      tracing::info!("Permit2 Pre {:?}", permit2_pre);
      tracing::info!("ERC20 Pre {:?}", erc20_pre);
   }

   let mut before = local_before_state(&ctx, chain.id(), from, &tokens_pre, &erc20_pre);

   let before_handle = if permit2_pre.is_empty() {
      None
   } else {
      Some(tokio::spawn(fetch_permit2_before(
         ctx.clone(),
         chain.id(),
         from,
         block_id,
         permit2_pre.clone(),
      )))
   };

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

   for token in portfolio.tokens() {
      accounts.push(AccountPrefetch::contract(token.address));
   }

   let accounts_info = fetch_accounts_info(ctx.clone(), chain.id(), block_id, accounts).await;
   let fork_client = ctx.get_client(chain.id()).await?;
   let mut factory =
      ForkFactory::new_sandbox_factory(fork_client, chain.id(), None, Some(block_id));

   for info in accounts_info {
      factory.insert_account_info(info.address, info.info);
   }

   let fork_db = factory.new_sandbox_fork();

   let (
      sim_res,
      balance_after,
      logs,
      tokens,
      candidates,
      after_tokens,
      after_approvals,
      extras_handle,
   ) = {
      let mut evm = new_evm(chain, Some(&block), fork_db);

      let time = Instant::now();
      let sim_res = simulate_transaction(
         &mut evm,
         from,
         interact_to,
         call_data.clone(),
         value,
         authorization_list,
      )?;

      tracing::info!(
         "simulate_transaction took {} ms",
         time.elapsed().as_millis()
      );

      let balance_after = evm.balance(from).map(|state| state.data).unwrap_or(U256::ZERO);
      let logs = sim_res.clone().into_logs();

      let tokens = collect_token_candidates(
         portfolio.tokens().iter().map(|t| t.address),
         interact_to,
         logs.iter().map(|log| log.address),
      );

      let candidates = collect_approval_candidates(
         from,
         interact_to,
         &call_data,
         &logs,
         known_erc20,
         known_permit2,
      );

      let (erc20_pairs, permit2_pairs) = split_approval_pairs(&candidates);

      let extra_tokens = not_in(&tokens, &tokens_pre);
      let extra_erc20 = not_in(&erc20_pairs, &erc20_pre);
      let extra_permit2 = not_in(&permit2_pairs, &permit2_pre);

      before.merge(local_before_state(
         &ctx,
         chain.id(),
         from,
         &extra_tokens,
         &extra_erc20,
      ));

      let extras_handle = if extra_permit2.is_empty() {
         None
      } else {
         Some(tokio::spawn(fetch_permit2_before(
            ctx.clone(),
            chain.id(),
            from,
            block_id,
            extra_permit2,
         )))
      };

      let time = Instant::now();
      let after_tokens = measure_token_after(from, &tokens, &mut evm);
      let after_approvals = measure_approval_after(from, &candidates, permit2, &mut evm);

      tracing::info!(
         "measure_after_diffs took {} ms",
         time.elapsed().as_millis()
      );

      (
         sim_res,
         balance_after,
         logs,
         tokens,
         candidates,
         after_tokens,
         after_approvals,
         extras_handle,
      )
   };

   if let Some(handle) = before_handle {
      let permit2 = handle.await.map_err(|e| anyhow!("permit2 before-state: {e}"))?;
      before.permit2.extend(permit2);
   }

   if let Some(handle) = extras_handle {
      let extra = handle.await.map_err(|e| anyhow!("permit2 before extras: {e}"))?;
      before.permit2.extend(extra);
   }

   let raw = combine_diffs(
      &tokens,
      &candidates,
      before,
      after_tokens,
      after_approvals,
   );

   let (balance_diff, approval_diff) = resolve_raw_diffs(
      ctx,
      chain.id(),
      balance_before,
      balance_after,
      raw,
   )
   .await;

   Ok(SimulatedTx {
      sim_res,
      logs,
      balance_after,
      contract_interact: !bytecode.is_empty(),
      balance_diff,
      approval_diff,
   })
}
