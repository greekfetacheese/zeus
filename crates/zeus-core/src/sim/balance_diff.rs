//! Signer ETH / ERC-20 balance changes from simulation state, not logs.
//!
//! Log `Transfer` values are untrusted (eg. a Fake Airdrop). Amounts here
//! come from native account balance and ERC-20 `balanceOf`.

use serde::{Deserialize, Serialize};
use zeus_eth::{
   abi::{erc721, erc1155},
   alloy_primitives::{Address, Log, U256},
   currency::{Currency, ERC20Token, NativeCurrency},
   nft::NftStandard,
   utils::NumericValue,
};

/// Max token contracts to probe per tx (portfolio + interact_to + log addresses).
pub const MAX_TOKEN_CANDIDATES: usize = 64;

/// Max NFT candidates per tx, shared by the balance and approval collectors.
///
/// One cap for both because a `TransferBatch` can name many ids in a single log: the limit has to be
/// applied *while expanding*, not per log, or one log could blow past any per-log budget.
pub const MAX_NFT_CANDIDATES: usize = 64;

/// One asset whose signer balance changed across the simulated tx.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BalanceChange {
   pub currency: Currency,
   /// USD price of the currency at the time of the tx.
   #[serde(default)]
   pub price: NumericValue,
   pub before: NumericValue,
   pub after: NumericValue,
}

impl BalanceChange {
   pub fn from_wei(
      currency: Currency,
      price: NumericValue,
      before: U256,
      after: U256,
   ) -> Option<Self> {
      if before == after {
         return None;
      }
      let decimals = currency.decimals();
      Some(Self {
         currency,
         price,
         before: NumericValue::format_wei(before, decimals),
         after: NumericValue::format_wei(after, decimals),
      })
   }

   pub fn is_increase(&self) -> bool {
      self.after.wei() > self.before.wei()
   }

   pub fn abs_delta(&self) -> NumericValue {
      let decimals = self.currency.decimals();
      let (hi, lo) = if self.after.wei() > self.before.wei() {
         (self.after.wei(), self.before.wei())
      } else {
         (self.before.wei(), self.after.wei())
      };
      NumericValue::format_wei(hi.saturating_sub(lo), decimals)
   }
}

/// One NFT whose signer ownership changed across the simulated tx.
///
/// Not a [`BalanceChange`], and not a `Currency`: an NFT has no decimals and no USD price, and what
/// moved is ownership of a *specific* id rather than an amount of a fungible thing. `before`/`after`
/// are therefore the shape's own quantity — `0`/`1` for ERC-721 (whether the signer owns it) and a
/// count for ERC-1155.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NftBalanceChange {
   pub collection: Address,
   pub token_id: U256,
   pub standard: NftStandard,
   pub before: U256,
   pub after: U256,
}

impl NftBalanceChange {
   /// Build a change, or `None` when ownership did not move — the same "no row for no delta"
   /// contract [`BalanceChange::from_wei`] has.
   pub fn new(
      collection: Address,
      token_id: U256,
      standard: NftStandard,
      before: U256,
      after: U256,
   ) -> Option<Self> {
      if before == after {
         return None;
      }
      Some(Self {
         collection,
         token_id,
         standard,
         before,
         after,
      })
   }

   /// Whether the signer came out of this holding it — a mint, a receive, or a batch that grew.
   pub fn is_received(&self) -> bool {
      self.after > self.before
   }

   /// How many moved. Always at least 1 for ERC-721.
   pub fn abs_delta(&self) -> U256 {
      if self.after > self.before {
         self.after - self.before
      } else {
         self.before - self.after
      }
   }
}

/// Signer-side balance outcome of a simulated transaction.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BalanceDiff {
   pub native: Option<BalanceChange>,
   pub tokens: Vec<BalanceChange>,
   /// NFT ownership changes, kept apart from `tokens` because a row here is a collection + an id
   /// rather than a currency + an amount. `serde(default)` so an analysis stored before NFT rows
   /// existed still loads.
   #[serde(default)]
   pub nfts: Vec<NftBalanceChange>,
}

