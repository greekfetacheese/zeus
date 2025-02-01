use alloy_primitives::address;
use alloy_provider::{ProviderBuilder, Provider, WsConnect};

use std::sync::Arc;

use zeus_eth::prelude::{UniswapV3Pool, DexKind, BlockTime, ERC20Token};
use zeus_eth::defi::amm::uniswap::v3::lp_provider::{simulate_position, PositionArgs};

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let url = "wss://eth.merkle.io";
    let client = ProviderBuilder::new().on_ws(WsConnect::new(url)).await?;
    let client = Arc::new(client);
    let chain_id = client.get_chain_id().await?;

    let wst_eth = address!("7f39C581F595B53c5cb19bD0b3f8dA6c935E2Ca0");
    let weth = address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2");
    let pool_address = address!("109830a1aaad605bbf02a9dfa7b0b92ec2fb7daa");

    let token0 = ERC20Token::new(client.clone(), wst_eth, chain_id).await?;
    let token1 = ERC20Token::new(client.clone(), weth, chain_id).await?;

    let pool = UniswapV3Pool::new(chain_id, pool_address, 100, token0, token1, DexKind::UniswapV3);

    let position = PositionArgs {
        lower_range: 1.1062672693587939,
        upper_range: 1.1969094065772878,
        price_assumption: 1.167293589301331,
        deposit_amount: 500_000.0,
        pool,
    };

    // go back exactly 1 day from the current block
    let block_time = BlockTime::Days(1);

    let result = simulate_position(client, block_time, position).await?;
    println!("{}", result.pretty());

    Ok(())
}