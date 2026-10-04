//! Signer ERC-20 / Permit2 allowance changes from simulation state, not logs.
//!
//! Log `Approval` amounts are untrusted. Amounts here come from `allowance()`.

use super::balance_diff::MAX_NFT_CANDIDATES;
use crate::utils::TimeStamp;
use serde::{Deserialize, Serialize};
use zeus_eth::{
   abi::{erc20, erc721, erc1155, permit},
   alloy_primitives::{Address, Bytes, Log, U256, aliases::U160},
   currency::{Currency, ERC20Token},
   nft::NftStandard,
   utils::NumericValue,
};

pub const MAX_APPROVAL_CANDIDATES: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ApprovalKind {
   Erc20,
   Permit2,
}

impl Default for ApprovalKind {
   fn default() -> Self {
      Self::Erc20
   }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ApprovalCandidate {
   pub kind: ApprovalKind,
   pub token: Address,
   pub spender: Address,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalChange {
   pub kind: ApprovalKind,
   pub token: Currency,
   pub spender: Address,
   pub before: NumericValue,
   pub after: NumericValue,
   /// USD price of the currency at the time of the tx.
   #[serde(default)]
   pub price: NumericValue,
   /// Permit2 expiration after the tx. `None` for ERC-20.
   #[serde(default)]
   pub expiration_after: Option<TimeStamp>,
}

impl ApprovalChange {
   pub fn from_wei(
      kind: ApprovalKind,
      token: ERC20Token,
      spender: Address,
      before: U256,
      after: U256,
      price: NumericValue,
      expiration_before: Option<u64>,
      expiration_after: Option<u64>,
   ) -> Option<Self> {
      if before == after && expiration_before == expiration_after {
         return None;
      }
      let decimals = token.decimals;
      Some(Self {
         kind,
         token: Currency::from(token),
         spender,
         before: NumericValue::format_wei(before, decimals),
         after: NumericValue::format_wei(after, decimals),
         price,
         expiration_after: expiration_after.map(TimeStamp::Seconds),
      })
   }

   pub fn is_increase(&self) -> bool {
      self.after.wei() > self.before.wei()
   }

   pub fn is_revoke(&self) -> bool {
      self.after.wei().is_zero()
   }

   pub fn is_unlimited(&self) -> bool {
      unlimited_allowance(self.kind, self.after.wei())
   }
}

/// Which probe answers an NFT approval candidate — the three shapes, as the approval store models
/// them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NftApprovalTarget {
   /// ERC-721 `getApproved(id)`. One id, and one approved address per id.
   Token(U256),
   /// `isApprovedForAll(owner, operator)` — both standards answer it identically.
   ForAll,
   /// ERC-5216 `allowance(owner, operator, id)`.
   Allowance(U256),
}

/// The measured state of one NFT approval, in its shape's own units.
///
/// One variant per shape rather than a single number, because the three do not measure the same
/// thing: an ERC-721 approval *is* an address (and a revoke is the zero address), an
/// `ApprovalForAll` is a flag, and ERC-5216 is an allowance. Encoding them as one integer would mean
/// giving one of them a meaning it does not have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NftApprovalValue {
   /// ERC-721 per-token: the approved address, the zero address when there is none.
   Approved(Address),
   /// `ApprovalForAll`: the flag.
   ForAll(bool),
   /// ERC-5216: the allowance for this operator and id.
   Allowance(U256),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NftApprovalChange {
   pub collection: Address,
   /// Who the row is about — see [`Self::from_state`] for how that is decided per shape.
   pub operator: Address,
   pub target: NftApprovalTarget,
   pub standard: NftStandard,
   pub before: NftApprovalValue,
   pub after: NftApprovalValue,
}

impl NftApprovalChange {
   /// Build a change from the measured before/after state, or `None` when nothing changed.
   ///
   /// `operator` is the candidate's operator. For the per-token shape that is the address the log
   /// named, and a revoke names the **zero address** — so the row takes its operator from the
   /// measured state instead: the address it is now, or the one it was when the approval was
   /// cleared. Naming zero there would leave the row saying "approved to 0x0" for a revocation.
   pub fn from_state(
      collection: Address,
      operator: Address,
      target: NftApprovalTarget,
      standard: NftStandard,
      before: NftApprovalValue,
      after: NftApprovalValue,
   ) -> Option<Self> {
      if before == after {
         return None;
      }

      let operator = match (&before, &after) {
         (_, NftApprovalValue::Approved(address)) if !address.is_zero() => *address,
         (NftApprovalValue::Approved(address), _) => *address,
         _ => operator,
      };

      Some(Self {
         collection,
         operator,
         target,
         standard,
         before,
         after,
      })
   }

