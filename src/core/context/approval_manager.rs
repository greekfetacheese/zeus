use crate::core::serde_hashmap;
use crate::core::{
   DecodedEvent, NftApproveParams, PermitParams, TokenApproveParams, TransactionRich,
};
use crate::utils::TimeStamp;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use zeus_eth::alloy_primitives::{Address, U256};
use zeus_eth::utils::NumericValue;

/// Latest ERC20 allowance for `(chain, owner, token, spender)`.
pub type TokenApprovals = HashMap<(u64, Address, Address, Address), TokenApproveParams>;

/// Latest Permit2 allowance for `(chain, owner, token, spender)`.
pub type PermitApprovals = HashMap<(u64, Address, Address, Address), PermitParams>;

/// Latest NFT approval for `(chain, owner, collection, token_id, operator)`.
///
/// `token_id` is `None` for a collection-wide `ApprovalForAll`, which is a **different key** from any
/// single token's approval: granting an operator the whole collection must not overwrite — or hide
/// behind — a grant on one token, since the two are revoked independently.
pub type NftApprovals = HashMap<(u64, Address, Address, Option<U256>, Address), NftApproveParams>;

#[derive(Clone)]
pub struct ApprovalManagerHandle(Arc<RwLock<ApprovalManager>>);

impl Default for ApprovalManagerHandle {
   fn default() -> Self {
      Self::new()
   }
}

impl Serialize for ApprovalManagerHandle {
   fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
   where
      S: serde::Serializer,
   {
      self.read(|m| m.serialize(serializer))
   }
}

impl<'de> Deserialize<'de> for ApprovalManagerHandle {
   fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
   where
      D: serde::Deserializer<'de>,
   {
      let manager = ApprovalManager::deserialize(deserializer)?;
      Ok(Self(Arc::new(RwLock::new(manager))))
   }
}

impl ApprovalManagerHandle {
   pub fn new() -> Self {
      Self(Arc::new(RwLock::new(ApprovalManager::new())))
   }

   pub fn read<R>(&self, reader: impl FnOnce(&ApprovalManager) -> R) -> R {
      reader(&self.0.read().unwrap())
   }

   pub fn write<R>(&self, writer: impl FnOnce(&mut ApprovalManager) -> R) -> R {
      writer(&mut self.0.write().unwrap())
   }

   /// Extract ERC20 / Permit2 approvals from a rich tx and store the latest state.
   ///
   /// Only successful transactions update state. For a given
   /// `(chain, owner, token, spender)` key the previous entry is replaced.
   pub fn add_from_tx(&self, tx: &TransactionRich) {
      self.write(|db| db.add_from_tx(tx))
   }

   pub fn get_token_approval(
      &self,
      chain: u64,
      owner: Address,
      token: Address,
      spender: Address,
   ) -> Option<TokenApproveParams> {
      self.read(|db| db.get_token_approval(chain, owner, token, spender).cloned())
   }

   pub fn get_token_approvals(&self, chain: u64, owner: Address) -> Vec<TokenApproveParams> {
      self.read(|db| db.get_token_approvals(chain, owner))
   }

   pub fn get_permit(
      &self,
      chain: u64,
      owner: Address,
      token: Address,
      spender: Address,
   ) -> Option<PermitParams> {
      self.read(|db| db.get_permit(chain, owner, token, spender).cloned())
   }

   pub fn get_permits(&self, chain: u64, owner: Address) -> Vec<PermitParams> {
      self.read(|db| db.get_permits(chain, owner))
   }

   /// Permits that still have a non-zero amount and have not expired.
   pub fn get_active_permits(&self, chain: u64, owner: Address) -> Vec<PermitParams> {
      self.read(|db| db.get_active_permits(chain, owner))
   }

   /// All ERC20 approvals with a non-zero allowance.
   pub fn get_all_active_token_approvals(&self) -> Vec<(u64, TokenApproveParams)> {
      self.read(|db| db.get_all_active_token_approvals())
   }

   /// All Permit2 allowances that still have a non-zero amount and have not expired.
   pub fn get_all_active_permits(&self) -> Vec<PermitParams> {
      self.read(|db| db.get_all_active_permits())
   }

   pub fn get_nft_approval(
      &self,
      chain: u64,
      owner: Address,
      collection: Address,
      token_id: Option<U256>,
      operator: Address,
   ) -> Option<NftApproveParams> {
      self.read(|db| db.get_nft_approval(chain, owner, collection, token_id, operator).cloned())
   }