impl BalanceDiff {
   pub fn is_empty(&self) -> bool {
      self.native.is_none() && self.tokens.is_empty() && self.nfts.is_empty()
   }

   pub fn len(&self) -> usize {
      self.native.is_some() as usize + self.tokens.len() + self.nfts.len()
   }

   /// Native first, then token rows. Outflows before inflows within tokens.
   pub fn changes(&self) -> Vec<&BalanceChange> {
      let mut tokens: Vec<&BalanceChange> = self.tokens.iter().collect();
      tokens.sort_by_key(|c| c.is_increase());
      let mut out = Vec::with_capacity(tokens.len() + usize::from(self.native.is_some()));
      if let Some(native) = self.native.as_ref() {
         out.push(native);
      }
      out.extend(tokens);
      out
   }

   /// NFT rows, sent before received — the same "outflows first" convention [`Self::changes`] uses
   /// for tokens, so a row you are losing is never below one you are gaining.
   pub fn nft_changes(&self) -> Vec<&NftBalanceChange> {
      let mut rows: Vec<&NftBalanceChange> = self.nfts.iter().collect();
      rows.sort_by_key(|c| c.is_received());
      rows
   }
}

pub fn native_change(
   chain: u64,
   price: NumericValue,
   before: U256,
   after: U256,
) -> Option<BalanceChange> {
   let currency = Currency::from(NativeCurrency::from(chain));
   BalanceChange::from_wei(currency, price, before, after)
}

pub fn token_change(
   token: ERC20Token,
   price: NumericValue,
   before: U256,
   after: U256,
) -> Option<BalanceChange> {
   BalanceChange::from_wei(Currency::from(token), price, before, after)
}

/// Which contracts a diff probe reads.
///
/// The default, `DiffWatch::Discovered`, is what every path has always done: the signer's whole
/// portfolio plus whatever the call and its logs name. That is the safe shape for a callee nobody
/// vetted — a malicious token's `transfer` runs arbitrary code and can move a token the call never
/// mentions, and only the portfolio sweep sees it.
///
/// `DiffWatch::Exact` is for the flows whose movement is known before the fork runs: Zeus builds the
/// calldata itself, to a router or protocol it names, and the set of contracts the signer can gain or lose
/// is exactly the one the flow can list. It probes those and the call target, and nothing else.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum DiffWatch {
   /// The portfolio, the call target and every log address. Today's behaviour, and the default.
   #[default]
   Discovered,
   /// Only these contracts: each one is probed for the signer's balance, and each pair for its allowance.
   Exact {
      tokens: Vec<Address>,
      /// `(token, spender)` allowances to read. The token side is read for its balance as well.
      approvals: Vec<(Address, Address)>,
   },
}

impl DiffWatch {
   /// The contracts the watch names, for a flow that knows them before it builds its calldata.
   pub fn exact(tokens: impl IntoIterator<Item = Address>) -> Self {
      let tokens = tokens.into_iter().filter(|token| !token.is_zero()).collect();

      Self::Exact {
         tokens: dedup(tokens),
         approvals: Vec::new(),
      }
   }

   /// Add the allowances to read, `(token, spender)`. A token joins the balance list too: an allowance
   /// change is always reported beside the balance it belongs to.
   #[must_use]
   pub fn with_approvals(
      mut self,
      approvals: impl IntoIterator<Item = (Address, Address)>,
   ) -> Self {
      let approvals: Vec<(Address, Address)> = approvals.into_iter().collect();

      if let Self::Exact {
         tokens,
         approvals: held,
      } = &mut self
      {
         for (token, _) in &approvals {
            tokens.push(*token);
         }

         *tokens = dedup(std::mem::take(tokens));
         *held = dedup_pairs(approvals);
      }

      self
   }

