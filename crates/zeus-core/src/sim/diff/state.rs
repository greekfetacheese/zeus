//! The before/after state a diff is measured from, and the candidate helpers that fill it.

use crate::sim::approval_diff::{
   ApprovalCandidate, ApprovalKind, NftApprovalCandidate, NftApprovalTarget, NftApprovalValue,
   collect_nft_approval_candidates,
};
use crate::sim::balance_diff::{NftCandidate, collect_nft_balance_candidates};
use crate::{NftApproveParams, ZeusCtx};
use std::collections::HashMap;
use zeus_eth::alloy_primitives::{Address, Bytes, Log, U256};
use zeus_eth::nft::NftStandard;

/// The NFT side of a measured state: what the signer owns, and the approval state of every candidate.
///
/// Both ownership maps hold a **count of what the signer has** — `0`/`1` for ERC-721, where
/// `ownerOf` answers with an address that is either theirs or not, and the real count for ERC-1155.
/// Normalizing at the probe is what lets the two be compared the same way, and the row only ever
/// asks "did the signer gain or lose it".
#[derive(Default)]
pub(super) struct NftState {
   /// ERC-721 ownership per `(collection, id)`, as `0` or `1`.
   pub(super) ownership721: HashMap<(Address, U256), U256>,
   /// ERC-1155 balance per `(collection, id)`.
   pub(super) balances1155: HashMap<(Address, U256), U256>,
   /// Measured approval state, per candidate. A missing key means the probe did not answer.
   pub(super) approvals: HashMap<NftApprovalCandidate, NftApprovalValue>,
}

impl NftState {
   pub(super) fn merge(&mut self, other: Self) {
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
pub(super) struct NftRequest {
   pub(super) balances: Vec<NftCandidate>,
   pub(super) approvals: Vec<NftApprovalCandidate>,
}

impl NftRequest {
   /// Everything `call_data` and `logs` imply for the signer's NFTs.
   pub(super) fn collect(
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

   pub(super) fn is_empty(&self) -> bool {
      self.balances.is_empty() && self.approvals.is_empty()
   }

   /// The candidates `self` asks for that `pre` did not already ask for.
   pub(super) fn extras(&self, pre: &Self) -> Self {
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
pub(super) fn nft_approval_candidate(params: &NftApproveParams) -> Option<NftApprovalCandidate> {
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
pub(super) fn nft_standard(
   ctx: &ZeusCtx,
   chain: u64,
   candidate: &NftApprovalCandidate,
) -> NftStandard {
   match candidate.target {
      NftApprovalTarget::Token(_) => NftStandard::Erc721,
      NftApprovalTarget::Allowance(_) => NftStandard::Erc1155,
      NftApprovalTarget::ForAll => ctx
         .read(|ctx| ctx.nft_db.get_collection(chain, candidate.collection))
         .map(|collection| collection.standard)
         .unwrap_or(NftStandard::Erc721),
   }
}

pub(super) struct BeforeState {
   pub(super) tokens: HashMap<Address, U256>,
   pub(super) erc20: HashMap<(Address, Address), U256>,
   pub(super) permit2: HashMap<(Address, Address), (U256, u64)>,
   pub(super) nft: NftState,
}

impl BeforeState {
   pub(super) fn empty() -> Self {
      Self {
         tokens: HashMap::new(),
         erc20: HashMap::new(),
         permit2: HashMap::new(),
         nft: NftState::default(),
      }
   }

   pub(super) fn is_empty_request(
      tokens: &[Address],
      erc20: &[(Address, Address)],
      permit2: &[(Address, Address)],
      nft: &NftRequest,
   ) -> bool {
      tokens.is_empty() && erc20.is_empty() && permit2.is_empty() && nft.is_empty()
   }

   pub(super) fn merge(&mut self, other: Self) {
      self.tokens.extend(other.tokens);
      self.erc20.extend(other.erc20);
      self.permit2.extend(other.permit2);
      self.nft.merge(other.nft);
   }
}

/// The measured "after" side of a diff, in the same shape as [`BeforeState`], so the two can be
/// combined without either side knowing which path produced it.
pub(super) struct AfterState {
   pub(super) tokens: HashMap<Address, U256>,
   pub(super) approvals: HashMap<(ApprovalKind, Address, Address), (U256, Option<u64>)>,
   pub(super) nft: NftState,
}

pub(super) fn split_approval_pairs(
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

pub(super) fn not_in<T: Copy + PartialEq>(full: &[T], pre: &[T]) -> Vec<T> {
   full.iter().copied().filter(|item| !pre.contains(item)).collect()
}

pub(super) fn known_approvals(
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

/// The value an approval read's own *nothing* takes: the zero address for a per-token ERC-721
/// approval, `false` for a collection-wide one, zero for an ERC-5216 allowance.
///
/// The approval loop applies this to a probe the contract *answered* (a revert included), matching
/// the same shape defaults the before-state fetch applies to a dropped sub-call — while a probe that
/// never ran is dropped instead.
pub(super) fn nft_approval_nothing(target: NftApprovalTarget) -> NftApprovalValue {
   match target {
      NftApprovalTarget::Token(_) => NftApprovalValue::Approved(Address::ZERO),
      NftApprovalTarget::ForAll => NftApprovalValue::ForAll(false),
      NftApprovalTarget::Allowance(_) => NftApprovalValue::Allowance(U256::ZERO),
   }
}
