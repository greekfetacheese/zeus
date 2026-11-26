//! Reading a simulation against the promise it was asked to keep.
//!
//! The confirmation holds two things that never meet anywhere else: what the flow promised, and what the fork
//! says actually happens. This is where they are compared, and it is deliberately pure — no chain, no window,
//! no clock — so the numbers a refusal quotes are the numbers a test can check.
//!
//! Two measures come out of it, and they answer different questions:
//!
//! * **Price impact** — what the trade loses against the market, in USD, from the input side to the output
//!   side. This is the figure the swap page shows a human, and it only exists when both legs are priced.
//! * **Shortfall** — how much of what was promised actually arrives, in the token's own units. It needs no
//!   price at all, which is what makes it the guard for a token nobody quotes.
//!
//! Neither includes gas: the simulated native delta carries no fee (the EVM runs with fee checks off) and the
//! cost is priced separately as `tx_cost_usd`, exactly as the swap page's own `calc_price_impact` does.

use super::analysis::TransactionAnalysis;
use super::promise::TxPromise;
use serde::{Deserialize, Serialize};
use zeus_eth::currency::Currency;
use zeus_eth::utils::NumericValue;

/// How the simulation read against the promise.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SimReview {
   /// What the input leg is worth, when the app has a price for it.
   pub usd_in: Option<NumericValue>,
   /// What the arriving amount is worth, when it has one.
   pub usd_out: Option<NumericValue>,
   /// `1 - usd_out/usd_in` in basis points. `None` when either leg is unpriced, or the trade has one side.
   pub price_impact_bps: Option<u16>,
   /// `1 - measured/expected` in basis points, in the currency's units. `None` when nothing was measured.
   pub shortfall_bps: Option<u16>,
   /// The bridge's own cut, excluded from the comparison because it is part of the bargain.
   pub fee_bps: Option<u16>,
   /// Currencies the review needed a price for and did not get. A grant that refuses an unpriced output reads
   /// this; nothing here is ever reported as a zero dollar amount.
   pub unpriced: Vec<Currency>,
   /// Whether any amount was measured at all. A flow that has not been taught to report one reads as
   /// "nothing measured" rather than "nothing arrived".
   pub measured: bool,
}

impl SimReview {
   /// Whether the review can be enforced: at least one of the two measures was computed.
   pub fn is_complete(&self) -> bool {
      self.price_impact_bps.is_some() || self.shortfall_bps.is_some()
   }
}

/// Read a simulation against the promise.
///
/// `price` answers the USD price of a currency, or `None` when the app has none. It is a parameter rather
/// than the context so this stays pure — and so a test can price a token that no pool quotes.
pub fn review(
   promise: &TxPromise,
   analysis: &TransactionAnalysis,
   price: impl Fn(&Currency) -> Option<NumericValue>,
) -> SimReview {
   let mut out = SimReview::default();
   let output = promise.output_currency();
   let expected = promise.expected_out();

   // What arrived. The flow's own measurement wins; otherwise the analysis' diff for the currency, which is
   // what every flow that goes through `analyse` already carries.
   let measured = analysis.measured_out.clone().or_else(|| diff_of(analysis, output));

   let Some(measured) = measured else {
      // Nothing measured: say so instead of reviewing a zero.
      out.unpriced = unpriced_of(promise, &price);
      return out;
   };

   out.measured = true;

   // Units first: this needs no price and is what a token with no quote is still held to.
   out.shortfall_bps = shortfall_bps(expected, &measured, promise.fee());

   // USD second: both legs, or neither.
   let usd_out = price(output).map(|price| NumericValue::value(measured.f64(), price.f64()));
   let usd_in = promise.input().and_then(|input| {
      price(&input.currency).map(|price| NumericValue::value(input.amount.f64(), price.f64()))
   });

   out.usd_out = usd_out.filter(|value| !value.is_zero());
   out.usd_in = usd_in.filter(|value| !value.is_zero());

   if let (Some(input), Some(output)) = (&out.usd_in, &out.usd_out) {
      let fee_bps = promise
         .fee()
         .zip(promise.input())
         .and_then(|(fee, input)| share_bps(fee, &input.amount));

      out.fee_bps = fee_bps;
      out.price_impact_bps = price_impact_bps(input.f64(), output.f64(), fee_bps);
   }

   out.unpriced = unpriced_of(promise, &price);

   out
}

/// `1 - measured/expected` in basis points, less any fee the promise already allows.
///
/// A measurement at or above the promise is 0 rather than a negative: a trade that does better than its quote
/// has no shortfall.
fn shortfall_bps(
   expected: &NumericValue,
   measured: &NumericValue,
   fee: Option<&NumericValue>,
) -> Option<u16> {
   if expected.f64() <= 0.0 || !expected.f64().is_finite() {
      return None;
   }

   // The fee the bargain already allows comes off the promise before the two are compared.
   let allowed = fee.map(|fee| fee.f64()).unwrap_or(0.0);
   let promised = expected.f64() - allowed;

   if promised <= 0.0 {
      return None;
   }

   let shortfall = (1.0 - (measured.f64() / promised)) * 10_000.0;

   Some(clamp_bps(shortfall))
}

