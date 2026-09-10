//! Signer ETH / ERC-20 balance changes from simulation state, not logs.
//!
//! Log `Transfer` values are untrusted (eg. a Fake Airdrop). Amounts here
//! come from native account balance and ERC-20 `balanceOf`.

use serde::{Deserialize, Serialize};
use zeus_eth::{
   alloy_primitives::{Address, U256},
   currency::{Currency, ERC20Token, NativeCurrency},
   utils::NumericValue,
};

/// Max token contracts to probe per tx (portfolio + interact_to + log addresses).
pub const MAX_TOKEN_CANDIDATES: usize = 64;

/// One asset whose signer balance changed across the simulated tx.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BalanceChange {
   pub currency: Currency,
   pub before: NumericValue,
   pub after: NumericValue,
}

impl BalanceChange {
   pub fn from_wei(currency: Currency, before: U256, after: U256) -> Option<Self> {
      if before == after {
         return None;
      }
      let decimals = currency.decimals();
      Some(Self {
         currency,
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

/// Signer-side balance outcome of a simulated transaction.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BalanceDiff {
   pub native: Option<BalanceChange>,
   pub tokens: Vec<BalanceChange>,
}

impl BalanceDiff {
   pub fn is_empty(&self) -> bool {
      self.native.is_none() && self.tokens.is_empty()
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
}

pub fn native_change(chain: u64, before: U256, after: U256) -> Option<BalanceChange> {
   let currency = Currency::from(NativeCurrency::from(chain));
   BalanceChange::from_wei(currency, before, after)
}

pub fn token_change(token: ERC20Token, before: U256, after: U256) -> Option<BalanceChange> {
   BalanceChange::from_wei(Currency::from(token), before, after)
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
      assert!(native_change(1, U256::from(1u64), U256::from(1u64)).is_none());
   }

   #[test]
   fn native_receive() {
      let change = native_change(
         1,
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
      let change = token_change(wbtest(), hundred, U256::ZERO).unwrap();
      assert!(!change.is_increase());
      assert_eq!(change.abs_delta().f64(), 100.0);
      assert_eq!(change.currency.symbol(), "WBTEST");
   }

   #[test]
   fn spoofed_mint_without_balance_change_is_omitted() {
      // Fake Airdrop: Transfer(0x0 → signer, 1e18) on a non-token does not
      // change balanceOf. Equal wei → no row, regardless of the log.
      assert!(token_change(wbtest(), U256::ZERO, U256::ZERO).is_none());
   }

   #[test]
   fn empty_diff() {
      assert!(BalanceDiff::default().is_empty());
      let mut diff = BalanceDiff::default();
      diff.tokens.push(token_change(wbtest(), U256::from(1u64), U256::ZERO).unwrap());
      assert!(!diff.is_empty());
   }
}
