pub mod consts;
pub mod uniswap;

/// Enum to define in which DEX a pool belongs to
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum DexKind {
    Uniswap,
    PancakeSwap,
}