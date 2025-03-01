pub const ETHEREUM: &str = include_str!("mainnet.json");
pub const BASE: &str = include_str!("base.json");
pub const OPTIMISM: &str = include_str!("optimism.json");
pub const ARBITRUM: &str = include_str!("arbitrum.json");
pub const BINANCE_SMART_CHAIN: &str = include_str!("bnb.json");

/// Default token from the Uniswap's token list
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct UniswapToken {
   pub name: String,
   pub address: String,
   pub symbol: String,
   pub decimals: u8,
   #[serde(rename = "chainId")]
   pub chain_id: u64,
   #[serde(rename = "logoURI")]
   pub logo_uri: String,
   pub extensions: Option<Extensions>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Extensions {
   #[serde(rename = "bridgeInfo")]
   pub bridge_info: BridgeInfo,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct BridgeInfo {
   #[serde(rename = "10")]
   pub chain_10: Option<TokenAddress>,
   #[serde(rename = "137")]
   pub chain_137: Option<TokenAddress>,
   #[serde(rename = "42161")]
   pub chain_42161: Option<TokenAddress>,
   #[serde(rename = "42220")]
   pub chain_42220: Option<TokenAddress>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct TokenAddress {
   #[serde(rename = "tokenAddress")]
   pub token_address: String,
}

#[cfg(test)]
mod tests {

   use super::*;

   #[test]
   fn test_token_list() {
      let _ethereum: Vec<UniswapToken> = serde_json::from_str(ETHEREUM).unwrap();
      let _base: Vec<UniswapToken> = serde_json::from_str(BASE).unwrap();
      let _op: Vec<UniswapToken> = serde_json::from_str(OPTIMISM).unwrap();
      let _arbitrum: Vec<UniswapToken> = serde_json::from_str(ARBITRUM).unwrap();
      let _bnb: Vec<UniswapToken> = serde_json::from_str(BINANCE_SMART_CHAIN).unwrap();
   }
}
