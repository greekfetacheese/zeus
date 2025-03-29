pub mod v2;
pub mod v3;

use alloy_primitives::{Address, U256, utils::format_units};
use serde::{Deserialize, Serialize};

use currency::ERC20Token;


/// Represents the volume of a pool that occured at some point
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolVolume {
    pub buy_volume: U256,
    pub sell_volume: U256,
    pub swaps: Vec<SwapData>,
}

impl PoolVolume {
   /// Return the total buy volume in USD based on the token0 usd value
    pub fn buy_volume_usd(&self, usd_value: f64, decimals: u8) -> Result<f64, anyhow::Error> {
        let formatted = format_units(self.buy_volume, decimals)?.parse::<f64>()?;
        Ok(formatted * usd_value)
    }

    /// Return the total sell volume in USD based on the token1 usd value
    pub fn sell_volume_usd(&self, usd_value: f64, decimals: u8) -> Result<f64, anyhow::Error> {
        let formatted = format_units(self.sell_volume, decimals)?.parse::<f64>()?;
        Ok(formatted * usd_value)
}
}

/// A swap that took place on a DEX (Uniswap)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwapData {
   pub account: Option<Address>,
   pub token_in: ERC20Token,
   pub token_out: ERC20Token,
   pub amount_in: U256,
   pub amount_out: U256,
   pub block: u64,
   pub tx_hash: String,
}

impl SwapData {
   pub fn new(
      account: Option<Address>,
      token_in: ERC20Token,
      token_out: ERC20Token,
      amount_in: U256,
      amount_out: U256,
      block: u64,
      tx_hash: String,
   ) -> Self {
      Self {
         account,
         token_in,
         token_out,
         amount_in,
         amount_out,
         block,
         tx_hash,
      }
   }

   /// Return a formatted string to print in the console
   pub fn pretty(&self) -> Result<String, anyhow::Error> {
      let from = if let Some(account) = self.account {
         account.to_string()
      } else {
         "Unknown".to_string()
      };

      let s = format!(
         "Swap: {} -> {} | From: {} | Amount: {} -> {} | Block: {} | Tx: {}",
         self.token_in.symbol,
         self.token_out.symbol,
         from,
         format_units(self.amount_in, self.token_in.decimals)?,
         format_units(self.amount_out, self.token_out.decimals)?,
         self.block,
         self.tx_hash,
      );
      Ok(s)
   }
}