   /// Whether this row takes the approval away. Each shape says no its own way, and none of them is
   /// a missing value.
   pub fn is_revoke(&self) -> bool {
      match self.after {
         NftApprovalValue::Approved(address) => address.is_zero(),
         NftApprovalValue::ForAll(approved) => !approved,
         NftApprovalValue::Allowance(amount) => amount.is_zero(),
      }
   }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ApprovalDiff {
   pub changes: Vec<ApprovalChange>,
   /// NFT approval changes. Their own type because an NFT approval's shape decides what "before" and
   /// "after" are at all. `serde(default)` so an analysis stored before these rows existed loads.
   #[serde(default)]
   pub nft_changes: Vec<NftApprovalChange>,
}

impl ApprovalDiff {
   pub fn is_empty(&self) -> bool {
      self.changes.is_empty() && self.nft_changes.is_empty()
   }

   pub fn len(&self) -> usize {
      self.changes.len() + self.nft_changes.len()
   }

   /// Revokes first, then remaining grants.
   pub fn sorted(&self) -> Vec<&ApprovalChange> {
      let mut rows: Vec<&ApprovalChange> = self.changes.iter().collect();
      rows.sort_by_key(|c| (!c.is_revoke(), c.is_increase()));
      rows
   }

   /// NFT rows: revokes first, then grants — the same order [`Self::sorted`] uses, and for the same
   /// reason (a row that takes access away is the one worth reading first).
   pub fn nft_sorted(&self) -> Vec<&NftApprovalChange> {
      let mut rows: Vec<&NftApprovalChange> = self.nft_changes.iter().collect();
      rows.sort_by_key(|c| !c.is_revoke());
      rows
   }
}

pub fn unlimited_allowance(kind: ApprovalKind, amount: U256) -> bool {
   match kind {
      ApprovalKind::Erc20 => amount == U256::MAX,
      ApprovalKind::Permit2 => amount == U256::from(U160::MAX),
   }
}

/// One NFT approval to probe, as a *question*: which collection, to whom, and of which shape.
///
/// The collection's standard is deliberately absent — `ApprovalForAll` is byte-identical across
/// ERC-721 and ERC-1155 and the probe that answers it is identical too, so the standard only matters
/// once there is a row to draw, and it is resolved for display then.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NftApprovalCandidate {
   pub collection: Address,
   pub operator: Address,
   pub target: NftApprovalTarget,
}

/// NFT approval candidates to probe for the signer, from the same three sources the fungible ones
/// come from.
///
/// - the approval logs this tx emitted, of any of the three shapes;
/// - the calldata, when it is one of the three approval calls;
/// - approvals the user made in-app earlier, but **only when `operator == interact_to`**. That is
///   what catches an approval this tx *consumes* — an ERC-721 token's approval is cleared by its
///   transfer and an ERC-5216 allowance is spent by one — without probing every saved operator, which
///   would be a sequential RPC walk.
///
/// A zero operator is kept: for NFTs the zero address is a **revocation**, not a missing value, and
/// it is exactly what a revoke logs (or a `approve(0, id)` call carries).
pub fn collect_nft_approval_candidates(
   owner: Address,
   interact_to: Address,
   call_data: &Bytes,
   logs: &[Log],
   known_nft: impl IntoIterator<Item = NftApprovalCandidate>,
) -> Vec<NftApprovalCandidate> {
   let mut out = Vec::new();

   for candidate in known_nft {
      if candidate.operator == interact_to {
         push_nft_approval_candidate(&mut out, candidate);
      }
   }

   if let Ok((to, token_id)) = erc721::decode_approve_call(call_data) {
      push_nft_approval_candidate(
         &mut out,
         NftApprovalCandidate {
            collection: interact_to,
            operator: to,
            target: NftApprovalTarget::Token(token_id),
         },
      );
   }

   if let Ok((operator, _approved)) = erc721::decode_set_approval_for_all_call(call_data) {
      push_nft_approval_candidate(
         &mut out,
         NftApprovalCandidate {
            collection: interact_to,
            operator,
            target: NftApprovalTarget::ForAll,
         },
      );
   }

   if let Ok((operator, id, _amount)) = erc1155::decode_approve_call(call_data) {
      push_nft_approval_candidate(
         &mut out,
         NftApprovalCandidate {
            collection: interact_to,
            operator,
            target: NftApprovalTarget::Allowance(id),
         },
      );
   }

   for log in logs {
      // ERC-721 `Approval(owner, approved, tokenId)`. It has four topics where an ERC-20 `Approval`
      // has three, which is the only thing separating their shared event name.
      if let Ok(approval) = erc721::decode_approval_log(log) {
         if approval.owner == owner {
            push_nft_approval_candidate(
               &mut out,
               NftApprovalCandidate {
                  collection: log.address,
                  operator: approval.approved,
                  target: NftApprovalTarget::Token(approval.tokenId),
               },
            );
         }
         continue;
      }

      // `ApprovalForAll(owner, operator, approved)`. One decoder for both standards: the event and
      // its meaning are the same on either.
      if let Ok(for_all) = erc721::decode_approval_for_all_log(log) {
         if for_all.owner == owner {
            push_nft_approval_candidate(
               &mut out,
               NftApprovalCandidate {
                  collection: log.address,
                  operator: for_all.operator,
                  target: NftApprovalTarget::ForAll,
               },
            );
         }
         continue;
      }

      // ERC-5216 `Approval(account, operator, id, amount)` — note the id is **not** indexed, so it
      // lives in the data beside the amount.
      if let Ok(allowance) = erc1155::decode_approval_log(log) {
         if allowance.account == owner {
            push_nft_approval_candidate(
               &mut out,
               NftApprovalCandidate {
                  collection: log.address,
                  operator: allowance.operator,
                  target: NftApprovalTarget::Allowance(allowance.id),
               },
            );
         }
      }
   }

   out
}

fn push_nft_approval_candidate(
   out: &mut Vec<NftApprovalCandidate>,
   candidate: NftApprovalCandidate,
) {
   // Only the collection is vetted: a zero *operator* is a revocation here.
   if candidate.collection.is_zero() {
      return;
   }

   // Deduped on what the *row* is about, not on the candidate's fields. A per-token approval's row takes
   // its operator from the measured state (`getApproved(id)`), so the operator is that record's *value*
   // and not part of its identity — the same rule the store keys by (`approval_manager.rs`) — which means
   // the same id reached from the known store (operator A) and from a revoke log (operator 0x0) is one
   // row, probed once. The other two shapes are per operator by construction: an ERC-5216 allowance and an
   // `ApprovalForAll` each belong to their own operator, so those keep it in the key.
   let known = out.iter().any(|existing| {
      existing.collection == candidate.collection
         && existing.target == candidate.target
         && match candidate.target {
            NftApprovalTarget::Token(_) => true,
            _ => existing.operator == candidate.operator,
         }
   });

   if known {
      return;
   }

   if out.len() >= MAX_NFT_CANDIDATES {
      return;
   }
   out.push(candidate);
}

fn push_candidate(
   out: &mut Vec<ApprovalCandidate>,
   kind: ApprovalKind,
   token: Address,
   spender: Address,
) {
   if token.is_zero() || spender.is_zero() {
      return;
   }
   let cand = ApprovalCandidate {
      kind,
      token,
      spender,
   };
   if out.contains(&cand) {
      return;
   }
   if out.len() >= MAX_APPROVAL_CANDIDATES {
      return;
   }
   out.push(cand);
}

/// `(token, spender)` pairs to probe for the signer.
///
/// Known in-app approvals are only included when `spender == interact_to`
/// so we catch silent allowance spends on the contract being called without
/// probing every saved spender (those are sequential RPC calls on the ForkDB so they are slow).
pub fn collect_approval_candidates(
   owner: Address,
   interact_to: Address,
   call_data: &Bytes,
   logs: &[Log],
   known_erc20: impl IntoIterator<Item = (Address, Address)>,
   known_permit2: impl IntoIterator<Item = (Address, Address)>,
) -> Vec<ApprovalCandidate> {
   let mut out = Vec::new();

   for (token, spender) in known_erc20 {
      if spender == interact_to {
         push_candidate(&mut out, ApprovalKind::Erc20, token, spender);
      }
   }
   for (token, spender) in known_permit2 {
      if spender == interact_to {
         push_candidate(&mut out, ApprovalKind::Permit2, token, spender);
      }
   }

   if let Ok((spender, _amount)) = erc20::decode_approve_call(call_data) {
      push_candidate(
         &mut out,
         ApprovalKind::Erc20,
         interact_to,
         spender,
      );
   }

   for log in logs {
      if let Ok(decoded) = erc20::decode_approve_log(log) {
         if decoded.owner == owner {
            push_candidate(
               &mut out,
               ApprovalKind::Erc20,
               log.address,
               decoded.spender,
            );
         }
         continue;
      }
      if let Ok(decoded) = permit::decode_permit_log(log) {
         if decoded.owner == owner {
            push_candidate(
               &mut out,
               ApprovalKind::Permit2,
               decoded.token,
               decoded.spender,
            );
         }
         continue;
      }
      if let Ok(decoded) = permit::decode_approval_log(log) {
         if decoded.owner == owner {
            push_candidate(
               &mut out,
               ApprovalKind::Permit2,
               decoded.token,
               decoded.spender,
            );
         }
      }
   }

   out
}

#[cfg(test)]
mod tests {
   use super::*;
   use zeus_eth::{
      abi::{erc721::IERC721, erc1155::IERC5216, permit::Permit2},
      alloy_primitives::{Log, address, aliases::U48},
      alloy_sol_types::SolEvent,
   };