   pub fn get_nft_approvals(&self, chain: u64, owner: Address) -> Vec<NftApproveParams> {
      self.read(|db| db.get_nft_approvals(chain, owner))
   }

   /// All NFT approvals that still grant something — revocations are kept but not returned.
   pub fn get_all_active_nft_approvals(&self) -> Vec<NftApproveParams> {
      self.read(|db| db.get_all_active_nft_approvals())
   }

   /// Drop approval entries whose owner is not in `wallets`.
   ///
   /// Returns `(token_approvals_removed, permits_removed, nft_approvals_removed)`.
   pub fn retain_wallets(&self, wallets: &HashSet<Address>) -> (usize, usize, usize) {
      self.write(|db| db.retain_wallets(wallets))
   }
}

/// Approval cache persisted inside the encrypted vault.
///
/// Only tracks approvals observed from transactions Zeus itself recorded
/// (same local-first model as [`super::TxDBHandle`]).
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ApprovalManager {
   /// Latest ERC20 `Approval` per chain / owner / token / spender.
   #[serde(default, with = "serde_hashmap")]
   token_approvals: TokenApprovals,

   /// Latest Permit2 `Permit` / `Approval` per chain / owner / token / spender.
   #[serde(default, with = "serde_hashmap")]
   permits: PermitApprovals,

   /// Latest NFT `Approval` / `ApprovalForAll` per chain / owner / collection / token / operator.
   ///
   /// Kept in its own map rather than folded into `token_approvals`: an NFT is identified by a
   /// collection *and* a token id, and an approval can cover a whole collection, so there is no
   /// address that could stand in for either.
   #[serde(default, with = "serde_hashmap")]
   nft_approvals: NftApprovals,
}

impl ApprovalManager {
   pub fn new() -> Self {
      Self {
         token_approvals: HashMap::new(),
         permits: HashMap::new(),
         nft_approvals: HashMap::new(),
      }
   }

   pub fn add_from_tx(&mut self, tx: &TransactionRich) {
      if !tx.success {
         return;
      }

      let chain = tx.chain;

      // main_event is stored separately from analysis.decoded_events
      self.apply_event(chain, &tx.main_event);
      for event in &tx.analysis.decoded_events {
         self.apply_event(chain, event);
      }
   }

   fn apply_event(&mut self, chain: u64, event: &DecodedEvent) {
      match event {
         DecodedEvent::TokenApprove(params) => self.insert_token_approval(chain, params.clone()),
         DecodedEvent::Permit(params) => self.insert_permit(params.clone()),
         DecodedEvent::NftApprove(params) => self.insert_nft_approval(chain, params.clone()),
         _ => {}
      }
   }

   fn insert_token_approval(&mut self, chain: u64, params: TokenApproveParams) {
      let key = (
         chain,
         params.owner,
         params.token.address,
         params.spender,
      );
      // Always keep the latest event for this key (including amount == 0 revoke).
      self.token_approvals.insert(key, params);
   }

   fn insert_permit(&mut self, params: PermitParams) {
      let key = (
         params.chain,
         params.owner,
         params.token.address(),
         params.spender,
      );
      // Latest Permit2 allowance / expiration wins for this key.
      self.permits.insert(key, params);
   }

   fn insert_nft_approval(&mut self, chain: u64, params: NftApproveParams) {
      // The chain comes from the transaction, not from the params' own field — the same way the
      // ERC-20 approval store reads it, and the reason a misfiled row cannot happen if the two ever
      // disagree.
      let key = (
         chain,
         params.owner,
         params.collection,
         params.token_id,
         params.operator,
      );

      // An ERC-721 token can be approved to **one** address at a time: `approve` overwrites the
      // previous one, and a revoke sets the zero address. The operator is therefore that record's
      // *value*, not part of its identity — and without clearing the previous entry first, a revoke
      // would land on a key of its own (the zero address) and leave the very grant it revoked still
      // looking active.
      //
      // The other two shapes are the opposite: ERC-5216 and `ApprovalForAll` give every operator its
      // own independent grant for the same collection / id, so their operator stays part of the key
      // and nothing is cleared here.
      if params.is_erc721_per_token() {
         let (owner, collection, token_id) = (params.owner, params.collection, params.token_id);
         self.nft_approvals.retain(|(c, o, col, id, _), _| {
            !(*c == chain && *o == owner && *col == collection && *id == token_id)
         });
      }

      // Always keep the latest event for this key, **revocations included**: the revoke is what makes
      // the entry inactive, so dropping it would leave the earlier grant looking live.
      self.nft_approvals.insert(key, params);
   }

