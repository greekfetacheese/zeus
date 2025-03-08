use anyhow::bail;

pub const ETH: u64 = 1;
pub const OPTIMISM: u64 = 10;
pub const BSC: u64 = 56;
pub const BASE: u64 = 8453;
pub const ARBITRUM: u64 = 42161;

pub const SUPPORTED_CHAINS: [u64; 5] = [ETH, OPTIMISM, BSC, BASE, ARBITRUM];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChainId {
   Ethereum(u64),
   Optimism(u64),
   BinanceSmartChain(u64),
   Base(u64),
   Arbitrum(u64),
}

impl Default for ChainId {
   fn default() -> Self {
      ChainId::Ethereum(1)
   }
}

impl ChainId {
   pub fn new(id: u64) -> Result<Self, anyhow::Error> {
      let chain = match id {
         1 => ChainId::Ethereum(id),
         10 => ChainId::Optimism(id),
         56 => ChainId::BinanceSmartChain(id),
         8453 => ChainId::Base(id),
         42161 => ChainId::Arbitrum(id),
         _ => bail!("Unsupported chain id: {}", id),
      };
      Ok(chain)
   }

   pub fn is_ethereum(&self) -> bool {
      matches!(self, ChainId::Ethereum(_))
   }

   pub fn is_optimism(&self) -> bool {
      matches!(self, ChainId::Optimism(_))
   }

   pub fn is_base(&self) -> bool {
      matches!(self, ChainId::Base(_))
   }
   pub fn is_arbitrum(&self) -> bool {
      matches!(self, ChainId::Arbitrum(_))
   }
   pub fn is_bsc(&self) -> bool {
      matches!(self, ChainId::BinanceSmartChain(_))
   }

   /// Return all supported chains
   pub fn supported_chains() -> Vec<ChainId> {
      SUPPORTED_CHAINS
         .iter()
         .map(|id| ChainId::new(*id).unwrap())
         .collect()
   }

   pub fn coin_symbol(&self) -> &str {
      match self {
         ChainId::BinanceSmartChain(_) => "BNB",
         _ => "ETH",
      }
   }

   pub fn id(&self) -> u64 {
      match self {
         ChainId::Ethereum(id) => *id,
         ChainId::Optimism(id) => *id,
         ChainId::BinanceSmartChain(id) => *id,
         ChainId::Base(id) => *id,
         ChainId::Arbitrum(id) => *id,
      }
   }

   pub fn name(&self) -> &str {
      match self {
         ChainId::Ethereum(_) => "Ethereum",
         ChainId::Optimism(_) => "Optimism",
         ChainId::BinanceSmartChain(_) => "Binance Smart Chain",
         ChainId::Base(_) => "Base",
         ChainId::Arbitrum(_) => "Arbitrum",
      }
   }

   /// Block time in milliseconds
   pub fn block_time(&self) -> u64 {
      match self {
         ChainId::Ethereum(_) => 12000,
         ChainId::Optimism(_) => 2000,
         ChainId::BinanceSmartChain(_) => 3000,
         ChainId::Base(_) => 2000,
         // Arbitrum doesnt have a fixed block time but lets assume on average its 250ms (based on arbscan)
         ChainId::Arbitrum(_) => 250,
      }
   }

   /// Block gas limit
   pub fn block_gas_limit(&self) -> u64 {
      match self {
         ChainId::Ethereum(_) => 36_000_000,
         ChainId::Optimism(_) => 60_000_000,
         ChainId::BinanceSmartChain(_) => 140_000_000,
         ChainId::Base(_) => 264_000_000,
         ChainId::Arbitrum(_) => 32_000_000,
      }
   }

   /// Block Explorer URL
   pub fn block_explorer(&self) -> &str {
      match self {
         ChainId::Ethereum(_) => "https://etherscan.io",
         ChainId::Optimism(_) => "https://optimistic.etherscan.io/",
         ChainId::BinanceSmartChain(_) => "https://bscscan.com",
         ChainId::Base(_) => "https://basescan.org/",
         ChainId::Arbitrum(_) => "https://arbiscan.io",
      }
   }

   /// Minimum gas usage for a transaction
   pub fn min_gas(&self) -> u64 {
      match self {
         ChainId::Ethereum(_) => 21_000,
         ChainId::Optimism(_) => 21_000,
         ChainId::BinanceSmartChain(_) => 21_000,
         ChainId::Base(_) => 21_000,
         ChainId::Arbitrum(_) => 97_818,
      }
   }

   /// Gas needed for a transfer
   pub fn transfer_gas(&self) -> u64 {
      match self {
         ChainId::Ethereum(_) => 21_000,
         ChainId::Optimism(_) => 21_000,
         ChainId::BinanceSmartChain(_) => 21_000,
         ChainId::Base(_) => 21_000,
         ChainId::Arbitrum(_) => 97_818,
      }
   }

   /// Gas needed for an ERC20 Transfer
   pub fn erc20_transfer_gas(&self) -> u64 {
      match self {
         ChainId::Ethereum(_) => 50_000,
         ChainId::Optimism(_) => 50_000,
         ChainId::BinanceSmartChain(_) => 50_000,
         ChainId::Base(_) => 50_000,
         ChainId::Arbitrum(_) => 97_818,
      }
   }
}
