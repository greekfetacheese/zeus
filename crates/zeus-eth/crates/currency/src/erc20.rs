use abi::{
   alloy_contract::private::{Network, Provider},
   alloy_primitives::{Address, Bytes, U256, address},
   alloy_rpc_types::BlockId,
};
use types::{ARBITRUM, BASE, BSC, ChainId, ETH, OPTIMISM};
use utils::{
   address::{dai, usdc, usdt, wbnb, weth},
   batch,
};

use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};

/// Represents an ERC20 token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ERC20Token {
   pub chain_id: u64,
   pub address: Address,
   pub symbol: String,
   pub name: String,
   pub decimals: u8,
   pub total_supply: U256,
}

impl PartialEq for ERC20Token {
   fn eq(&self, other: &Self) -> bool {
      self.chain_id == other.chain_id && self.address == other.address
   }
}

impl Eq for ERC20Token {}

impl Hash for ERC20Token {
   fn hash<H: Hasher>(&self, state: &mut H) {
      self.chain_id.hash(state);
      self.address.hash(state);
   }
}

impl Default for ERC20Token {
   fn default() -> Self {
      Self {
         chain_id: 1,
         name: "Wrapped Ether".to_string(),
         address: address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"),
         decimals: 18,
         symbol: "WETH".to_string(),
         total_supply: U256::ZERO,
      }
   }
}

impl ERC20Token {
   /// Create a new ERC20Token by retrieving the token information from the blockchain
   pub async fn new<P, N>(client: P, token: Address, chain_id: u64) -> Result<Self, anyhow::Error>
   where
      P: Provider<N> + Clone + 'static,
      N: Network,
   {
      let info = batch::get_erc20_info(client, token).await?;

      Ok(Self {
         chain_id,
         address: token,
         symbol: info.symbol,
         name: info.name,
         decimals: info.decimals,
         total_supply: info.totalSupply,
      })
   }

   pub async fn from_batch<P, N>(client: P, chain: u64, tokens_addr: Vec<Address>) -> Result<Vec<Self>, anyhow::Error>
   where
      P: Provider<N> + Clone + 'static,
      N: Network,
   {
      let tokens_info = batch::get_erc20_tokens(client, tokens_addr.clone()).await?;

      let mut tokens_erc20 = Vec::new();
      for token_addr in tokens_addr {
         for token_info in &tokens_info {
            if token_info.addr == token_addr {
               tokens_erc20.push(Self {
                  chain_id: chain,
                  address: token_addr,
                  symbol: token_info.symbol.clone(),
                  name: token_info.name.clone(),
                  decimals: token_info.decimals,
                  total_supply: token_info.totalSupply,
               });
            }
         }
      }
      Ok(tokens_erc20)
   }

   pub fn from(
      chain_id: u64,
      address: Address,
      symbol: String,
      name: String,
      decimals: u8,
      total_supply: U256,
   ) -> Self {
      Self {
         chain_id,
         address,
         symbol,
         name,
         decimals,
         total_supply,
      }
   }

   /// - `block` If None the latest block is used.
   pub async fn balance_of<P, N>(
      &self,
      client: P,
      owner: Address,
      block: Option<BlockId>,
   ) -> Result<U256, anyhow::Error>
   where
      P: Provider<N> + Clone + 'static,
      N: Network,
   {
      let balance = abi::erc20::balance_of(self.address, owner, client, block).await?;
      Ok(balance)
   }

   pub async fn allowance<P, N>(&self, client: P, owner: Address, spender: Address) -> Result<U256, anyhow::Error>
   where
      P: Provider<N> + Clone + 'static,
      N: Network,
   {
      let allowance = abi::erc20::allowance(self.address, owner, spender, client).await?;
      Ok(allowance)
   }

   pub fn encode_balance_of(&self, owner: Address) -> Bytes {
      abi::erc20::encode_balance_of(owner)
   }

