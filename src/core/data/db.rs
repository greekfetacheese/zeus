use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
    str::FromStr
};

use crate::core::user::Portfolio;
use crate::core::utils::data_dir;
use zeus_eth::alloy_primitives::{Address, U256};
use zeus_eth::defi::currency::{Currency, native::NativeCurrency, erc20::ERC20Token};
use zeus_eth::prelude::{UniswapV2Pool, UniswapV3Pool};
use zeus_token_list::{ETHEREUM, OPTIMISM, BASE, ARBITRUM, BINANCE_SMART_CHAIN, tokens::UniswapToken};
use lazy_static::lazy_static;

lazy_static! {
    pub static ref ZEUS_DB: Arc<RwLock<ZeusDB>> = Arc::new(RwLock::new(ZeusDB::default()));
}

pub const ZEUS_DB_FILE: &str = "zeus_db.json";


/// Token Balances
/// 
/// Key: (chain_id, owner, token) -> Value: Balance
pub type TokenBalances = HashMap<(u64, Address, Address), U256>;

/// Eth Balances (or any native currency for evm compatable chains)
/// 
/// Key: (chain_id, owner) -> Value: Balance
pub type EthBalances = HashMap<(u64, Address), U256>;

/// Holds all currencies
/// 
/// Key: chain_id
pub type Currencies = HashMap<u64, Arc<Vec<Currency>>>;

/// Portfolios
/// 
/// Key: (chain_id, owner)
pub type Portfolios = HashMap<(u64, Address), Arc<Portfolio>>;

/// Token Prices in USD
/// 
/// Key: (chain_id, token) -> Value: Price in USD
pub type TokenPrice = HashMap<(u64, Address), f64>;


/// Uniswap V2 Pools
/// 
/// Key: (chain_id, tokenA, tokenB) -> Value: Pool
pub type UniswapV2Pools = HashMap<(u64, Address, Address), UniswapV2Pool>;

/// Uniswap V3 Pools
/// 
/// Key: (chain_id, fee, tokenA, tokenB) -> Value: Pool
pub type UniswapV3Pools = HashMap<(u64, u32, Address, Address), UniswapV3Pool>;


#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct ZeusDB {
    #[serde(with = "serde_helpers")]
    pub token_balance: TokenBalances,

    #[serde(with = "serde_helpers")]
    pub eth_balance: EthBalances,

    #[serde(with = "serde_helpers")]
    pub currency: Currencies,

    #[serde(with = "serde_helpers")]
    pub portfolios: Portfolios,

    #[serde(with = "serde_helpers")]
    pub token_price: TokenPrice,

    #[serde(with = "serde_helpers")]
    pub uniswap_v2_pools: UniswapV2Pools,

    #[serde(with = "serde_helpers")]
    pub uniswap_v3_pools: UniswapV3Pools
}

impl ZeusDB {

    pub fn load_from_file() -> Result<Self, anyhow::Error> {
        let dir = data_dir()?.join(ZEUS_DB_FILE);
        let data = std::fs::read(dir)?;
        let db = serde_json::from_slice(&data)?;
        Ok(db)
    }

    pub fn save_to_file(&self) -> Result<(), anyhow::Error> {
        let db_vec = serde_json::to_vec(&self)?;
        let dir = data_dir()?.join(ZEUS_DB_FILE);
        std::fs::write(dir, db_vec)?;
        Ok(())
    }

    pub fn get_token_balance(&self, chain_id: u64, owner: Address, token: Address) -> U256 {
        let key = (chain_id, owner, token);
        self.token_balance.get(&key).cloned().unwrap_or_default()
    }

    pub fn insert_token_balance(&mut self, chain_id: u64, owner: Address, token: Address, balance: U256) {
        let key = (chain_id, owner, token);
        self.token_balance.insert(key, balance);
    }

    pub fn get_eth_balance(&self, chain_id: u64, owner: Address) -> U256 {
        let key = (chain_id, owner);
        self.eth_balance.get(&key).cloned().unwrap_or_default()
    }

    pub fn insert_eth_balance(&mut self, chain_id: u64, owner: Address, balance: U256) {
        let key = (chain_id, owner);
        self.eth_balance.insert(key, balance);
    }

    pub fn get_currency(&self, chain_id: u64) -> Arc<Vec<Currency>> {
        self.currency.get(&chain_id).cloned().unwrap_or_default()
    }

    pub fn insert_currency(&mut self, chain_id: u64, currency: Currency) {
        if let Some(currencies_arc) = self.currency.get_mut(&chain_id) {
            let currencies = Arc::make_mut(currencies_arc);
            currencies.push(currency);
        } else {
            self.currency.insert(chain_id, Arc::new(vec![currency]));
        }
    }

    pub fn get_portfolio(&self, chain_id: u64, owner: Address) -> Arc<Portfolio> {
        let key = (chain_id, owner);
        self.portfolios.get(&key).cloned().unwrap_or_default()
    }

    pub fn insert_portfolio(&mut self, chain_id: u64, owner: Address, portfolio: Portfolio) {
        let key = (chain_id, owner);
        if let Some(portfolio_arc) = self.portfolios.get_mut(&key) {
            let portfolio_mut = Arc::make_mut(portfolio_arc);
            *portfolio_mut = portfolio;
        } else {
            self.portfolios.insert((chain_id, owner), Arc::new(portfolio));
        }
    }

    pub fn get_price(&self, chain_id: u64, token_address: Address) -> f64 {
        let key = (chain_id, token_address);
        self.token_price.get(&key).cloned().unwrap_or_default()
    }

