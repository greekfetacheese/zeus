use alloy_primitives::{
    address,
    utils::{format_units, parse_units},
};
use alloy_provider::{ProviderBuilder, Provider, WsConnect};

use std::sync::Arc;

use zeus_eth::prelude::{UniswapV2Pool, DexKind, usdc, weth, ERC20Token};



#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let url = "wss://eth.merkle.io";
    let ws_connect = WsConnect::new(url);
    let client = ProviderBuilder::new().on_ws(ws_connect).await?;
    let client = Arc::new(client);
    let chain_id = client.get_chain_id().await?;

    let weth = ERC20Token::new(client.clone(), weth(chain_id)?, chain_id).await?;
    let usdc = ERC20Token::new(client.clone(), usdc(chain_id)?, chain_id).await?;
    let pool_address = address!("b4e16d0168e52d35cacd2c6185b44281ec28c9dc");

    let mut pool = UniswapV2Pool::new(chain_id, pool_address, weth.clone(), usdc.clone(), DexKind::Uniswap);

    // populate the pool state based on the latest block
    let state = UniswapV2Pool::fetch_state(client.clone(), pool.address, None).await?;
    pool.update_state(state);


    let amount_in = parse_units("10", weth.decimals)?.get_absolute();

    let usdc_out = pool.simulate_swap(weth.address, amount_in)?;
    let usdc_out_formatted = format_units(usdc_out, usdc.decimals)?;
    let amount_in_formatted = format_units(amount_in, weth.decimals)?;

    println!(
        "Swapped {} {} For {} {}",
        amount_in_formatted,
        weth.symbol,
        usdc_out_formatted,
        usdc.symbol
    );

    let (token0_usd, token1_usd) = pool.tokens_usd(client, None).await?;
    println!("{} Price: ${}", pool.token0.symbol, token0_usd);
    println!("{} Price: ${}", pool.token1.symbol, token1_usd);

    Ok(())
}