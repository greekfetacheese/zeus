//! The diff tests, against in-memory and reverting EVM fixtures.

use super::probe::*;
use super::state::*;
use crate::sim::approval_diff::NftApprovalTarget;
use crate::sim::balance_diff::NftCandidate;
use zeus_eth::nft::NftStandard;
use zeus_eth::revm_utils::{Database, Evm2};

use super::*;
use zeus_eth::revm::database::{CacheDB, EmptyDB, InMemoryDB};
use zeus_eth::revm::primitives::B256;
use zeus_eth::revm::state::{AccountInfo, Bytecode};
use zeus_eth::revm_utils::fork_db::error::DatabaseError;
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

   new_evm(ChainId::Ethereum, None, db, None)
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

/// An EVM whose `collection` answers every call with a fixed word — the after-probe's success path.
fn fixed_word_collection_evm(collection: Address, word: U256) -> Evm2<InMemoryDB> {
   let mut db = CacheDB::new(EmptyDB::default());
   // PUSH32 <word> PUSH1 0x00 MSTORE PUSH1 0x20 PUSH1 0x00 RETURN
   let mut code = vec![0x7f];
   code.extend_from_slice(&word.to_be_bytes::<32>());
   code.extend_from_slice(&[0x60, 0x00, 0x52, 0x60, 0x20, 0x60, 0x00, 0xf3]);
   db.insert_account_info(
      collection,
      AccountInfo {
         code: Some(Bytecode::new_raw(Bytes::from(code))),
         ..Default::default()
      },
   );

   new_evm(ChainId::Ethereum, None, db, None)
}

/// A failed ERC-1155 after-probe is *unknown*, so it claims no change: the key stays out of the
/// after-state and `combine_diffs` drops the row rather than comparing the signer's real balance
/// against a fabricated zero.
///
/// Red against the old `unwrap_or_default()`: the first assertion finds `Some(&U256::ZERO)`, and the
/// diff then carries a full-loss row for a balance nobody measured.
#[test]
fn a_failed_1155_after_probe_claims_no_change() {
   let collection = addr(9);
   let owner = addr(1);
   let key = (collection, U256::from(1));
   let balances = balance_request(nft_candidate(collection, 1, NftStandard::Erc1155));

   let mut evm = reverting_collection_evm(collection);
   let after = measure_nft_after(owner, &balances, &mut evm);

   assert!(
      !after.balances1155.contains_key(&key),
      "a probe that could not answer is a hole, not a zero"
   );

   let mut before = BeforeState::empty();
   before.nft.balances1155.insert(key, U256::from(5));

   let raw = combine_diffs(&[], &[], &balances, before, nft_after(after));

   assert!(
      raw.nft_balances.is_empty(),
      "an unreadable side yields no row, so the failure cannot claim a loss"
   );
}

/// The success path is unchanged: a probe that answers still produces the row it always did.
#[test]
fn an_1155_after_probe_that_answers_still_yields_the_row() {
   let collection = addr(9);
   let owner = addr(1);
   let key = (collection, U256::from(3));
   let balances = balance_request(nft_candidate(collection, 3, NftStandard::Erc1155));

   let mut evm = fixed_word_collection_evm(collection, U256::from(2));
   let after = measure_nft_after(owner, &balances, &mut evm);

   assert_eq!(after.balances1155.get(&key), Some(&U256::from(2)));

   let mut before = BeforeState::empty();
   before.nft.balances1155.insert(key, U256::from(5));

   let raw = combine_diffs(&[], &[], &balances, before, nft_after(after));

   assert_eq!(raw.nft_balances.len(), 1);
   assert_eq!(raw.nft_balances[0].before, U256::from(5));
   assert_eq!(raw.nft_balances[0].after, U256::from(2));
   assert!(!raw.nft_balances[0].is_received());
}

/// A `Database` that answers nothing: every account/read access fails, which is the fork read a
/// post-sim probe makes when the RPC never answered. `measure_nft_after` goes through
/// `evm.transact`, so the probes report this as "never ran".
#[derive(Debug, Default)]
struct UnreachableDb;

impl Database for UnreachableDb {
   type Error = DatabaseError;

   fn basic(&mut self, _address: Address) -> Result<Option<AccountInfo>, Self::Error> {
      Err(DatabaseError::BackendFetchRequestError)
   }

   fn code_by_hash(&mut self, _code_hash: B256) -> Result<Bytecode, Self::Error> {
      Err(DatabaseError::BackendFetchRequestError)
   }

   fn storage(&mut self, _address: Address, _index: U256) -> Result<U256, Self::Error> {
      Err(DatabaseError::BackendFetchRequestError)
   }

   fn block_hash(&mut self, _number: u64) -> Result<B256, Self::Error> {
      Err(DatabaseError::BackendFetchRequestError)
   }
}