   pub fn encode_allowance(&self, owner: Address, spender: Address) -> Bytes {
      abi::erc20::encode_allowance(owner, spender)
   }

   pub fn encode_approve(&self, spender: Address, amount: U256) -> Bytes {
      abi::erc20::encode_approve(spender, amount)
   }

   pub fn encode_transfer(&self, to: Address, amount: U256) -> Bytes {
      abi::erc20::encode_transfer(to, amount)
   }

   pub fn encode_deposit(&self) -> Bytes {
      abi::weth9::encode_deposit()
   }

   pub fn encode_withdraw(&self, amount: U256) -> Bytes {
      abi::weth9::encode_withdraw(amount)
   }

   pub fn decode_balance_of(&self, data: &Bytes) -> Result<U256, anyhow::Error> {
      abi::erc20::decode_balance_of(data)
   }

   pub fn decode_allowance(&self, data: &Bytes) -> Result<U256, anyhow::Error> {
      abi::erc20::decode_allowance(data)
   }
}

// * Builders

impl ERC20Token {
   /// Wrapped Native Token based on the chain_id
   ///
   /// Panics if the chain is not supported
   pub fn wrapped_native_token(chain_id: u64) -> ERC20Token {
      let chain = ChainId::new(chain_id).unwrap();
      match chain {
         ChainId::Ethereum(_) => ERC20Token::weth(),
         ChainId::Optimism(_) => ERC20Token::weth_optimism(),
         ChainId::Base(_) => ERC20Token::weth_base(),
         ChainId::Arbitrum(_) => ERC20Token::weth_arbitrum(),
         ChainId::BinanceSmartChain(_) => ERC20Token::wbnb(),
      }
   }

   /// Get a base token based on its address and chain
   pub fn base_token(chain_id: u64, address: Address) -> Option<ERC20Token> {
      let tokens = ERC20Token::base_tokens(chain_id);

      for token in tokens {
         if token.address == address && token.chain_id == chain_id {
            return Some(token);
         }
      }
      None
   }

   /// Return a list of base tokens based on the chain id.
   pub fn base_tokens(chain_id: u64) -> Vec<ERC20Token> {
      let chain = ChainId::new(chain_id).unwrap_or(ChainId::Ethereum(1));
      match chain {
         ChainId::Ethereum(_) => vec![
            ERC20Token::weth(),
            ERC20Token::usdc(),
            ERC20Token::usdt(),
            ERC20Token::dai(),
         ],
         ChainId::Optimism(_) => vec![
            ERC20Token::weth_optimism(),
            ERC20Token::usdc_optimism(),
            ERC20Token::usdt_optimism(),
            ERC20Token::dai_optimism(),
         ],
         ChainId::Base(_) => vec![
            ERC20Token::weth_base(),
            ERC20Token::usdc_base(),
            ERC20Token::dai_base(),
         ],
         ChainId::Arbitrum(_) => vec![
            ERC20Token::weth_arbitrum(),
            ERC20Token::usdc_arbitrum(),
            ERC20Token::usdt_arbitrum(),
            ERC20Token::dai_arbitrum(),
         ],
         ChainId::BinanceSmartChain(_) => vec![
            ERC20Token::wbnb(),
            ERC20Token::usdc_bsc(),
            ERC20Token::usdt_bsc(),
            ERC20Token::dai_bsc(),
         ],
      }
   }

   /// Default weth instance (ETH)
   pub fn weth() -> ERC20Token {
      ERC20Token::default()
   }

   /// Default WBTC instance (ETH)
   pub fn wbtc() -> ERC20Token {
      ERC20Token {
         chain_id: ETH,
         name: "Wrapped BTC".to_string(),
         address: address!("0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599"),
         decimals: 8,
         symbol: "WBTC".to_string(),
         total_supply: U256::ZERO,
      }
   }