   fn token() -> Address {
      address!("0x1111111111111111111111111111111111111111")
   }

   fn spender() -> Address {
      address!("0x2222222222222222222222222222222222222222")
   }

   fn owner() -> Address {
      address!("0x3333333333333333333333333333333333333333")
   }

   fn wbtest() -> ERC20Token {
      ERC20Token::from_components(
         1,
         token(),
         "WBTEST",
         "Walletbeat Testing ERC20",
         18,
         U256::ZERO,
      )
   }

   fn approve_calldata(spender: Address, amount: U256) -> Bytes {
      erc20::encode_approve(spender, amount)
   }

   fn other_owner() -> Address {
      address!("0x4444444444444444444444444444444444444444")
   }

   fn erc20_approval_log(token: Address, owner: Address, spender: Address) -> Log {
      let data = erc20::IERC20::Approval {
         owner,
         spender,
         value: U256::MAX,
      }
      .encode_log_data();
      Log {
         address: token,
         data,
      }
   }

   fn permit2_permit_log(owner: Address, token: Address, spender: Address) -> Log {
      let data = Permit2::Permit {
         owner,
         token,
         spender,
         amount: U160::from(1u64),
         expiration: U48::from(100u64),
         nonce: U48::from(0u64),
      }
      .encode_log_data();
      Log {
         address: Address::repeat_byte(0x99),
         data,
      }
   }

