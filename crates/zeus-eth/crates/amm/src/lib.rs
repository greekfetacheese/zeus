use alloy_primitives::{Address, U256, utils::parse_units};
use anyhow::bail;
use currency::ERC20Token;
use types::ChainId;
use utils::address;

use serde::{Deserialize, Serialize};

pub mod consts;
pub mod pool_manager;
pub mod sync;
pub mod uniswap;

pub use uniswap::v2::pool::UniswapV2Pool;
pub use uniswap::v3::pool::{FEE_TIERS, UniswapV3Pool};

/// Minimum liquidity we consider to be required for a pool to able to swap
// TODO: This should be based on a USD value
pub fn minimum_liquidity(token: ERC20Token) -> U256 {
   if token.is_weth() {
      parse_units("40", token.decimals).unwrap().get_absolute()
   } else if token.is_wbnb() {
      parse_units("200", token.decimals).unwrap().get_absolute()
   } else {
      parse_units("100_000", token.decimals)
         .unwrap()
         .get_absolute()
   }
}

/// Get all the possible v2 pairs for the given token based on:
///
/// - The token's chain id
/// - The [DexKind]
/// - A vec of base tokens for liquidity
pub fn get_possible_v2_pairs(dex_kind: DexKind, token: ERC20Token, base_tokens: Vec<ERC20Token>) -> Vec<UniswapV2Pool> {
   // create a vec of v2 pools but without populating with real data just the pairs
   let mut pools = Vec::new();
   for base_token in base_tokens {
      if token.address == base_token.address {
         continue;
      }

      let pool = UniswapV2Pool::new(
         token.chain_id,
         Address::ZERO,
         token.clone(),
         base_token.clone(),
         dex_kind,
      );
      pools.push(pool);
   }
   pools
}

/// Get all the possible v3 pairs for the given token based on:
///
/// - The token's chain id
/// - The [DexKind]
/// - A vec of base tokens for liquidity
/// - The fee tiers
pub fn get_possible_v3_pairs(dex_kind: DexKind, token: ERC20Token, base_tokens: Vec<ERC20Token>) -> Vec<UniswapV3Pool> {
   // create a vec of v3 pools but without populating with real data just the pairs
   let mut pools = Vec::new();
   for base_token in base_tokens {
      if token.address == base_token.address {
         continue;
      }

      for fee in FEE_TIERS {
         let pool = UniswapV3Pool::new(
            token.chain_id,
            Address::ZERO,
            fee,
            token.clone(),
            base_token.clone(),
            dex_kind,
         );
         pools.push(pool);
      }
   }
   pools
}

/// Enum to define in which DEX a pool belongs to
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum DexKind {
   UniswapV2,
   UniswapV3,
   PancakeSwapV2,
   PancakeSwapV3,
}

impl DexKind {
   /// Get the main DexKind for the given chain
   pub fn main_dexes(chain: u64) -> Vec<DexKind> {
      let chain = ChainId::new(chain).unwrap_or_default();
      match chain {
         ChainId::Ethereum(_) => vec![DexKind::UniswapV2, DexKind::UniswapV3],
         ChainId::BinanceSmartChain(_) => vec![DexKind::PancakeSwapV2, DexKind::PancakeSwapV3],
         ChainId::Base(_) => vec![DexKind::UniswapV2, DexKind::UniswapV3],
         ChainId::Optimism(_) => vec![DexKind::UniswapV3],
         ChainId::Arbitrum(_) => vec![DexKind::UniswapV3],
      }
   }

   /// Get all possible DEX kinds based on the chain
   pub fn all(chain: u64) -> Vec<DexKind> {
      let chain = ChainId::new(chain).unwrap_or_default();
      match chain {
         ChainId::Ethereum(_) => vec![
            DexKind::UniswapV2,
            DexKind::UniswapV3,
            DexKind::PancakeSwapV2,
            DexKind::PancakeSwapV3,
         ],
         ChainId::BinanceSmartChain(_) => vec![
            DexKind::PancakeSwapV3,
            DexKind::UniswapV2,
            DexKind::UniswapV3,
         ],
         ChainId::Base(_) => vec![
            DexKind::UniswapV2,
            DexKind::UniswapV3,
            DexKind::PancakeSwapV3,
         ],
         ChainId::Optimism(_) => vec![DexKind::UniswapV3],
         ChainId::Arbitrum(_) => vec![
            DexKind::UniswapV2,
            DexKind::UniswapV3,
            DexKind::PancakeSwapV3,
         ],
      }
   }

   /// Get the DexKind from the factory address
   pub fn from_factory(chain: u64, factory: Address) -> Result<Self, anyhow::Error> {
      let uniswap_v2 = address::uniswap_v2_factory(chain)?;
      let uniswap_v3 = address::uniswap_v3_factory(chain)?;
      let pancake_v2 = address::pancakeswap_v2_factory(chain)?;
      let pancake_v3 = address::pancakeswap_v3_factory(chain)?;

      if factory == uniswap_v2 {
         Ok(DexKind::UniswapV2)
      } else if factory == uniswap_v3 {
         Ok(DexKind::UniswapV3)
      } else if factory == pancake_v2 {
         Ok(DexKind::PancakeSwapV2)
      } else if factory == pancake_v3 {
         Ok(DexKind::PancakeSwapV3)
      } else {
         bail!("Unknown factory address: {:?}", factory);
      }
   }

