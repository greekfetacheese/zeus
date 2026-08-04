use crate::core::serde_hashmap;
use crate::core::{DecodedEvent, PermitParams, TokenApproveParams, TransactionRich};
use crate::utils::TimeStamp;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use zeus_eth::alloy_primitives::Address;
use zeus_eth::utils::NumericValue;

/// Latest ERC20 allowance for `(chain, owner, token, spender)`.
pub type TokenApprovals = HashMap<(u64, Address, Address, Address), TokenApproveParams>;

/// Latest Permit2 allowance for `(chain, owner, token, spender)`.
pub type PermitApprovals = HashMap<(u64, Address, Address, Address), PermitParams>;

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

   /// Drop approval entries whose owner is not in `wallets`.
   ///
   /// Returns `(token_approvals_removed, permits_removed)`.
   pub fn retain_wallets(&self, wallets: &HashSet<Address>) -> (usize, usize) {
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
}

impl ApprovalManager {
   pub fn new() -> Self {
      Self {
         token_approvals: HashMap::new(),
         permits: HashMap::new(),
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
      let now = TimeStamp::now_as_secs();
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
      let now = TimeStamp::now_as_secs();
      self
         .permits
         .values()
         .filter(|v| !is_zero_amount(&v.amount) && !permit_expired(&v.expiration, now))
         .cloned()
         .collect()
   }

   pub fn retain_wallets(&mut self, wallets: &HashSet<Address>) -> (usize, usize) {
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

      (token_removed, permit_removed)
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
