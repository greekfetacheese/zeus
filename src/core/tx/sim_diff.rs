//! Measure signer balance / approval diffs from a simulated tx.
//!
//! Before-state is taken at the fork `block_id` (native from the pre-sim EVM,
//! ERC-20 / Permit2 via RPC). After-state is probed on the post-sim EVM —
//! amounts still come from `balanceOf` / `allowance`, not logs.

use super::approval_diff::{
   ApprovalCandidate, ApprovalChange, ApprovalDiff, ApprovalKind, NftApprovalCandidate,
   NftApprovalChange, NftApprovalTarget, NftApprovalValue, collect_approval_candidates,
   collect_nft_approval_candidates,
};
use super::balance_diff::{
   BalanceDiff, NftBalanceChange, NftCandidate, collect_nft_balance_candidates,
   collect_token_candidates, native_change, token_change,
};
use crate::core::{NftApproveParams, ZeusCtx};
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
   nft::NftStandard,
   revm_utils::{
      Database, Evm2,
      simulate::{
         erc20_allowance, erc20_balance, erc721_get_approved, erc721_is_approved_for_all,
         erc721_owner_of, erc1155_allowance, erc1155_balance_of, permit2_allowance,
      },
   },
   types::ChainId,
   utils::{address_book, batch, batch::NftRef},
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

/// An NFT approval delta before the standard is known.
///
/// The values are already final: what is missing is only which standard the row's collection is,
/// which needs the catalog and so cannot happen while the EVMs are still in scope.
struct NftApprovalWei {
   cand: NftApprovalCandidate,
   before: NftApprovalValue,
   after: NftApprovalValue,
}

/// Wei-level deltas. Resolve with [`resolve_raw_diffs`] after dropping the EVMs.
pub struct RawSimDiffs {
   tokens: Vec<TokenWei>,
   approvals: Vec<ApprovalWei>,
   /// NFT ownership changes, already in their final shape: an NFT row has no decimals and no price,
   /// so there is nothing left to resolve.
   nft_balances: Vec<NftBalanceChange>,
   nft_approvals: Vec<NftApprovalWei>,
}

/// The NFT side of a measured state: what the signer owns, and the approval state of every candidate.
///
/// Both ownership maps hold a **count of what the signer has** — `0`/`1` for ERC-721, where
/// `ownerOf` answers with an address that is either theirs or not, and the real count for ERC-1155.
/// Normalizing at the probe is what lets the two be compared the same way, and the row only ever
/// asks "did the signer gain or lose it".
#[derive(Default)]
struct NftState {
   /// ERC-721 ownership per `(collection, id)`, as `0` or `1`.
   ownership721: HashMap<(Address, U256), U256>,
   /// ERC-1155 balance per `(collection, id)`.
   balances1155: HashMap<(Address, U256), U256>,
   /// Measured approval state, per candidate. A missing key means the probe did not answer.
   approvals: HashMap<NftApprovalCandidate, NftApprovalValue>,
}

impl NftState {
   fn merge(&mut self, other: Self) {
      self.ownership721.extend(other.ownership721);
      self.balances1155.extend(other.balances1155);
      self.approvals.extend(other.approvals);
   }
}

/// The NFT candidates to read for a `(call_data, logs)` pair, before anything is measured.
///
/// Its own type because the fork path computes this twice — once before simulating and again with
/// the logs the simulation produced — and subtracts the two to find what still has to be read.
#[derive(Clone, Default)]
struct NftRequest {
   balances: Vec<NftCandidate>,
   approvals: Vec<NftApprovalCandidate>,
}

impl NftRequest {
   /// Everything `call_data` and `logs` imply for the signer's NFTs.
   fn collect(
      ctx: &ZeusCtx,
      chain: u64,
      from: Address,
      interact_to: Address,
      call_data: &Bytes,
      logs: &[Log],
   ) -> Self {
      let portfolio = ctx.get_portfolio(chain, from);
      let held = portfolio.nfts().iter().map(|token| NftCandidate {
         collection: token.collection,
         token_id: token.token_id,
         standard: token.standard,
      });

      let known = ctx
         .approval_manager()
         .get_nft_approvals(chain, from)
         .into_iter()
         .filter_map(|params| nft_approval_candidate(&params));

      Self {
         balances: collect_nft_balance_candidates(from, logs, held),
         approvals: collect_nft_approval_candidates(from, interact_to, call_data, logs, known),
      }
   }

   fn is_empty(&self) -> bool {
      self.balances.is_empty() && self.approvals.is_empty()
   }