    pub fn insert_price(&mut self, chain_id: u64, token_address: Address, price: f64) {
        self.token_price.insert((chain_id, token_address), price);
    }

    pub fn get_v2_pool(&self, chain_id: u64, token_a: Address, token_b: Address) -> Option<UniswapV2Pool> {
        let key = (chain_id, token_a, token_b);
        self.uniswap_v2_pools.get(&key).cloned()
    }

    pub fn insert_v2_pool(&mut self, chain_id: u64, token_a: Address, token_b: Address, pool: UniswapV2Pool) {
        let key = (chain_id, token_a, token_b);
        self.uniswap_v2_pools.insert(key, pool);
    }

    pub fn get_v3_pool(&self, chain_id: u64, fee: u32, token_a: Address, token_b: Address) -> Option<UniswapV3Pool> {
        let key = (chain_id, fee, token_a, token_b);
        self.uniswap_v3_pools.get(&key).cloned()
    }

    pub fn insert_v3_pool(&mut self, chain_id: u64, fee: u32, token_a: Address, token_b: Address, pool: UniswapV3Pool) {
        let key = (chain_id, fee, token_a, token_b);
        self.uniswap_v3_pools.insert(key, pool);
    }

    pub fn load_default_currencies(&mut self) -> Result<(), anyhow::Error> {

        // Native Currencies
    
        // Ethereum
        let eth_native = NativeCurrency::from_chain_id(1);
        self.insert_currency(1, Currency::from_native(eth_native.clone()));
    
        // Binance Smart Chain
        let bnb_native = NativeCurrency::from_chain_id(56);
        self.insert_currency(56, Currency::from_native(bnb_native));
    
        // Optimism
        self.insert_currency(10, Currency::from_native(eth_native.clone()));
    
        // Base Network
        self.insert_currency(8453, Currency::from_native(eth_native.clone()));
    
        // Arbitrum
        self.insert_currency(42161, Currency::from_native(eth_native));
    
        // Load the default token list
        let mut default_tokens: Vec<ERC20Token> = Vec::new();
        let eth_tokens: Vec<UniswapToken> = serde_json::from_str(ETHEREUM)?;
        let op_tokens: Vec<UniswapToken> = serde_json::from_str(OPTIMISM)?;
        let base_tokens: Vec<UniswapToken> = serde_json::from_str(BASE)?;
        let arbitrum_tokens: Vec<UniswapToken> = serde_json::from_str(ARBITRUM)?;
        let bnb_tokens: Vec<UniswapToken> = serde_json::from_str(BINANCE_SMART_CHAIN)?;
    
        for token in eth_tokens {
            let erc20 = ERC20Token {
                address: Address::from_str(&token.address)?,
                chain_id: token.chain_id,
                symbol: token.symbol,
                name: token.name,
                decimals: token.decimals,
                total_supply: U256::ZERO,
                icon: None
            };
            default_tokens.push(erc20)
        }
        
        for token in op_tokens {
            let erc20 = ERC20Token {
                address: Address::from_str(&token.address)?,
                chain_id: token.chain_id,
                symbol: token.symbol,
                name: token.name,
                decimals: token.decimals,
                total_supply: U256::ZERO,
                icon: None
            };
            default_tokens.push(erc20)
        }
    
        for token in base_tokens {
            let erc20 = ERC20Token {
                address: Address::from_str(&token.address)?,
                chain_id: token.chain_id,
                symbol: token.symbol,
                name: token.name,
                decimals: token.decimals,
                total_supply: U256::ZERO,
                icon: None
            };
            default_tokens.push(erc20)
        }
    
        for token in arbitrum_tokens {
            let erc20 = ERC20Token {
                address: Address::from_str(&token.address)?,
                chain_id: token.chain_id,
                symbol: token.symbol,
                name: token.name,
                decimals: token.decimals,
                total_supply: U256::ZERO,
                icon: None
            };
            default_tokens.push(erc20)
        }
    
        for token in bnb_tokens {
            let erc20 = ERC20Token {
                address: Address::from_str(&token.address)?,
                chain_id: token.chain_id,
                symbol: token.symbol,
                name: token.name,
                decimals: token.decimals,
                total_supply: U256::ZERO,
                icon: None
            };
            default_tokens.push(erc20)
        }
    
        for token in default_tokens {
            let chain_id = token.chain_id;
            let currency = Currency::from_erc20(token);
            self.insert_currency(chain_id, currency)
        }
    
        Ok(())
    
    }
    
}

mod serde_helpers {

use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize, Serializer};
use std::collections::HashMap;

pub fn serialize<S, K, V>(map: &HashMap<K, V>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
    K: Serialize,
    V: Serialize,
{
    let stringified_map: HashMap<String, &V> = map
        .iter()
        .map(|(k, v)| (serde_json::to_string(k).unwrap(), v))
        .collect();
    stringified_map.serialize(serializer)
}

pub fn deserialize<'de, D, K, V>(deserializer: D) -> Result<HashMap<K, V>, D::Error>
where
    D: Deserializer<'de>,
    K: DeserializeOwned + std::cmp::Eq + std::hash::Hash,
    V: DeserializeOwned,
{
    let stringified_map: HashMap<String, V> = HashMap::deserialize(deserializer)?;
    stringified_map
        .into_iter()
        .map(|(k, v)| {
            let key = serde_json::from_str(&k).map_err(serde::de::Error::custom)?;
            Ok((key, v))
        })
        .collect()
}


}