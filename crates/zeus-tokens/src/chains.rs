use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use zeus_eth::types::ChainId;

/// Trust Wallet chain folder names under `blockchains/`.
pub const ETHEREUM: &str = "ethereum";
pub const OPTIMISM: &str = "optimism";
pub const BINANCE: &str = "binance";
pub const BASE: &str = "base";
pub const ARBITRUM: &str = "arbitrum";

/// Chains included in the default Zeus token list (Trust Wallet asset dirs).
/// Sepolia has no Trust Wallet assets tree and is not part of this list.
pub const TOKEN_CHAIN_IDS: [u64; 5] = [1, 10, 56, 8453, 42161];

#[derive(Clone, Serialize, Deserialize)]
pub struct TokenInfo {
   pub name: String,
   #[serde(rename = "type")]
   pub type2: String,
   pub symbol: String,
   pub decimals: u8,
   pub website: String,
   pub description: String,
   pub explorer: String,
   pub status: String,
   #[serde(rename = "id")]
   pub address: String,
}

pub fn token_chains() -> Vec<ChainId> {
   TOKEN_CHAIN_IDS
      .iter()
      .map(|id| ChainId::new(*id).expect("TOKEN_CHAIN_IDS are supported"))
      .collect()
}

pub fn trustwallet_slug(chain: ChainId) -> Option<&'static str> {
   match chain {
      ChainId::Ethereum => Some(ETHEREUM),
      ChainId::Optimism => Some(OPTIMISM),
      ChainId::BinanceSmartChain => Some(BINANCE),
      ChainId::Base => Some(BASE),
      ChainId::Arbitrum => Some(ARBITRUM),
      ChainId::EthereumSepolia => None,
   }
}

pub fn parse_chain(s: &str) -> Result<u64, String> {
   match s.to_ascii_lowercase().as_str() {
      "eth" | "ethereum" | "mainnet" | "1" => Ok(1),
      "op" | "optimism" | "10" => Ok(10),
      "bsc" | "binance" | "bnb" | "56" => Ok(56),
      "base" | "8453" => Ok(8453),
      "arb" | "arbitrum" | "42161" => Ok(42161),
      other => other.parse::<u64>().map_err(|e| format!("invalid chain `{other}`: {e}")),
   }
}

/// Resolve `{work_dir}/blockchains/{slug}/assets` (git sparse clone) or the
/// legacy flat layout `{work_dir}/{slug}/assets`.
pub fn assets_dir(work_dir: &Path, slug: &str) -> PathBuf {
   let nested = work_dir.join("blockchains").join(slug).join("assets");
   if nested.exists() {
      nested
   } else {
      work_dir.join(slug).join("assets")
   }
}

pub fn assets_dir_for_chain(work_dir: &Path, chain: ChainId) -> anyhow::Result<PathBuf> {
   let slug = trustwallet_slug(chain).with_context(|| {
      format!(
         "chain {} has no Trust Wallet assets directory",
         chain.id()
      )
   })?;
   Ok(assets_dir(work_dir, slug))
}

pub fn has_assets(work_dir: &Path, chains: &[ChainId]) -> bool {
   chains
      .iter()
      .all(|chain| assets_dir_for_chain(work_dir, *chain).map(|dir| dir.is_dir()).unwrap_or(false))
}

pub fn require_chain(id: u64) -> anyhow::Result<ChainId> {
   let chain = ChainId::new(id)?;
   if trustwallet_slug(chain).is_none() {
      bail!(
         "chain {} is not part of the Trust Wallet token list (ethereum, optimism, binance, base, arbitrum)",
         id
      );
   }
   Ok(chain)
}
