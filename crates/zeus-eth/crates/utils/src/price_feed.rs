use super::address::*;
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
      ChainId::Ethereum(_) => super::address::eth_usd_price_feed(chain_id)?,
      ChainId::Optimism(_) => super::address::eth_usd_price_feed(chain_id)?,
      ChainId::Base(_) => super::address::eth_usd_price_feed(chain_id)?,
      ChainId::Arbitrum(_) => super::address::eth_usd_price_feed(chain_id)?,
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

   let feed = super::address::bnb_usd_price_feed();
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
         return get_bnb_price(client, block).await;
      } else {
         return Ok(1.0); // Assuming Stablecoins are stable
      }
   } else {
      if token == weth(chain_id)? {
         return get_eth_price(client, chain_id, block).await;
      } else {
         return Ok(1.0);
      }
   }
}
