use alloy_primitives::{Address, U256, utils::parse_units};
use utils::address;
use currency::ERC20Token;
use types::ChainId;
use anyhow::bail;

use serde::{ Serialize, Deserialize };

pub mod uniswap;
pub mod consts;
pub mod sync;
pub mod pool_manager;

pub use uniswap::v2::pool::UniswapV2Pool;
pub use uniswap::v3::pool::{FEE_TIERS, UniswapV3Pool};



/// Minimum liquidity required for a pool to able to swap
// TODO: This should be based on a USD value
pub fn minimum_liquidity(token: ERC20Token) -> U256 {
    if token.is_weth() {
        parse_units("40", token.decimals).unwrap().get_absolute()
    } else if token.is_wbnb() {
        parse_units("200", token.decimals).unwrap().get_absolute()
    } else {
        parse_units("100_000", token.decimals).unwrap().get_absolute()
    }
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
    /// Get all possible DEX kinds based on the chain
    pub fn all(chain: u64) -> Vec<DexKind> {
        let chain = ChainId::new(chain).unwrap_or_default();
        match chain {
            ChainId::Ethereum(_) =>
                vec![
                    DexKind::UniswapV2,
                    DexKind::UniswapV3,
                    DexKind::PancakeSwapV2,
                    DexKind::PancakeSwapV3
                ],
            ChainId::BinanceSmartChain(_) =>
                vec![DexKind::PancakeSwapV3, DexKind::UniswapV2, DexKind::UniswapV3],
            ChainId::Base(_) =>
                vec![DexKind::UniswapV2, DexKind::UniswapV3, DexKind::PancakeSwapV3],
            ChainId::Optimism(_) => vec![DexKind::UniswapV3],
            ChainId::Arbitrum(_) =>
                vec![DexKind::UniswapV2, DexKind::UniswapV3, DexKind::PancakeSwapV3],
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
        ChainId::BinanceSmartChain(_) => Ok(26324014
        ),
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