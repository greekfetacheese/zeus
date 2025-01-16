pub mod defi;
pub mod revm_utils;
pub mod abi;
pub mod utils;
pub mod prelude;


// RE-EXPORTS

// Alloy
pub use alloy_chains;
pub use alloy_primitives;
pub use alloy_signer;
pub use alloy_signer_local;
pub use alloy_provider;
pub use alloy_rpc_types;
pub use alloy_sol_types;
pub use alloy_transport;
pub use alloy_pubsub;
pub use alloy_network;
pub use alloy_contract;

// Revm
pub use revm;

pub const ETH: u64 = 1;
pub const OPTIMISM: u64 = 10;
pub const BSC: u64 = 56;
pub const BASE: u64 = 8453;
pub const ARBITRUM: u64 = 42161;

pub const SUPPORTED_CHAINS: [u64; 5] = [ETH, OPTIMISM, BSC, BASE, ARBITRUM];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChainId {
    Ethereum(u64),
    Optimism(u64),
    BinanceSmartChain(u64),
    Base(u64),
    Arbitrum(u64),
}

impl ChainId {

    pub fn new(id: u64) -> Result<Self, anyhow::Error> {
        let chain = match id {
            1 => ChainId::Ethereum(id),
            10 => ChainId::Optimism(id),
            56 => ChainId::BinanceSmartChain(id),
            8453 => ChainId::Base(id),
            42161 => ChainId::Arbitrum(id),
            _ => anyhow::bail!("Unsupported chain id: {}", id),
        };
        Ok(chain)
    }

    /// Return all supported chains
    pub fn supported_chains() -> Vec<ChainId> {
        SUPPORTED_CHAINS.iter().map(|id| ChainId::new(*id).unwrap()).collect()
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
            // Arbitrum doesnt have a fixed block time but on average its 250ms (based on arbscan)
            ChainId::Arbitrum(_) => 250,
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
}