   /// Default LINK instance (ETH)
   pub fn link() -> ERC20Token {
      ERC20Token {
         chain_id: ETH,
         name: "ChainLink Token".to_string(),
         address: address!("0x514910771AF9Ca656af840dff83E8264EcF986CA"),
         decimals: 18,
         symbol: "LINK".to_string(),
         total_supply: U256::ZERO,
      }
   }

   /// Default WBNB instance (BSC)
   pub fn wbnb() -> ERC20Token {
      ERC20Token {
         chain_id: BSC,
         name: "Wrapped BNB".to_string(),
         address: wbnb(BSC).unwrap(),
         decimals: 18,
         symbol: "WBNB".to_string(),
         total_supply: U256::ZERO,
      }
   }

   /// WETH (BSC)
   pub fn weth_bsc() -> ERC20Token {
      let mut weth_token = ERC20Token::default();
      weth_token.address = weth(BSC).unwrap();
      weth_token.chain_id = BSC;
      weth_token
   }

   /// WETH (Optimism)
   pub fn weth_optimism() -> ERC20Token {
      let mut weth_token = ERC20Token::default();
      weth_token.address = weth(OPTIMISM).unwrap();
      weth_token.chain_id = OPTIMISM;
      weth_token
   }

   /// WETH (Base)
   pub fn weth_base() -> ERC20Token {
      let mut weth_token = ERC20Token::default();
      weth_token.address = weth(BASE).unwrap();
      weth_token.chain_id = BASE;
      weth_token
   }

   /// WETH (Arbitrum)
   pub fn weth_arbitrum() -> ERC20Token {
      let mut weth_token = ERC20Token::default();
      weth_token.address = weth(ARBITRUM).unwrap();
      weth_token.chain_id = ARBITRUM;
      weth_token
   }

   /// Default USDC instance (ETH)
   pub fn usdc() -> ERC20Token {
      ERC20Token {
         chain_id: ETH,
         name: "USD Coin".to_string(),
         address: usdc(ETH).unwrap(),
         decimals: 6,
         symbol: "USDC".to_string(),
         total_supply: U256::ZERO,
      }
   }

   /// USDC (Optimism)
   pub fn usdc_optimism() -> ERC20Token {
      let mut token = ERC20Token::usdc();
      token.chain_id = OPTIMISM;
      token.address = usdc(OPTIMISM).unwrap();
      token
   }

   /// USDC (BSC)
   pub fn usdc_bsc() -> ERC20Token {
      let mut token = ERC20Token::usdc();
      token.chain_id = BSC;
      token.address = usdc(BSC).unwrap();
      token
   }

   /// USDC (Base)
   pub fn usdc_base() -> ERC20Token {
      let mut token = ERC20Token::usdc();
      token.chain_id = BASE;
      token.address = usdc(BASE).unwrap();
      token
   }

   /// USDC (Arbitrum)
   pub fn usdc_arbitrum() -> ERC20Token {
      let mut token = ERC20Token::usdc();
      token.chain_id = ARBITRUM;
      token.address = usdc(ARBITRUM).unwrap();
      token
   }

   /// Default USDT instance (ETH)
   pub fn usdt() -> ERC20Token {
      ERC20Token {
         chain_id: ETH,
         name: "Tether USD".to_string(),
         address: usdt(ETH).unwrap(),
         decimals: 6,
         symbol: "USDT".to_string(),
         total_supply: U256::ZERO,
      }
   }

   /// USDT (Optimism)
   pub fn usdt_optimism() -> ERC20Token {
      let mut token = ERC20Token::usdt();
      token.chain_id = OPTIMISM;
      token.address = usdt(OPTIMISM).unwrap();
      token
   }

   /// USDT (BSC)
   pub fn usdt_bsc() -> ERC20Token {
      let mut token = ERC20Token::usdt();
      token.chain_id = BSC;
      token.address = usdt(BSC).unwrap();
      token
   }

