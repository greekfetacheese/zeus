use alloy_contract::private::Network;
use alloy_primitives::{ Address, Bytes, U256 };
use alloy_provider::Provider;
use alloy_rpc_types::BlockId;
use alloy_sol_types::SolCall;
use alloy_transport::Transport;

use crate::abi::erc20::ERC20;
use crate::utils::batch_request;
use crate::defi::utils::common_addr;
use crate::{ BSC, BASE, ARBITRUM, OPTIMISM };
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
    pub async fn new<T, P, N>(client: P, address: Address, chain_id: u64) -> Result<Self, anyhow::Error>
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

    pub async fn allowance<T, P, N>(&self, owner: Address, spender: Address, client: P) -> Result<U256, anyhow::Error>
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

    /// Default weth instance (ETH)
    pub fn weth() -> ERC20Token {
        ERC20Token::default()
    }

    /// WETH (BSC)
    pub fn weth_bsc() -> ERC20Token {
        let mut weth = ERC20Token::default();
        weth.address = common_addr::weth(BSC).unwrap();
        weth.chain_id = BSC;
        weth
    }

    /// WETH (Optimism)
    pub fn weth_op() -> ERC20Token {
        let mut weth = ERC20Token::default();
        weth.address = common_addr::weth(OPTIMISM).unwrap();
        weth.chain_id = OPTIMISM;
        weth
    }

    /// WETH (BASE)
    pub fn weth_base() -> ERC20Token {
        let mut weth = ERC20Token::default();
        weth.address = common_addr::weth(BASE).unwrap();
        weth.chain_id = BASE;
        weth
    }

    /// WETH (Arbitrum)
    pub fn weth_arbitrum() -> ERC20Token {
        let mut weth = ERC20Token::default();
        weth.address = common_addr::weth(ARBITRUM).unwrap();
        weth.chain_id = ARBITRUM;
        weth
    }

    /// Default USDC instance (ETH)
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

    /// USDC (Optimism)
    pub fn usdc_op() -> ERC20Token {
        let mut usdc = ERC20Token::usdc();
        usdc.address = common_addr::usdc(OPTIMISM).unwrap();
        usdc.chain_id = OPTIMISM;
        usdc
    }

    /// USDC (BSC)
    pub fn usdc_bsc() -> ERC20Token {
        let mut usdc = ERC20Token::usdc();
        usdc.address = common_addr::usdc(BSC).unwrap();
        usdc.chain_id = BSC;
        usdc
    }

    /// USDC (BASE)
    pub fn usdc_base() -> ERC20Token {
        let mut usdc = ERC20Token::usdc();
        usdc.address = common_addr::usdc(BASE).unwrap();
        usdc.chain_id = BASE;
        usdc
    }

    /// USDC (Arbitrum)
    pub fn usdc_arbitrum() -> ERC20Token {
        let mut usdc = ERC20Token::usdc();
        usdc.address = common_addr::usdc(ARBITRUM).unwrap();
        usdc.chain_id = ARBITRUM;
        usdc
    }

    /// Default USDT instance (ETH)
    pub fn usdt() -> ERC20Token {
        ERC20Token {
            chain_id: 1,
            name: "Tether USD".to_string(),
            address: Address::from_str("0xdAC17F958D2ee523a2206206994597C13D831ec7").unwrap(),
            decimals: 6,
            symbol: "USDT".to_string(),
            total_supply: U256::ZERO,
            icon: None,
        }
    }

    /// USDT (Optimism)
    pub fn usdt_op() -> ERC20Token {
        let mut usdt = ERC20Token::usdt();
        usdt.address = common_addr::usdt(OPTIMISM).unwrap();
        usdt.chain_id = OPTIMISM;
        usdt
    }

    /// USDT (BSC)
    pub fn usdt_bsc() -> ERC20Token {
        let mut usdt = ERC20Token::usdt();
        usdt.address = common_addr::usdt(BSC).unwrap();
        usdt.chain_id = BSC;
        usdt
    }

    /// USDT (Arbitrum)
    pub fn usdt_arbitrum() -> ERC20Token {
        let mut usdt = ERC20Token::usdt();
        usdt.address = common_addr::usdt(ARBITRUM).unwrap();
        usdt.chain_id = ARBITRUM;
        usdt
    }

    /// Default DAI instance (ETH)
    pub fn dai() -> ERC20Token {
        ERC20Token {
            chain_id: 1,
            name: "Dai Stablecoin".to_string(),
            address: Address::from_str("0x6B175474E89094C44Da98b954EedeAC495271d0F").unwrap(),
            decimals: 18,
            symbol: "DAI".to_string(),
            total_supply: U256::ZERO,
            icon: None,
        }
    }

    /// DAI (Optimism)
    pub fn dai_op() -> ERC20Token {
        let mut dai = ERC20Token::dai();
        dai.address = common_addr::dai(OPTIMISM).unwrap();
        dai.chain_id = OPTIMISM;
        dai
    }

    /// DAI (BSC)
    pub fn dai_bsc() -> ERC20Token {
        let mut dai = ERC20Token::dai();
        dai.address = common_addr::dai(BSC).unwrap();
        dai.chain_id = BSC;
        dai
    }

    /// DAI (BASE)
    pub fn dai_base() -> ERC20Token {
        let mut dai = ERC20Token::dai();
        dai.address = common_addr::dai(BASE).unwrap();
        dai.chain_id = BASE;
        dai
    }

    /// DAI (Arbitrum)
    pub fn dai_arbitrum() -> ERC20Token {
        let mut dai = ERC20Token::dai();
        dai.address = common_addr::dai(ARBITRUM).unwrap();
        dai.chain_id = ARBITRUM;
        dai
    }

    /// Default WBNB instance (BSC)
    pub fn wbnb() -> ERC20Token {
        ERC20Token {
            chain_id: 56,
            name: "Wrapped BNB".to_string(),
            address: Address::from_str("0xbb4CdB9CBd36B01bD1cBaEBF2De08d9173bc095c").unwrap(),
            decimals: 18,
            symbol: "WBNB".to_string(),
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