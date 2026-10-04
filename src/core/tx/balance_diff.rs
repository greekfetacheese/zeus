//! Signer ETH / ERC-20 balance changes from simulation state, not logs.
//!
//! Log `Transfer` values are untrusted (eg. a Fake Airdrop). Amounts here
//! come from native account balance and ERC-20 `balanceOf`.

use serde::{Deserialize, Serialize};
use zeus_eth::{
   alloy_primitives::{Address, U256},
   currency::{Currency, ERC20Token, NativeCurrency},
   nft::NftStandard,
   utils::NumericValue,
};

/// Max token contracts to probe per tx (portfolio + interact_to + log addresses).
pub const MAX_TOKEN_CANDIDATES: usize = 64;

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

#[cfg(test)]
mod tests {
   use super::*;
   use zeus_eth::alloy_primitives::address;

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
   }

   #[test]
   fn candidates_cap_at_max() {
      let portfolio: Vec<Address> = (1u8..=80).map(Address::repeat_byte).collect();
      let got = collect_token_candidates(portfolio, Address::ZERO, []);
      assert_eq!(got.len(), MAX_TOKEN_CANDIDATES);
   }

   #[test]
   fn native_equal_is_none() {
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

   #[test]
   fn native_receive() {
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
   }

   #[test]
   fn token_burn_is_decrease() {
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

   #[test]
   fn spoofed_mint_without_balance_change_is_omitted() {
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
}
