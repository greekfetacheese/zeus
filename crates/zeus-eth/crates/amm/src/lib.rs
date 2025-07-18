use alloy_primitives::{Address, B256, U256, utils::parse_units};
use anyhow::bail;
use currency::{Currency, ERC20Token};
use types::ChainId;
use utils::address_book;

use serde::{Deserialize, Serialize};

pub mod consts;
pub mod sync;
pub mod uniswap;

pub use uniswap::v2::pool::UniswapV2Pool;
pub use uniswap::v3::pool::{FEE_TIERS, UniswapV3Pool};
pub use uniswap::v4::pool::UniswapV4Pool;
pub use uniswap::{AnyUniswapPool, FeeAmount, UniswapPool};
pub use uniswap_v3_math;

/// A simple struct to identify a V2/V3/V4 pool
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PoolID {
   pub chain_id: u64,
   /// For V4 this is zero
   pub address: Address,
   /// For V2/V3 this is zero
   pub pool_id: B256,
}

impl PoolID {
   pub fn new(chain_id: u64, address: Address, pool_id: B256) -> Self {
      Self {
         chain_id,
         address,
         pool_id,
      }
   }
}

pub fn sorts_before(currency_a: &Currency, currency_b: &Currency) -> bool {
   if currency_a.is_native() {
      return true;
   }

   if currency_b.is_native() {
      return false;
   }

   // Both are ERC20 tokens
   currency_a.address() < currency_b.address()
}

/// Minimum liquidity we consider to be required for a pool to able to swap
///
/// for V4 we set a threshold 10-11x higher than other protocols since we cannot just query the token balances
/// of the pool and we rely on the `compute_virtual_reserves` to give an idea of the liquidity
// TODO: This should be based on a USD value
pub fn minimum_liquidity(token: &ERC20Token, dex: DexKind) -> U256 {
   let weth_amount = if !dex.is_v4() {
      parse_units("20", token.decimals).unwrap().get_absolute()
   } else {
      parse_units("200", token.decimals).unwrap().get_absolute()
   };

   let wbnb_amount = if !dex.is_v4() {
      parse_units("200", token.decimals).unwrap().get_absolute()
   } else {
      parse_units("2000", token.decimals).unwrap().get_absolute()
   };

   let stable_amount = if !dex.is_v4() {
      parse_units("40_000", token.decimals)
         .unwrap()
         .get_absolute()
   } else {
      parse_units("4_000_000", token.decimals)
         .unwrap()
         .get_absolute()
   };

   if token.is_weth() {
      weth_amount
   } else if token.is_wbnb() {
      wbnb_amount
   } else {
      stable_amount
   }
}

/// Enum to define in which DEX a pool belongs to
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum DexKind {
   UniswapV2,
   UniswapV3,
   UniswapV4,
   PancakeSwapV2,
   PancakeSwapV3,
}

impl DexKind {
   /// Get the main DexKind for the given chain
   ///
   /// Panics if the chain is not supported
   pub fn main_dexes(chain: u64) -> Vec<DexKind> {
      let chain = ChainId::new(chain).unwrap();
      match chain {
         ChainId::Ethereum(_) => vec![DexKind::UniswapV2, DexKind::UniswapV3, DexKind::UniswapV4],
         ChainId::BinanceSmartChain(_) => vec![DexKind::PancakeSwapV2, DexKind::PancakeSwapV3],
         ChainId::Base(_) => vec![DexKind::UniswapV2, DexKind::UniswapV3, DexKind::UniswapV4],
         ChainId::Optimism(_) => vec![DexKind::UniswapV3, DexKind::UniswapV4],
         ChainId::Arbitrum(_) => vec![DexKind::UniswapV3, DexKind::UniswapV4],
      }
   }