   /// Whether the probe should look for addresses it was not told about.
   pub fn discovers(&self) -> bool {
      matches!(self, Self::Discovered)
   }
}

/// The same addresses in order, first occurrence kept.
fn dedup(addresses: Vec<Address>) -> Vec<Address> {
   let mut out = Vec::with_capacity(addresses.len());

   for address in addresses {
      if !out.contains(&address) {
         out.push(address);
      }
   }

   out
}

fn dedup_pairs(pairs: Vec<(Address, Address)>) -> Vec<(Address, Address)> {
   let mut out = Vec::with_capacity(pairs.len());

   for pair in pairs {
      if !out.contains(&pair) {
         out.push(pair);
      }
   }

   out
}
/// Token contracts to `balanceOf` for the signer.
///
/// Portfolio holdings catch silent drains. `interact_to` and log addresses
/// catch unknown inbound tokens. Values of those logs are ignored.
pub fn collect_token_candidates(
   portfolio: impl IntoIterator<Item = Address>,
   interact_to: Address,
   log_addresses: impl IntoIterator<Item = Address>,
) -> Vec<Address> {
   let mut out = Vec::new();
   for addr in portfolio.into_iter().chain(std::iter::once(interact_to)).chain(log_addresses) {
      if addr.is_zero() {
         continue;
      }
      if out.contains(&addr) {
         continue;
      }
      out.push(addr);
      if out.len() >= MAX_TOKEN_CANDIDATES {
         break;
      }
   }
   out
}

/// One NFT to probe the signer's ownership of.
///
/// The standard travels with the candidate because it is the log (or the held token's own metadata)
/// that knows it: `Transfer` is ERC-721's, `TransferSingle`/`TransferBatch` are ERC-1155's. Without
/// it the probe would have to be guessed or doubled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NftCandidate {
   pub collection: Address,
   pub token_id: U256,
   pub standard: NftStandard,
}

/// The NFT ownership candidates a `(call_data, logs)` pair implies.
///
/// The signer's side of a transfer log is what names them: `Transfer` (ERC-721), `TransferSingle`
/// and every id of a `TransferBatch` — a batch carries all of its ids in one log, so the cap is
/// applied while expanding. Mints and burns come along for free, since the signer is on one side of
/// those too.
///
/// The ids the signer already holds are appended, the same way [`collect_token_candidates`] appends
/// the portfolio: they cost one batched call between them, and they are what still reports a loss
/// when the simulation yielded no usable logs.
///
/// The log shapes cannot be confused with an ERC-20's: ERC-721 indexes `tokenId`, so its `Transfer`
/// carries four topics where an ERC-20 `Transfer` carries three, and the ERC-1155 events have topics
/// of their own. Only a collection that emitted the right event becomes a candidate at all.
pub fn collect_nft_balance_candidates(
   owner: Address,
   logs: &[Log],
   held: impl IntoIterator<Item = NftCandidate>,
) -> Vec<NftCandidate> {
   let mut out: Vec<NftCandidate> = Vec::new();

   for log in logs {
      // ERC-721 `Transfer(from, to, tokenId)`. The three shapes cannot decode into each other — their
      // topics and their data lengths differ — so one log can only be one of them.
      if let Ok(transfer) = erc721::decode_transfer_log(log) {
         if transfer.from == owner || transfer.to == owner {
            push_nft_candidate(
               &mut out,
               NftCandidate {
                  collection: log.address,
                  token_id: transfer.tokenId,
                  standard: NftStandard::Erc721,
               },
            );
         }
         continue;
      }

      if let Ok(single) = erc1155::decode_transfer_single_log(log) {
         if single.from == owner || single.to == owner {
            push_nft_candidate(
               &mut out,
               NftCandidate {
                  collection: log.address,
                  token_id: single.id,
                  standard: NftStandard::Erc1155,
               },
            );
         }
         continue;
      }

      if let Ok(batch) = erc1155::decode_transfer_batch_log(log)
         && (batch.from == owner || batch.to == owner)
      {
         for id in batch.ids {
            push_nft_candidate(
               &mut out,
               NftCandidate {
                  collection: log.address,
                  token_id: id,
                  standard: NftStandard::Erc1155,
               },
            );
         }
      }
   }

   for candidate in held {
      push_nft_candidate(&mut out, candidate);
   }

   out
}

