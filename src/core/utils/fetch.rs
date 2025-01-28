use crate::core::ZeusCtx;
use zeus_eth::alloy_primitives::{ Address, U256 };
use zeus_eth::defi::currency::Currency;
use zeus_eth::defi::utils::common_addr;
use zeus_eth::prelude::{ get_v3_pools, DexKind, ERC20Token, UniswapV2Pool, UniswapV3Pool };
use zeus_eth::{ ETH, BSC, BASE, ARBITRUM, OPTIMISM };

/// Get the ERC20 Token from the blockchain and update the db
pub async fn get_erc20_token(ctx: ZeusCtx, token_address: Address, chain_id: u64) -> Result<ERC20Token, anyhow::Error> {
    let client = ctx.get_client()?;
    let owner = ctx.wallet().key.address();

    let token = ERC20Token::new(client.clone(), token_address, chain_id).await?;

    let balance = if owner != Address::ZERO {
        token.balance_of(owner, client.clone(), None).await?
    } else {
        U256::ZERO
    };

    // Update the db
    ctx.write(|ctx| {
        let currency = Currency::from_erc20(token.clone());

        ctx.db.insert_currency(chain_id, currency);
        ctx.db.insert_token_balance(chain_id, owner, token.address, balance);

        let time = std::time::Instant::now();
        ctx.db.save_to_file().unwrap();
        tracing::info!("Time to save zeus db {:?}", time.elapsed().as_millis());
    });

    Ok(token)
}

/// Get the USD price of a token
pub async fn get_token_price(ctx: ZeusCtx, token: ERC20Token) -> Result<f64, anyhow::Error> {

    let mut price = 0.0;
    let client = ctx.get_client_with_id(token.chain_id)?;
    let v2_pools = get_v2_pools_for_token(ctx.clone(), token.clone()).await?;

    if !v2_pools.is_empty() {
        let mut pool = v2_pools.first().cloned().unwrap();
        let state = UniswapV2Pool::fetch_state(client.clone(), pool.address, None).await?;
        pool.update_state(state);

        let (token0_usd, token1_usd) = pool.tokens_usd(client.clone(), None).await?;

        price = if pool.is_token0(token.address) {
            token0_usd
        } else {
            token1_usd
        };
    }

    if price == 0.0 {
        let v3_pools = get_v3_pools_for_token(ctx.clone(), token.clone()).await?;

        if !v3_pools.is_empty() {
            let mut pool = v3_pools.first().cloned().unwrap();
            let state = UniswapV3Pool::fetch_state(client.clone(), pool.address, None).await?;
            pool.update_state(state);

            let (token0_usd, token1_usd) = pool.tokens_usd(client, None).await?;

            price = if pool.is_token0(token.address) {
                token0_usd
            } else {
                token1_usd
            };
        }
    }


    Ok(price)
}

pub async fn get_v2_pools_for_token(ctx: ZeusCtx, token: ERC20Token) -> Result<Vec<UniswapV2Pool>, anyhow::Error> {
    let chain = token.chain_id;
    let client = ctx.get_client_with_id(chain)?;
    let dex_kind = if chain == BSC { DexKind::PancakeSwap } else { DexKind::Uniswap };

    let mut pools = Vec::new();
    if chain == BSC {
        // main liquidity token is WBNB
        let wbnb = ERC20Token::wbnb();
        let token_wbnb = UniswapV2Pool::from(client.clone(), chain, token.clone(), wbnb, dex_kind).await;
        if let Ok(pool) = token_wbnb {
            pools.push(pool);
        }
    } else if chain == ETH {
        // main liquidity token is WETH
        let token_weth = UniswapV2Pool::from(client.clone(), chain, token.clone(), weth_erc20(chain), dex_kind).await;
        if let Ok(pool) = token_weth {
            pools.push(pool);
        }
    }

    let token_usdc = UniswapV2Pool::from(client.clone(), chain, token.clone(), usdc_erc20(chain), dex_kind).await;

    if let Ok(pool) = token_usdc {
        pools.push(pool);
    }

    // USDT is not available on base chain
    if chain != BASE {
        let token_usdt = UniswapV2Pool::from(client.clone(), chain, token.clone(), usdt_erc20(chain), dex_kind).await;
        if let Ok(pool) = token_usdt {
            pools.push(pool);
        }
    }

    if pools.is_empty() {
        anyhow::bail!("No pool found for token: {}", token.address);
    }

    Ok(pools)
}

