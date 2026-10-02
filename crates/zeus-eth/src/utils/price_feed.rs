use super::address_book::*;
use crate::{ERC20Token, types::ChainId};
use alloy_primitives::{U256, utils::format_units};
use alloy_rpc_types::BlockId;
use alloy_sol_types::sol;
use anyhow::bail;

use alloy_contract::private::{Network, Provider};

sol!(
    #[sol(rpc)]
    contract ChainLinkOracle {
        function latestAnswer() external view returns (int256);
    }
);

/// Get the ETH price on supported chains
///
/// - `block_id` The block to query the price at. If None, the latest block is used.
pub async fn get_eth_price<P, N>(
   client: P,
   chain_id: u64,
   block_id: Option<BlockId>,
) -> Result<f64, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let chain = ChainId::new(chain_id)?;

   let feed = match chain {
      ChainId::Ethereum => super::address_book::eth_usd_price_feed(chain_id)?,
      ChainId::EthereumSepolia => super::address_book::eth_usd_price_feed(chain_id)?,
      ChainId::Optimism => super::address_book::eth_usd_price_feed(chain_id)?,
      ChainId::Base => super::address_book::eth_usd_price_feed(chain_id)?,
      ChainId::Arbitrum => super::address_book::eth_usd_price_feed(chain_id)?,
      ChainId::RobinHood => super::address_book::eth_usd_price_feed(chain_id)?,
      ChainId::BinanceSmartChain => bail!("ETH-USD price feed not available on BSC"),
   };

   let block_id = block_id.unwrap_or(BlockId::latest());

   let oracle = ChainLinkOracle::new(feed, client);
   let eth_usd = oracle.latestAnswer().block(block_id).call().await?;

   let eth_usd = eth_usd.to_string().parse::<U256>()?;
   let formatted = format_units(eth_usd, 8)?.parse::<f64>()?;
   Ok(formatted)
}

/// Get the BNB price on the Binance Smart Chain
///
/// - `block_id` The block to query the price at. If None, the latest block is used.
pub async fn get_bnb_price<P, N>(client: P, block_id: Option<BlockId>) -> Result<f64, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let block_id = block_id.unwrap_or(BlockId::latest());

   let feed = super::address_book::bnb_usd_price_feed();
   let oracle = ChainLinkOracle::new(feed, client);
   let bnb_usd = oracle.latestAnswer().block(block_id).call().await?;

   let bnb_usd = bnb_usd.to_string().parse::<U256>()?;
   let formatted = format_units(bnb_usd, 8)?.parse::<f64>()?;
   Ok(formatted)
}

/// Get the USD price of a base token
pub async fn get_base_token_price<P, N>(
   client: P,
   token: ERC20Token,
   block: Option<BlockId>,
) -> Result<f64, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let chain = ChainId::new(token.chain_id)?;

   if chain == ChainId::BinanceSmartChain {
      if token.is_wbnb() {
         get_bnb_price(client, block).await
      } else {
         get_stablecoin_price(client, token, block).await
      }
   } else if token.is_weth() {
      get_eth_price(client, token.chain_id, block).await
   } else {
      get_stablecoin_price(client, token, block).await
   }
}

pub async fn get_stablecoin_price<P, N>(
   client: P,
   token: ERC20Token,
   block: Option<BlockId>,
) -> Result<f64, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let is_stable = token.is_stablecoin();

   if !is_stable {
      return Err(anyhow::anyhow!(
         "Token is not a stablecoin, token: {} chain: {}",
         token.address,
         token.chain_id
      ));
   }

   let price_feed = if token.is_usdc() {
      usdc_usd_price_feed(token.chain_id)?
   } else if token.is_usdt() {
      usdt_usd_price_feed(token.chain_id)?
   } else if token.is_dai() {
      dai_usd_price_feed(token.chain_id)?
   } else if token.is_usdg() {
      usdg_usd_price_feed(token.chain_id)?
   } else {
      bail!(
         "Token is not a stablecoin, token: {} chain: {}",
         token.address,
         token.chain_id
      );
   };

   let block_id = block.unwrap_or(BlockId::latest());
   let oracle = ChainLinkOracle::new(price_feed, client);
   let price = oracle.latestAnswer().block(block_id).call().await?;
   let price = price.to_string().parse::<U256>()?;
   let formatted = format_units(price, 8)?.parse::<f64>()?;
   Ok(formatted)
}

#[cfg(test)]
mod tests {
   use super::*;
   use alloy_provider::ProviderBuilder;

   #[tokio::test]
   #[ignore = "needs an RPC that serves eth_call"]
   async fn test_get_eth_price() {
      let client = ProviderBuilder::new().connect_http(crate::test_utils::rpc_url());
      let price = get_eth_price(client, 1, None).await.unwrap();
      eprintln!("ETH Price: {}", price);
   }

   #[tokio::test]
   #[ignore = "needs an RPC that serves eth_call"]
   async fn test_get_usdc_price() {
      let client = ProviderBuilder::new().connect_http(crate::test_utils::rpc_url());
      let token = ERC20Token::usdc();
      let price = get_stablecoin_price(client, token, None).await.unwrap();
      eprintln!("USDC Price: {}", price);
   }

   #[tokio::test]
   #[ignore = "needs an RPC that serves eth_call"]
   async fn test_get_usdt_price() {
      let client = ProviderBuilder::new().connect_http(crate::test_utils::rpc_url());
      let token = ERC20Token::usdt();
      let price = get_stablecoin_price(client, token, None).await.unwrap();
      eprintln!("USDT Price: {}", price);
   }

   #[tokio::test]
   #[ignore = "needs an RPC that serves eth_call"]
   async fn test_get_dai_price() {
      let client = ProviderBuilder::new().connect_http(crate::test_utils::rpc_url());
      let token = ERC20Token::dai();
      let price = get_stablecoin_price(client, token, None).await.unwrap();
      eprintln!("DAI Price: {}", price);
   }
}
