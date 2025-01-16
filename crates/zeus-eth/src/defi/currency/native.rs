use serde::{Deserialize, Serialize};

/// Represents a Native Currency to its chain
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NativeCurrency {
    pub chain_id: u64,
    pub symbol: String,
    pub name: String,
    pub decimals: u8,
    pub icon: Option<Vec<u8>>,
}

impl NativeCurrency {
    pub fn new(
        chain_id: u64,
        symbol: String,
        name: String,
        decimals: u8,
        icon: Option<Vec<u8>>,
    ) -> Self {
        Self {
            chain_id,
            symbol,
            name,
            decimals,
            icon,
        }
    }

    /// Create a new Native Currency from the chain id
    pub fn from_chain_id(id: u64) -> Self {
        match id {
            56 => Self {
                chain_id: 56,
                symbol: "BNB".to_string(),
                name: "Binance Smart Chain".to_string(),
                decimals: 18,
                icon: None,
            },
            _ => Self {
                ..Default::default()
            },
        }
    }
}

impl Default for NativeCurrency {
    fn default() -> Self {
        Self {
            chain_id: 1,
            symbol: "ETH".to_string(),
            name: "Ethereum".to_string(),
            decimals: 18,
            icon: None,
        }
    }
}