/// `1 - usd_out/usd_in` in basis points, with the bridge's own cut taken off the input first.
fn price_impact_bps(usd_in: f64, usd_out: f64, fee_bps: Option<u16>) -> Option<u16> {
   if usd_in <= 0.0 || !usd_in.is_finite() {
      return None;
   }

   // The fee is part of the bargain, so the denominator is what the trade actually had to spend.
   let fee = f64::from(fee_bps.unwrap_or(0)) / 10_000.0;
   let spend = usd_in * (1.0 - fee);

   if spend <= 0.0 {
      return None;
   }

   let impact = (1.0 - (usd_out / spend)) * 10_000.0;

   Some(clamp_bps(impact))
}

/// One amount as a share of another, in basis points.
fn share_bps(part: &NumericValue, whole: &NumericValue) -> Option<u16> {
   if whole.f64() <= 0.0 {
      return None;
   }

   Some(clamp_bps((part.f64() / whole.f64()) * 10_000.0))
}

/// Basis points, saturating and never negative — an improvement is 0, not a credit.
fn clamp_bps(value: f64) -> u16 {
   if !value.is_finite() {
      return 0;
   }

   value.clamp(0.0, f64::from(u16::MAX)) as u16
}

/// The analysis' own change for a currency, when the flow did not measure it itself.
fn diff_of(analysis: &TransactionAnalysis, currency: &Currency) -> Option<NumericValue> {
   let change = analysis
      .balance_diff
      .tokens
      .iter()
      .find(|change| &change.currency == currency)
      .or_else(|| {
         analysis
            .balance_diff
            .native
            .as_ref()
            .filter(|change| &change.currency == currency)
      })?;

   change.is_increase().then(|| change.abs_delta())
}

/// Which of the currencies the review wanted a price for and did not get.
fn unpriced_of(
   promise: &TxPromise,
   price: &impl Fn(&Currency) -> Option<NumericValue>,
) -> Vec<Currency> {
   let mut wanted: Vec<Currency> = vec![promise.output_currency().clone()];

   if let Some(input) = promise.input() {
      wanted.push(input.currency.clone());
   }

   wanted.into_iter().filter(|currency| price(currency).is_none()).collect()
}

#[cfg(test)]
mod tests {
   use super::*;
   use crate::sim::balance_diff::{BalanceChange, BalanceDiff};
   use crate::tx::promise::Amount;
   use std::sync::Arc;
   use zeus_eth::{
      alloy_primitives::{Address, U256},
      currency::{ERC20Token, NativeCurrency},
   };

   const PEPE: Address = Address::repeat_byte(0x2a);

   fn eth() -> Currency {
      Currency::Native(NativeCurrency::eth())
   }

   fn pepe() -> Currency {
      Currency::ERC20(ERC20Token {
         chain_id: 1,
         address: PEPE,
         symbol: Arc::from("PEPE"),
         name: Arc::from("Pepe"),
         decimals: 18,
         total_supply: U256::MAX,
      })
   }

   fn units(value: f64) -> NumericValue {
      NumericValue::from_f64(value)
   }

   /// A balance, the way the diff builder makes one: carry-side amounts hold their wei at the currency's
   /// own scale, which is what `abs_delta` reformats.
   fn balance(tokens: u64) -> NumericValue {
      NumericValue::format_wei(
         U256::from(tokens) * U256::from(10u64).pow(U256::from(18)),
         18,
      )
   }

   fn swap(expected: f64, floor: f64) -> TxPromise {
      TxPromise::Swap {
         input: Amount::new(eth(), units(1.0)),
         output: pepe(),
         expected_out: units(expected),
         floor: units(floor),
      }
   }

   /// Prices: ETH at $1700, PEPE at $16.95 — the brief's own example, where the quote promises $1695.
   fn priced(currency: &Currency) -> Option<NumericValue> {
      if currency.is_native() {
         return Some(NumericValue::currency_price(1700.0));
      }

      (currency.address() == PEPE).then(|| NumericValue::currency_price(16.95))
   }

   /// The app knows no price for the token at all.
   fn unpriced(_: &Currency) -> Option<NumericValue> {
      None
   }

   fn measured(analysis: &mut TransactionAnalysis, amount: f64) {
      analysis.set_measured_out(units(amount));
   }

   #[test]
   fn the_brief_example_reads_as_a_half_percent_of_impact() {
      let mut analysis = TransactionAnalysis::default();
      measured(&mut analysis, 100.0);

      let review = review(&swap(100.0, 99.5), &analysis, priced);

      // $1700 in, $1695 out.
      assert_eq!(
         review.usd_in.as_ref().map(|usd| usd.f64()),
         Some(1700.0)
      );
      assert_eq!(
         review.usd_out.as_ref().map(|usd| usd.f64()),
         Some(1695.0)
      );
      assert_eq!(review.price_impact_bps, Some(29));
      // 100 promised, 100 arrived.
      assert_eq!(review.shortfall_bps, Some(0));
      assert!(review.unpriced.is_empty());
      assert!(review.measured);
      assert!(review.is_complete());
   }