   fn permit2_approval_log(owner: Address, token: Address, spender: Address) -> Log {
      let data = Permit2::Approval {
         owner,
         token,
         spender,
         amount: U160::from(1u64),
         expiration: U48::from(100u64),
      }
      .encode_log_data();
      Log {
         address: Address::repeat_byte(0x99),
         data,
      }
   }

   #[test]
   fn calldata_approve_is_a_candidate() {
      let data = approve_calldata(spender(), U256::MAX);
      let got = collect_approval_candidates(owner(), token(), &data, &[], [], []);
      assert_eq!(
         got,
         vec![ApprovalCandidate {
            kind: ApprovalKind::Erc20,
            token: token(),
            spender: spender(),
         }]
      );
   }

   #[test]
   fn known_pairs_matching_interact_to_are_included_and_deduped() {
      let got = collect_approval_candidates(
         owner(),
         spender(),
         &Bytes::new(),
         &[],
         [(token(), spender()), (token(), spender())],
         [(token(), spender())],
      );
      assert_eq!(got.len(), 2);
      assert_eq!(got[0].kind, ApprovalKind::Erc20);
      assert_eq!(got[1].kind, ApprovalKind::Permit2);
   }

   #[test]
   fn known_pairs_other_spender_are_skipped() {
      let got = collect_approval_candidates(
         owner(),
         token(),
         &Bytes::new(),
         &[],
         [(token(), spender())],
         [(token(), spender())],
      );
      assert!(got.is_empty());
   }

