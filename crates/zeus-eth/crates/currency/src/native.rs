use serde::{Deserialize, Serialize};
use types::{BSC, ChainId};

/// Represents a Native Currency to its chain
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NativeCurrency {
   pub chain_id: u64,
   pub symbol: String,
   pub name: String,
   pub decimals: u8,
}

impl Default for NativeCurrency {
   fn default() -> Self {
      Self {
         chain_id: 1,
         symbol: "ETH".to_string(),
         name: "Ethereum".to_string(),
         decimals: 18,
      }
   }
}

impl NativeCurrency {
   pub fn new(chain_id: u64, symbol: String, name: String, decimals: u8) -> Self {
      Self {
         chain_id,
         symbol,
         name,
         decimals,
      }
   }

   /// Create a new Native Currency from the chain id
   pub fn from_chain_id(id: u64) -> Result<Self, anyhow::Error> {
      let chain = ChainId::new(id)?;
      match chain {
         ChainId::BinanceSmartChain(_) => Ok(Self {
            chain_id: BSC,
            symbol: "BNB".to_string(),
            name: "Binance Smart Chain".to_string(),
            decimals: 18,
         }),
         _ => Ok(NativeCurrency::default()),
      }
   }
}
