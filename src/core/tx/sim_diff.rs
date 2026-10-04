//! Measure signer balance / approval diffs from a simulated tx.
//!
//! Before-state is taken at the fork `block_id` (native from the pre-sim EVM,
//! ERC-20 / Permit2 via RPC). After-state is probed on the post-sim EVM —
//! amounts still come from `balanceOf` / `allowance`, not logs.

use super::approval_diff::{
   ApprovalCandidate, ApprovalChange, ApprovalDiff, ApprovalKind, collect_approval_candidates,
};
use super::balance_diff::{BalanceDiff, collect_token_candidates, native_change, token_change};
use crate::core::ZeusCtx;
use crate::utils::simulate::{
   AccountPrefetch, ForkPrefetch, ForkSim, ForkSimRequest, StoragePrefetch, eip7702_implementation,
   pinned_head, simulate_on_fork_with,
};
use alloy_eips::eip7702::SignedAuthorization;
use anyhow::anyhow;
use std::collections::HashMap;
use std::time::Instant;
use zeus_eth::{
   alloy_contract::private::Provider,
   alloy_primitives::{Address, Bytes, Log, U256},
   alloy_rpc_types::BlockId,
   revm_utils::{
      Database, Evm2,
      simulate::{erc20_allowance, erc20_balance, permit2_allowance},
   },
   types::ChainId,
   utils::{address_book, batch},
};

/// Max ERC-20 tokens per `getERC20Balance` eth_call so it stays under gas limits.
const TOKEN_BALANCE_BATCH: usize = 20;

/// Max `(token, spender)` allowance pairs per Multicall3 aggregate so the eth_call stays under gas limits.
const ALLOWANCE_PAIR_BATCH: usize = 20;

pub struct SimulatedTx {
   pub sim_res: zeus_eth::revm_utils::ExecutionResult,
   pub logs: Vec<Log>,
   pub balance_before: U256,
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
   expiration_before: Option<u64>,
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
   fn empty() -> Self {
      Self {
         tokens: HashMap::new(),
         erc20: HashMap::new(),
         permit2: HashMap::new(),
      }
   }

   fn is_empty_request(
      tokens: &[Address],
      erc20: &[(Address, Address)],
      permit2: &[(Address, Address)],
   ) -> bool {
      tokens.is_empty() && erc20.is_empty() && permit2.is_empty()
   }

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
      let (allow_before, expiration_before) = match cand.kind {
         ApprovalKind::Erc20 => {
            let Some(amount) = before.erc20.get(&(cand.token, cand.spender)).copied() else {
               continue;
            };
            (amount, None)
         }
         ApprovalKind::Permit2 => {
            let Some(&(amount, exp)) = before.permit2.get(&(cand.token, cand.spender)) else {
               continue;
            };
            (amount, Some(exp))
         }
      };
      if allow_before == allow_after && expiration_before == expiration_after {
         continue;
      }
      approval_deltas.push(ApprovalWei {
         cand: *cand,
         before: allow_before,
         after: allow_after,
         expiration_before,
         expiration_after,
      });
   }

   RawSimDiffs {
      tokens: token_deltas,
      approvals: approval_deltas,
   }
}

// ! This may return an empty or partial map if any requests fail
async fn fetch_token_before(
   ctx: ZeusCtx,
   chain: u64,
   from: Address,
   block_id: BlockId,
   tokens: Vec<Address>,
) -> HashMap<Address, U256> {
   if tokens.is_empty() {
      return HashMap::new();
   }

   let client = ctx.get_zeus_client();
   let mut out = HashMap::new();

   for chunk in tokens.chunks(TOKEN_BALANCE_BATCH) {
      let chunk = chunk.to_vec();
      match client
         .request(chain, |client| {
            let chunk = chunk.clone();
            async move {
               batch::get_erc20_balances(client, chain, Some(block_id), from, chunk).await
            }
         })
         .await
      {
         Ok(rows) => {
            for row in rows {
               out.insert(row.token, row.balance);
            }
         }
         Err(e) => {
            tracing::warn!("ERC-20 balances at block failed: {:?}", e);
         }
      }
   }

   out
}

// ! This may return an empty or partial map if any requests fail
async fn fetch_erc20_allowance_before(
   ctx: ZeusCtx,
   chain: u64,
   from: Address,
   block_id: BlockId,
   pairs: Vec<(Address, Address)>,
) -> HashMap<(Address, Address), U256> {
   if pairs.is_empty() {
      return HashMap::new();
   }

   let client = ctx.get_zeus_client();
   let mut out = HashMap::new();

   for chunk in pairs.chunks(ALLOWANCE_PAIR_BATCH) {
      let chunk = chunk.to_vec();
      match client
         .request(chain, |client| {
            let chunk = chunk.clone();
            async move { batch::get_erc20_allowances(client, from, chunk, Some(block_id)).await }
         })
         .await
      {
         Ok(rows) => {
            for (token, spender, amount) in rows {
               out.insert((token, spender), amount);
            }
         }
         Err(e) => {
            tracing::warn!("ERC-20 allowances at block failed: {:?}", e);
         }
      }
   }

   out
}

