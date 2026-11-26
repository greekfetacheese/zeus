//! What a transaction promised to do, carried to the confirmation so the simulation can be read against it.
//!
//! A flow builds its calldata and simultaneously knows the bargain it is asking the wallet to strike: this
//! much leaves, at least this much arrives, and the quote that promised it. Nothing else in the pipeline
//! knows those numbers — the analysis can say what moved, never what was wanted — which is why a transfer
//! tax or a bad route can only be noticed by comparing the two.
//!
//! The promise is deliberately **values, not policy**: it says what was asked for, never what is acceptable.
//! Whether a shortfall is a refusal is the grant's business, and it is decided in one place: `review` and
//! the agent's `authorise`.

use serde::{Deserialize, Serialize};
use zeus_eth::{currency::Currency, utils::NumericValue};

/// One side of a trade: a currency and an amount of it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Amount {
   pub currency: Currency,
   pub amount: NumericValue,
}

impl Amount {
   pub fn new(currency: Currency, amount: NumericValue) -> Self {
      Self { currency, amount }
   }
}

/// What a flow asked of the chain, in the terms the user and the agent were promised.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TxPromise {
   /// A swap: `input` leaves, the quote said `expected_out` arrives, and `floor` is the least the
   /// calldata will accept.
   Swap {
      input: Amount,
      output: Currency,
      expected_out: NumericValue,
      floor: NumericValue,
   },
   /// A bridge deposit: `input` leaves on this chain, `expected_out` of `output` is promised on the
   /// other one. `fee` is the bridge's own cut, which is part of the bargain rather than a loss.
   Bridge {
      input: Amount,
      output: Currency,
      expected_out: NumericValue,
      fee: Option<NumericValue>,
   },
   /// A transfer: `amount` is what the recipient is meant to receive.
   Transfer { amount: Amount },
   /// A shield: the same amount leaves the public balance and enters the private one.
   Shield { amount: Amount },
   /// An unshield: the same amount leaves the private balance, less any protocol fee already taken.
   Unshield { amount: Amount },
}

impl TxPromise {
   /// The currency the promise is measured in: what the wallet is meant to end up holding.
   ///
   /// It is the arrival side for every flow — a swap's output token, a transfer's and a shield's own token,
   /// a bridge's destination token. A shortfall is always a shortfall of this.
   pub fn output_currency(&self) -> &Currency {
      match self {
         Self::Swap { output, .. } | Self::Bridge { output, .. } => output,
         Self::Transfer { amount } | Self::Shield { amount } | Self::Unshield { amount } => {
            &amount.currency
         }
      }
   }

   /// What was promised to arrive, in `output_currency`.
   pub fn expected_out(&self) -> &NumericValue {
      match self {
         Self::Swap { expected_out, .. } | Self::Bridge { expected_out, .. } => expected_out,
         Self::Transfer { amount } | Self::Shield { amount } | Self::Unshield { amount } => {
            &amount.amount
         }
      }
   }

   /// What it costs — the input side, for the USD leg of the review.
   ///
   /// Only a trade has two sides. A transfer, a shield and an unshield move one asset and trade nothing, so
   /// their weight is the amount itself and there is no ratio to take.
   pub fn input(&self) -> Option<&Amount> {
      match self {
         Self::Swap { input, .. } | Self::Bridge { input, .. } => Some(input),
         Self::Transfer { .. } | Self::Shield { .. } | Self::Unshield { .. } => None,
      }
   }

   /// The bridge fee the destination is allowed to be short by.
   pub fn fee(&self) -> Option<&NumericValue> {
      match self {
         Self::Bridge { fee, .. } => fee.as_ref(),
         _ => None,
      }
   }
}

#[cfg(test)]
mod tests {
   use super::*;
   use std::sync::Arc;
   use zeus_eth::{
      alloy_primitives::{Address, U256},
      currency::{ERC20Token, NativeCurrency},
   };

   fn token(symbol: &str, decimals: u8) -> Currency {
      Currency::ERC20(ERC20Token {
         chain_id: 1,
         address: Address::repeat_byte(0x2a),
         symbol: Arc::from(symbol),
         name: Arc::from(symbol),
         decimals,
         total_supply: U256::MAX,
      })
   }

   fn eth() -> Currency {
      Currency::Native(NativeCurrency::eth())
   }

   fn units(value: f64) -> NumericValue {
      NumericValue::from_f64(value)
   }

   fn swap() -> TxPromise {
      TxPromise::Swap {
         input: Amount::new(eth(), units(1.0)),
         output: token("PEPE", 18),
         expected_out: units(100.0),
         floor: units(99.5),
      }
   }

   #[test]
   fn a_swap_is_measured_in_its_output_currency() {
      let promise = swap();

      assert_eq!(promise.output_currency().symbol(), "PEPE");
      assert_eq!(promise.expected_out().f64(), 100.0);
      assert_eq!(
         promise.input().map(|side| side.currency.symbol()),
         Some("ETH")
      );
      assert!(promise.fee().is_none());
   }

   #[test]
   fn a_bridge_carries_its_own_fee() {
      let promise = TxPromise::Bridge {
         input: Amount::new(token("WETH", 18), units(1.0)),
         output: token("WETH", 18),
         expected_out: units(0.99),
         fee: Some(units(0.01)),
      };

      assert_eq!(promise.fee().map(NumericValue::f64), Some(0.01));
      assert_eq!(promise.expected_out().f64(), 0.99);
   }

   /// A one-sided move trades nothing, so the review has no input leg to weigh it against.
   #[test]
   fn a_transfer_has_one_side() {
      let promise = TxPromise::Transfer {
         amount: Amount::new(token("USDC", 6), units(250.0)),
      };

      assert!(promise.input().is_none());
      assert_eq!(promise.expected_out().f64(), 250.0);
      assert_eq!(promise.output_currency().symbol(), "USDC");
   }
}
