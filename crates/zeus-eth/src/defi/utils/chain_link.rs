use alloy_primitives::{address, utils::format_units, Address, U256};
use alloy_rpc_types::BlockId;
use alloy_sol_types::sol;

use alloy_contract::private::Network;
use alloy_provider::Provider;
use alloy_transport::Transport;
use super::common_addr::*;


// Ethereum mainnet
const ETH_USD_FEED: Address = address!("5f4eC3Df9cbd43714FE2740f5E3616155c5b8419");

#[allow(dead_code)]
const CBETH_ETH_FEED: Address = address!("F017fcB346A1885194689bA23Eff2fE6fA5C483b");

#[allow(dead_code)]
const ETH_BTC_FEED: Address = address!("Ac559F25B1619171CbC396a50854A3240b6A4e99");

// Binance Smart Chain
const BNB_USD_FEED: Address = address!("0567F2323251f0Aab15c8dFb1967E4e8A7D42aeE");

// OP Base
const BASE_ETH_USD_FEED: Address = address!("71041dddad3595F9CEd3DcCFBe3D1F4b0a16Bb70");

// Arbitrum
const ARB_ETH_USD_FEED: Address = address!("639Fe6ab55C921f74e7fac1ee960C0B6293ba612");

sol!(
    #[sol(rpc)]
    contract ChainLinkOracle {
        function latestAnswer() external view returns (int256);
    }
);

/// Get the ETH price on supported chains
pub async fn get_eth_price<T, P, N>(
    client: P,
    block_id: Option<BlockId>,
    chain_id: u64,
) -> Result<f64, anyhow::Error>
where
    T: Transport + Clone,
    P: Provider<T, N> + Clone,
    N: Network,
{
    let feed = match chain_id {
        1 => ETH_USD_FEED,
        8453 => BASE_ETH_USD_FEED,
        42161 => ARB_ETH_USD_FEED,
        _ => return Err(anyhow::anyhow!("Unsupported chain id {}", chain_id)),
    };

    let block_id = block_id.unwrap_or(BlockId::latest());

    let oracle = ChainLinkOracle::new(feed, client);
    let eth_usd = oracle.latestAnswer().block(block_id).call().await?._0;

    let eth_usd = eth_usd.to_string().parse::<U256>()?;
    let formatted = format_units(eth_usd, 8)?.parse::<f64>()?;
    Ok(formatted)
}

/// Get the BNB price on the Binance Smart Chain
pub async fn get_bnb_price<T, P, N>(
    client: P,
    block_id: Option<BlockId>,
    chain_id: u64,
) -> Result<f64, anyhow::Error>
where
    T: Transport + Clone,
    P: Provider<T, N> + Clone,
    N: Network,
{
    if chain_id != 56 {
        return Err(anyhow::anyhow!("Wrong ChainId expected 56 but got {}", chain_id));
    }

    let block_id = block_id.unwrap_or(BlockId::latest());

    let oracle = ChainLinkOracle::new(BNB_USD_FEED, client);
    let bnb_usd = oracle.latestAnswer().block(block_id).call().await?._0;

    let bnb_usd = bnb_usd.to_string().parse::<U256>()?;
    let formatted = format_units(bnb_usd, 8)?.parse::<f64>()?;
    Ok(formatted)
}

/// Get the USD value of commonly paired tokens
pub async fn get_token_price<T, P, N>(
    client: P,
    block_id: Option<BlockId>,
    chain_id: u64,
    token: Address,
) -> Result<f64, anyhow::Error>
where
    T: Transport + Clone,
    P: Provider<T, N> + Clone,
    N: Network,
{
    let mut price = 0.0;

    if chain_id == 1 || chain_id == 10 || chain_id == 42161 {
        if token == usdc(chain_id)? {
            price = 1.0;
        } else if token == usdt(chain_id)? {
            price = 1.0;
        } else if token == dai(chain_id)? {
            price = 1.0;
        } else if token == weth(chain_id)? {
            price = get_eth_price(client, block_id, chain_id).await?;
        }
    } else if chain_id == 8453 {
        // USDT not available on Base
        if token == usdc(chain_id)? {
            price = 1.0;
        } else if token == dai(chain_id)? {
            price = 1.0;
        } else if token == weth(chain_id)? {
            price = get_eth_price(client, block_id, chain_id).await?;
        }
    } else if chain_id == 56 {
        if token == usdc(chain_id)? {
            price = 1.0;
        } else if token == usdt(chain_id)? {
            price = 1.0;
        } else if token == dai(chain_id)? {
            price = 1.0;
        } else if token == wbnb(chain_id)? {
            price = get_bnb_price(client, block_id, chain_id).await?;
        }
    }


    Ok(price)
}