   #[test]
   fn skip_zero_spender_and_token() {
      let data = approve_calldata(Address::ZERO, U256::MAX);
      let got = collect_approval_candidates(
         owner(),
         Address::ZERO,
         &data,
         &[],
         [(Address::ZERO, spender()), (token(), Address::ZERO)],
         [],
      );
      assert!(got.is_empty());
   }

   #[test]
   fn candidates_cap_at_max() {
      let known: Vec<(Address, Address)> =
         (1u8..=80).map(|i| (Address::repeat_byte(i), spender())).collect();
      let got = collect_approval_candidates(owner(), spender(), &Bytes::new(), &[], known, []);
      assert_eq!(got.len(), MAX_APPROVAL_CANDIDATES);
   }

   #[test]
   fn equal_allowance_is_none() {
      assert!(
         ApprovalChange::from_wei(
            ApprovalKind::Erc20,
            wbtest(),
            spender(),
            U256::from(1u64),
            U256::from(1u64),
            NumericValue::default(),
            None,
            None
         )
         .is_none()
      );
   }

   #[test]
   fn revoke_and_unlimited() {
      let revoke = ApprovalChange::from_wei(
         ApprovalKind::Erc20,
         wbtest(),
         spender(),
         U256::MAX,
         U256::ZERO,
         NumericValue::default(),
         None,
         None,
      )
      .unwrap();
      assert!(revoke.is_revoke());
      assert!(!revoke.is_unlimited());
      assert!(!revoke.is_increase());

      let grant = ApprovalChange::from_wei(
         ApprovalKind::Erc20,
         wbtest(),
         spender(),
         U256::ZERO,
         U256::MAX,
         NumericValue::default(),
         None,
         None,
      )
      .unwrap();
      assert!(grant.is_unlimited());
      assert!(grant.is_increase());
   }

   #[test]
   fn permit2_unlimited_is_uint160_max() {
      assert!(unlimited_allowance(
         ApprovalKind::Permit2,
         U256::from(U160::MAX)
      ));
      assert!(!unlimited_allowance(
         ApprovalKind::Permit2,
         U256::MAX
      ));
   }

   #[test]
   fn permit2_expiry_only_is_some() {
      let change = ApprovalChange::from_wei(
         ApprovalKind::Permit2,
         wbtest(),
         spender(),
         U256::from(1u64),
         U256::from(1u64),
         NumericValue::default(),
         Some(100),
         Some(200),
      )
      .unwrap();
      assert!(!change.is_increase());
      assert!(!change.is_revoke());
      assert_eq!(
         change.expiration_after,
         Some(TimeStamp::Seconds(200))
      );
   }

   #[test]
   fn erc20_approval_log_owner_only() {
      let logs = [
         erc20_approval_log(token(), owner(), spender()),
         erc20_approval_log(token(), other_owner(), spender()),
      ];
      let got = collect_approval_candidates(owner(), token(), &Bytes::new(), &logs, [], []);
      assert_eq!(
         got,
         vec![ApprovalCandidate {
            kind: ApprovalKind::Erc20,
            token: token(),
            spender: spender(),
         }]
      );
   }

   #[test]
   fn permit2_permit_and_approval_logs() {
      let other_spender = address!("0x5555555555555555555555555555555555555555");
      let logs = [
         permit2_permit_log(owner(), token(), spender()),
         permit2_approval_log(owner(), token(), other_spender),
         permit2_permit_log(other_owner(), token(), spender()),
      ];
      let got = collect_approval_candidates(owner(), token(), &Bytes::new(), &logs, [], []);
      assert_eq!(
         got,
         vec![
            ApprovalCandidate {
               kind: ApprovalKind::Permit2,
               token: token(),
               spender: spender(),
            },
            ApprovalCandidate {
               kind: ApprovalKind::Permit2,
               token: token(),
               spender: other_spender,
            },
         ]
      );
   }

