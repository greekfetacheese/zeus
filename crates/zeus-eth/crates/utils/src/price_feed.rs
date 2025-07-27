use super::address_book::*;
use alloy_primitives::{Address, U256, utils::format_units};
use alloy_rpc_types::BlockId;
use alloy_sol_types::sol;
use anyhow::bail;
use types::ChainId;

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
pub async fn get_eth_price<P, N>(client: P, chain_id: u64, block_id: Option<BlockId>) -> Result<f64, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let chain = ChainId::new(chain_id)?;

   let feed = match chain {
      ChainId::Ethereum(_) => super::address_book::eth_usd_price_feed(chain_id)?,
      ChainId::Optimism(_) => super::address_book::eth_usd_price_feed(chain_id)?,
      ChainId::Base(_) => super::address_book::eth_usd_price_feed(chain_id)?,
      ChainId::Arbitrum(_) => super::address_book::eth_usd_price_feed(chain_id)?,
      ChainId::BinanceSmartChain(_) => bail!("ETH-USD price feed not available on BSC"),
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
   chain_id: u64,
   token: Address,
   block: Option<BlockId>,
) -> Result<f64, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let chain = ChainId::new(chain_id)?;

   if chain == ChainId::BinanceSmartChain(chain_id) {
      if token == wbnb(chain_id)? {
         get_bnb_price(client, block).await
      } else {
         get_stablecoin_price(client, chain_id, token, block).await
      }
   } else if token == weth(chain_id)? {
      get_eth_price(client, chain_id, block).await
   } else {
      get_stablecoin_price(client, chain_id, token, block).await
   }
}

pub async fn get_stablecoin_price<P, N>(
   client: P,
   chain_id: u64,
   token: Address,
   block: Option<BlockId>,
) -> Result<f64, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   let is_usdc = usdc(chain_id).is_ok_and(|usdc| usdc == token);
   let is_usdt = usdt(chain_id).is_ok_and(|usdt| usdt == token);
   let is_dai = dai(chain_id).is_ok_and(|dai| dai == token);
   let is_stable = is_usdc || is_usdt || is_dai;

   if !is_stable {
      return Err(anyhow::anyhow!("Token is not a stablecoin"));
   }

   let price_feed = if is_usdc {
      usdc_usd_price_feed(chain_id)?
   } else if is_usdt {
      usdt_usd_price_feed(chain_id)?
   } else if is_dai {
      dai_usd_price_feed(chain_id)?
   } else {
      bail!("Token is not a stablecoin");
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
   use url::Url;

   #[tokio::test]
   async fn test_get_eth_price() {
      let url = Url::parse("https://eth.merkle.io").unwrap();
      let client = ProviderBuilder::new().connect_http(url);
      let price = get_eth_price(client, 1, None).await.unwrap();
      eprintln!("ETH Price: {}", price);
   }

   #[tokio::test]
   async fn test_get_usdc_price() {
      let url = Url::parse("https://eth.merkle.io").unwrap();
      let client = ProviderBuilder::new().connect_http(url);
      let price = get_stablecoin_price(client, 1, usdc(1).unwrap(), None)
         .await
         .unwrap();
      eprintln!("USDC Price: {}", price);
   }

   #[tokio::test]
   async fn test_get_usdt_price() {
      let url = Url::parse("https://eth.merkle.io").unwrap();
      let client = ProviderBuilder::new().connect_http(url);
      let price = get_stablecoin_price(client, 1, usdt(1).unwrap(), None)
         .await
         .unwrap();
      eprintln!("USDT Price: {}", price);
   }

   #[tokio::test]
   async fn test_get_dai_price() {
      let url = Url::parse("https://eth.merkle.io").unwrap();
      let client = ProviderBuilder::new().connect_http(url);
      let price = get_stablecoin_price(client, 1, dai(1).unwrap(), None)
         .await
         .unwrap();
      eprintln!("DAI Price: {}", price);
   }
}