// ! This may return an empty or partial map if any requests fail
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
   let mut out = HashMap::new();

   for chunk in pairs.chunks(ALLOWANCE_PAIR_BATCH) {
      let chunk = chunk.to_vec();
      match client
         .request(chain, |client| {
            let chunk = chunk.clone();
            async move {
               batch::get_permit2_allowances(client, permit2, from, chunk, Some(block_id)).await
            }
         })
         .await
      {
         Ok(rows) => {
            for (token, spender, amount, expiration) in rows {
               out.insert((token, spender), (amount, expiration));
            }
         }
         Err(e) => {
            tracing::warn!("Multicall3 Permit2 allowances failed: {:?}", e);
         }
      }
   }

   out
}

async fn fetch_before_state(
   ctx: ZeusCtx,
   chain: u64,
   from: Address,
   block_id: BlockId,
   tokens: Vec<Address>,
   erc20_pairs: Vec<(Address, Address)>,
   permit2_pairs: Vec<(Address, Address)>,
) -> BeforeState {
   let time = Instant::now();

   let tokens_fut = fetch_token_before(ctx.clone(), chain, from, block_id, tokens);
   let erc20_fut = fetch_erc20_allowance_before(ctx.clone(), chain, from, block_id, erc20_pairs);
   let permit2_fut = fetch_permit2_before(ctx, chain, from, block_id, permit2_pairs);

   let (tokens, erc20, permit2) = tokio::join!(tokens_fut, erc20_fut, permit2_fut);

   tracing::info!(
      "fetch_before_state took {} ms",
      time.elapsed().as_millis()
   );

   BeforeState {
      tokens,
      erc20,
      permit2,
   }
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

   let eth_price = ctx.get_eth_price(chain);

   (
      BalanceDiff {
         native: native_change(chain, eth_price, native_before, native_after),
         tokens: token_changes,
         nfts: Vec::new(),
      },
      ApprovalDiff {
         changes: approval_changes,
         nft_changes: Vec::new(),
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

   let client = ctx.get_zeus_client();
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
   );
   let after_fut = fetch_before_state(
      ctx.clone(),
      chain,
      from,
      mined,
      tokens.clone(),
      erc20_pairs,
      permit2_pairs,
   );

   let (native_before, before, after) = tokio::join!(native_before_fut, before_fut, after_fut);
   let native_before = native_before?;

   let after_approvals = approvals_from_state(&candidates, &after);
   let raw = combine_diffs(
      &tokens,
      &candidates,
      before,
      after.tokens,
      after_approvals,
   );

   Ok(resolve_raw_diffs(ctx, chain, native_before, native_after, raw).await)
}

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
   known_erc20: Vec<(Address, Address)>,
   known_permit2: Vec<(Address, Address)>,
   permit2: Option<Address>,
   before_handle: Option<tokio::task::JoinHandle<BeforeState>>,
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

      #[cfg(feature = "dev")]
      {
         tracing::info!("Permit2 Pre {:?}", permit2_pre);
         tracing::info!("ERC20 Pre {:?}", erc20_pre);
      }

      let before_handle = if BeforeState::is_empty_request(&tokens_pre, &erc20_pre, &permit2_pre) {
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
         )))
      };

      Self {
         from,
         interact_to,
         call_data,
         tokens_pre,
         erc20_pre,
         permit2_pre,
         known_erc20,
         known_permit2,
         permit2,
         before_handle,
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

      let (erc20_pairs, permit2_pairs) = split_approval_pairs(&candidates);

      let extra_tokens = not_in(&tokens, &self.tokens_pre);
      let extra_erc20 = not_in(&erc20_pairs, &self.erc20_pre);
      let extra_permit2 = not_in(&permit2_pairs, &self.permit2_pre);

      let extras_handle =
         if BeforeState::is_empty_request(&extra_tokens, &extra_erc20, &extra_permit2) {
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
            )))
         };

      let time = Instant::now();
      let after_tokens = measure_token_after(self.from, &tokens, evm);
      let after_approvals = measure_approval_after(self.from, &candidates, self.permit2, evm);

      tracing::info!(
         "measure_after_diffs took {} ms",
         time.elapsed().as_millis()
      );

      MeasuredDiffs {
         before_handle: self.before_handle,
         tokens,
         candidates,
         after_tokens,
         after_approvals,
         extras_handle,
      }
   }
}

/// A [`DiffProbe`] measured against a finished simulation.
pub struct MeasuredDiffs {
   before_handle: Option<tokio::task::JoinHandle<BeforeState>>,
   tokens: Vec<Address>,
   candidates: Vec<ApprovalCandidate>,
   after_tokens: HashMap<Address, U256>,
   after_approvals: HashMap<(ApprovalKind, Address, Address), (U256, Option<u64>)>,
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
         before,
         self.after_tokens,
         self.after_approvals,
      );

      Ok(resolve_raw_diffs(ctx, chain.id(), native_before, native_after, raw).await)
   }
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
   let client = ctx.get_zeus_client();

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
   })
}

