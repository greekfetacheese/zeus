use zeus_eth::alloy_primitives::Address;
use zeus_eth::currency::{Currency, ERC20Token};

pub mod wallet;
pub mod profile;

pub use profile::{Profile, PROFILE_FILE};


#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Portfolio {

    /// The currencies that we have in the portofolio
    pub currencies: Vec<Currency>,

    /// The owner of the portfolio
    pub owner: Address,

}

impl Portfolio {
    pub fn new(currencies: Vec<Currency>, owner: Address) -> Self {
        Self {
            currencies,
            owner
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