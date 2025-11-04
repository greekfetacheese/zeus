use anyhow::bail;

pub const ETH: u64 = 1;
pub const OPTIMISM: u64 = 10;
pub const BSC: u64 = 56;
pub const BASE: u64 = 8453;
pub const ARBITRUM: u64 = 42161;

pub const SUPPORTED_CHAINS: [u64; 5] = [ETH, OPTIMISM, BSC, BASE, ARBITRUM];

const ERR_MSG: &str =
   "Supported chains are: Ethereum(1), Optimism(10), Binance Smart Chain(56), Base(8453), Arbitrum(42161)";

#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u64)]
pub enum ChainId {
   Ethereum = 1,
   Optimism = 10,
   BinanceSmartChain = 56,
   Base = 8453,
   Arbitrum = 42161,
}

impl Default for ChainId {
   fn default() -> Self {
      ChainId::Ethereum
   }
}

impl From<u64> for ChainId {
   fn from(id: u64) -> Self {
      ChainId::new(id).unwrap()
   }
}

impl ChainId {
   pub fn new(id: u64) -> Result<Self, anyhow::Error> {
      let chain = match id {
         1 => ChainId::Ethereum,
         10 => ChainId::Optimism,
         56 => ChainId::BinanceSmartChain,
         8453 => ChainId::Base,
         42161 => ChainId::Arbitrum,
         _ => bail!(format!("Unsupported chain id: {}\n{}", id, ERR_MSG)),
      };
      Ok(chain)
   }

   pub fn eth() -> Self {
      ChainId::Ethereum
   }

   pub fn optimism() -> Self {
      ChainId::Optimism
   }

   pub fn bsc() -> Self {
      ChainId::BinanceSmartChain
   }

   pub fn base() -> Self {
      ChainId::Base
   }

   pub fn arbitrum() -> Self {
      ChainId::Arbitrum
   }

   pub fn is_ethereum(&self) -> bool {
      matches!(self, ChainId::Ethereum)
   }

   pub fn is_optimism(&self) -> bool {
      matches!(self, ChainId::Optimism)
   }

   pub fn is_base(&self) -> bool {
      matches!(self, ChainId::Base)
   }
   pub fn is_arbitrum(&self) -> bool {
      matches!(self, ChainId::Arbitrum)
   }
   pub fn is_bsc(&self) -> bool {
      matches!(self, ChainId::BinanceSmartChain)
   }

   /// Return all supported chains
   pub fn supported_chains() -> Vec<ChainId> {
      SUPPORTED_CHAINS
         .iter()
         .map(|id| ChainId::new(*id).unwrap())
         .collect()
   }

   pub fn is_supported(chain_id: u64) -> bool {
      SUPPORTED_CHAINS.contains(&chain_id)
   }

   pub fn coin_symbol(&self) -> &str {
      match self {
         ChainId::BinanceSmartChain => "BNB",
         _ => "ETH",
      }
   }

   pub fn id(&self) -> u64 {
      match self {
         ChainId::Ethereum => 1,
         ChainId::Optimism => 10,
         ChainId::BinanceSmartChain => 56,
         ChainId::Base => 8453,
         ChainId::Arbitrum => 42161,
      }
   }

   pub fn id_as_hex(&self) -> String {
      format!("0x{:x}", self.id())
   }

   pub fn name(&self) -> &str {
      match self {
         ChainId::Ethereum => "Ethereum",
         ChainId::Optimism => "Optimism",
         ChainId::BinanceSmartChain => "Binance Smart Chain",
         ChainId::Base => "Base",
         ChainId::Arbitrum => "Arbitrum",
      }
   }

   /// Block time in milliseconds
   pub fn block_time_millis(&self) -> u64 {
      match self {
         ChainId::Ethereum => 12000,
         ChainId::Optimism => 2000,
         ChainId::BinanceSmartChain => 3000,
         ChainId::Base => 2000,
         // Arbitrum doesnt have a fixed block time but lets assume on average its 250ms (based on arbscan)
         ChainId::Arbitrum => 250,
      }
   }

   /// Block time in seconds
   pub fn block_time_secs(&self) -> f32 {
      self.block_time_millis() as f32 / 1000.0
   }

   /// Block gas limit
   pub fn block_gas_limit(&self) -> u64 {
      match self {
         ChainId::Ethereum => 45_000_000,
         ChainId::Optimism => 60_000_000,
         ChainId::BinanceSmartChain => 140_000_000,
         ChainId::Base => 264_000_000,
         ChainId::Arbitrum => 32_000_000,
      }
   }

   /// Block Explorer URL
   pub fn block_explorer(&self) -> &str {
      match self {
         ChainId::Ethereum => "https://etherscan.io",
         ChainId::Optimism => "https://optimistic.etherscan.io",
         ChainId::BinanceSmartChain => "https://bscscan.com",
         ChainId::Base => "https://basescan.org",
         ChainId::Arbitrum => "https://arbiscan.io",
      }
   }

   /// Minimum gas usage for a transaction
   pub fn min_gas(&self) -> u64 {
      match self {
         ChainId::Ethereum => 21_000,
         ChainId::Optimism => 21_000,
         ChainId::BinanceSmartChain => 21_000,
         ChainId::Base => 21_000,
         ChainId::Arbitrum => 97_818,
      }
   }

   /// Gas needed for a transfer
   pub fn transfer_gas(&self) -> u64 {
      match self {
         ChainId::Ethereum => 21_000,
         ChainId::Optimism => 21_000,
         ChainId::BinanceSmartChain => 21_000,
         ChainId::Base => 21_000,
         ChainId::Arbitrum => 97_818,
      }
   }

   /// Gas needed for an ERC20 Transfer
   ///
   /// This is an estimate since the actual gas cost may vary depending on the token
   pub fn erc20_transfer_gas(&self) -> u64 {
      match self {
         ChainId::Ethereum => 50_000,
         ChainId::Optimism => 50_000,
         ChainId::BinanceSmartChain => 50_000,
         ChainId::Base => 50_000,
         ChainId::Arbitrum => 97_818,
      }
   }

   pub fn uses_priority_fee(&self) -> bool {
      match self {
         ChainId::Ethereum => true,
         ChainId::Optimism => true,
         ChainId::BinanceSmartChain => false,
         ChainId::Base => true,
         ChainId::Arbitrum => false,
      }
   }
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn chain_new_err() {
      let _chain = ChainId::new(1000).unwrap();
   }
}
