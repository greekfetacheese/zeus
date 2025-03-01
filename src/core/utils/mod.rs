use crate::core::ZeusCtx;
use lazy_static::lazy_static;
use std::path::PathBuf;
use tokio::runtime::Runtime;
use zeus_eth::{
   alloy_primitives::{Address, utils::format_units},
   currency::{Currency, ERC20Token},
};
use tracing::info;

pub mod eth;
pub mod trace;
pub mod tx;
pub mod update;

lazy_static! {
   pub static ref RT: Runtime = Runtime::new().unwrap();
}

/// Zeus data directory
pub fn data_dir() -> Result<PathBuf, anyhow::Error> {
   let dir = std::env::current_dir()?.join("data");

   if !dir.exists() {
      std::fs::create_dir_all(dir.clone())?;
   }

   Ok(dir)
}

/// Pool data directory
pub fn pool_data_dir() -> Result<PathBuf, anyhow::Error> {
   let dir = data_dir()?.join("pool_data.json");
   Ok(dir)
}

/// Calculate the total value of a wallet in USD
pub fn wallet_value(ctx: ZeusCtx, chain: u64, owner: Address) -> f64 {
   let portfolio = ctx.get_portfolio(chain, owner);
   let mut value = 0.0;

   if let Some(portfolio) = portfolio {
      info!("Calculating wallet value for owner {}, chain {}", owner, chain);
      let currencies = portfolio.currencies();

      for currency in currencies {
         let usd_price: f64 = currency_price(ctx.clone(), currency).parse().unwrap_or(0.0);
         let balance: f64 = currency_balance(ctx.clone(), owner, currency)
            .parse()
            .unwrap_or(0.0);
         value += currency_value_f64(usd_price, balance);
      }

      // update portfolio value
      ctx.update_portfolio_value(chain, owner, value);
   } else {
      info!("No portfolio found for owner {}, chain {}", owner, chain);
   }

   value
}

/// Return a [String] that displays the formatted balance of the selected currency
// TODO: Use something like numformat to deal with very large numbers
pub fn currency_balance(ctx: ZeusCtx, owner: Address, currency: &Currency) -> String {
   let balance_text;

   if currency.is_native() {
      let balance = ctx.get_eth_balance(owner);
      balance_text = format_units(balance, currency.decimals().clone()).unwrap_or("0.0".to_string());
   } else {
      let currency = currency.erc20().unwrap();
      let balance = ctx.get_token_balance(owner, currency.address);
      balance_text = format_units(balance, currency.decimals).unwrap_or("0.0".to_string());
   }

   format!("{:.4}", balance_text)
}

/// Return the USD price of a token in String format
pub fn currency_price(ctx: ZeusCtx, currency: &Currency) -> String {
   let price;
   let chain = ctx.chain().id();

   if currency.is_native() {
      let wrapped_token = ERC20Token::native_wrapped_token(chain);
      price = ctx.get_token_price(&wrapped_token).unwrap_or(0.0);
   } else {
      let currency = currency.erc20().unwrap();
      price = ctx.get_token_price(&currency).unwrap_or(0.0);
   }

   format!("{:.2}", price)
}

/// Return the USD Value of a token in String format
pub fn currency_value(price: f64, balance: f64) -> String {
   if price == 0.0 || balance == 0.0 {
      return "0.00".to_string();
   }
   format!("{:.2}", price * balance)
}

pub fn currency_value_f64(price: f64, balance: f64) -> f64 {
   if price == 0.0 || balance == 0.0 {
      return 0.0;
   }
   price * balance
}