pub async fn get_v3_pools_for_token(ctx: ZeusCtx, token: ERC20Token) -> Result<Vec<UniswapV3Pool>, anyhow::Error> {
    let chain = token.chain_id;
    let client = ctx.get_client_with_id(chain)?;
    let dex_kind =  if chain == BSC { DexKind::PancakeSwap } else { DexKind::Uniswap };

    let factory = if dex_kind.is_uniswap() {
        common_addr::uniswap_v3_factory(chain)?
    } else {
        common_addr::pancakeswap_v3_factory(chain)?
    };

    let mut pools = Vec::new();

    if chain == BSC {
        // main liquidity token is WBNB
        let wbnb = ERC20Token::wbnb();
        let token_wbnb = get_v3_pools(client.clone(), token.address, wbnb.address, factory).await?;

        for pool in token_wbnb {
            if !pool.pool.is_zero() {
                let fee: u32 = pool.fee.to_string().parse()?;
                let v3 = UniswapV3Pool::new(chain, pool.pool, fee, token.clone(), wbnb.clone(), dex_kind);
                pools.push(v3);
            }
        }
    } else if chain == ETH {
        // main liquidity token is WETH
        let weth = weth_erc20(chain);
        let token_weth = get_v3_pools(client.clone(), token.address, weth.address, factory).await?;

        for pool in token_weth {
            if !pool.pool.is_zero() {
                let fee: u32 = pool.fee.to_string().parse()?;
                let v3 = UniswapV3Pool::new(chain, pool.pool, fee, token.clone(), weth.clone(), dex_kind);
                pools.push(v3);
            }
        }
    }

    let usdc = usdc_erc20(chain);
    let token_usdc = get_v3_pools(client.clone(), token.address, usdc.address, factory).await?;

    for pool in token_usdc {
        if !pool.pool.is_zero() {
            let fee: u32 = pool.fee.to_string().parse()?;
            let v3 = UniswapV3Pool::new(chain, pool.pool, fee, token.clone(), usdc_erc20(chain), dex_kind);
            pools.push(v3);
        }
    }

    if chain != BASE {
        let usdt = usdt_erc20(chain);
        let token_usdt = get_v3_pools(client.clone(), token.address, usdt.address, factory).await?;

        for pool in token_usdt {
            if !pool.pool.is_zero() {
                let fee: u32 = pool.fee.to_string().parse()?;
                let v3 = UniswapV3Pool::new(chain, pool.pool, fee, token.clone(), usdt_erc20(chain), dex_kind);
                pools.push(v3);
            }
        }
    }

    if pools.is_empty() {
        anyhow::bail!("No pool found for token: {}", token.address);
    }

    Ok(pools)
}

fn weth_erc20(id: u64) -> ERC20Token {
    match id {
        ETH => ERC20Token::weth(),
        BSC => ERC20Token::weth_bsc(),
        BASE => ERC20Token::weth_base(),
        ARBITRUM => ERC20Token::weth_arbitrum(),
        OPTIMISM => ERC20Token::weth_op(),
        _ => panic!("Unsupported chain id: {}", id),
    }
}

fn usdc_erc20(id: u64) -> ERC20Token {
    match id {
        ETH => ERC20Token::usdc(),
        BSC => ERC20Token::usdc_bsc(),
        BASE => ERC20Token::usdc_base(),
        ARBITRUM => ERC20Token::usdc_arbitrum(),
        OPTIMISM => ERC20Token::usdc_op(),
        _ => panic!("Unsupported chain id: {}", id),
    }
}

fn usdt_erc20(id: u64) -> ERC20Token {
    match id {
        ETH => ERC20Token::usdt(),
        BSC => ERC20Token::usdt_bsc(),
        ARBITRUM => ERC20Token::usdt_arbitrum(),
        OPTIMISM => ERC20Token::usdt_op(),
        _ => panic!("Unsupported chain id: {}", id),
    }
}