   #[test]
   fn approval_sorted_revokes_first() {
      let grant = ApprovalChange::from_wei(
         ApprovalKind::Erc20,
         wbtest(),
         spender(),
         U256::ZERO,
         U256::MAX,
         NumericValue::default(),
         None,
         None,
      )
      .unwrap();
      let decrease = ApprovalChange::from_wei(
         ApprovalKind::Erc20,
         wbtest(),
         other_owner(),
         U256::from(100u64),
         U256::from(50u64),
         NumericValue::default(),
         None,
         None,
      )
      .unwrap();
      let revoke = ApprovalChange::from_wei(
         ApprovalKind::Erc20,
         wbtest(),
         token(),
         U256::MAX,
         U256::ZERO,
         NumericValue::default(),
         None,
         None,
      )
      .unwrap();

      let diff = ApprovalDiff {
         changes: vec![grant.clone(), decrease.clone(), revoke.clone()],
         nft_changes: Vec::new(),
      };
      let sorted = diff.sorted();
      assert!(sorted[0].is_revoke());
      assert!(!sorted[1].is_revoke());
      assert!(!sorted[1].is_increase());
      assert!(sorted[2].is_increase());
      assert_eq!(sorted[0].spender, revoke.spender);
      assert_eq!(sorted[1].spender, decrease.spender);
      assert_eq!(sorted[2].spender, grant.spender);
   }

   fn collection() -> Address {
      address!("0xaf5aa7b670ef209e23d3f7b39a8f42f84bd002ac")
   }

   fn nft(
      operator: Address,
      before: NftApprovalValue,
      after: NftApprovalValue,
   ) -> NftApprovalChange {
      NftApprovalChange::from_state(
         collection(),
         operator,
         NftApprovalTarget::Token(U256::from(7)),
         NftStandard::Erc721,
         before,
         after,
      )
      .expect("a real delta")
   }

   #[test]
   fn an_nft_change_needs_a_real_delta() {
      assert!(
         NftApprovalChange::from_state(
            collection(),
            spender(),
            NftApprovalTarget::ForAll,
            NftStandard::Erc721,
            NftApprovalValue::ForAll(true),
            NftApprovalValue::ForAll(true),
         )
         .is_none(),
         "an unchanged flag is not a row"
      );
   }

   /// A per-token grant names the address that is now approved.
   #[test]
   fn a_per_token_grant_names_the_approved_address() {
      let change = nft(
         spender(),
         NftApprovalValue::Approved(Address::ZERO),
         NftApprovalValue::Approved(spender()),
      );

      assert_eq!(change.operator, spender());
      assert!(!change.is_revoke());
   }

   /// A per-token **revoke** carries the zero address in the log, so the row must name the address
   /// being removed instead — otherwise it reads "approved to 0x0", the opposite of what happened.
   #[test]
   fn a_per_token_revoke_names_the_address_it_removed() {
      let change = nft(
         Address::ZERO,
         NftApprovalValue::Approved(spender()),
         NftApprovalValue::Approved(Address::ZERO),
      );

      assert_eq!(change.operator, spender());
      assert!(change.is_revoke());
   }

   /// Switching operators is a change like any other, and the row names the new one.
   #[test]
   fn a_per_token_switch_names_the_new_operator() {
      let switched = nft(
         token(),
         NftApprovalValue::Approved(spender()),
         NftApprovalValue::Approved(token()),
      );

      assert_eq!(switched.operator, token());
      assert!(!switched.is_revoke());
   }

   /// The other two shapes keep the operator from the candidate: their probe is keyed by it, and
   /// their value never holds an address.
   #[test]
   fn collection_wide_and_allowance_rows_keep_their_operator() {
      let for_all = NftApprovalChange::from_state(
         collection(),
         spender(),
         NftApprovalTarget::ForAll,
         NftStandard::Erc721,
         NftApprovalValue::ForAll(false),
         NftApprovalValue::ForAll(true),
      )
      .unwrap();
      assert_eq!(for_all.operator, spender());
      assert!(!for_all.is_revoke());

      let allowance = NftApprovalChange::from_state(
         collection(),
         spender(),
         NftApprovalTarget::Allowance(U256::from(7)),
         NftStandard::Erc1155,
         NftApprovalValue::Allowance(U256::from(5)),
         NftApprovalValue::Allowance(U256::ZERO),
      )
      .unwrap();
      assert_eq!(allowance.operator, spender());
      assert!(allowance.is_revoke());
   }