   /// Get all possible DEX kinds based on the chain
   ///
   /// Panics if the chain is not supported
   pub fn all(chain: u64) -> Vec<DexKind> {
      let chain = ChainId::new(chain).unwrap();
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

   pub fn as_vec() -> Vec<DexKind> {
      vec![
         DexKind::UniswapV2,
         DexKind::UniswapV3,
         DexKind::UniswapV4,
         DexKind::PancakeSwapV2,
         DexKind::PancakeSwapV3,
      ]
   }

   /// Get the DexKind from the factory address
   pub fn from_factory(chain: u64, factory: Address) -> Result<Self, anyhow::Error> {
      let uniswap_v2 = address_book::uniswap_v2_factory(chain)?;
      let uniswap_v3 = address_book::uniswap_v3_factory(chain)?;
      let pancake_v2 = address_book::pancakeswap_v2_factory(chain)?;
      let pancake_v3 = address_book::pancakeswap_v3_factory(chain)?;

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
         DexKind::UniswapV2 => address_book::uniswap_v2_factory(chain)?,
         DexKind::UniswapV3 => address_book::uniswap_v3_factory(chain)?,
         DexKind::UniswapV4 => panic!("Uniswap V4 does not have a factory"),
         DexKind::PancakeSwapV2 => address_book::pancakeswap_v2_factory(chain)?,
         DexKind::PancakeSwapV3 => address_book::pancakeswap_v3_factory(chain)?,
      };

      Ok(addr)
   }

   /// Return the creation block of this Dex
   ///
   /// For V2 & V3 this is the block in which the factory was deployed
   ///
   /// For V4 is the block which the PoolManager contract was deployed
   pub fn creation_block(&self, chain: u64) -> Result<u64, anyhow::Error> {
      match self {
         DexKind::UniswapV2 => uniswap_v2_factory_creation_block(chain),
         DexKind::UniswapV3 => uniswap_v3_factory_creation_block(chain),
         DexKind::UniswapV4 => uniswap_v4_pool_manager_creation_block(chain),
         DexKind::PancakeSwapV2 => pancakeswap_v2_factory_creation_block(chain),
         DexKind::PancakeSwapV3 => pancakeswap_v3_factory_creation_block(chain),
      }
   }

   pub fn is_uniswap(&self) -> bool {
      matches!(
         self,
         DexKind::UniswapV2 | DexKind::UniswapV3 | DexKind::UniswapV4
      )
   }

   pub fn is_pancake(&self) -> bool {
      matches!(self, DexKind::PancakeSwapV2 | DexKind::PancakeSwapV3)
   }

   pub fn is_v2(&self) -> bool {
      matches!(self, DexKind::UniswapV2 | DexKind::PancakeSwapV2)
   }

   pub fn is_v3(&self) -> bool {
      matches!(self, DexKind::UniswapV3 | DexKind::PancakeSwapV3)
   }

   pub fn is_v4(&self) -> bool {
      matches!(self, DexKind::UniswapV4)
   }

   pub fn is_uniswap_v2(&self) -> bool {
      matches!(self, DexKind::UniswapV2)
   }

   pub fn is_uniswap_v3(&self) -> bool {
      matches!(self, DexKind::UniswapV3)
   }

   pub fn is_uniswap_v4(&self) -> bool {
      matches!(self, DexKind::UniswapV4)
   }

   pub fn is_pancakeswap_v2(&self) -> bool {
      matches!(self, DexKind::PancakeSwapV2)
   }

   pub fn is_pancakeswap_v3(&self) -> bool {
      matches!(self, DexKind::PancakeSwapV3)
   }

   pub fn as_str(&self) -> &'static str {
      match self {
         DexKind::UniswapV2 => "Uniswap V2",
         DexKind::UniswapV3 => "Uniswap V3",
         DexKind::UniswapV4 => "Uniswap V4",
         DexKind::PancakeSwapV2 => "PancakeSwap V2",
         DexKind::PancakeSwapV3 => "PancakeSwap V3",
      }
   }
}

/// Uniswap V3 NFT Position Manager contract creation block
pub fn nft_position_manager_creation_block(chain: u64) -> Result<u64, anyhow::Error> {
   let chain = ChainId::new(chain)?;
   match chain {
      ChainId::Ethereum(_) => Ok(12369651),
      ChainId::Optimism(_) => Ok(0), // Genesis
      ChainId::BinanceSmartChain(_) => Ok(26324045),
      ChainId::Base(_) => Ok(1371714),
      ChainId::Arbitrum(_) => Ok(173),
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

fn uniswap_v4_pool_manager_creation_block(chain: u64) -> Result<u64, anyhow::Error> {
   let chain = ChainId::new(chain)?;
   match chain {
      ChainId::Ethereum(_) => Ok(21688329),
      ChainId::Optimism(_) => Ok(130947675),
      ChainId::BinanceSmartChain(_) => Ok(45970610),
      ChainId::Base(_) => Ok(25350988),
      ChainId::Arbitrum(_) => Ok(297842872),
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
