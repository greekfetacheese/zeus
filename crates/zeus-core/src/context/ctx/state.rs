use super::super::{WalletPortfolio, ZeusContext, ZeusCtx};
use super::DEFAULT_POOL_MINIMUM_LIQUIDITY;
use crate::{TransactionRich, WalletValue};
use zeus_eth::{
   alloy_primitives::{Address, U256},
   amm::uniswap::{AnyUniswapPool, UniswapPool},
   currency::{Currency, erc20::ERC20Token},
   types::{ChainId, SUPPORTED_CHAINS},
   utils::NumericValue,
};

impl ZeusCtx {
   pub fn is_chain_supported(&self, chain: u64) -> bool {
      SUPPORTED_CHAINS.contains(&chain)
   }

   pub fn is_chain_disabled(&self, chain: u64) -> bool {
      self.read(|ctx| ctx.is_chain_disabled(chain))
   }

   pub fn is_chain_syncing(&self, chain: u64) -> bool {
      self.read(|ctx| ctx.state_sync.get(&chain).cloned().unwrap_or(false))
   }

   pub fn set_chain_syncing(&self, chain: u64, syncing: bool) {
      self.write(|ctx| {
         ctx.state_sync.insert(chain, syncing);
      });
   }

   /// Append a rich transaction.
   ///
   /// Also feeds ERC20 / Permit2 approvals into the approval manager.
   pub fn add_transaction(&self, chain: u64, owner: Address, tx: TransactionRich) {
      self.approval_manager().add_from_tx(&tx);
      let db = self.tx_db();
      if let Err(e) = db.add_tx(chain, owner, tx) {
         tracing::error!("Error adding transaction to TxDB: {:?}", e);
      }

      match self.save_wallet_state() {
         Ok(_) => {}
         Err(e) => tracing::error!("Error saving WalletState: {:?}", e),
      }
   }

   /// Return the chains which the owner has balance in
   pub fn get_chains_that_have_balance(&self, owner: Address) -> Vec<u64> {
      let mut chains = Vec::new();
      for chain in SUPPORTED_CHAINS {
         let balance = self.get_eth_balance(chain, owner);
         if !balance.is_zero() {
            chains.push(chain);
         }
      }
      chains
   }

   pub fn get_eth_balance(&self, chain: u64, owner: Address) -> NumericValue {
      self.read(|ctx| ctx.get_eth_balance(chain, owner))
   }

   pub fn get_token_balance(&self, chain: u64, owner: Address, token: Address) -> NumericValue {
      self.read(|ctx| ctx.get_token_balance(chain, owner, token))
   }

   pub fn get_currencies(&self, chain: u64) -> Vec<Currency> {
      self.read(|ctx| ctx.currency_db.get_currencies(chain))
   }

   pub fn get_portfolio(&self, chain: u64, owner: Address) -> WalletPortfolio {
      self.read_wallet_state(|ws| ws.portfolio_db.get(chain, owner))
   }

   pub fn has_portfolio(&self, chain: u64, owner: Address) -> bool {
      self.read_wallet_state(|ws| ws.portfolio_db.portfolios.contains_key(&(chain, owner)))
   }

   /// Get the total value for the given owner across all of its wallets and chains
   pub fn get_total_value(&self, owner: Address, include_testnets: bool) -> WalletValue {
      self.read(|ctx| ctx.get_total_value(owner, include_testnets))
   }

   /// Get all tokens from all portfolios
   pub fn get_all_tokens_from_portfolios(&self, chain: u64) -> Vec<ERC20Token> {
      let mut tokens = Vec::new();
      let portfolios = self.read_wallet_state(|ws| ws.portfolio_db.get_all(chain));

      for portfolio in portfolios {
         tokens.extend(portfolio.tokens().iter().cloned());
      }
      tokens
   }

   /// Update the public data of a portfolio for the given chain and owner
   ///
   /// What it does:
   ///
   /// - Calculates the public token list and sorts it by value
   /// - Updates the portfolio public value based on the latest price data
   pub fn update_public_data(&self, chain: u64, owner: Address) {
      let mut portfolio = self.get_portfolio(chain, owner);
      portfolio.update_public_data(self.clone());
      self.write_wallet_state(|ws| {
         ws.portfolio_db.insert_portfolio(chain, owner, portfolio);
      });
   }

   /// Update the private data (Railgun) of a portfolio for the given chain and owner
   ///
   /// What it does:
   ///
   /// - Indexes the private tokens and sorts them by value
   /// - Updates the portfolio private value based on the latest price data
   pub async fn update_private_data(&self, chain: u64, owner: Address) {
      let mut portfolio = self.get_portfolio(chain, owner);
      portfolio.update_private_data(self.clone()).await;
      self.write_wallet_state(|ws| {
         ws.portfolio_db.insert_portfolio(chain, owner, portfolio);
      });
   }

   pub fn get_eth_price(&self, chain: u64) -> NumericValue {
      let token = ERC20Token::wrapped_native_token(chain);
      self.get_token_price(&token)
   }

   /// Get the USD price of an ERC20 token
   pub fn get_token_price(&self, token: &ERC20Token) -> NumericValue {
      self.read(|ctx| ctx.get_token_price(token))
   }

   /// Get the USD price of a currency
   pub fn get_currency_price(&self, currency: &Currency) -> NumericValue {
      self.read(|ctx| ctx.get_currency_price(currency))
   }