fn push_nft_candidate(out: &mut Vec<NftCandidate>, candidate: NftCandidate) {
   if candidate.collection.is_zero() {
      return;
   }
   if out.contains(&candidate) {
      return;
   }
   if out.len() >= MAX_NFT_CANDIDATES {
      return;
   }
   out.push(candidate);
}

#[cfg(test)]
mod tests {
   use super::*;
   use zeus_eth::{
      abi::{erc721::IERC721, erc1155::IERC1155},
      alloy_primitives::address,
      alloy_sol_types::SolEvent,
   };

   fn wbtest() -> ERC20Token {
      ERC20Token::from_components(
         1,
         address!("0x1111111111111111111111111111111111111111"),
         "WBTEST",
         "Walletbeat Testing ERC20",
         18,
         U256::ZERO,
      )
   }

   #[test]
   fn candidates_skip_zero_dedupe_and_cap() {
      let portfolio = [
         address!("0x1111111111111111111111111111111111111111"),
         Address::ZERO,
      ];
      let interact = address!("0xB64D604640963d37c7A5c20e2d9551E4532ef841");
      let logs = [
         address!("0x1111111111111111111111111111111111111111"),
         interact,
         address!("0x2222222222222222222222222222222222222222"),
         Address::ZERO,
      ];
      let got = collect_token_candidates(portfolio, interact, logs);
      assert_eq!(
         got,
         vec![
            address!("0x1111111111111111111111111111111111111111"),
            interact,
            address!("0x2222222222222222222222222222222222222222"),
         ]
      );

      // Case: candidates_cap_at_max
      {
         let portfolio: Vec<Address> = (1u8..=80).map(Address::repeat_byte).collect();
         let got = collect_token_candidates(portfolio, Address::ZERO, []);
         assert_eq!(got.len(), MAX_TOKEN_CANDIDATES);
      }
   }

   #[test]
   fn native_and_token_amounts_are_formatted_in_their_own_decimals() {
      let change = native_change(
         1,
         NumericValue::default(),
         U256::ZERO,
         U256::from(10u64).pow(U256::from(18u64)),
      )
      .unwrap();
      assert!(change.is_increase());
      assert_eq!(change.currency.symbol(), "ETH");
      assert_eq!(change.abs_delta().f64(), 1.0);

      // Case: token_burn_is_decrease
      {
         let one = U256::from(10u64).pow(U256::from(18u64));
         let hundred = one * U256::from(100u64);
         let change = token_change(
            wbtest(),
            NumericValue::default(),
            hundred,
            U256::ZERO,
         )
         .unwrap();
         assert!(!change.is_increase());
         assert_eq!(change.abs_delta().f64(), 100.0);
         assert_eq!(change.currency.symbol(), "WBTEST");
      }
   }

   #[test]
   fn a_row_needs_a_real_change_in_wei() {
      // Fake Airdrop: Transfer(0x0 → signer, 1e18) on a non-token does not
      // change balanceOf. Equal wei → no row, regardless of the log.
      assert!(
         token_change(
            wbtest(),
            NumericValue::default(),
            U256::ZERO,
            U256::ZERO
         )
         .is_none()
      );

      // Case: native_equal_is_none
      {
         assert!(
            native_change(
               1,
               NumericValue::default(),
               U256::from(1u64),
               U256::from(1u64)
            )
            .is_none()
         );
      }
   }

