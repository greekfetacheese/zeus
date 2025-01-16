use alloy_contract::private::Network;
use alloy_primitives::{ Address, Bytes, U256 };
use alloy_provider::Provider;
use alloy_rpc_types::BlockId;
use alloy_sol_types::SolCall;
use alloy_transport::Transport;

use crate::abi::erc20::ERC20;
use crate::utils::batch_request;
use serde::{ Deserialize, Serialize };
use std::str::FromStr;

pub mod socials;

/// Represents an ERC20 Token
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ERC20Token {
    pub chain_id: u64,
    pub address: Address,
    pub symbol: String,
    pub name: String,
    pub decimals: u8,
    pub total_supply: U256,
    pub icon: Option<Vec<u8>>,
}

impl ERC20Token {
    /// Create a new ERC20Token by retrieving the token information from the blockchain
    pub async fn new<T, P, N>(
        client: P,
        address: Address,
        chain_id: u64
    )
        -> Result<Self, anyhow::Error>
        where T: Transport + Clone, P: Provider<T, N> + Clone, N: Network
    {
        let erc20 = batch_request::get_erc20_info(client, address, chain_id).await?;
        Ok(Self {
            chain_id: erc20.chain_id,
            address: erc20.address,
            symbol: erc20.symbol,
            name: erc20.name,
            decimals: erc20.decimals,
            total_supply: erc20.total_supply,
            icon: None,
        })
    }

    pub async fn balance_of<T, P, N>(
        &self,
        owner: Address,
        client: P,
        block_id: Option<BlockId>
    )
        -> Result<U256, anyhow::Error>
        where T: Transport + Clone, P: Provider<T, N> + Clone, N: Network
    {
        let block = block_id.unwrap_or(BlockId::latest());
        let contract = ERC20::new(self.address, client);
        let b = contract.balanceOf(owner).block(block).call().await?;
        Ok(b.balance)
    }

    pub async fn allowance<T, P, N>(
        &self,
        owner: Address,
        spender: Address,
        client: P
    )
        -> Result<U256, anyhow::Error>
        where T: Transport + Clone, P: Provider<T, N> + Clone, N: Network
    {
        let contract = ERC20::new(self.address, client);
        let allowance = contract.allowance(owner, spender).call().await?._0;
        Ok(allowance)
    }

    pub fn encode_balance_of(&self, owner: Address) -> Bytes {
        let contract = ERC20::balanceOfCall { owner };
        Bytes::from(contract.abi_encode())
    }

    pub fn encode_allowance(&self, owner: Address, spender: Address) -> Bytes {
        let contract = ERC20::allowanceCall { owner, spender };
        Bytes::from(contract.abi_encode())
    }

    pub fn encode_approve(&self, spender: Address, amount: U256) -> Bytes {
        let contract = ERC20::approveCall { spender, amount };
        Bytes::from(contract.abi_encode())
    }

    pub fn encode_transfer(&self, recipient: Address, amount: U256) -> Bytes {
        let contract = ERC20::transferCall { recipient, amount };
        Bytes::from(contract.abi_encode())
    }

    pub fn encode_deposit(&self) -> Bytes {
        let contract = ERC20::depositCall {};
        Bytes::from(contract.abi_encode())
    }

    pub fn encode_withdraw(&self, amount: U256) -> Bytes {
        let contract = ERC20::withdrawCall { amount };
        Bytes::from(contract.abi_encode())
    }

    pub fn decode_balance_of(&self, bytes: &Bytes) -> Result<U256, anyhow::Error> {
        let balance = ERC20::balanceOfCall::abi_decode_returns(&bytes, true)?;
        Ok(balance.balance)
    }

    pub fn decode_allowance(&self, bytes: &Bytes) -> Result<U256, anyhow::Error> {
        let allowance = ERC20::allowanceCall::abi_decode_returns(&bytes, true)?;
        Ok(allowance._0)
    }

    #[allow(dead_code)]
    async fn symbol<T, P, N>(address: Address, client: P) -> Result<String, anyhow::Error>
        where T: Transport + Clone, P: Provider<T, N> + Clone, N: Network
    {
        // ! There are cases like the MKR token where the symbol and name are not available
        let contract = ERC20::new(address, client.clone());
        let symbol = match contract.symbol().call().await {
            Ok(s) => s._0,
            Err(_) => "Unknown".to_string(),
        };
        Ok(symbol)
    }

    #[allow(dead_code)]
    async fn name<T, P, N>(address: Address, client: P) -> Result<String, anyhow::Error>
        where T: Transport + Clone, P: Provider<T, N> + Clone, N: Network
    {
        // ! There are cases like the MKR token where the symbol and name are not available
        let contract = ERC20::new(address, client.clone());
        let name = match contract.name().call().await {
            Ok(n) => n._0,
            Err(_) => "Unknown".to_string(),
        };
        Ok(name)
    }

    #[allow(dead_code)]
    async fn decimals<T, P, N>(address: Address, client: P) -> Result<u8, anyhow::Error>
        where T: Transport + Clone, P: Provider<T, N> + Clone, N: Network
    {
        let contract = ERC20::new(address, client.clone());
        let d = contract.decimals().call().await?._0;
        Ok(d)
    }