   #[test]
   fn nft_rows_are_counted_and_sorted_revokes_first() {
      let grant = NftApprovalChange::from_state(
         collection(),
         spender(),
         NftApprovalTarget::ForAll,
         NftStandard::Erc1155,
         NftApprovalValue::ForAll(false),
         NftApprovalValue::ForAll(true),
      )
      .unwrap();
      let revoke = NftApprovalChange::from_state(
         collection(),
         token(),
         NftApprovalTarget::Allowance(U256::from(7)),
         NftStandard::Erc1155,
         NftApprovalValue::Allowance(U256::from(5)),
         NftApprovalValue::Allowance(U256::ZERO),
      )
      .unwrap();

      let diff = ApprovalDiff {
         changes: Vec::new(),
         nft_changes: vec![grant, revoke],
      };

      assert!(!diff.is_empty());
      assert_eq!(diff.len(), 2);
      assert!(
         diff.sorted().is_empty(),
         "NFT rows are not fungible rows"
      );

      let rows = diff.nft_sorted();
      assert!(rows[0].is_revoke());
      assert!(!rows[1].is_revoke());
   }

   /// A payload stored before NFT approval rows existed has no `nft_changes` key.
   #[test]
   fn a_payload_written_before_nft_approval_rows_still_loads() {
      let diff = ApprovalDiff::default();
      let mut json = serde_json::to_value(&diff).unwrap();
      json.as_object_mut().expect("an object").remove("nft_changes");

      let loaded: ApprovalDiff = serde_json::from_value(json).expect("an older payload loads");
      assert!(loaded.nft_changes.is_empty());
   }

   fn erc721_approval_log(collection: Address, owner: Address, approved: Address) -> Log {
      Log {
         address: collection,
         data: IERC721::Approval {
            owner,
            approved,
            tokenId: U256::from(7),
         }
         .encode_log_data(),
      }
   }

   fn approval_for_all_log(collection: Address, owner: Address, operator: Address) -> Log {
      Log {
         address: collection,
         data: IERC721::ApprovalForAll {
            owner,
            operator,
            approved: true,
         }
         .encode_log_data(),
      }
   }

   fn erc5216_approval_log(collection: Address, account: Address, operator: Address) -> Log {
      Log {
         address: collection,
         data: IERC5216::Approval {
            account,
            operator,
            id: U256::from(7),
            amount: U256::from(5),
         }
         .encode_log_data(),
      }
   }

   /// Every shape the tx emits becomes a candidate, and a per-token revoke — which logs the zero address —
   /// is kept rather than treated as a missing operator.
   ///
   /// The two per-token logs name the *same* id, so they are one candidate: the row is built from
   /// `getApproved(7)` and takes its operator from there, which makes the operator the row's value rather
   /// than part of its identity. Whichever log is seen first is the one that survives — the other would be
   /// the same probe and the same row.
   #[test]
   fn approval_logs_name_the_three_shapes() {
      let logs = [
         erc721_approval_log(collection(), owner(), Address::ZERO),
         erc721_approval_log(collection(), owner(), spender()),
         approval_for_all_log(collection(), owner(), spender()),
         erc5216_approval_log(collection(), owner(), spender()),
      ];

      let got = collect_nft_approval_candidates(owner(), token(), &Bytes::new(), &logs, []);
      assert_eq!(
         got,
         vec![
            NftApprovalCandidate {
               collection: collection(),
               operator: Address::ZERO,
               target: NftApprovalTarget::Token(U256::from(7)),
            },
            NftApprovalCandidate {
               collection: collection(),
               operator: spender(),
               target: NftApprovalTarget::ForAll,
            },
            NftApprovalCandidate {
               collection: collection(),
               operator: spender(),
               target: NftApprovalTarget::Allowance(U256::from(7)),
            },
         ]
      );
   }

   /// The reported case: an approval the store knows about, and the revoke the same transaction logs for
   /// it, are one row — not two.
   ///
   /// The store's entry carries the operator the approval *had*; the log names the zero address. Both probe
   /// `getApproved(7)` and both draw "… #7 → A Revoked", so probing both would put the row on screen twice.
   #[test]
   fn a_token_approval_the_store_knows_and_the_log_revokes_is_one_candidate() {
      let id = U256::from(7);

      // `token()` is the contract being called, which is what lets a stored approval through at all.
      let known = [NftApprovalCandidate {
         collection: collection(),
         operator: token(),
         target: NftApprovalTarget::Token(id),
      }];
      let revoke = erc721_approval_log(collection(), owner(), Address::ZERO);

      let got = collect_nft_approval_candidates(owner(), token(), &Bytes::new(), &[revoke], known);

      assert_eq!(got.len(), 1, "one id, one row: {got:?}");
      assert_eq!(got[0].target, NftApprovalTarget::Token(id));
   }

