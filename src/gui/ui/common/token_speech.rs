//! The words a screen reader gets for a token: its name, where Zeus has one.
//!
//! A symbol is a ticker, not a word. `ETH`, `USDC` and `WBTC` are read out letter by letter — "E, T,
//! H" — and a listener walking a table of rows has strings to tell apart that were never meant to be
//! pronounced. Almost every token Zeus tracks has a name behind its symbol, from the chain's token
//! list or the native currency's own, and a name is the thing a reader can say, hold in mind and
//! search for.
//!
//! So speech names a token by its name. What is drawn keeps the symbol: the page is laid out around
//! the ticker, and a row that drew `USD Coin` where it drew `USDC` would be a different row. Nothing
//! is lost — the symbol rides on the node as its description, the way a whole address rides behind
//! the name an address book gave it (see [`address_for_speech`](super::address_for_speech)).

use super::name_text;
use egui::{Response, Ui};
use zeus_eth::currency::Currency;

/// The words for one token: the name a reader hears, and the symbol it may keep as the second form.
pub struct SpokenToken {
   /// The accessible name: the token's name, or its symbol when Zeus has no other word for it.
   pub name: String,
   /// The symbol the page draws, when the name above is a different word.
   pub symbol: Option<String>,
}

/// The words for the token called `name` and drawn as `symbol`.
///
/// A name that is empty or only whitespace counts as none: a contract that answered `""` has nothing
/// to say, and reading the blank out would lose the one word that identifies the token. A name that
/// only repeats the symbol is said once, so it is not carried again as the description.
pub fn token_for_speech(name: &str, symbol: &str) -> SpokenToken {
   let name = name.trim();
   let symbol = symbol.trim();

   match name.is_empty() || name == symbol {
      true => SpokenToken {
         name: symbol.to_string(),
         symbol: None,
      },
      false => SpokenToken {
         name: name.to_string(),
         symbol: Some(symbol.to_string()),
      },
   }
}

/// The same words for a `currency`, which is what most call sites hold.
pub fn currency_for_speech(currency: &Currency) -> SpokenToken {
   token_for_speech(currency.name(), currency.symbol())
}

/// Put a token's spoken form on the node `response` already created for its drawn symbol.
///
/// The name goes through [`name_text`], the one place a drawn label is named; the symbol the cell
/// draws is what the node keeps as its description.
pub fn name_token(ui: &Ui, response: &Response, spoken: &SpokenToken) {
   name_text(ui, response, spoken.name.clone());

   if let Some(symbol) = &spoken.symbol {
      ui.ctx().accesskit_node_builder(response.id, |node| {
         node.set_description(symbol.as_str());
      });
   }
}

#[cfg(test)]
mod tests {
   use super::*;
   use zeus_eth::alloy_primitives::{Address, U256};
   use zeus_eth::currency::{ERC20Token, NativeCurrency};

   /// A token as the token list holds one: a ticker, the contract's own name, and six decimals.
   fn token(symbol: &str, name: &str) -> Currency {
      Currency::from(ERC20Token::from_components(
         1,
         Address::from([0xaa; 20]),
         symbol,
         name,
         6,
         U256::ZERO,
      ))
   }

   /// A ticker is spelled out letter by letter, so the reader is given the name behind it and the
   /// drawn symbol waits in the description.
   #[test]
   fn a_token_is_named_by_its_name_and_keeps_its_symbol() {
      let spoken = currency_for_speech(&token("USDC", "USD Coin"));

      assert_eq!(spoken.name, "USD Coin");
      assert_eq!(spoken.symbol.as_deref(), Some("USDC"));
   }

   /// The native currency is the common case: the chain names it, and the ticker is the shorthand.
   #[test]
   fn the_native_currency_is_named_the_way_the_chain_names_it() {
      let spoken = currency_for_speech(&Currency::from(NativeCurrency::eth()));

      assert_eq!(spoken.name, "Ethereum");
      assert_eq!(spoken.symbol.as_deref(), Some("ETH"));
   }

   /// A contract that answered `""` has no name to say, so the symbol it is drawn as is what a
   /// listener gets. Nothing is left blank on the node.
   #[test]
   fn a_token_with_no_name_is_named_by_its_symbol() {
      let spoken = currency_for_speech(&token("USDC", ""));

      assert_eq!(spoken.name, "USDC");
      assert_eq!(
         spoken.symbol, None,
         "a symbol said as the name is not repeated"
      );
   }

   /// A name that is only the ticker written again is the same word, so it is said once.
   #[test]
   fn a_name_that_only_repeats_the_symbol_is_said_once() {
      let spoken = currency_for_speech(&token("USDC", "USDC"));

      assert_eq!(spoken.name, "USDC");
      assert_eq!(spoken.symbol, None);
   }

   /// A name that is only whitespace is no name: it would be read as a pause where the token should
   /// be said.
   #[test]
   fn a_blank_name_falls_back_to_the_symbol() {
      let spoken = currency_for_speech(&token("USDC", "   "));

      assert_eq!(spoken.name, "USDC");
      assert_eq!(spoken.symbol, None);
   }
}