    #[allow(dead_code)]
    async fn total_supply<T, P, N>(address: Address, client: P) -> Result<U256, anyhow::Error>
        where T: Transport + Clone, P: Provider<T, N> + Clone, N: Network
    {
        let contract = ERC20::new(address, client.clone());
        let t = contract.totalSupply().call().await?._0;
        Ok(t)
    }

    pub fn weth() -> ERC20Token {
        ERC20Token::default()
    }

    pub fn usdc() -> ERC20Token {
        ERC20Token {
            chain_id: 1,
            name: "USD Coin".to_string(),
            address: Address::from_str("0xA0b86991c6218b36c1d19d4a2e9eb0ce3606eb48").unwrap(),
            decimals: 6,
            symbol: "USDC".to_string(),
            total_supply: U256::ZERO,
            icon: None,
        }
    }
}

impl Default for ERC20Token {
    fn default() -> Self {
        Self {
            chain_id: 1,
            name: "Wrapped Ether".to_string(),
            address: Address::from_str("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2").unwrap(),
            decimals: 18,
            symbol: "WETH".to_string(),
            total_supply: U256::ZERO,
            icon: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use alloy_provider::{ ProviderBuilder, WsConnect };
    use alloy_transport_http::reqwest::Url;
    use alloy_primitives::{ address, U256 };
    use super::ERC20Token;

    #[tokio::test]
    async fn can_get_erc20_eth() {
        let url = "wss://eth.merkle.io";
        let ws_connect = WsConnect::new(url);
        let client = ProviderBuilder::new().on_ws(ws_connect).await.unwrap();

        let weth = ERC20Token::weth();

        let fetched_weth = ERC20Token::new(client, weth.address, weth.chain_id).await.unwrap();

        assert_eq!(fetched_weth.symbol, weth.symbol);
        assert_eq!(fetched_weth.name, weth.name);
        assert_eq!(fetched_weth.decimals, weth.decimals);
    }

    #[tokio::test]
    async fn can_get_erc20_bsc() {
        let url = Url::parse("https://bscrpc.com").unwrap();
        let client = ProviderBuilder::new().on_http(url);

        let wbnb = ERC20Token {
            chain_id: 56,
            address: address!("bb4CdB9CBd36B01bD1cBaEBF2De08d9173bc095c"),
            symbol: "WBNB".to_string(),
            name: "Wrapped BNB".to_string(),
            decimals: 18,
            total_supply: U256::ZERO,
            icon: None,
        };

        let fetched_wbnb = ERC20Token::new(client, wbnb.address, wbnb.chain_id).await.unwrap();

        assert_eq!(fetched_wbnb.symbol, wbnb.symbol);
        assert_eq!(fetched_wbnb.name, wbnb.name);
        assert_eq!(fetched_wbnb.decimals, wbnb.decimals);
    }

    #[tokio::test]
    async fn can_get_erc20_base() {
        let url = Url::parse("https://base.llamarpc.com").unwrap();
        let client = ProviderBuilder::new().on_http(url);

        let weth = ERC20Token {
            chain_id: 8453,
            address: address!("4200000000000000000000000000000000000006"),
            symbol: "WETH".to_string(),
            name: "Wrapped Ether".to_string(),
            decimals: 18,
            total_supply: U256::ZERO,
            icon: None,
        };

        let fetched_weth = ERC20Token::new(client, weth.address, weth.chain_id).await.unwrap();

        assert_eq!(fetched_weth.symbol, weth.symbol);
        assert_eq!(fetched_weth.name, weth.name);
        assert_eq!(fetched_weth.decimals, weth.decimals);
    }

    #[tokio::test]
    async fn can_get_erc20_arbitrum() {
        let url = Url::parse("https://arbitrum.llamarpc.com").unwrap();
        let client = ProviderBuilder::new().on_http(url);

        let weth = ERC20Token {
            chain_id: 42161,
            address: address!("82aF49447D8a07e3bd95BD0d56f35241523fBab1"),
            symbol: "WETH".to_string(),
            name: "Wrapped Ether".to_string(),
            decimals: 18,
            total_supply: U256::ZERO,
            icon: None,
        };

        let fetched_weth = ERC20Token::new(client, weth.address, weth.chain_id).await.unwrap();

        assert_eq!(fetched_weth.symbol, weth.symbol);
        assert_eq!(fetched_weth.name, weth.name);
        assert_eq!(fetched_weth.decimals, weth.decimals);
    }

    #[tokio::test]
    async fn can_get_erc20_optimism() {
        let url = Url::parse("https://optimism.llamarpc.com").unwrap();
        let client = ProviderBuilder::new().on_http(url);

        let weth = ERC20Token {
            chain_id: 10,
            address: address!("4200000000000000000000000000000000000006"),
            symbol: "WETH".to_string(),
            name: "Wrapped Ether".to_string(),
            decimals: 18,
            total_supply: U256::ZERO,
            icon: None,
        };

        let fetched_weth = ERC20Token::new(client, weth.address, weth.chain_id).await.unwrap();

        assert_eq!(fetched_weth.symbol, weth.symbol);
        assert_eq!(fetched_weth.name, weth.name);
        assert_eq!(fetched_weth.decimals, weth.decimals);
    }
}