   #[test]
   fn a_transfer_tax_shows_as_a_shortfall_and_as_impact() {
      let mut analysis = TransactionAnalysis::default();
      // The token takes 30% on transfer: 70 arrive where 100 were promised.
      measured(&mut analysis, 70.0);

      let review = review(&swap(100.0, 99.5), &analysis, priced);

      assert_eq!(review.shortfall_bps, Some(3_000));
      // 1700 in, 70 * 16.95 = 1186.5 out.
      assert_eq!(review.price_impact_bps, Some(3_020));
   }

   /// The whole point of the units measure: a token nobody prices is still held to what was promised.
   #[test]
   fn an_unpriced_token_still_yields_a_shortfall() {
      let mut analysis = TransactionAnalysis::default();
      measured(&mut analysis, 70.0);

      let review = review(&swap(100.0, 99.5), &analysis, unpriced);

      assert_eq!(review.shortfall_bps, Some(3_000));
      assert_eq!(review.price_impact_bps, None);
      assert!(review.usd_in.is_none() && review.usd_out.is_none());
      assert_eq!(review.unpriced, vec![pepe(), eth()]);
      assert!(review.is_complete());
   }

   /// No measurement is not a zero measurement.
   #[test]
   fn nothing_measured_is_reported_as_nothing() {
      let analysis = TransactionAnalysis::default();

      let review = review(&swap(100.0, 99.5), &analysis, priced);

      assert!(!review.measured);
      assert_eq!(review.shortfall_bps, None);
      assert_eq!(review.price_impact_bps, None);
      assert!(!review.is_complete());
   }

   /// A flow that did not measure still gets a review when its diffs carry the arrival.
   #[test]
   fn the_analysis_diff_stands_in_for_a_missing_measurement() {
      let mut analysis = TransactionAnalysis::default();
      let mut diff = BalanceDiff::default();
      diff.tokens.push(BalanceChange {
         currency: pepe(),
         price: NumericValue::currency_price(16.95),
         before: balance(0),
         after: balance(100),
      });
      analysis.set_diffs(diff, Default::default());

      let review = review(&swap(100.0, 99.5), &analysis, priced);

      assert_eq!(review.shortfall_bps, Some(0));
      assert_eq!(review.price_impact_bps, Some(29));
   }

   /// A trade that does better than its quote has no shortfall — never a negative one.
   #[test]
   fn doing_better_than_the_quote_is_zero_not_a_credit() {
      let mut analysis = TransactionAnalysis::default();
      measured(&mut analysis, 120.0);

      let review = review(&swap(100.0, 99.5), &analysis, priced);

      assert_eq!(review.shortfall_bps, Some(0));
      // 1700 in, 120 * 16.95 = 2034 out: an improvement, still 0.
      assert_eq!(review.price_impact_bps, Some(0));
   }

   #[test]
   fn a_bridge_fee_is_excluded_from_the_comparison() {
      let mut analysis = TransactionAnalysis::default();
      // 1 WETH in, 0.99 out: the 1% is Across's fee, not a loss.
      measured(&mut analysis, 0.99);

      let promise = TxPromise::Bridge {
         input: Amount::new(eth(), units(1.0)),
         output: eth(),
         expected_out: units(1.0),
         fee: Some(units(0.01)),
      };

      let review = review(&promise, &analysis, priced);

      assert_eq!(review.fee_bps, Some(100));
      assert_eq!(review.shortfall_bps, Some(0));
      assert_eq!(review.price_impact_bps, Some(0));
   }

   /// A bridge whose destination arrives short by more than its fee is still caught.
   #[test]
   fn a_bridge_that_loses_more_than_its_fee_is_short() {
      let mut analysis = TransactionAnalysis::default();
      measured(&mut analysis, 0.95);

      let promise = TxPromise::Bridge {
         input: Amount::new(eth(), units(1.0)),
         output: eth(),
         expected_out: units(1.0),
         fee: Some(units(0.01)),
      };

      let review = review(&promise, &analysis, priced);

      // 0.95 against the 0.99 the fee left room for.
      assert_eq!(review.shortfall_bps, Some(404));
   }

   #[test]
   fn a_transfer_has_no_impact_only_a_shortfall() {
      let mut analysis = TransactionAnalysis::default();
      measured(&mut analysis, 250.0);

      let promise = TxPromise::Transfer {
         amount: Amount::new(pepe(), units(250.0)),
      };

      let review = review(&promise, &analysis, priced);

      assert_eq!(review.shortfall_bps, Some(0));
      assert_eq!(review.price_impact_bps, None);
      assert!(review.usd_in.is_none());
   }

   /// A price the app does not have must never turn into a zero-dollar output.
   #[test]
   fn a_missing_price_is_absent_not_zero() {
      let mut analysis = TransactionAnalysis::default();
      measured(&mut analysis, 100.0);

      let review = review(&swap(100.0, 99.5), &analysis, |_| None);

      assert!(review.usd_in.is_none());
      assert!(review.usd_out.is_none());
      assert_eq!(review.price_impact_bps, None);
   }
}
