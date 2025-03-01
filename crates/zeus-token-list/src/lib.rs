pub mod tokens;

pub use tokens::{ARBITRUM, BASE, BINANCE_SMART_CHAIN, ETHEREUM, OPTIMISM};

/// Downloaded token icon data [TokenIconData]
pub const TOKEN_ICONS: &str = include_str!("../token-icons.json");

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct TokenIconData {
   pub address: String,
   pub chain_id: u64,
   pub icon_data: Vec<u8>,
}

impl TokenIconData {
   pub fn new(address: String, chain_id: u64, icon_data: Vec<u8>) -> Self {
      Self {
         address,
         chain_id,
         icon_data,
      }
   }
}