#[cfg(test)]
mod tests {
   use super::*;

   fn addr(b: u8) -> Address {
      Address::repeat_byte(b)
   }

   fn erc20_cand(token: Address, spender: Address) -> ApprovalCandidate {
      ApprovalCandidate {
         kind: ApprovalKind::Erc20,
         token,
         spender,
      }
   }

   fn permit2_cand(token: Address, spender: Address) -> ApprovalCandidate {
      ApprovalCandidate {
         kind: ApprovalKind::Permit2,
         token,
         spender,
      }
   }

   #[test]
   fn token_delta_is_emitted() {
      let token = addr(1);
      let mut before = BeforeState::empty();
      before.tokens.insert(token, U256::from(10u64));
      let mut after = HashMap::new();
      after.insert(token, U256::from(7u64));

      let raw = combine_diffs(&[token], &[], before, after, HashMap::new());
      assert_eq!(raw.tokens.len(), 1);
      assert_eq!(raw.tokens[0].before, U256::from(10u64));
      assert_eq!(raw.tokens[0].after, U256::from(7u64));
   }

   #[test]
   fn equal_token_wei_is_omitted() {
      let token = addr(1);
      let mut before = BeforeState::empty();
      before.tokens.insert(token, U256::from(5u64));
      let mut after = HashMap::new();
      after.insert(token, U256::from(5u64));

      let raw = combine_diffs(&[token], &[], before, after, HashMap::new());
      assert!(raw.tokens.is_empty());
   }

   #[test]
   fn missing_before_or_after_is_omitted() {
      let token = addr(1);
      let mut after_only = HashMap::new();
      after_only.insert(token, U256::from(1u64));
      let raw = combine_diffs(
         &[token],
         &[],
         BeforeState::empty(),
         after_only,
         HashMap::new(),
      );
      assert!(raw.tokens.is_empty());

      let mut before = BeforeState::empty();
      before.tokens.insert(token, U256::from(1u64));
      let raw = combine_diffs(
         &[token],
         &[],
         before,
         HashMap::new(),
         HashMap::new(),
      );
      assert!(raw.tokens.is_empty());
   }

   #[test]
   fn erc20_allowance_change_is_emitted() {
      let token = addr(1);
      let spender = addr(2);
      let cand = erc20_cand(token, spender);
      let mut before = BeforeState::empty();
      before.erc20.insert((token, spender), U256::ZERO);
      let mut after = HashMap::new();
      after.insert(
         (ApprovalKind::Erc20, token, spender),
         (U256::MAX, None),
      );

      let raw = combine_diffs(&[], &[cand], before, HashMap::new(), after);
      assert_eq!(raw.approvals.len(), 1);
      assert_eq!(raw.approvals[0].after, U256::MAX);
      assert!(raw.approvals[0].expiration_after.is_none());
   }

   #[test]
   fn permit2_amount_change_keeps_expiry() {
      let token = addr(1);
      let spender = addr(2);
      let cand = permit2_cand(token, spender);
      let mut before = BeforeState::empty();
      before.permit2.insert((token, spender), (U256::from(1u64), 100));
      let mut after = HashMap::new();
      after.insert(
         (ApprovalKind::Permit2, token, spender),
         (U256::from(2u64), Some(200)),
      );

      let raw = combine_diffs(&[], &[cand], before, HashMap::new(), after);
      assert_eq!(raw.approvals.len(), 1);
      assert_eq!(raw.approvals[0].expiration_before, Some(100));
      assert_eq!(raw.approvals[0].expiration_after, Some(200));
   }

   #[test]
   fn permit2_expiry_only_is_emitted() {
      let token = addr(1);
      let spender = addr(2);
      let cand = permit2_cand(token, spender);
      let mut before = BeforeState::empty();
      before.permit2.insert((token, spender), (U256::from(5u64), 100));
      let mut after = HashMap::new();
      after.insert(
         (ApprovalKind::Permit2, token, spender),
         (U256::from(5u64), Some(999)),
      );

      let raw = combine_diffs(&[], &[cand], before, HashMap::new(), after);
      assert_eq!(raw.approvals.len(), 1);
      assert_eq!(raw.approvals[0].before, U256::from(5u64));
      assert_eq!(raw.approvals[0].after, U256::from(5u64));
      assert_eq!(raw.approvals[0].expiration_after, Some(999));
   }

   #[test]
   fn permit2_unchanged_amount_and_expiry_is_omitted() {
      let token = addr(1);
      let spender = addr(2);
      let cand = permit2_cand(token, spender);
      let mut before = BeforeState::empty();
      before.permit2.insert((token, spender), (U256::from(5u64), 100));
      let mut after = HashMap::new();
      after.insert(
         (ApprovalKind::Permit2, token, spender),
         (U256::from(5u64), Some(100)),
      );

      let raw = combine_diffs(&[], &[cand], before, HashMap::new(), after);
      assert!(raw.approvals.is_empty());
   }
}
