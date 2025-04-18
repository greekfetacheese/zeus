use serde::{Deserialize, Serialize};
use types::ChainId;

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

impl From<u64> for NativeCurrency {
   fn from(id: u64) -> Self {
      Self::from_chain_id(id).unwrap()
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
         ChainId::Ethereum(_) => Ok(NativeCurrency::default()),
         ChainId::Optimism(_) => Ok(NativeCurrency::new(
            chain.id(),
            "ETH".to_string(),
            "Ethereum".to_string(),
            18,
         )),
         ChainId::BinanceSmartChain(_) => Ok(NativeCurrency::new(
            chain.id(),
            "BNB".to_string(),
            "Binance Smart Chain".to_string(),
            18,
         )),
         ChainId::Base(_) => Ok(NativeCurrency::new(
            chain.id(),
            "ETH".to_string(),
            "Ethereum".to_string(),
            18,
         )),
         ChainId::Arbitrum(_) => Ok(NativeCurrency::new(
            chain.id(),
            "ETH".to_string(),
            "Ethereum".to_string(),
            18,
         )),
      }
   }
}
