//! Signer ERC-20 / Permit2 allowance changes from simulation state, not logs.
//!
//! Log `Approval` amounts are untrusted. Amounts here come from `allowance()`.

use crate::utils::TimeStamp;
use serde::{Deserialize, Serialize};
use zeus_eth::{
   abi::{erc20, permit},
   alloy_primitives::{Address, Bytes, Log, U256, aliases::U160},
   currency::{Currency, ERC20Token},
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
      expiration_after: Option<u64>,
   ) -> Option<Self> {
      if before == after {
         return None;
      }
      let decimals = token.decimals;
      Some(Self {
         kind,
         token: Currency::from(token),
         spender,
         before: NumericValue::format_wei(before, decimals),
         after: NumericValue::format_wei(after, decimals),
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ApprovalDiff {
   pub changes: Vec<ApprovalChange>,
}

impl ApprovalDiff {
   pub fn is_empty(&self) -> bool {
      self.changes.is_empty()
   }

   /// Revokes first, then remaining grants.
   pub fn sorted(&self) -> Vec<&ApprovalChange> {
      let mut rows: Vec<&ApprovalChange> = self.changes.iter().collect();
      rows.sort_by_key(|c| (!c.is_revoke(), c.is_increase()));
      rows
   }
}

pub fn unlimited_allowance(kind: ApprovalKind, amount: U256) -> bool {
   match kind {
      ApprovalKind::Erc20 => amount == U256::MAX,
      ApprovalKind::Permit2 => amount == U256::from(U160::MAX),
   }
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
   use zeus_eth::alloy_primitives::address;

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
}