   pub fn get_token_approval(
      &self,
      chain: u64,
      owner: Address,
      token: Address,
      spender: Address,
   ) -> Option<&TokenApproveParams> {
      self.token_approvals.get(&(chain, owner, token, spender))
   }

   pub fn get_token_approvals(&self, chain: u64, owner: Address) -> Vec<TokenApproveParams> {
      self
         .token_approvals
         .iter()
         .filter_map(|((c, o, _, _), v)| {
            if *c == chain && *o == owner {
               Some(v.clone())
            } else {
               None
            }
         })
         .collect()
   }

   pub fn get_permit(
      &self,
      chain: u64,
      owner: Address,
      token: Address,
      spender: Address,
   ) -> Option<&PermitParams> {
      self.permits.get(&(chain, owner, token, spender))
   }

   pub fn get_permits(&self, chain: u64, owner: Address) -> Vec<PermitParams> {
      self
         .permits
         .iter()
         .filter_map(|((c, o, _, _), v)| {
            if *c == chain && *o == owner {
               Some(v.clone())
            } else {
               None
            }
         })
         .collect()
   }

   pub fn get_active_permits(&self, chain: u64, owner: Address) -> Vec<PermitParams> {
      let now = TimeStamp::now_as_secs().unwrap_or_default();
      self
         .permits
         .iter()
         .filter_map(|((c, o, _, _), v)| {
            if *c != chain || *o != owner {
               return None;
            }
            if is_zero_amount(&v.amount) {
               return None;
            }
            if permit_expired(&v.expiration, now) {
               return None;
            }
            Some(v.clone())
         })
         .collect()
   }

   pub fn get_all_active_token_approvals(&self) -> Vec<(u64, TokenApproveParams)> {
      self
         .token_approvals
         .iter()
         .filter_map(|((chain, _, _, _), v)| {
            if is_zero_amount(&v.amount) {
               None
            } else {
               Some((*chain, v.clone()))
            }
         })
         .collect()
   }

   pub fn get_all_active_permits(&self) -> Vec<PermitParams> {
      let now = TimeStamp::now_as_secs().unwrap_or_default();
      self
         .permits
         .values()
         .filter(|v| !is_zero_amount(&v.amount) && !permit_expired(&v.expiration, now))
         .cloned()
         .collect()
   }

   pub fn get_nft_approval(
      &self,
      chain: u64,
      owner: Address,
      collection: Address,
      token_id: Option<U256>,
      operator: Address,
   ) -> Option<&NftApproveParams> {
      self.nft_approvals.get(&(chain, owner, collection, token_id, operator))
   }

   pub fn get_nft_approvals(&self, chain: u64, owner: Address) -> Vec<NftApproveParams> {
      self
         .nft_approvals
         .iter()
         .filter_map(|((c, o, _, _, _), v)| {
            if *c == chain && *o == owner {
               Some(v.clone())
            } else {
               None
            }
         })
         .collect()
   }

   /// Every NFT approval that still grants something.
   ///
   /// Revocations stay in the map — they are what makes an entry inactive — so they are filtered here
   /// rather than deleted, exactly as a zero ERC-20 allowance is.
   pub fn get_all_active_nft_approvals(&self) -> Vec<NftApproveParams> {
      self.nft_approvals.values().filter(|v| !v.is_revoke()).cloned().collect()
   }

   pub fn retain_wallets(&mut self, wallets: &HashSet<Address>) -> (usize, usize, usize) {
      let token_before = self.token_approvals.len();
      self
         .token_approvals
         .retain(|(_chain, owner, _token, _spender), _| wallets.contains(owner));
      self.token_approvals.shrink_to_fit();
      let token_removed = token_before.saturating_sub(self.token_approvals.len());

      let permit_before = self.permits.len();
      self
         .permits
         .retain(|(_chain, owner, _token, _spender), _| wallets.contains(owner));
      self.permits.shrink_to_fit();
      let permit_removed = permit_before.saturating_sub(self.permits.len());

      let nft_before = self.nft_approvals.len();
      self
         .nft_approvals
         .retain(|(_chain, owner, _collection, _token_id, _operator), _| wallets.contains(owner));
      self.nft_approvals.shrink_to_fit();
      let nft_removed = nft_before.saturating_sub(self.nft_approvals.len());

      (token_removed, permit_removed, nft_removed)
   }
}