   #[test]
   fn empty_diff() {
      assert!(BalanceDiff::default().is_empty());
      let mut diff = BalanceDiff::default();
      diff.tokens.push(
         token_change(
            wbtest(),
            NumericValue::default(),
            U256::from(1u64),
            U256::ZERO,
         )
         .unwrap(),
      );
      assert!(!diff.is_empty());
   }

   fn token_b() -> ERC20Token {
      ERC20Token::from_components(
         1,
         address!("0x2222222222222222222222222222222222222222"),
         "TB",
         "Token B",
         18,
         U256::ZERO,
      )
   }

   #[test]
   fn changes_native_first_outflows_before_inflows() {
      let native = native_change(
         1,
         NumericValue::default(),
         U256::from(2u64),
         U256::from(1u64),
      )
      .unwrap();
      let inflow = token_change(
         wbtest(),
         NumericValue::default(),
         U256::ZERO,
         U256::from(5u64),
      )
      .unwrap();
      let outflow = token_change(
         token_b(),
         NumericValue::default(),
         U256::from(9u64),
         U256::from(1u64),
      )
      .unwrap();

      let diff = BalanceDiff {
         native: Some(native.clone()),
         tokens: vec![inflow.clone(), outflow.clone()],
         nfts: Vec::new(),
      };
      let rows = diff.changes();
      assert_eq!(rows.len(), 3);
      assert_eq!(rows[0].currency.symbol(), "ETH");
      assert!(!rows[1].is_increase());
      assert_eq!(rows[1].currency.symbol(), "TB");
      assert!(rows[2].is_increase());
      assert_eq!(rows[2].currency.symbol(), "WBTEST");
   }

   fn collection() -> Address {
      address!("0x3E6F909dDBD068c6299ee2A47AD9FE44760D61E0")
   }

   #[test]
   fn an_nft_row_needs_a_real_delta() {
      assert!(
         NftBalanceChange::new(
            collection(),
            U256::from(7),
            NftStandard::Erc721,
            U256::from(1),
            U256::from(1)
         )
         .is_none(),
         "an unchanged id is not a row"
      );

      let received = NftBalanceChange::new(
         collection(),
         U256::from(7),
         NftStandard::Erc721,
         U256::ZERO,
         U256::from(1),
      )
      .unwrap();
      assert!(received.is_received());
      assert_eq!(received.abs_delta(), U256::from(1));

      // ERC-1155 counts: a batch send of 3 of an id the signer held 5 of.
      let sent = NftBalanceChange::new(
         collection(),
         U256::from(9),
         NftStandard::Erc1155,
         U256::from(5),
         U256::from(2),
      )
      .unwrap();
      assert!(!sent.is_received());
      assert_eq!(sent.abs_delta(), U256::from(3));
   }

   #[test]
   fn nft_rows_are_counted_and_ordered_outflows_first() {
      let sent = NftBalanceChange::new(
         collection(),
         U256::from(7),
         NftStandard::Erc721,
         U256::from(1),
         U256::ZERO,
      )
      .unwrap();
      let received = NftBalanceChange::new(
         collection(),
         U256::from(8),
         NftStandard::Erc721,
         U256::ZERO,
         U256::from(1),
      )
      .unwrap();

      let diff = BalanceDiff {
         native: None,
         tokens: Vec::new(),
         nfts: vec![received.clone(), sent.clone()],
      };

      assert!(!diff.is_empty());
      assert_eq!(diff.len(), 2);
      assert!(
         diff.changes().is_empty(),
         "NFT rows are not fungible rows"
      );

      let rows = diff.nft_changes();
      assert_eq!(rows[0].token_id, U256::from(7));
      assert_eq!(rows[1].token_id, U256::from(8));
   }