   /// The candidates `self` asks for that `pre` did not already ask for.
   fn extras(&self, pre: &Self) -> Self {
      Self {
         balances: not_in(&self.balances, &pre.balances),
         approvals: not_in(&self.approvals, &pre.approvals),
      }
   }
}

/// The candidate an in-app NFT approval stands for.
///
/// Translating, not guessing: `NftApproveParams`'s three `Option` fields *are* the three shapes, and
/// the decoders never produce a fourth combination — so `None` here would mean a shape that cannot
/// exist, and the approval is simply left out rather than probed as something it is not.
fn nft_approval_candidate(params: &NftApproveParams) -> Option<NftApprovalCandidate> {
   let target = match (params.token_id, params.approved, params.amount) {
      (Some(id), None, None) => NftApprovalTarget::Token(id),
      (None, Some(_), None) => NftApprovalTarget::ForAll,
      (Some(id), None, Some(_)) => NftApprovalTarget::Allowance(id),
      _ => return None,
   };

   Some(NftApprovalCandidate {
      collection: params.collection,
      operator: params.operator,
      target,
   })
}

/// Which standard an approval row's collection is.
///
/// Two of the three shapes answer it themselves — a per-token approval is ERC-721's and an
/// allowance is ERC-5216's, an ERC-1155 extension. `ApprovalForAll` is the one shape both standards
/// share, so only the catalog can say, and an unknown collection reads as ERC-721: it is the older
/// standard, and its collection-wide approval means the same thing.
fn nft_standard(ctx: &ZeusCtx, chain: u64, candidate: &NftApprovalCandidate) -> NftStandard {
   match candidate.target {
      NftApprovalTarget::Token(_) => NftStandard::Erc721,
      NftApprovalTarget::Allowance(_) => NftStandard::Erc1155,
      NftApprovalTarget::ForAll => ctx
         .read(|ctx| ctx.nft_db.get_collection(chain, candidate.collection))
         .map(|collection| collection.standard)
         .unwrap_or(NftStandard::Erc721),
   }
}

struct BeforeState {
   tokens: HashMap<Address, U256>,
   erc20: HashMap<(Address, Address), U256>,
   permit2: HashMap<(Address, Address), (U256, u64)>,
   nft: NftState,
}

impl BeforeState {
   fn empty() -> Self {
      Self {
         tokens: HashMap::new(),
         erc20: HashMap::new(),
         permit2: HashMap::new(),
         nft: NftState::default(),
      }
   }

   fn is_empty_request(
      tokens: &[Address],
      erc20: &[(Address, Address)],
      permit2: &[(Address, Address)],
      nft: &NftRequest,
   ) -> bool {
      tokens.is_empty() && erc20.is_empty() && permit2.is_empty() && nft.is_empty()
   }

   fn merge(&mut self, other: Self) {
      self.tokens.extend(other.tokens);
      self.erc20.extend(other.erc20);
      self.permit2.extend(other.permit2);
      self.nft.merge(other.nft);
   }
}