fn is_zero_amount(amount: &NumericValue) -> bool {
   amount.is_zero()
}

/// Permit2 expirations are unix seconds. Treat equal timestamps as still valid.
fn permit_expired(expiration: &TimeStamp, now: TimeStamp) -> bool {
   let exp_secs = match expiration {
      TimeStamp::Seconds(s) => *s,
      TimeStamp::Millis(m) => m / 1000,
   };
   let now_secs = match now {
      TimeStamp::Seconds(s) => s,
      TimeStamp::Millis(m) => m / 1000,
   };
   exp_secs < now_secs
}

#[cfg(test)]
mod tests {
   use super::*;
   use crate::core::TransactionRich;
   use alloy_sol_types::SolEvent;
   use zeus_eth::{
      abi::{erc721::IERC721, erc1155::IERC5216},
      alloy_primitives::{Log, address},
      nft::NftStandard,
   };

   const OWNER: Address = address!("1111111111111111111111111111111111111111");
   const OTHER_OWNER: Address = address!("2222222222222222222222222222222222222222");
   const COLLECTION: Address = address!("BC4CA0EdA7647A8aB7C2061c2E118A18a936f13D");
   const OPERATOR: Address = address!("f39fd6e51aad88f6f4ce6ab8827279cfffb92266");

   /// These go through the real decoders, so the tests consume what the ladder emits rather than
   /// hand-built lookalikes — a key that disagrees with the decoder's output would otherwise go
   /// unnoticed here and only surface as a missing row in the UI.

   fn per_token(owner: Address, token_id: u64, approved: Address) -> NftApproveParams {
      let log = Log {
         address: COLLECTION,
         data: IERC721::Approval {
            owner,
            approved,
            tokenId: U256::from(token_id),
         }
         .encode_log_data(),
      };
      NftApproveParams::from_erc721_approval(1, &log).unwrap()
   }

   fn collection_wide(owner: Address, approved: bool) -> NftApproveParams {
      let log = Log {
         address: COLLECTION,
         data: IERC721::ApprovalForAll {
            owner,
            operator: OPERATOR,
            approved,
         }
         .encode_log_data(),
      };
      NftApproveParams::from_approval_for_all(1, &log).unwrap()
   }

   fn allowance(owner: Address, id: u64, amount: u64) -> NftApproveParams {
      let log = Log {
         address: COLLECTION,
         data: IERC5216::Approval {
            account: owner,
            operator: OPERATOR,
            id: U256::from(id),
            amount: U256::from(amount),
         }
         .encode_log_data(),
      };
      NftApproveParams::from_erc1155_approval(1, &log).unwrap()
   }

   /// A transaction carrying `events`, of which the first is also the main event — the split
   /// `add_from_tx` has to read, since `main_event` is stored apart from `analysis.decoded_events`.
   fn tx(events: Vec<DecodedEvent>) -> TransactionRich {
      let mut tx = TransactionRich::dummy_clear_signed();
      tx.chain = 1;
      tx.success = true;
      tx.main_event = events[0].clone();
      tx.analysis.decoded_events = events;
      tx
   }

   fn approved(events: Vec<NftApproveParams>) -> TransactionRich {
      tx(events.into_iter().map(DecodedEvent::NftApprove).collect())
   }

   /// Each shape is filed under a key that reads back what was put in.
   #[test]
   fn each_shape_is_stored_under_its_own_key() {
      let mut manager = ApprovalManager::new();
      manager.add_from_tx(&approved(vec![
         per_token(OWNER, 7, OPERATOR),
         collection_wide(OWNER, true),
         allowance(OWNER, 9, 5),
      ]));

      let token = manager
         .get_nft_approval(
            1,
            OWNER,
            COLLECTION,
            Some(U256::from(7)),
            OPERATOR,
         )
         .expect("the per-token approval");
      assert_eq!(token.standard, Some(NftStandard::Erc721));
      assert_eq!(token.token_id, Some(U256::from(7)));
      assert_eq!(token.operator, OPERATOR);

      let collection = manager
         .get_nft_approval(1, OWNER, COLLECTION, None, OPERATOR)
         .expect("the collection-wide approval");
      assert!(collection.is_collection_wide());
      assert_eq!(collection.approved, Some(true));

      let erc5216 = manager
         .get_nft_approval(
            1,
            OWNER,
            COLLECTION,
            Some(U256::from(9)),
            OPERATOR,
         )
         .expect("the ERC-5216 allowance");
      assert_eq!(erc5216.standard, Some(NftStandard::Erc1155));
      assert_eq!(erc5216.amount, Some(U256::from(5)));

      assert_eq!(manager.get_all_active_nft_approvals().len(), 3);
   }