   /// Return the factory address of the DEX
   pub fn factory(&self, chain: u64) -> Result<Address, anyhow::Error> {
      let addr = match self {
         DexKind::UniswapV2 => address::uniswap_v2_factory(chain)?,
         DexKind::UniswapV3 => address::uniswap_v3_factory(chain)?,
         DexKind::PancakeSwapV2 => address::pancakeswap_v2_factory(chain)?,
         DexKind::PancakeSwapV3 => address::pancakeswap_v3_factory(chain)?,
      };

      Ok(addr)
   }

   /// Return the factory creation block
   pub fn factory_creation_block(&self, chain: u64) -> Result<u64, anyhow::Error> {
      match self {
         DexKind::UniswapV2 => uniswap_v2_factory_creation_block(chain),
         DexKind::UniswapV3 => uniswap_v3_factory_creation_block(chain),
         DexKind::PancakeSwapV2 => pancakeswap_v2_factory_creation_block(chain),
         DexKind::PancakeSwapV3 => pancakeswap_v3_factory_creation_block(chain),
      }
   }

   pub fn is_uniswap_v2(&self) -> bool {
      matches!(self, DexKind::UniswapV2)
   }

   pub fn is_uniswap_v3(&self) -> bool {
      matches!(self, DexKind::UniswapV3)
   }

   pub fn is_pancakeswap_v2(&self) -> bool {
      matches!(self, DexKind::PancakeSwapV2)
   }

   pub fn is_pancakeswap_v3(&self) -> bool {
      matches!(self, DexKind::PancakeSwapV3)
   }

   pub fn to_str(&self) -> &'static str {
      match self {
         DexKind::UniswapV2 => "UniswapV2",
         DexKind::UniswapV3 => "UniswapV3",
         DexKind::PancakeSwapV2 => "PancakeSwapV2",
         DexKind::PancakeSwapV3 => "PancakeSwapV3",
      }
   }
}

fn uniswap_v2_factory_creation_block(chain: u64) -> Result<u64, anyhow::Error> {
   let chain = ChainId::new(chain)?;
   match chain {
      ChainId::Ethereum(_) => Ok(10000835),
      ChainId::Optimism(_) => Ok(112197986),
      ChainId::BinanceSmartChain(_) => Ok(33496018),
      ChainId::Base(_) => Ok(6601915),
      ChainId::Arbitrum(_) => Ok(150442611),
   }
}

fn uniswap_v3_factory_creation_block(chain: u64) -> Result<u64, anyhow::Error> {
   let chain = ChainId::new(chain)?;
   match chain {
      ChainId::Ethereum(_) => Ok(12369621),
      ChainId::Optimism(_) => Ok(0), // Genesis
      ChainId::BinanceSmartChain(_) => Ok(26324014),
      ChainId::Base(_) => Ok(1371680),
      ChainId::Arbitrum(_) => Ok(165),
   }
}

fn pancakeswap_v2_factory_creation_block(chain: u64) -> Result<u64, anyhow::Error> {
   let chain = ChainId::new(chain)?;
   match chain {
      ChainId::Ethereum(_) => Ok(15614590),
      ChainId::Optimism(_) => bail!("PancakeSwap V2 is not available on Optimism"),
      ChainId::BinanceSmartChain(_) => Ok(6809737),
      ChainId::Base(_) => Ok(2910387),
      ChainId::Arbitrum(_) => Ok(101022992),
   }
}

fn pancakeswap_v3_factory_creation_block(chain: u64) -> Result<u64, anyhow::Error> {
   let chain = ChainId::new(chain)?;
   match chain {
      ChainId::Ethereum(_) => Ok(16950686),
      ChainId::Optimism(_) => bail!("PancakeSwap V3 is not available on Optimism"),
      ChainId::BinanceSmartChain(_) => Ok(26956207),
      ChainId::Base(_) => Ok(2912007),
      ChainId::Arbitrum(_) => Ok(101028949),
   }
}

mod tests {
   #[allow(unused_imports)]
   use super::*;

   #[test]
   fn test_get_possible_v2_pairs() {
      let usdc = ERC20Token::usdc();

      let base_tokens = ERC20Token::base_tokens(1);
      let pools = get_possible_v2_pairs(DexKind::UniswapV2, usdc.clone(), base_tokens.clone());
      for pool in pools {
         println!("{}/{}", pool.token0.symbol, pool.token1.symbol);
      }
   }

   #[test]
   fn test_get_possible_v3_pairs() {
      let usdc = ERC20Token::usdc();

      let base_tokens = ERC20Token::base_tokens(1);
      let pools = get_possible_v3_pairs(DexKind::UniswapV3, usdc.clone(), base_tokens.clone());
      for pool in pools {
         println!("{}/{} - Fee: {}", pool.token0.symbol, pool.token1.symbol, pool.fee);
      }
   }
}
