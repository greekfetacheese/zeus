use crate::core::ZeusCtx;
use crate::core::serde_hashmap;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use tracing::{debug, error};
use zeus_eth::{
   alloy_primitives::{Address, U256},
   currency::{Currency, ERC20Token},
   utils::NumericValue,
};
use zeus_railgun::{RailgunSigner, caip::AssetId};

type Balance = NumericValue;
type Value = NumericValue;
type Price = NumericValue;

type TokenList = Vec<(ERC20Token, Balance, Value, Price)>;

/// Helper struct that represents the total public & private value of a wallet
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WalletValue {
   pub public: NumericValue,
   pub private: NumericValue,
}

/// Portfolio DB persisted inside the encrypted vault.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PortfolioDB {
   #[serde(default, with = "serde_hashmap")]
   pub portfolios: HashMap<(u64, Address), WalletPortfolio>,
}

impl PortfolioDB {
   pub fn new() -> Self {
      Self {
         portfolios: HashMap::new(),
      }
   }

   /// Get the wallet portfolio for the given chain and owner
   pub fn get(&self, chain_id: u64, owner: Address) -> WalletPortfolio {
      let key = (chain_id, owner);
      self
         .portfolios
         .get(&key)
         .cloned()
         .unwrap_or(WalletPortfolio::new(owner, chain_id))
   }

   /// Get all portfolios for the given chain
   pub fn get_all(&self, chain_id: u64) -> Vec<WalletPortfolio> {
      let mut portfolios = self.portfolios.iter().map(|(_, p)| p.clone()).collect::<Vec<_>>();
      portfolios.retain(|p| p.chain_id == chain_id);
      portfolios
   }

   pub fn insert_portfolio(&mut self, chain_id: u64, owner: Address, portfolio: WalletPortfolio) {
      let key = (chain_id, owner);
      self.portfolios.insert(key, portfolio);
   }

   /// Get all tokens for the given chain and owner
   pub fn get_tokens(&self, chain_id: u64, owner: Address) -> Vec<ERC20Token> {
      let portfolio = self.get(chain_id, owner);
      portfolio.tokens.clone()
   }

   /// Drop portfolios whose owner is not in `wallets`. Returns how many entries were removed.
   pub fn retain_wallets(&mut self, wallets: &HashSet<Address>) -> usize {
      let before = self.portfolios.len();
      self.portfolios.retain(|(_chain, owner), _| wallets.contains(owner));
      self.portfolios.shrink_to_fit();
      before.saturating_sub(self.portfolios.len())
   }
}

/// Wallet portfolio persisted inside the encrypted vault.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WalletPortfolio {
   /// All the tokens in the wallet
   #[serde(default)]
   tokens: Vec<ERC20Token>,
   /// Chain ID
   #[serde(default)]
   chain_id: u64,
   /// Wallet owner
   #[serde(default)]
   owner: Address,
   /// Estimated USD of the public value of the portfolio
   #[serde(default)]
   public_value: NumericValue,
   /// Estimated USD of the private value of the portfolio
   #[serde(default)]
   private_value: NumericValue,
   /// Cached and sorted list of public tokens by value
   #[serde(default)]
   public_tokens: TokenList,
   /// Cached and sorted list of private tokens by value
   #[serde(default)]
   private_tokens: TokenList,
}

impl WalletPortfolio {
   pub fn new(owner: Address, chain_id: u64) -> Self {
      Self {
         tokens: Vec::new(),
         chain_id,
         owner,
         public_value: NumericValue::default(),
         private_value: NumericValue::default(),
         public_tokens: Vec::new(),
         private_tokens: Vec::new(),
      }
   }

   pub fn tokens(&self) -> &Vec<ERC20Token> {
      &self.tokens
   }

   pub fn public_tokens(&self) -> &TokenList {
      &self.public_tokens
   }

   pub fn private_tokens(&self) -> &TokenList {
      &self.private_tokens
   }

   pub fn chain_id(&self) -> u64 {
      self.chain_id
   }

   pub fn owner(&self) -> Address {
      self.owner
   }

   /// Returns the total value of the portfolio (public + private)
   pub fn total_value(&self) -> WalletValue {
      WalletValue {
         public: self.public_value.clone(),
         private: self.private_value.clone(),
      }
   }

   pub fn public_value(&self) -> NumericValue {
      self.public_value.clone()
   }

   pub fn private_value(&self) -> NumericValue {
      self.private_value.clone()
   }

   pub fn set_public_value(&mut self, value: NumericValue) {
      self.public_value = value;
   }

   pub fn set_private_value(&mut self, value: NumericValue) {
      self.private_value = value;
   }

   pub fn add_token(&mut self, token: ERC20Token) {
      if self.tokens.contains(&token) {
         return;
      }
      self.tokens.push(token);
   }