   /// A reverted transaction approved nothing, so it must record nothing.
   #[test]
   fn a_failed_transaction_stores_nothing() {
      let mut failed = approved(vec![per_token(OWNER, 7, OPERATOR)]);
      failed.success = false;

      let mut manager = ApprovalManager::new();
      manager.add_from_tx(&failed);

      assert!(manager.get_nft_approvals(1, OWNER).is_empty());
      assert!(manager.get_all_active_nft_approvals().is_empty());
   }

   /// An ERC-721 approval is cleared by the **zero address**, so the revoke carries a different
   /// operator than the grant did. It still has to replace that grant: a revocation that left the
   /// grant behind would tell the user they had approved someone they had just un-approved.
   #[test]
   fn a_zero_address_revoke_replaces_the_grant_it_revokes() {
      let mut manager = ApprovalManager::new();
      manager.add_from_tx(&approved(vec![per_token(OWNER, 7, OPERATOR)]));
      assert_eq!(manager.get_all_active_nft_approvals().len(), 1);

      manager.add_from_tx(&approved(vec![per_token(
         OWNER,
         7,
         Address::ZERO,
      )]));

      assert_eq!(
         manager.get_nft_approvals(1, OWNER).len(),
         1,
         "the revoke replaces the grant, it does not sit beside it"
      );
      assert!(
         manager.get_all_active_nft_approvals().is_empty(),
         "nothing is approved after a revoke"
      );
   }

   /// Approving a *different* operator for the same token also replaces the previous one — an
   /// ERC-721 token has one approved address at a time.
   #[test]
   fn approving_another_operator_replaces_the_previous_one() {
      let mut manager = ApprovalManager::new();
      manager.add_from_tx(&approved(vec![per_token(OWNER, 7, OPERATOR)]));
      manager.add_from_tx(&approved(vec![per_token(OWNER, 7, OTHER_OWNER)]));

      assert_eq!(manager.get_nft_approvals(1, OWNER).len(), 1);

      let active = manager.get_all_active_nft_approvals();
      assert_eq!(active.len(), 1);
      assert_eq!(active[0].operator, OTHER_OWNER);
   }

   /// `ApprovalForAll` and a per-token grant for the same operator are two entries: they are granted
   /// and revoked independently, so folding them together would lose one of them.
   #[test]
   fn a_collection_wide_approval_and_a_per_token_one_coexist() {
      let mut manager = ApprovalManager::new();
      manager.add_from_tx(&approved(vec![
         collection_wide(OWNER, true),
         per_token(OWNER, 7, OPERATOR),
      ]));

      assert_eq!(manager.get_nft_approvals(1, OWNER).len(), 2);

      // Revoking the collection-wide one leaves the per-token grant standing.
      manager.add_from_tx(&approved(vec![collection_wide(OWNER, false)]));

      let active = manager.get_all_active_nft_approvals();
      assert_eq!(active.len(), 1);
      assert!(
         !active[0].is_collection_wide(),
         "the survivor is the per-token grant"
      );
      assert_eq!(
         manager.get_nft_approvals(1, OWNER).len(),
         2,
         "the revoked collection row is kept, just inactive"
      );
   }

   /// Two tokens of one collection, and every operator, are separate records.
   ///
   /// The ERC-5216 id is deliberately a *different* id from the ERC-721 token id: a collection is one
   /// standard, never both, but the key does not carry the standard (Zeus stores one per collection),
   /// so reusing the id here would assert on an unrepresentable state.
   #[test]
   fn tokens_and_operators_are_separate_entries() {
      let mut manager = ApprovalManager::new();
      manager.add_from_tx(&approved(vec![
         per_token(OWNER, 7, OPERATOR),
         per_token(OWNER, 8, OPERATOR),
         allowance(OWNER, 9, 5),
      ]));

      assert_eq!(manager.get_nft_approvals(1, OWNER).len(), 3);

      // Revoking token 7 leaves token 8 and the allowance alone.
      manager.add_from_tx(&approved(vec![per_token(
         OWNER,
         7,
         Address::ZERO,
      )]));

      let active = manager.get_all_active_nft_approvals();
      assert_eq!(active.len(), 2);
      assert!(active.iter().all(|a| a.token_id != Some(U256::from(7))));
   }

