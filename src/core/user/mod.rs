use zeus_eth::{
   alloy_primitives::Address,
   currency::{Currency, ERC20Token},
};

pub mod profile;
pub mod wallet;

pub use profile::{PROFILE_FILE, Profile};

/// Currencies that the user owns,
///
/// since we dont have access to any 3rd party indexers to auto populate this data
///
/// the user has to add them manually
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Portfolio {
   /// The currencies that we have in the portofolio
   pub currencies: Vec<Currency>,

   /// The owner of the portfolio
   pub owner: Address,

   /// USD value
   pub value: f64,
}

impl Portfolio {
   pub fn new(currencies: Vec<Currency>, owner: Address) -> Self {
      Self {
         currencies,
         owner,
         value: 0.0,
      }
   }

   pub fn add_currency(&mut self, currency: Currency) {
      self.currencies.push(currency);
   }

   pub fn remove_currency(&mut self, currency: &Currency) {
      self.currencies.retain(|c| c != currency);
   }

   /// Return all the ERC20 tokens in the portfolio
   pub fn erc20_tokens(&self) -> Vec<ERC20Token> {
      let mut tokens = Vec::new();
      for currency in &self.currencies {
         if currency.is_erc20() {
            tokens.push(currency.erc20().cloned().unwrap());
         }
      }
      tokens
   }

   pub fn currencies(&self) -> &Vec<Currency> {
      &self.currencies
   }
}