   /// A payload stored before NFT rows existed has no `nfts` key, and must still load with the rest
   /// of its rows intact.
   #[test]
   fn a_payload_written_before_nft_rows_still_loads() {
      let diff = BalanceDiff {
         native: native_change(
            1,
            NumericValue::default(),
            U256::from(2u64),
            U256::from(1u64),
         ),
         tokens: Vec::new(),
         nfts: Vec::new(),
      };

      let mut json = serde_json::to_value(&diff).unwrap();
      json.as_object_mut().expect("an object").remove("nfts");

      let loaded: BalanceDiff = serde_json::from_value(json).expect("an older payload loads");
      assert!(loaded.nfts.is_empty());
      assert!(
         loaded.native.is_some(),
         "the rest of the row survives"
      );
   }

   fn owner() -> Address {
      address!("0x1111111111111111111111111111111111111111")
   }

   fn stranger() -> Address {
      address!("0x2222222222222222222222222222222222222222")
   }

   fn erc721_log(collection: Address, from: Address, to: Address, token_id: u64) -> Log {
      Log {
         address: collection,
         data: IERC721::Transfer {
            from,
            to,
            tokenId: U256::from(token_id),
         }
         .encode_log_data(),
      }
   }

   fn erc1155_single_log(collection: Address, from: Address, to: Address, id: u64) -> Log {
      Log {
         address: collection,
         data: IERC1155::TransferSingle {
            operator: stranger(),
            from,
            to,
            id: U256::from(id),
            value: U256::from(1),
         }
         .encode_log_data(),
      }
   }

   fn erc1155_batch_log(collection: Address, from: Address, to: Address, ids: &[u64]) -> Log {
      let ids: Vec<U256> = ids.iter().map(|id| U256::from(*id)).collect();
      let values = vec![U256::from(1); ids.len()];

      Log {
         address: collection,
         data: IERC1155::TransferBatch {
            operator: stranger(),
            from,
            to,
            ids,
            values,
         }
         .encode_log_data(),
      }
   }

   fn erc721_candidate(collection: Address, token_id: u64) -> NftCandidate {
      NftCandidate {
         collection,
         token_id: U256::from(token_id),
         standard: NftStandard::Erc721,
      }
   }

   fn erc1155_candidate(collection: Address, token_id: u64) -> NftCandidate {
      NftCandidate {
         collection,
         token_id: U256::from(token_id),
         standard: NftStandard::Erc1155,
      }
   }

   /// Only the signer's side of a transfer names a candidate, and the log's own shape names the
   /// standard.
   #[test]
   fn transfer_logs_name_the_signer_side() {
      let logs = [
         erc721_log(collection(), stranger(), owner(), 7),
         erc721_log(collection(), owner(), stranger(), 8),
         erc721_log(collection(), stranger(), stranger(), 9),
      ];

      let got = collect_nft_balance_candidates(owner(), &logs, []);
      assert_eq!(
         got,
         vec![
            erc721_candidate(collection(), 7),
            erc721_candidate(collection(), 8)
         ]
      );

      // Case: mints_and_burns_are_candidates
      // A mint and a burn put the signer on one side too, so they need no special case.
      {
         let logs = [
            erc721_log(collection(), Address::ZERO, owner(), 7),
            erc721_log(collection(), owner(), Address::ZERO, 8),
         ];

         assert_eq!(
            collect_nft_balance_candidates(owner(), &logs, []).len(),
            2
         );
      }

      // Case: single_1155_transfers_are_candidates
      {
         let logs = [
            erc1155_single_log(collection(), owner(), stranger(), 5),
            erc1155_single_log(collection(), stranger(), stranger(), 6),
         ];

         assert_eq!(
            collect_nft_balance_candidates(owner(), &logs, []),
            vec![erc1155_candidate(collection(), 5)]
         );
      }
   }

   /// A batch names every id in one log, so all of them are candidates.
   #[test]
   fn a_batch_expands_every_id() {
      let logs = [erc1155_batch_log(
         collection(),
         owner(),
         stranger(),
         &[1, 2, 3],
      )];

      let got = collect_nft_balance_candidates(owner(), &logs, []);
      assert_eq!(got.len(), 3);
      assert_eq!(got[2], erc1155_candidate(collection(), 3));
   }