   /// USDT (Arbitrum)
   pub fn usdt_arbitrum() -> ERC20Token {
      let mut token = ERC20Token::usdt();
      token.chain_id = ARBITRUM;
      token.address = usdt(ARBITRUM).unwrap();
      token
   }

   /// Default DAI instance (ETH)
   pub fn dai() -> ERC20Token {
      ERC20Token {
         chain_id: ETH,
         name: "Dai Stablecoin".to_string(),
         address: dai(ETH).unwrap(),
         decimals: 18,
         symbol: "DAI".to_string(),
         total_supply: U256::ZERO,
      }
   }

   /// DAI (Optimism)
   pub fn dai_optimism() -> ERC20Token {
      let mut token = ERC20Token::dai();
      token.chain_id = OPTIMISM;
      token.address = dai(OPTIMISM).unwrap();
      token
   }

   /// DAI (BSC)
   pub fn dai_bsc() -> ERC20Token {
      let mut token = ERC20Token::dai();
      token.chain_id = BSC;
      token.address = dai(BSC).unwrap();
      token
   }

   /// DAI (Base)
   pub fn dai_base() -> ERC20Token {
      let mut token = ERC20Token::dai();
      token.chain_id = BASE;
      token.address = dai(BASE).unwrap();
      token
   }

   /// DAI (Arbitrum)
   pub fn dai_arbitrum() -> ERC20Token {
      let mut token = ERC20Token::dai();
      token.chain_id = ARBITRUM;
      token.address = dai(ARBITRUM).unwrap();
      token
   }
}

// ** Helpers
impl ERC20Token {
   pub fn is_weth(&self) -> bool {
      self.address == weth(self.chain_id).unwrap_or_default()
   }

   pub fn is_usdc(&self) -> bool {
      self.address == usdc(self.chain_id).unwrap_or_default()
   }

   pub fn is_usdt(&self) -> bool {
      self.address == usdt(self.chain_id).unwrap_or_default()
   }

   pub fn is_dai(&self) -> bool {
      self.address == dai(self.chain_id).unwrap_or_default()
   }

   pub fn is_wbnb(&self) -> bool {
      self.address == wbnb(self.chain_id).unwrap_or_default()
   }

   pub fn is_stablecoin(&self) -> bool {
      self.is_usdc() || self.is_usdt() || self.is_dai()
   }
}

#[cfg(test)]
mod tests {
   use super::ERC20Token;
   use abi::alloy_provider::ProviderBuilder;
   use url::Url;

   #[tokio::test]
   async fn can_get_erc20() {
      let url = Url::parse("https://eth.merkle.io").unwrap();
      let client = ProviderBuilder::new().connect_http(url);

      let weth = ERC20Token::weth();

      let fetched_weth = ERC20Token::new(client, weth.address, weth.chain_id)
         .await
         .unwrap();

      assert_eq!(weth.symbol, fetched_weth.symbol);
      assert_eq!(weth.name, fetched_weth.name);
      assert_eq!(weth.decimals, fetched_weth.decimals);
   }

   #[tokio::test]
   async fn can_get_erc20_batch() {
      let url = Url::parse("https://eth.merkle.io").unwrap();
      let client = ProviderBuilder::new().connect_http(url);

      let weth = ERC20Token::weth();
      let usdc = ERC20Token::usdc();
      let usdt = ERC20Token::usdt();
      let dai = ERC20Token::dai();
      let addr = vec![weth.address, usdc.address, usdt.address, dai.address];

      let tokens_erc20 = ERC20Token::from_batch(client.clone(), 1, addr.clone())
         .await
         .unwrap();

      assert_eq!(tokens_erc20.len(), addr.len());
      assert_eq!(tokens_erc20[0], weth);
      assert_eq!(tokens_erc20[1], usdc);
      assert_eq!(tokens_erc20[2], usdt);
      assert_eq!(tokens_erc20[3], dai);
   }
}