   /// An ERC-5216 allowance is per `(id, operator)`, so revoking one operator's allowance leaves
   /// another's untouched — this is where the key must *not* drop the operator.
   #[test]
   fn erc5216_allowances_are_per_operator() {
      let mut manager = ApprovalManager::new();
      manager.add_from_tx(&approved(vec![allowance(OWNER, 9, 5)]));
      assert_eq!(manager.get_all_active_nft_approvals().len(), 1);

      // A zero allowance is ERC-5216's revocation, and it keeps the same operator in the key.
      manager.add_from_tx(&approved(vec![allowance(OWNER, 9, 0)]));

      assert_eq!(manager.get_nft_approvals(1, OWNER).len(), 1);
      assert!(manager.get_all_active_nft_approvals().is_empty());
   }

   /// Another wallet's approvals are not this wallet's, on either axis.
   #[test]
   fn approvals_are_scoped_to_chain_and_owner() {
      let mut manager = ApprovalManager::new();
      manager.add_from_tx(&approved(vec![per_token(OWNER, 7, OPERATOR)]));

      assert!(manager.get_nft_approvals(1, OTHER_OWNER).is_empty());
      assert!(manager.get_nft_approvals(2, OWNER).is_empty());

      let mut other_chain = approved(vec![per_token(OWNER, 7, OPERATOR)]);
      other_chain.chain = 2;
      manager.add_from_tx(&other_chain);

      assert_eq!(manager.get_nft_approvals(1, OWNER).len(), 1);
      assert_eq!(manager.get_nft_approvals(2, OWNER).len(), 1);
      assert_eq!(manager.get_all_active_nft_approvals().len(), 2);
   }

   /// Removing a wallet takes its NFT approvals with it, and the counts come back.
   #[test]
   fn retain_wallets_drops_nft_approvals_and_counts_them() {
      let mut manager = ApprovalManager::new();
      manager.add_from_tx(&approved(vec![
         per_token(OWNER, 7, OPERATOR),
         collection_wide(OWNER, true),
      ]));

      let stranger = approved(vec![per_token(OTHER_OWNER, 7, OPERATOR)]);
      manager.add_from_tx(&stranger);

      assert_eq!(manager.get_all_active_nft_approvals().len(), 3);

      let (tokens, permits, nfts) = manager.retain_wallets(&HashSet::from([OWNER]));

      assert_eq!(
         (tokens, permits, nfts),
         (0, 0, 1),
         "only the stranger's row goes"
      );
      assert_eq!(manager.get_all_active_nft_approvals().len(), 2);
      assert!(manager.get_nft_approvals(1, OTHER_OWNER).is_empty());
   }

   /// The map is persisted inside the vault, and its key is a 5-tuple — which cannot key a JSON
   /// object, so `serde_hashmap` stringifies it. A round trip is the only thing that proves the key
   /// survives that, since the failure mode is a load-time error rather than a wrong row.
   #[test]
   fn nft_approvals_survive_a_serde_round_trip() {
      let mut manager = ApprovalManager::new();
      manager.add_from_tx(&approved(vec![
         per_token(OWNER, 7, OPERATOR),
         collection_wide(OWNER, true),
         allowance(OWNER, 9, 5),
      ]));

      let json = serde_json::to_string(&manager).unwrap();
      let loaded: ApprovalManager = serde_json::from_str(&json).unwrap();

      assert_eq!(loaded.get_all_active_nft_approvals().len(), 3);
      assert!(
         loaded
            .get_nft_approval(
               1,
               OWNER,
               COLLECTION,
               Some(U256::from(7)),
               OPERATOR
            )
            .is_some()
      );
      assert!(loaded.get_nft_approval(1, OWNER, COLLECTION, None, OPERATOR).is_some());
   }

   /// A vault written before NFT approvals existed has no `nft_approvals` key at all, and must still
   /// load — with everything else it did have.
   #[test]
   fn a_payload_written_before_nft_approvals_still_loads() {
      let mut manager = ApprovalManager::new();
      manager.add_from_tx(&approved(vec![per_token(OWNER, 7, OPERATOR)]));

      let mut json = serde_json::to_value(&manager).unwrap();
      json.as_object_mut().expect("an object").remove("nft_approvals");

      let loaded: ApprovalManager =
         serde_json::from_value(json).expect("an older payload still loads");

      assert!(
         loaded.get_all_active_nft_approvals().is_empty(),
         "the missing field defaults to empty"
      );
   }
}