   /// Held ids are appended after the logs and deduped against them.
   #[test]
   fn held_ids_are_appended_and_deduped() {
      let logs = [erc721_log(collection(), stranger(), owner(), 7)];
      let held = [
         erc721_candidate(collection(), 7),
         erc1155_candidate(stranger(), 9),
      ];

      let got = collect_nft_balance_candidates(owner(), &logs, held);
      assert_eq!(
         got,
         vec![
            erc721_candidate(collection(), 7),
            erc1155_candidate(stranger(), 9)
         ]
      );
   }

   /// The cap is applied while the batch expands, so one big log cannot blow past it.
   #[test]
   fn the_cap_holds_while_a_batch_expands() {
      let ids: Vec<u64> = (1..=100).collect();
      let logs = [erc1155_batch_log(collection(), owner(), stranger(), &ids)];
      let held = [erc721_candidate(stranger(), 1_000)];

      let got = collect_nft_balance_candidates(owner(), &logs, held);
      assert_eq!(got.len(), MAX_NFT_CANDIDATES);
   }

   /// An ERC-20 `Transfer` carries three topics where the ERC-721 one carries four (its `tokenId` is
   /// indexed), so a fungible transfer is not an NFT candidate — the two never decode into each
   /// other, and a fungible transfer of any size is simply not this candidate's business.
   #[test]
   fn an_erc20_transfer_is_not_a_candidate() {
      let log = Log {
         address: stranger(),
         data: zeus_eth::abi::erc20::IERC20::Transfer {
            from: stranger(),
            to: owner(),
            value: U256::from(1_000),
         }
         .encode_log_data(),
      };

      assert!(
         collect_nft_balance_candidates(owner(), &[log], []).is_empty(),
         "a three-topic Transfer is not an ERC-721 one"
      );
   }

   fn token(byte: u8) -> Address {
      Address::repeat_byte(byte)
   }

   /// The refactor's whole safety property: a request that does not name a watch keeps the sweep every
   /// existing path has always done.
   #[test]
   fn the_default_watch_is_the_portfolio_sweep() {
      assert_eq!(DiffWatch::default(), DiffWatch::Discovered);
      assert!(DiffWatch::default().discovers());
   }

   #[test]
   fn an_exact_watch_names_only_what_it_was_given() {
      let watch = DiffWatch::exact([token(0x11), token(0x22), token(0x11)]);

      let DiffWatch::Exact { tokens, approvals } = &watch else {
         panic!("an exact watch is exact");
      };

      // Deduplicated, order kept, and nothing discovered behind it.
      assert_eq!(tokens, &vec![token(0x11), token(0x22)]);
      assert!(approvals.is_empty());
      assert!(!watch.discovers());
   }

   /// The native currency is the zero address, so a flow that lists it must not spend a probe on it.
   #[test]
   fn the_zero_address_is_not_probed() {
      let watch = DiffWatch::exact([Address::ZERO, token(0x11)]);

      let DiffWatch::Exact { tokens, .. } = &watch else {
         panic!("an exact watch is exact");
      };

      assert_eq!(tokens, &vec![token(0x11)]);
   }

   #[test]
   fn an_allowance_brings_its_token_into_the_balance_list() {
      let watch = DiffWatch::exact([token(0x11)])
         .with_approvals([(token(0x22), token(0x33)), (token(0x22), token(0x33))]);

      let DiffWatch::Exact { tokens, approvals } = &watch else {
         panic!("an exact watch is exact");
      };

      // The allowance's token joins the balances, so its row can be read; the pair is not duplicated.
      assert_eq!(tokens, &vec![token(0x11), token(0x22)]);
      assert_eq!(approvals, &vec![(token(0x22), token(0x33))]);
   }
}