   pub fn get_token_value_for_owner(
      &self,
      chain: u64,
      owner: Address,
      token: &ERC20Token,
   ) -> NumericValue {
      let price = self.get_token_price(token);
      let balance = self.get_token_balance(chain, owner, token.address);
      NumericValue::value(balance.f64(), price.f64())
   }

   pub fn get_currency_value_for_amount(&self, amount: f64, currency: &Currency) -> NumericValue {
      self.read(|ctx| ctx.get_currency_value_for_amount(amount, currency))
   }

   pub fn get_token_value_for_amount(&self, amount: f64, token: &ERC20Token) -> NumericValue {
      self.read(|ctx| ctx.get_token_value_for_amount(amount, token))
   }

   pub fn get_currency_balance(
      &self,
      chain: u64,
      owner: Address,
      currency: &Currency,
   ) -> NumericValue {
      self.read(|ctx| ctx.get_currency_balance(chain, owner, currency))
   }

   pub fn pool_has_sufficient_liquidity(&self, pool: &AnyUniswapPool) -> Option<bool> {
      if pool.state().is_none() {
         return None;
      }

      let base_balance = pool.base_balance();
      let base_price = self.get_token_price(&pool.base_currency().to_erc20());
      let base_value = NumericValue::value(base_balance.f64(), base_price.f64());

      Some(base_value.f64() >= DEFAULT_POOL_MINIMUM_LIQUIDITY)
   }
}

impl ZeusContext {
   pub fn get_eth_balance(&self, chain: u64, owner: Address) -> NumericValue {
      self.wallet_state.read(|ws| ws.balance_manager.get_eth_balance(chain, owner))
   }

   pub fn get_token_balance(&self, chain: u64, owner: Address, token: Address) -> NumericValue {
      self
         .wallet_state
         .read(|ws| ws.balance_manager.get_token_balance(chain, owner, token))
   }

   /// How many of this NFT the wallet holds, or `None` when the balance manager has never been asked.
   ///
   /// A row must read `None` as "no claim" and `Some(0)` as `NOT OWNED`, which is why this is not the
   /// `NumericValue` the token getters return: for a token the two cases can be the same thing, for an
   /// NFT they cannot.
   pub fn get_nft_balance(
      &self,
      chain: u64,
      owner: Address,
      collection: Address,
      token_id: U256,
   ) -> Option<u64> {
      self
         .wallet_state
         .read(|ws| ws.balance_manager.get_nft_balance(chain, owner, collection, token_id))
   }

   pub fn get_currency_value_for_amount(&self, amount: f64, currency: &Currency) -> NumericValue {
      let price = self.get_currency_price(currency);
      NumericValue::value(amount, price.f64())
   }

   pub fn get_token_value_for_amount(&self, amount: f64, token: &ERC20Token) -> NumericValue {
      let price = self.get_token_price(token);
      NumericValue::value(amount, price.f64())
   }

   pub fn get_currency_balance(
      &self,
      chain: u64,
      owner: Address,
      currency: &Currency,
   ) -> NumericValue {
      if currency.is_native() {
         self.get_eth_balance(chain, owner)
      } else {
         let token = currency.erc20_opt().unwrap();
         self.get_token_balance(chain, owner, token.address)
      }
   }

   /// Get the USD price of a currency
   pub fn get_currency_price(&self, currency: &Currency) -> NumericValue {
      if currency.is_native() {
         let wrapped_token = ERC20Token::wrapped_native_token(currency.chain_id());
         self.get_token_price(&wrapped_token)
      } else {
         let token = currency.erc20_opt().unwrap();
         self.get_token_price(token)
      }
   }

   pub fn get_currency_value_for_owner(
      &self,
      chain: u64,
      owner: Address,
      currency: &Currency,
   ) -> NumericValue {
      let price = self.get_currency_price(currency);
      let balance = self.get_currency_balance(chain, owner, currency);
      NumericValue::value(balance.f64(), price.f64())
   }

   fn get_token_price(&self, token: &ERC20Token) -> NumericValue {
      self.price_manager.get_token_price(token).unwrap_or_default()
   }

   /// Get the total value for the given owner across all of its wallets and chains
   pub fn get_total_value(&self, owner: Address, include_testnets: bool) -> WalletValue {
      let mut total_public = 0.0;
      let mut total_private = 0.0;

      for chain in ChainId::supported_chains() {
         if !include_testnets && chain.is_testnet() {
            continue;
         }

         let portfolio = self.wallet_state.read(|ws| ws.portfolio_db.get(chain.id(), owner));
         total_public += portfolio.public_value().f64();
         total_private += portfolio.private_value().f64();
      }

      WalletValue {
         public: NumericValue::from_f64(total_public),
         private: NumericValue::from_f64(total_private),
      }
   }

   /// Disable the given chain
   ///
   /// It wont show up in the UI anymore and wont be used for RPC calls
   pub fn disable_chain(&mut self, chain: u64) {
      self.disabled_chains.disable(chain);
   }

   /// Enable the given chain
   ///
   /// It will show up in the UI again and will be used for RPC calls
   pub fn enable_chain(&mut self, chain: u64) {
      self.disabled_chains.enable(chain);
   }

   pub fn is_chain_disabled(&self, chain: u64) -> bool {
      self.disabled_chains.is_disabled(chain)
   }
}