   pub fn has_token(&self, token: &ERC20Token) -> bool {
      self.tokens.contains(token)
   }

   pub fn has_private_tokens(&self) -> bool {
      self.private_tokens().len() > 0
   }

   pub fn remove_token(&mut self, token: &ERC20Token) {
      self.tokens.retain(|t| t != token);
   }

   /// Update the public data for the portfolio
   ///
   /// What it does:
   ///
   /// - Calculates the public token list and sorts it by value
   /// - Updates the portfolio public value based on the latest price data
   pub fn update_public_data(&mut self, ctx: ZeusCtx) {
      let chain_id = self.chain_id;
      let owner = self.owner;
      let tokens = &self.tokens;
      let mut value = 0.0;

      let public_tokens = process_public_tokens(ctx.clone(), chain_id, owner, tokens);

      for (_token, _balance, token_value, _price) in &public_tokens {
         value += token_value.f64();
      }

      let eth = Currency::native(chain_id);
      let eth_price = ctx.get_currency_price(&eth);
      let balance = ctx.get_eth_balance(chain_id, owner);
      let eth_value = eth_price.f64() * balance.f64();
      value += eth_value;

      let new_value = NumericValue::from_f64(value);

      self.set_public_value(new_value);
      self.public_tokens = public_tokens;
   }

   /// Update the private data for the portfolio
   ///
   /// What it does:
   ///
   /// - Indexes the private tokens and sorts them by value
   /// - Updates the portfolio private value based on the latest price data
   pub async fn update_private_data(&mut self, ctx: ZeusCtx) {
      let chain_id = self.chain_id;
      let owner = self.owner;

      let mut private_tokens = self.private_tokens.clone();

      let updated_tokens = match process_private_tokens(ctx.clone(), chain_id, owner).await {
         Ok(tokens) => tokens,
         Err(e) => {
            error!("Error calculating private tokens: {:?}", e);
            private_tokens
         }
      };

      private_tokens = updated_tokens;

      let mut value = 0.0;

      for (_token, _balance, token_value, _price) in &private_tokens {
         value += token_value.f64();
      }

      let new_value = NumericValue::from_f64(value);

      self.set_private_value(new_value);
      self.private_tokens = private_tokens;
   }
}

fn process_public_tokens(
   ctx: ZeusCtx,
   chain_id: u64,
   owner: Address,
   tokens: &Vec<ERC20Token>,
) -> TokenList {
   let mut token_list: TokenList = tokens
      .iter()
      .map(|token| {
         let price = ctx.get_token_price(token);
         let balance = ctx.get_token_balance(chain_id, owner, token.address);
         let value = ctx.get_token_value_for_owner(chain_id, owner, token);
         (token.clone(), balance, value, price)
      })
      .collect();

   token_list
      .sort_by(|a, b| b.2.f64().partial_cmp(&a.2.f64()).unwrap_or(std::cmp::Ordering::Equal));

   token_list
}

async fn process_private_tokens(
   ctx: ZeusCtx,
   chain_id: u64,
   owner: Address,
) -> Result<TokenList, anyhow::Error> {
   let mut token_list: TokenList = Vec::new();

   if !ctx.railgun_is_supported(chain_id.into()) {
      return Ok(token_list);
   }

   let mut provider = ctx.get_railgun_provider(chain_id, false).await?;

   let wallet = ctx.get_wallet(owner);

   if wallet.is_none() {
      #[cfg(feature = "debug")]
      error!("Wallet not found for address {}", owner);
      return Ok(token_list);
   }

   let wallet = wallet.unwrap();

   if !wallet.can_derive_zk_address() {
      debug!(
         "Wallet {} cannot derive a zkAddress",
         wallet.name_with_id()
      );
      return Ok(token_list);
   }

   let seed = wallet.seed()?;
   let raligun_signer = RailgunSigner::from_seed(&seed, 0, chain_id)?;
   let railgun_address = raligun_signer.address().clone();

   let private_balances = provider.balance(railgun_address).await;

   #[cfg(feature = "debug")]
   debug!(
      "Found {} private balances",
      private_balances.len()
   );

   for entry in private_balances {
      let token_address = match entry.asset {
         AssetId::Erc20(address) => address,
         _ => continue,
      };

      let erc20 = ctx.get_token(chain_id, token_address).await?;
      let balance = NumericValue::format_wei(U256::from(entry.amount), erc20.decimals);
      let price = ctx.get_token_price(&erc20);
      let value = NumericValue::value(balance.f64(), price.f64());
      token_list.push((erc20.clone(), balance, value, price));
   }

   token_list
      .sort_by(|a, b| b.2.f64().partial_cmp(&a.2.f64()).unwrap_or(std::cmp::Ordering::Equal));

   Ok(token_list)
}
