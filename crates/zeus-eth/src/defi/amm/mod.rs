pub mod consts;
pub mod uniswap;

use alloy_primitives::Address;
use crate::defi::utils::common_addr;
use crate::{ETH, BSC, BASE, OPTIMISM, ARBITRUM};

/// Enum to define in which DEX a pool belongs to
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum DexKind {
    UniswapV2,
    UniswapV3,
    PancakeSwapV2,
    PancakeSwapV3,
}

impl DexKind {

    /// Get all possible DEX kinds based on the chain
    pub fn all(chain: u64) -> Vec<DexKind> {
        match chain {
            ETH => vec![DexKind::UniswapV2, DexKind::UniswapV3, DexKind::PancakeSwapV2, DexKind::PancakeSwapV3],
            BSC => vec![DexKind::PancakeSwapV3, DexKind::UniswapV2, DexKind::UniswapV3],
            BASE => vec![DexKind::UniswapV2, DexKind::UniswapV3, DexKind::PancakeSwapV3],
            OPTIMISM => vec![DexKind::UniswapV3],
            ARBITRUM => vec![DexKind::UniswapV2, DexKind::UniswapV3, DexKind::PancakeSwapV3],
            _ => vec![DexKind::UniswapV2, DexKind::UniswapV3],
        }
    }

    /// Get the DexKind from the factory address
    pub fn from_factory(chain: u64, factory: Address) -> Result<Self, anyhow::Error> {
        let uniswap_v2 = common_addr::uniswap_v2_factory(chain)?;
        let uniswap_v3 = common_addr::uniswap_v3_factory(chain)?;
        let pancakeswap_v2 = common_addr::pancakeswap_v2_factory(chain)?;
        let pancakeswap_v3 = common_addr::pancakeswap_v3_factory(chain)?;

        if factory == uniswap_v2 {
            Ok(DexKind::UniswapV2)
        } else if factory == uniswap_v3 {
            Ok(DexKind::UniswapV3)
        } else if factory == pancakeswap_v2 {
            Ok(DexKind::PancakeSwapV2)
        } else if factory == pancakeswap_v3 {
            Ok(DexKind::PancakeSwapV3)
        } else {
            anyhow::bail!("Unknown DEX factory address: {:?}", factory);
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

    /// Return the factory address of the DEX
    pub fn factory(&self, chain: u64) -> Result<Address, anyhow::Error> {
        let addr = match self {
            DexKind::UniswapV2 => common_addr::uniswap_v2_factory(chain)?,
            DexKind::UniswapV3 => common_addr::uniswap_v3_factory(chain)?,
            DexKind::PancakeSwapV2 => common_addr::pancakeswap_v2_factory(chain)?,
            DexKind::PancakeSwapV3 => common_addr::pancakeswap_v3_factory(chain)?,
        };

        Ok(addr)
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