/// The measured "after" side of a diff, in the same shape as [`BeforeState`], so the two can be
/// combined without either side knowing which path produced it.
struct AfterState {
   tokens: HashMap<Address, U256>,
   approvals: HashMap<(ApprovalKind, Address, Address), (U256, Option<u64>)>,
   nft: NftState,
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

/// The NFT half of the post-sim state, read on the fork EVM.
///
/// A read that failed reads as the shape's own *nothing* rather than as a hole in the map — the same
/// conclusion the before-state fetch draws from a dropped sub-call. `combine_diffs` drops a candidate
/// whose either side is missing, so a hole here would silently lose a real change: a burn's `ownerOf`
/// reverts, and the change would never reach the confirmation window.
fn measure_nft_after<DB: Database>(
   owner: Address,
   nft: &NftRequest,
   after_evm: &mut Evm2<DB>,
) -> NftState {
   let mut state = NftState::default();

   for candidate in &nft.balances {
      let key = (candidate.collection, candidate.token_id);
      match candidate.standard {
         NftStandard::Erc721 => {
            // A reverted `ownerOf` after the tx means the token no longer exists — "not the signer's",
            // which is exactly how the before-state fetch reads the same revert (`owner == Some(from)`
            // with a dropped sub-call). Leaving the key out instead keeps that reading asymmetric:
            // `combine_diffs` drops a candidate whose *either* side is missing, so a burn would lose the
            // row rather than compare it.
            let owner_of = erc721_owner_of(
               after_evm,
               candidate.collection,
               candidate.token_id,
            );
            state.ownership721.insert(
               key,
               U256::from(u8::from(
                  owner_of.is_ok_and(|owner_of| owner_of == owner),
               )),
            );
         }
         NftStandard::Erc1155 => {
            // The same reading the ERC-721 branch above applies to `ownerOf`: a probe that failed is the
            // shape's own *nothing* — an id this contract never held — exactly as the before-state fetch
            // reads a dropped sub-call (`unwrap_or_default`). Leaving the key out instead would drop the
            // row in `combine_diffs`, which is the outcome the note on `measure_nft_after` argues against.
            let balance = erc1155_balance_of(
               after_evm,
               candidate.collection,
               owner,
               candidate.token_id,
            )
            .unwrap_or_default();

            state.balances1155.insert(key, balance);
         }
      }
   }

   for candidate in &nft.approvals {
      // A probe that failed on the fork reads as the shape's own *nothing* — the zero address, `false`,
      // zero — the same default the before-state fetch applies to a dropped sub-call
      // (`unwrap_or_default` there too). Dropping the candidate instead is asymmetric: a token burnt in
      // the tx reverts `getApproved`, and the before side already answered "no operator" for it, so the
      // row would be lost rather than compared.
      let value = match candidate.target {
         NftApprovalTarget::Token(id) => NftApprovalValue::Approved(
            erc721_get_approved(after_evm, candidate.collection, id).unwrap_or_default(),
         ),
         NftApprovalTarget::ForAll => NftApprovalValue::ForAll(
            erc721_is_approved_for_all(
               after_evm,
               candidate.collection,
               owner,
               candidate.operator,
            )
            .unwrap_or_default(),
         ),
         NftApprovalTarget::Allowance(id) => NftApprovalValue::Allowance(
            erc1155_allowance(
               after_evm,
               candidate.collection,
               owner,
               candidate.operator,
               id,
            )
            .unwrap_or_default(),
         ),
      };

      state.approvals.insert(*candidate, value);
   }

   state
}

fn combine_diffs(
   tokens: &[Address],
   candidates: &[ApprovalCandidate],
   nft: &NftRequest,
   before: BeforeState,
   after: AfterState,
) -> RawSimDiffs {
   let after_tokens = after.tokens;
   let after_approvals = after.approvals;
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

   // NFT ownership: both standards answer the same question — how many the signer has — so they
   // differ only in which map holds the answer.
   let mut nft_balances = Vec::new();
   for candidate in &nft.balances {
      let key = (candidate.collection, candidate.token_id);
      let (Some(&before_count), Some(&after_count)) = (
         match candidate.standard {
            NftStandard::Erc721 => before.nft.ownership721.get(&key),
            NftStandard::Erc1155 => before.nft.balances1155.get(&key),
         },
         match candidate.standard {
            NftStandard::Erc721 => after.nft.ownership721.get(&key),
            NftStandard::Erc1155 => after.nft.balances1155.get(&key),
         },
      ) else {
         // One side never answered, so there is nothing to compare — the same rule the fungible rows
         // follow.
         continue;
      };

      if let Some(change) = NftBalanceChange::new(
         candidate.collection,
         candidate.token_id,
         candidate.standard,
         before_count,
         after_count,
      ) {
         nft_balances.push(change);
      }
   }

   let mut nft_approvals = Vec::new();
   for candidate in &nft.approvals {
      let (Some(&before_value), Some(&after_value)) = (
         before.nft.approvals.get(candidate),
         after.nft.approvals.get(candidate),
      ) else {
         continue;
      };

      // Only a change is a row — the same rule the fungible approvals follow.
      if before_value == after_value {
         continue;
      }

      nft_approvals.push(NftApprovalWei {
         cand: *candidate,
         before: before_value,
         after: after_value,
      });
   }

   RawSimDiffs {
      tokens: token_deltas,
      approvals: approval_deltas,
      nft_balances,
      nft_approvals,
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

   let client = ctx.get_client_manager();
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

   let client = ctx.get_client_manager();
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

   let client = ctx.get_client_manager();
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

/// Max NFT `(collection, id)` refs per Multicall3 aggregate so the eth_call stays under gas limits.
const NFT_REF_BATCH: usize = 20;

// ! Each of these may return an empty or partial map if any requests fail. A reverted *call*, on the
// ! other hand, is the contract answering — and each answers with the shape's own "nothing", so a
// ! candidate whose call reverted still gets an entry rather than silently dropping out of the diff.

/// ERC-721 ownership at `block_id`, as a count of what the signer holds (`0` or `1`).
async fn fetch_nft_ownership_before(
   ctx: ZeusCtx,
   chain: u64,
   from: Address,
   block_id: BlockId,
   refs: Vec<NftRef>,
) -> HashMap<(Address, U256), U256> {
   if refs.is_empty() {
      return HashMap::new();
   }

   let client = ctx.get_client_manager();
   let mut out = HashMap::new();

   for chunk in refs.chunks(NFT_REF_BATCH) {
      let chunk = chunk.to_vec();
      match client
         .request(chain, |client| {
            let chunk = chunk.clone();
            async move { batch::get_erc721_owners(client, chunk, Some(block_id)).await }
         })
         .await
      {
         Ok(rows) => {
            for (collection, token_id, owner) in rows {
               // A reverted `ownerOf` says the token has no owner — "not the signer's" is the answer
               // the row needs, not a hole in the map.
               out.insert(
                  (collection, token_id),
                  U256::from(u8::from(owner == Some(from))),
               );
            }
         }
         Err(e) => tracing::warn!("ERC-721 owners at block failed: {:?}", e),
      }
   }

   out
}

/// ERC-1155 balances at `block_id`.
async fn fetch_nft_balances_before(
   ctx: ZeusCtx,
   chain: u64,
   from: Address,
   block_id: BlockId,
   refs: Vec<NftRef>,
) -> HashMap<(Address, U256), U256> {
   if refs.is_empty() {
      return HashMap::new();
   }

   let client = ctx.get_client_manager();
   let mut out = HashMap::new();

   for chunk in refs.chunks(NFT_REF_BATCH) {
      let chunk = chunk.to_vec();
      match client
         .request(chain, |client| {
            let chunk = chunk.clone();
            async move { batch::get_erc1155_balances(client, from, chunk, Some(block_id)).await }
         })
         .await
      {
         Ok(rows) => {
            for (collection, token_id, balance) in rows {
               if let Some(balance) = balance {
                  out.insert((collection, token_id), balance);
               }
            }
         }
         Err(e) => tracing::warn!("ERC-1155 balances at block failed: {:?}", e),
      }
   }

   out
}

/// ERC-721 per-token approvals at `block_id`, for the `Token(id)` candidates.
async fn fetch_nft_token_approvals_before(
   ctx: ZeusCtx,
   chain: u64,
   block_id: BlockId,
   candidates: Vec<NftApprovalCandidate>,
) -> HashMap<NftApprovalCandidate, NftApprovalValue> {
   if candidates.is_empty() {
      return HashMap::new();
   }

   let client = ctx.get_client_manager();
   let mut out = HashMap::new();

   for chunk in candidates.chunks(NFT_REF_BATCH) {
      debug_assert!(
         chunk
            .iter()
            .all(|candidate| matches!(candidate.target, NftApprovalTarget::Token(_))),
         "this fetch is only handed per-token candidates"
      );

      let refs: Vec<NftRef> = chunk
         .iter()
         .filter_map(|candidate| match candidate.target {
            NftApprovalTarget::Token(id) => Some((candidate.collection, id)),
            _ => None,
         })
         .collect();
      let chunk = chunk.to_vec();

      match client
         .request(chain, |client| {
            let refs = refs.clone();
            async move { batch::get_erc721_approved(client, refs, Some(block_id)).await }
         })
         .await
      {
         Ok(rows) => {
            // The helper returns one row per ref, in order, so the zip keeps each answer with its
            // own candidate.
            for (row, candidate) in rows.into_iter().zip(chunk) {
               // A reverted `getApproved` is the zero address: no approval, which a mint that also
               // approves someone still shows up as a change from.
               out.insert(
                  candidate,
                  NftApprovalValue::Approved(row.2.unwrap_or_default()),
               );
            }
         }
         Err(e) => tracing::warn!("ERC-721 approvals at block failed: {:?}", e),
      }
   }

   out
}

/// `isApprovedForAll` at `block_id`, for the `ForAll` candidates.
async fn fetch_nft_for_all_before(
   ctx: ZeusCtx,
   chain: u64,
   from: Address,
   block_id: BlockId,
   candidates: Vec<NftApprovalCandidate>,
) -> HashMap<NftApprovalCandidate, NftApprovalValue> {
   if candidates.is_empty() {
      return HashMap::new();
   }

   let client = ctx.get_client_manager();
   let mut out = HashMap::new();

   for chunk in candidates.chunks(NFT_REF_BATCH) {
      let targets: Vec<(Address, Address)> = chunk
         .iter()
         .map(|candidate| (candidate.collection, candidate.operator))
         .collect();
      let chunk = chunk.to_vec();

      match client
         .request(chain, |client| {
            let targets = targets.clone();
            async move {
               batch::get_erc721_is_approved_for_all(client, from, targets, Some(block_id)).await
            }
         })
         .await
      {
         Ok(rows) => {
            for (row, candidate) in rows.into_iter().zip(chunk) {
               out.insert(
                  candidate,
                  NftApprovalValue::ForAll(row.2.unwrap_or(false)),
               );
            }
         }
         Err(e) => tracing::warn!("isApprovedForAll at block failed: {:?}", e),
      }
   }

   out
}

/// ERC-5216 allowances at `block_id`, for the `Allowance(id)` candidates.
async fn fetch_nft_allowances_before(
   ctx: ZeusCtx,
   chain: u64,
   from: Address,
   block_id: BlockId,
   candidates: Vec<NftApprovalCandidate>,
) -> HashMap<NftApprovalCandidate, NftApprovalValue> {
   if candidates.is_empty() {
      return HashMap::new();
   }

   let client = ctx.get_client_manager();
   let mut out = HashMap::new();

   for chunk in candidates.chunks(NFT_REF_BATCH) {
      let refs: Vec<(Address, Address, U256)> = chunk
         .iter()
         .filter_map(|candidate| match candidate.target {
            NftApprovalTarget::Allowance(id) => {
               Some((candidate.collection, candidate.operator, id))
            }
            _ => None,
         })
         .collect();
      let chunk = chunk.to_vec();

      match client
         .request(chain, |client| {
            let refs = refs.clone();
            async move { batch::get_erc1155_allowances(client, from, refs, Some(block_id)).await }
         })
         .await
      {
         Ok(rows) => {
            for (row, candidate) in rows.into_iter().zip(chunk) {
               out.insert(
                  candidate,
                  NftApprovalValue::Allowance(row.3.unwrap_or_default()),
               );
            }
         }
         Err(e) => tracing::warn!("ERC-5216 allowances at block failed: {:?}", e),
      }
   }

   out
}

/// The NFT half of the "before" state.
async fn fetch_nft_state(
   ctx: ZeusCtx,
   chain: u64,
   from: Address,
   block_id: BlockId,
   nft: &NftRequest,
) -> NftState {
   if nft.is_empty() {
      return NftState::default();
   }

   let mut refs721 = Vec::new();
   let mut refs1155 = Vec::new();
   for candidate in &nft.balances {
      match candidate.standard {
         NftStandard::Erc721 => refs721.push((candidate.collection, candidate.token_id)),
         NftStandard::Erc1155 => refs1155.push((candidate.collection, candidate.token_id)),
      }
   }

   let mut token_approvals = Vec::new();
   let mut for_all = Vec::new();
   let mut allowances = Vec::new();
   for candidate in &nft.approvals {
      match candidate.target {
         NftApprovalTarget::Token(_) => token_approvals.push(*candidate),
         NftApprovalTarget::ForAll => for_all.push(*candidate),
         NftApprovalTarget::Allowance(_) => allowances.push(*candidate),
      }
   }

   let (ownership721, balances1155, token_apps, for_all_apps, allowance_apps) = tokio::join!(
      fetch_nft_ownership_before(ctx.clone(), chain, from, block_id, refs721),
      fetch_nft_balances_before(ctx.clone(), chain, from, block_id, refs1155),
      fetch_nft_token_approvals_before(ctx.clone(), chain, block_id, token_approvals),
      fetch_nft_for_all_before(ctx.clone(), chain, from, block_id, for_all),
      fetch_nft_allowances_before(ctx, chain, from, block_id, allowances),
   );

   let mut approvals = token_apps;
   approvals.extend(for_all_apps);
   approvals.extend(allowance_apps);

   NftState {
      ownership721,
      balances1155,
      approvals,
   }
}

async fn fetch_before_state(
   ctx: ZeusCtx,
   chain: u64,
   from: Address,
   block_id: BlockId,
   tokens: Vec<Address>,
   erc20_pairs: Vec<(Address, Address)>,
   permit2_pairs: Vec<(Address, Address)>,
   nft: NftRequest,
) -> BeforeState {
   let time = Instant::now();

   let tokens_fut = fetch_token_before(ctx.clone(), chain, from, block_id, tokens);
   let erc20_fut = fetch_erc20_allowance_before(ctx.clone(), chain, from, block_id, erc20_pairs);
   let permit2_fut = fetch_permit2_before(ctx.clone(), chain, from, block_id, permit2_pairs);
   let nft_fut = fetch_nft_state(ctx, chain, from, block_id, &nft);

   let (tokens, erc20, permit2, nft) = tokio::join!(tokens_fut, erc20_fut, permit2_fut, nft_fut);

   tracing::info!(
      "fetch_before_state took {} ms",
      time.elapsed().as_millis()
   );

   BeforeState {
      tokens,
      erc20,
      permit2,
      nft,
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
   use zeus_eth::revm::database::{CacheDB, EmptyDB, InMemoryDB};
   use zeus_eth::revm::state::{AccountInfo, Bytecode};
   use zeus_eth::revm_utils::new_evm;

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

   fn after_tokens(tokens: HashMap<Address, U256>) -> AfterState {
      AfterState {
         tokens,
         approvals: HashMap::new(),
         nft: NftState::default(),
      }
   }

   fn after_approvals(
      approvals: HashMap<(ApprovalKind, Address, Address), (U256, Option<u64>)>,
   ) -> AfterState {
      AfterState {
         tokens: HashMap::new(),
         approvals,
         nft: NftState::default(),
      }
   }

   #[test]
   fn token_delta_is_emitted() {
      let token = addr(1);
      let mut before = BeforeState::empty();
      before.tokens.insert(token, U256::from(10u64));
      let mut after = HashMap::new();
      after.insert(token, U256::from(7u64));

      let raw = combine_diffs(
         &[token],
         &[],
         &NftRequest::default(),
         before,
         after_tokens(after),
      );
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

      let raw = combine_diffs(
         &[token],
         &[],
         &NftRequest::default(),
         before,
         after_tokens(after),
      );
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
         &NftRequest::default(),
         BeforeState::empty(),
         after_tokens(after_only),
      );
      assert!(raw.tokens.is_empty());

      let mut before = BeforeState::empty();
      before.tokens.insert(token, U256::from(1u64));
      let raw = combine_diffs(
         &[token],
         &[],
         &NftRequest::default(),
         before,
         after_tokens(HashMap::new()),
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

      let raw = combine_diffs(
         &[],
         &[cand],
         &NftRequest::default(),
         before,
         after_approvals(after),
      );
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

      let raw = combine_diffs(
         &[],
         &[cand],
         &NftRequest::default(),
         before,
         after_approvals(after),
      );
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

      let raw = combine_diffs(
         &[],
         &[cand],
         &NftRequest::default(),
         before,
         after_approvals(after),
      );
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

      let raw = combine_diffs(
         &[],
         &[cand],
         &NftRequest::default(),
         before,
         after_approvals(after),
      );
      assert!(raw.approvals.is_empty());
   }

   fn nft_candidate(collection: Address, id: u64, standard: NftStandard) -> NftCandidate {
      NftCandidate {
         collection,
         token_id: U256::from(id),
         standard,
      }
   }

   fn balance_request(candidate: NftCandidate) -> NftRequest {
      NftRequest {
         balances: vec![candidate],
         approvals: Vec::new(),
      }
   }

   fn approval_request(candidate: NftApprovalCandidate) -> NftRequest {
      NftRequest {
         balances: Vec::new(),
         approvals: vec![candidate],
      }
   }

   fn nft_after(nft: NftState) -> AfterState {
      AfterState {
         tokens: HashMap::new(),
         approvals: HashMap::new(),
         nft,
      }
   }

   #[test]
   fn nft_ownership_gain_is_emitted() {
      let collection = addr(9);
      let candidate = nft_candidate(collection, 1, NftStandard::Erc721);

      let mut before = BeforeState::empty();
      before.nft.ownership721.insert((collection, U256::from(1)), U256::ZERO);

      let mut nft = NftState::default();
      nft.ownership721.insert((collection, U256::from(1)), U256::from(1));

      let raw = combine_diffs(
         &[],
         &[],
         &balance_request(candidate),
         before,
         nft_after(nft),
      );

      assert_eq!(raw.nft_balances.len(), 1);
      assert_eq!(raw.nft_balances[0].before, U256::ZERO);
      assert_eq!(raw.nft_balances[0].after, U256::from(1));
      assert!(raw.nft_balances[0].is_received());
      // An NFT row never leaks into the fungible vectors.
      assert!(raw.tokens.is_empty());
      assert!(raw.approvals.is_empty());
   }

   #[test]
   fn nft_1155_balance_delta_is_emitted() {
      let collection = addr(9);
      let candidate = nft_candidate(collection, 3, NftStandard::Erc1155);

      let mut before = BeforeState::empty();
      before.nft.balances1155.insert((collection, U256::from(3)), U256::from(5));

      let mut nft = NftState::default();
      nft.balances1155.insert((collection, U256::from(3)), U256::from(2));

      let raw = combine_diffs(
         &[],
         &[],
         &balance_request(candidate),
         before,
         nft_after(nft),
      );

      assert_eq!(raw.nft_balances.len(), 1);
      assert_eq!(raw.nft_balances[0].abs_delta(), U256::from(3));
      assert!(!raw.nft_balances[0].is_received());
   }

   #[test]
   fn unchanged_nft_ownership_is_omitted() {
      let collection = addr(9);
      let candidate = nft_candidate(collection, 1, NftStandard::Erc721);

      let mut before = BeforeState::empty();
      before.nft.ownership721.insert((collection, U256::from(1)), U256::from(1));

      let mut nft = NftState::default();
      nft.ownership721.insert((collection, U256::from(1)), U256::from(1));

      let raw = combine_diffs(
         &[],
         &[],
         &balance_request(candidate),
         before,
         nft_after(nft),
      );

      assert!(raw.nft_balances.is_empty());
   }

   /// The standard decides which map holds the answer, so a value filed under the other one is
   /// simply not the answer.
   #[test]
   fn nft_ownership_is_read_from_the_map_its_standard_names() {
      let collection = addr(9);
      let candidate = nft_candidate(collection, 1, NftStandard::Erc721);
      let key = (collection, U256::from(1));

      let mut before = BeforeState::empty();
      before.nft.balances1155.insert(key, U256::ZERO);

      let mut nft = NftState::default();
      nft.balances1155.insert(key, U256::from(1));

      let raw = combine_diffs(
         &[],
         &[],
         &balance_request(candidate),
         before,
         nft_after(nft),
      );

      assert!(raw.nft_balances.is_empty());
   }

   #[test]
   fn nft_with_one_side_missing_is_omitted() {
      let collection = addr(9);
      let candidate = nft_candidate(collection, 1, NftStandard::Erc721);

      let mut nft = NftState::default();
      nft.ownership721.insert((collection, U256::from(1)), U256::from(1));

      let raw = combine_diffs(
         &[],
         &[],
         &balance_request(candidate),
         BeforeState::empty(),
         nft_after(nft),
      );

      assert!(raw.nft_balances.is_empty());
   }

   #[test]
   fn nft_for_all_approval_change_is_emitted() {
      let collection = addr(9);
      let operator = addr(7);
      let candidate = NftApprovalCandidate {
         collection,
         operator,
         target: NftApprovalTarget::ForAll,
      };

      let mut before = BeforeState::empty();
      before.nft.approvals.insert(candidate, NftApprovalValue::ForAll(false));

      let mut nft = NftState::default();
      nft.approvals.insert(candidate, NftApprovalValue::ForAll(true));

      let raw = combine_diffs(
         &[],
         &[],
         &approval_request(candidate),
         before,
         nft_after(nft),
      );

      assert_eq!(raw.nft_approvals.len(), 1);
      assert_eq!(
         raw.nft_approvals[0].before,
         NftApprovalValue::ForAll(false)
      );
      assert_eq!(
         raw.nft_approvals[0].after,
         NftApprovalValue::ForAll(true)
      );
   }

   #[test]
   fn nft_per_token_approval_change_is_emitted() {
      let collection = addr(9);
      let operator = addr(7);
      let candidate = NftApprovalCandidate {
         collection,
         operator,
         target: NftApprovalTarget::Token(U256::from(4)),
      };

      let mut before = BeforeState::empty();
      before.nft.approvals.insert(
         candidate,
         NftApprovalValue::Approved(Address::repeat_byte(3)),
      );

      let mut nft = NftState::default();
      nft.approvals.insert(
         candidate,
         NftApprovalValue::Approved(Address::ZERO),
      );

      let raw = combine_diffs(
         &[],
         &[],
         &approval_request(candidate),
         before,
         nft_after(nft),
      );

      // The measured address is the value, nothing else — and zero means revoked.
      assert_eq!(raw.nft_approvals.len(), 1);
      assert_eq!(
         raw.nft_approvals[0].after,
         NftApprovalValue::Approved(Address::ZERO)
      );
   }

   #[test]
   fn unchanged_nft_approval_is_omitted() {
      let collection = addr(9);
      let operator = addr(7);
      let candidate = NftApprovalCandidate {
         collection,
         operator,
         target: NftApprovalTarget::ForAll,
      };

      let mut before = BeforeState::empty();
      before.nft.approvals.insert(candidate, NftApprovalValue::ForAll(true));

      let mut nft = NftState::default();
      nft.approvals.insert(candidate, NftApprovalValue::ForAll(true));

      let raw = combine_diffs(
         &[],
         &[],
         &approval_request(candidate),
         before,
         nft_after(nft),
      );

      assert!(raw.nft_approvals.is_empty());
   }

   /// A collection-wide approval and a per-token one are different candidates, so one being granted
   /// cannot be read as the other changing.
   #[test]
   fn for_all_and_per_token_are_separate_candidates() {
      let collection = addr(9);
      let operator = addr(7);
      let for_all = NftApprovalCandidate {
         collection,
         operator,
         target: NftApprovalTarget::ForAll,
      };
      let per_token = NftApprovalCandidate {
         collection,
         operator,
         target: NftApprovalTarget::Token(U256::from(4)),
      };

      let mut before = BeforeState::empty();
      before.nft.approvals.insert(for_all, NftApprovalValue::ForAll(false));
      before.nft.approvals.insert(
         per_token,
         NftApprovalValue::Approved(Address::ZERO),
      );

      let mut nft = NftState::default();
      nft.approvals.insert(for_all, NftApprovalValue::ForAll(true));
      nft.approvals.insert(
         per_token,
         NftApprovalValue::Approved(Address::ZERO),
      );

      let raw = combine_diffs(
         &[],
         &[],
         &NftRequest {
            balances: Vec::new(),
            approvals: vec![for_all, per_token],
         },
         before,
         nft_after(nft),
      );

      assert_eq!(raw.nft_approvals.len(), 1);
      assert_eq!(raw.nft_approvals[0].cand, for_all);
   }

   /// An EVM whose `collection` reverts on every call — a burnt token's `ownerOf`/`getApproved`, which
   /// is the read the after-state probe has to interpret.
   fn reverting_collection_evm(collection: Address) -> Evm2<InMemoryDB> {
      let mut db = CacheDB::new(EmptyDB::default());
      db.insert_account_info(
         collection,
         AccountInfo {
            // PUSH1 0x00 PUSH1 0x00 REVERT
            code: Some(Bytecode::new_raw(Bytes::from_static(&[
               0x60, 0x00, 0x60, 0x00, 0xfd,
            ]))),
            ..Default::default()
         },
      );

      new_evm(ChainId::Ethereum, None, db)
   }

   /// A failed post-tx read is the shape's *nothing*, not a hole in the map.
   ///
   /// A burn is the case that matters: the post-tx `ownerOf` reverts, and before this fix the key was
   /// simply left out, so `combine_diffs` dropped the row and the confirmation window showed **no** NFT
   /// change for a token the signer owned and no longer does — while the receipt/history path, which
   /// reads both sides over RPC, still emitted it. `git stash`-free red proof: with the `if let Ok`
   /// form restored, the first assertion below fails (the key is absent).
   #[test]
   fn a_reverted_balance_read_after_the_tx_reads_as_not_owned() {
      let collection = addr(9);
      let owner = addr(1);
      let mut evm = reverting_collection_evm(collection);

      let balances = balance_request(nft_candidate(collection, 1, NftStandard::Erc721));
      let after = measure_nft_after(owner, &balances, &mut evm);

      assert_eq!(
         after.ownership721.get(&(collection, U256::from(1))),
         Some(&U256::ZERO),
         "a reverted `ownerOf` is \"not the signer's\", not a missing answer"
      );

      let mut before = BeforeState::empty();
      before.nft.ownership721.insert((collection, U256::from(1)), U256::from(1));

      let raw = combine_diffs(
         &[],
         &[],
         &balances,
         before,
         AfterState {
            tokens: HashMap::new(),
            approvals: HashMap::new(),
            nft: after,
         },
      );

      assert_eq!(
         raw.nft_balances.len(),
         1,
         "the burn is still a row"
      );
      assert_eq!(raw.nft_balances[0].after, U256::ZERO);
      assert!(!raw.nft_balances[0].is_received());
   }

   /// The same rule for an approval: a `getApproved` that reverts after the tx reads as the zero
   /// address — the value the before-state fetch would already have produced for it.
   #[test]
   fn a_reverted_approval_read_after_the_tx_reads_as_the_zero_address() {
      let collection = addr(9);
      let owner = addr(1);
      let candidate = NftApprovalCandidate {
         collection,
         operator: addr(7),
         target: NftApprovalTarget::Token(U256::from(4)),
      };

      let mut evm = reverting_collection_evm(collection);
      let after = measure_nft_after(owner, &approval_request(candidate), &mut evm);

      assert_eq!(
         after.approvals.get(&candidate),
         Some(&NftApprovalValue::Approved(Address::ZERO))
      );
   }
}