/// A fork read that never ran is *unknown*, not the shape's own nothing — for all three approval
/// probes.
///
/// This is the defect: with the old `unwrap_or_default` every shape fabricated a value for a read
/// that never reached the contract, and a grant whose after-probe never ran rendered as
/// **Revoked**. Red against the pass-through: the first assertion finds three inserted defaults.
#[test]
fn an_approval_read_that_never_ran_claims_no_change() {
   let collection = addr(9);
   let owner = addr(1);
   let token = NftApprovalCandidate {
      collection,
      operator: addr(7),
      target: NftApprovalTarget::Token(U256::from(4)),
   };
   let for_all = NftApprovalCandidate {
      collection,
      operator: addr(7),
      target: NftApprovalTarget::ForAll,
   };
   let allowance = NftApprovalCandidate {
      collection,
      operator: addr(7),
      target: NftApprovalTarget::Allowance(U256::from(5)),
   };
   let request = NftRequest {
      balances: Vec::new(),
      approvals: vec![token, for_all, allowance],
   };

   let mut evm = new_evm(ChainId::Ethereum, None, UnreachableDb, None);
   let after = measure_nft_after(owner, &request, &mut evm);

   assert!(
      after.approvals.is_empty(),
      "a read that never ran is a hole, not a fabricated default: {:?}",
      after.approvals
   );

   // With a readable before value, the failed read must not become a row.
   let mut before = BeforeState::empty();
   before.nft.approvals.insert(token, NftApprovalValue::Approved(addr(3)));
   before.nft.approvals.insert(for_all, NftApprovalValue::ForAll(true));
   before.nft.approvals.insert(
      allowance,
      NftApprovalValue::Allowance(U256::from(9)),
   );

   let raw = combine_diffs(&[], &[], &request, before, nft_after(after));

   assert!(
      raw.nft_approvals.is_empty(),
      "an unreadable side yields no row, so a fork failure cannot claim a change"
   );
}

/// The `ForAll` shape keeps the pinned rule: a reverted read is a real answer, so it reads as
/// `false` rather than a hole.
#[test]
fn a_reverted_for_all_read_after_the_tx_reads_as_false() {
   let collection = addr(9);
   let owner = addr(1);
   let candidate = NftApprovalCandidate {
      collection,
      operator: addr(7),
      target: NftApprovalTarget::ForAll,
   };

   let mut evm = reverting_collection_evm(collection);
   let after = measure_nft_after(owner, &approval_request(candidate), &mut evm);

   assert_eq!(
      after.approvals.get(&candidate),
      Some(&NftApprovalValue::ForAll(false))
   );
}

/// The `Allowance` shape keeps the pinned rule: a reverted read reads as zero, not a hole.
#[test]
fn a_reverted_allowance_read_after_the_tx_reads_as_zero() {
   let collection = addr(9);
   let owner = addr(1);
   let candidate = NftApprovalCandidate {
      collection,
      operator: addr(7),
      target: NftApprovalTarget::Allowance(U256::from(5)),
   };

   let mut evm = reverting_collection_evm(collection);
   let after = measure_nft_after(owner, &approval_request(candidate), &mut evm);

   assert_eq!(
      after.approvals.get(&candidate),
      Some(&NftApprovalValue::Allowance(U256::ZERO))
   );
}

/// The success path is unchanged for all three approval shapes: a probe that answers yields its
/// value, not the shape's nothing.
#[test]
fn an_approval_after_probe_that_answers_still_yields_the_row() {
   let collection = addr(9);
   let owner = addr(1);
   let token = NftApprovalCandidate {
      collection,
      operator: addr(7),
      target: NftApprovalTarget::Token(U256::from(4)),
   };
   let for_all = NftApprovalCandidate {
      collection,
      operator: addr(7),
      target: NftApprovalTarget::ForAll,
   };
   let allowance = NftApprovalCandidate {
      collection,
      operator: addr(7),
      target: NftApprovalTarget::Allowance(U256::from(5)),
   };

   let mut evm = fixed_word_collection_evm(collection, U256::from(11));
   let after = measure_nft_after(owner, &approval_request(token), &mut evm);
   assert_eq!(
      after.approvals.get(&token),
      Some(&NftApprovalValue::Approved(
         Address::with_last_byte(11)
      ))
   );

   let mut evm = fixed_word_collection_evm(collection, U256::from(1));
   let after = measure_nft_after(owner, &approval_request(for_all), &mut evm);
   assert_eq!(
      after.approvals.get(&for_all),
      Some(&NftApprovalValue::ForAll(true))
   );

   let mut evm = fixed_word_collection_evm(collection, U256::from(7));
   let after = measure_nft_after(owner, &approval_request(allowance), &mut evm);
   assert_eq!(
      after.approvals.get(&allowance),
      Some(&NftApprovalValue::Allowance(U256::from(7)))
   );
}
