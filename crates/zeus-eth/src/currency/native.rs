use serde::{Deserialize, Serialize};
use crate::types::ChainId;

/// Represents a Native Currency to its chain
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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
   pub fn new(chain_id: u64, symbol: impl Into<String>, name: impl Into<String>, decimals: u8) -> Self {
      Self {
         chain_id,
         symbol: symbol.into(),
         name: name.into(),
         decimals,
      }
   }

   /// Create a new Native Currency from the chain id
   pub fn from_chain_id(id: u64) -> Result<Self, anyhow::Error> {
      let chain = ChainId::new(id)?;
      match chain {
         ChainId::Ethereum(_) => Ok(Self::eth()),
         ChainId::Optimism(_) => Ok(Self::eth_optimism()),
         ChainId::Base(_) => Ok(Self::eth_base()),
         ChainId::Arbitrum(_) => Ok(Self::eth_arbitrum()),
         ChainId::BinanceSmartChain(_) => Ok(Self::bnb()),
      }
   }

   pub fn eth() -> Self {
      Self::default()
   }

   pub fn eth_optimism() -> Self {
      Self::new(10, "ETH", "Ethereum", 18)
   }

   pub fn eth_base() -> Self {
      Self::new(8453, "ETH", "Ethereum", 18)
   }

   pub fn eth_arbitrum() -> Self {
      Self::new(42161, "ETH", "Ethereum", 18)
   }

   pub fn bnb() -> Self {
      Self::new(56, "BNB", "Binance Smart Chain", 18)
   }
}