   /// The per-operator shapes keep the operator in the key: the same collection and id granted to two
   /// operators is two rows, because each grant belongs to its own operator.
   #[test]
   fn a_per_operator_shape_is_one_candidate_per_operator() {
      let logs = [
         erc5216_approval_log(collection(), owner(), token()),
         erc5216_approval_log(collection(), owner(), spender()),
      ];

      let got = collect_nft_approval_candidates(owner(), token(), &Bytes::new(), &logs, []);

      assert_eq!(
         got.len(),
         2,
         "an ERC-5216 allowance belongs to (owner, operator, id): {got:?}"
      );
      assert_ne!(got[0].operator, got[1].operator);
   }

   #[test]
   fn another_owners_approvals_are_not_candidates() {
      let logs = [
         erc721_approval_log(collection(), other_owner(), spender()),
         approval_for_all_log(collection(), other_owner(), spender()),
         erc5216_approval_log(collection(), other_owner(), spender()),
      ];

      assert!(
         collect_nft_approval_candidates(owner(), token(), &Bytes::new(), &logs, []).is_empty()
      );
   }

   /// Each of the three calls names its shape, on the contract being called.
   #[test]
   fn calldata_names_the_three_shapes() {
      let approve = erc721::encode_approve(spender(), U256::from(7));
      let got = collect_nft_approval_candidates(owner(), collection(), &approve, &[], []);
      assert_eq!(
         got,
         vec![NftApprovalCandidate {
            collection: collection(),
            operator: spender(),
            target: NftApprovalTarget::Token(U256::from(7)),
         }]
      );

      let for_all = erc721::encode_set_approval_for_all(spender(), true);
      let got = collect_nft_approval_candidates(owner(), collection(), &for_all, &[], []);
      assert_eq!(got[0].target, NftApprovalTarget::ForAll);

      let allowance = erc1155::encode_approve(spender(), U256::from(7), U256::from(5));
      let got = collect_nft_approval_candidates(owner(), collection(), &allowance, &[], []);
      assert_eq!(
         got[0].target,
         NftApprovalTarget::Allowance(U256::from(7))
      );
   }

   /// A revoke call carries the zero address, and the candidate keeps it: the combine step resolves
   /// the operator from the measured state, so zero here is what a revoke *is*.
   #[test]
   fn a_revoke_call_is_a_candidate_with_a_zero_operator() {
      let revoke = erc721::encode_approve(Address::ZERO, U256::from(7));

      let got = collect_nft_approval_candidates(owner(), collection(), &revoke, &[], []);
      assert_eq!(got.len(), 1);
      assert_eq!(got[0].operator, Address::ZERO);
   }

   /// Approvals the user made earlier are only probed when the operator is the contract being called
   /// — the rule that catches a consumed approval without walking every saved operator.
   #[test]
   fn known_approvals_only_for_the_called_contract() {
      let known = [
         NftApprovalCandidate {
            collection: collection(),
            operator: token(),
            target: NftApprovalTarget::Token(U256::from(7)),
         },
         NftApprovalCandidate {
            collection: collection(),
            operator: spender(),
            target: NftApprovalTarget::ForAll,
         },
      ];

      let got = collect_nft_approval_candidates(owner(), token(), &Bytes::new(), &[], known);
      assert_eq!(got.len(), 1);
      assert_eq!(
         got[0].target,
         NftApprovalTarget::Token(U256::from(7)),
         "only the one whose operator is the contract being called"
      );
   }

   #[test]
   fn nft_candidates_cap_at_max() {
      let known: Vec<NftApprovalCandidate> = (1u8..=80)
         .map(|i| NftApprovalCandidate {
            collection: Address::repeat_byte(i),
            operator: token(),
            target: NftApprovalTarget::ForAll,
         })
         .collect();

      let got = collect_nft_approval_candidates(owner(), token(), &Bytes::new(), &[], known);
      assert_eq!(got.len(), MAX_NFT_CANDIDATES);
   }

   /// An ERC-20 `Approval` has three topics where the ERC-721 one has four, so the two collections of
   /// approvals never decode into each other — the same separation the decode ladder relies on.
   #[test]
   fn an_erc20_approval_log_is_not_an_nft_one() {
      let erc20_log = erc20_approval_log(token(), owner(), spender());

      assert!(
         collect_nft_approval_candidates(owner(), token(), &Bytes::new(), &[erc20_log], [])
            .is_empty(),
         "a token approval must not become an NFT one"
      );
   }
}
