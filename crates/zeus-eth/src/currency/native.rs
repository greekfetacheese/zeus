use crate::types::{ChainId, ETH, ETH_SEPOLIA, OPTIMISM, BSC, BASE, ARBITRUM};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Represents a Native Currency to its chain
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct NativeCurrency {
   pub chain_id: u64,
   pub symbol: Arc<str>,
   pub name: Arc<str>,
   pub decimals: u8,
}

impl Default for NativeCurrency {
   fn default() -> Self {
      Self {
         chain_id: ETH,
         symbol: "ETH".into(),
         name: "Ethereum".into(),
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
   pub fn new(chain_id: u64, symbol: impl Into<Arc<str>>, name: impl Into<Arc<str>>, decimals: u8) -> Self {
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
         ChainId::Ethereum => Ok(Self::eth()),
         ChainId::EthereumSepolia => Ok(Self::eth_sepolia()),
         ChainId::Optimism => Ok(Self::eth_optimism()),
         ChainId::Base => Ok(Self::eth_base()),
         ChainId::Arbitrum => Ok(Self::eth_arbitrum()),
         ChainId::BinanceSmartChain => Ok(Self::bnb()),
      }
   }

   pub fn eth() -> Self {
      Self::default()
   }

   pub fn eth_sepolia() -> Self {
      Self::new(ETH_SEPOLIA, "ETH", "Ethereum", 18)
   }

   pub fn eth_optimism() -> Self {
      Self::new(OPTIMISM, "ETH", "Ethereum", 18)
   }

   pub fn eth_base() -> Self {
      Self::new(BASE, "ETH", "Ethereum", 18)
   }

   pub fn eth_arbitrum() -> Self {
      Self::new(ARBITRUM, "ETH", "Ethereum", 18)
   }

   pub fn bnb() -> Self {
      Self::new(BSC, "BNB", "Binance Smart Chain", 18)
   }
}
