pub mod consts;
pub mod uniswap;

use super::super::{ETH, BSC, BASE, ARBITRUM, OPTIMISM};

/// Enum to define in which DEX a pool belongs to
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum DexKind {
    Uniswap,
    PancakeSwap,
}

impl DexKind {
    pub fn is_uniswap(&self) -> bool {
        matches!(self, DexKind::Uniswap)
    }

    pub fn is_pancakeswap(&self) -> bool {
        matches!(self, DexKind::PancakeSwap)
    }
}