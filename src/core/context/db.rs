use std::{ collections::HashMap, sync::Arc, str::FromStr };

use crate::core::user::Portfolio;
use crate::core::utils::data_dir;
use zeus_eth::{
    alloy_primitives::{ Address, U256 },
    currency::{ erc20::ERC20Token, native::NativeCurrency, Currency },
    types,
};
use zeus_token_list::{
    ETHEREUM,
    OPTIMISM,
    BASE,
    ARBITRUM,
    BINANCE_SMART_CHAIN,
    tokens::UniswapToken,
};
use anyhow::anyhow;

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


/// Saved contact by the user
#[derive(Default, Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Contact {
    pub name: String,
    pub address: String,
    pub notes: String,
}

impl Contact {
    pub fn new(name: String, address: String, notes: String) -> Self {
        Self {
            name,
            address,
            notes,
        }
    }

    pub fn address_short(&self) -> String {
        format!("{}...{}", &self.address[..6], &self.address[36..])
    }

    /// Serialize to JSON String
    pub fn serialize(&self) -> Result<String, anyhow::Error> {
        Ok(serde_json::to_string(self)?)
    }

    /// Deserialize from slice
    pub fn from_slice(data: &[u8]) -> Result<Self, anyhow::Error> {
        Ok(serde_json::from_slice::<Contact>(data)?)
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct ZeusDB {
    #[serde(with = "serde_helpers")]
    pub token_balance: TokenBalances,

    #[serde(with = "serde_helpers")]
    pub eth_balance: EthBalances,

    #[serde(with = "serde_helpers")]
    pub currencies: Currencies,

    #[serde(with = "serde_helpers")]
    pub portfolios: Portfolios,

    pub contacts: Vec<Contact>,
}

impl Default for ZeusDB {
    fn default() -> Self {
        Self {
            token_balance: Default::default(),
            eth_balance: Default::default(),
            currencies: Default::default(),
            portfolios: Default::default(),
            contacts: Default::default(),
        }
    }
}

impl ZeusDB {
    pub fn load_from_file() -> Result<Self, anyhow::Error> {
        let dir = data_dir()?.join(ZEUS_DB_FILE);
        let data = std::fs::read(dir)?;
        let db = serde_json::from_slice(&data)?;
        Ok(db)
    }

    pub fn save_to_file(&self) -> Result<(), anyhow::Error> {
        let db = serde_json::to_string(&self)?;
        let dir = data_dir()?.join(ZEUS_DB_FILE);
        std::fs::write(dir, db)?;
        Ok(())
    }

    pub fn get_token_balance(&self, chain_id: u64, owner: Address, token: Address) -> U256 {
        let key = (chain_id, owner, token);
        self.token_balance.get(&key).cloned().unwrap_or_default()
    }

    pub fn insert_token_balance(
        &mut self,
        chain_id: u64,
        owner: Address,
        token: Address,
        balance: U256
    ) {
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

    pub fn get_currencies(&self, chain_id: u64) -> Arc<Vec<Currency>> {
        self.currencies.get(&chain_id).cloned().unwrap_or_default()
    }

    pub fn insert_currency(&mut self, chain_id: u64, currency: Currency) {
        if let Some(currencies_arc) = self.currencies.get_mut(&chain_id) {
            let currencies = Arc::make_mut(currencies_arc);
            currencies.push(currency);
        } else {
            self.currencies.insert(chain_id, Arc::new(vec![currency]));
        }
    }

    pub fn get_portfolio(&self, chain_id: u64, owner: Address) -> Option<Arc<Portfolio>> {
        let key = (chain_id, owner);
        self.portfolios.get(&key).cloned()
    }

    pub fn get_portfolio_mut(&mut self, chain_id: u64, owner: Address) -> Option<&mut Portfolio> {
        let key = (chain_id, owner);
        self.portfolios.get_mut(&key).map(|arc| Arc::make_mut(arc))
    }

    pub fn insert_portfolio(&mut self, chain_id: u64, owner: Address, portfolio: Portfolio) {
        let key = (chain_id, owner);
        self.portfolios.insert(key, Arc::new(portfolio));
    }

    pub fn add_contact(&mut self, contact: Contact) -> Result<(), anyhow::Error> {
        // make sure name and address are unique
        if self.contacts.iter().any(|c| c.name == contact.name) {
            return Err(anyhow!("Contact with name {} already exists", contact.name));
        } else if self.contacts.iter().any(|c| c.address == contact.address) {
            return Err(anyhow!("Contact with address {} already exists", contact.address));
        }
        self.contacts.push(contact);
        Ok(())
    }

    pub fn remove_contact(&mut self, address: String) {
        self.contacts.retain(|c| c.address != address);
    }           

    pub fn load_default_currencies(&mut self) -> Result<(), anyhow::Error> {
        // Native Currencies

        // Ethereum
        let eth_native = NativeCurrency::from_chain_id(types::ETH)?;
        self.insert_currency(types::ETH, Currency::from_native(eth_native.clone()));

        // Binance Smart Chain
        let bnb_native = NativeCurrency::from_chain_id(types::BSC)?;
        self.insert_currency(types::BSC, Currency::from_native(bnb_native));

        // Optimism
        self.insert_currency(types::OPTIMISM, Currency::from_native(eth_native.clone()));

        // Base Network
        self.insert_currency(types::BASE, Currency::from_native(eth_native.clone()));

        // Arbitrum
        self.insert_currency(types::ARBITRUM, Currency::from_native(eth_native));

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
            };
            default_tokens.push(erc20);
        }

        for token in op_tokens {
            let erc20 = ERC20Token {
                address: Address::from_str(&token.address)?,
                chain_id: token.chain_id,
                symbol: token.symbol,
                name: token.name,
                decimals: token.decimals,
                total_supply: U256::ZERO,
            };
            default_tokens.push(erc20);
        }

        for token in base_tokens {
            let erc20 = ERC20Token {
                address: Address::from_str(&token.address)?,
                chain_id: token.chain_id,
                symbol: token.symbol,
                name: token.name,
                decimals: token.decimals,
                total_supply: U256::ZERO,
            };
            default_tokens.push(erc20);
        }

        for token in arbitrum_tokens {
            let erc20 = ERC20Token {
                address: Address::from_str(&token.address)?,
                chain_id: token.chain_id,
                symbol: token.symbol,
                name: token.name,
                decimals: token.decimals,
                total_supply: U256::ZERO,
            };
            default_tokens.push(erc20);
        }

        for token in bnb_tokens {
            let erc20 = ERC20Token {
                address: Address::from_str(&token.address)?,
                chain_id: token.chain_id,
                symbol: token.symbol,
                name: token.name,
                decimals: token.decimals,
                total_supply: U256::ZERO,
            };
            default_tokens.push(erc20);
        }

        for token in default_tokens {
            let chain_id = token.chain_id;
            let currency = Currency::from_erc20(token);
            self.insert_currency(chain_id, currency);
        }

        Ok(())
    }
}

mod serde_helpers {
    use serde::{ de::DeserializeOwned, Deserialize, Deserializer, Serialize, Serializer };
    use std::collections::HashMap;

    pub fn serialize<S, K, V>(map: &HashMap<K, V>, serializer: S) -> Result<S::Ok, S::Error>
        where S: Serializer, K: Serialize, V: Serialize
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
            V: DeserializeOwned
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

mod tests {
    #[test]
    fn db_serde_test() {
        let mut db = super::ZeusDB::default();
        db.load_default_currencies().expect("Failed to load default currencies");
        let db_str = serde_json::to_string(&db).expect("Failed to serialize db");
        let _db2: super::ZeusDB = serde_json::from_str(&db_str).expect("Failed to deserialize db");
    }
}
