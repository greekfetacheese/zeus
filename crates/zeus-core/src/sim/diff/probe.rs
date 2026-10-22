//! Read the post-simulation state off the fork EVM, then combine both sides into raw diffs.

use super::state::{AfterState, BeforeState, NftRequest, NftState, nft_approval_nothing};
use super::{ApprovalWei, NftApprovalWei, RawSimDiffs, TokenWei};
use crate::sim::approval_diff::{
   ApprovalCandidate, ApprovalKind, NftApprovalTarget, NftApprovalValue,
};
use crate::sim::balance_diff::NftBalanceChange;
use std::collections::HashMap;
use zeus_eth::alloy_primitives::{Address, U256};
use zeus_eth::nft::NftStandard;
use zeus_eth::revm_utils::simulate::{
   NftApprovalProbeError, erc20_allowance, erc20_balance, erc721_get_approved,
   erc721_is_approved_for_all, erc721_owner_of, erc1155_allowance, erc1155_balance_of,
   permit2_allowance,
};
use zeus_eth::revm_utils::{Database, Evm2};

/// Max ERC-20 tokens per `getERC20Balance` eth_call so it stays under gas limits.
pub(super) const TOKEN_BALANCE_BATCH: usize = 20;

/// Max `(token, spender)` allowance pairs per Multicall3 aggregate so the eth_call stays under gas limits.
pub(super) const ALLOWANCE_PAIR_BATCH: usize = 20;

pub(super) fn measure_token_after<DB: Database>(
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

pub(super) fn measure_approval_after<DB: Database>(
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
/// A read the fork could not answer is **unknown**, not zero: there is nothing to compare, so the key
/// stays out and `combine_diffs` drops the row. Fabricating a zero instead would let a probe failure
/// claim a change — a balance the wallet really holds reading as a full loss — which is the same
/// reading the before-state fetch draws from a dropped sub-call.
///
/// The ERC-721 **ownership** arm is the one exception, and a deliberate one: a reverted `ownerOf` is
/// the standard's own answer for a token that no longer exists (a burn), not a failed read, so that
/// arm maps it to "not the signer's" rather than leaving it out. See the note on that arm for where
/// the two are *not* told apart.
pub(super) fn measure_nft_after<DB: Database>(
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
            // and that is the answer the row needs: dropping the key would lose a burn, since
            // `combine_diffs` drops a candidate whose *either* side is missing.
            //
            // Deliberate, and narrower than it looks: this arm reads *every* `Err` from the probe as a
            // revert, and the probe cannot be asked which it was — `erc721_owner_of` collapses a
            // contract revert, an EVM/DB error and a decode failure into the same `anyhow::Error`. A
            // probe that could not run (a fork read the RPC never answered) therefore still reads as
            // "lost the token" here, which is the ERC-1155 defect this file fixes on the other arm. The
            // before-state fetch *does* separate the two (its `Option<Address>` row is `None` only for a
            // revert; a transport failure loses the row), so the after side is the asymmetric one.
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
            // A probe that failed is *unknown*, not zero — an unreadable side claims no change. The
            // before-state fetch drops the same failed read (only `Some(balance)` is inserted), and
            // `combine_diffs` drops a candidate whose either side is missing, so leaving the key out is
            // what keeps "could not read" from becoming a claimable change: a balance the wallet really
            // holds would otherwise read as a full loss, and the failure would invent it.
            match erc1155_balance_of(
               after_evm,
               candidate.collection,
               owner,
               candidate.token_id,
            ) {
               Ok(balance) => {
                  state.balances1155.insert(key, balance);
               }
               Err(e) => {
                  tracing::warn!("ERC-1155 balanceOf after the tx failed: {:?}", e);
               }
            }
         }
      }
   }

   for candidate in &nft.approvals {
      // The two ways an approval probe can fail are not the same, and the probes report which one it
      // was. A revert is the contract's own answer — the shape's *nothing* — so it keeps the value
      // the before-state fetch would have produced for it (the zero address, `false`, zero): a token
      // burnt in the tx reverts `getApproved`, and the before side already answered "no operator" for
      // it, so dropping the row would lose a change the tx really made.
      //
      // A read that never reached the contract is *unknown*, not that default: nothing was measured,
      // so the key stays out and `combine_diffs` drops the candidate, exactly as the ERC-1155 balance
      // arm drops a failed read. Letting a fork failure fabricate a value here would render a grant
      // whose after-probe never ran as a **Revoked** row.
      let measured = match candidate.target {
         NftApprovalTarget::Token(id) => {
            erc721_get_approved(after_evm, candidate.collection, id).map(NftApprovalValue::Approved)
         }
         NftApprovalTarget::ForAll => erc721_is_approved_for_all(
            after_evm,
            candidate.collection,
            owner,
            candidate.operator,
         )
         .map(NftApprovalValue::ForAll),
         NftApprovalTarget::Allowance(id) => erc1155_allowance(
            after_evm,
            candidate.collection,
            owner,
            candidate.operator,
            id,
         )
         .map(NftApprovalValue::Allowance),
      };

      match measured {
         Ok(value) => {
            state.approvals.insert(*candidate, value);
         }
         Err(NftApprovalProbeError::Answered(_)) => {
            state.approvals.insert(*candidate, nft_approval_nothing(candidate.target));
         }
         Err(e @ NftApprovalProbeError::NeverRan(_)) => {
            tracing::warn!(
               "NFT approval read after the tx never ran: {:?}",
               e
            );
         }
      }
   }

   state
}

pub(super) fn combine_diffs(
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
