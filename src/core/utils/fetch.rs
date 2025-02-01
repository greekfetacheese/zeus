use crate::core::ZeusCtx;
use zeus_eth::alloy_primitives::{ Address, U256 };
use zeus_eth::defi::currency::Currency;
use zeus_eth::prelude::{ base_tokens, DexKind, ERC20Token, UniswapV2Pool, UniswapV3Pool };
use zeus_eth::utils::batch_request;

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

/// Get all the possible v2 pools for the given token based on:
///
/// - The token's chain id
/// - All the possible [DexKind] for the chain
/// - Base Tokens [base_tokens]
pub async fn get_v2_pools_for_token(ctx: ZeusCtx, token: ERC20Token) -> Result<Vec<UniswapV2Pool>, anyhow::Error> {
    let chain = token.chain_id;
    let client = ctx.get_client_with_id(chain)?;
    let dex_kinds = DexKind::all(chain);
    let base_tokens = base_tokens(chain);

    let mut pools = Vec::new();
    for base_token in base_tokens {
        if base_token.address == token.address {
            continue;
        }

        for dex in &dex_kinds {
            if dex.is_pancakeswap_v3() || dex.is_uniswap_v3() {
                continue;
            }
            tracing::debug!("Getting v2 pool for: {}-{} on: {}", token.symbol, base_token.symbol, dex.to_str());
            let pool = UniswapV2Pool::from(client.clone(), chain, token.clone(), base_token.clone(), *dex).await;
            if let Ok(pool) = pool {
                pools.push(pool);
            }
        }
    }

    Ok(pools)
}

/// Get all the possible v3 pools for the given token based on:
///
/// - The token's chain id
/// - All the possible [DexKind] for the chain
/// - Base Tokens [base_tokens]
pub async fn get_v3_pools_for_token(ctx: ZeusCtx, token: ERC20Token) -> Result<Vec<UniswapV3Pool>, anyhow::Error> {
    let chain = token.chain_id;
    let client = ctx.get_client_with_id(chain)?;
    let dex_kinds = DexKind::all(chain);
    let base_tokens = base_tokens(chain);

    let mut requests = Vec::new();
    for base_token in &base_tokens {
        if base_token.address == token.address {
            continue;
        }

        for dex in &dex_kinds {
            if dex.is_pancakeswap_v2() || dex.is_uniswap_v2() {
                continue;
            }
            let factory = dex.factory(chain)?;
            tracing::debug!("Getting v3 pools for: {}-{} on: {} with factory {}", token.symbol, base_token.symbol, dex.to_str(), factory);
            let request = batch_request::get_v3_pools(client.clone(), token.address, base_token.address, factory);
            requests.push(request);
        }
    }

    let results: Vec<batch_request::V3Pool> = futures::future
        ::try_join_all(requests).await
        .map_err(|e| anyhow::anyhow!(e))?
        .into_iter()
        .flatten()
        .collect();
    let mut pools = Vec::new();

    for base_token in base_tokens {
        if base_token.address == token.address {
            continue;
        }
        
        for dex in &dex_kinds {
            if dex.is_pancakeswap_v2() || dex.is_uniswap_v2() {
                continue;
            }
            for pool in &results {
                if !pool.addr.is_zero() {
                    let fee: u32 = pool.fee.to_string().parse()?;
                    pools.push(UniswapV3Pool::new(chain, pool.addr, fee, token.clone(), base_token.clone(), *dex));
                }
            }
        }
    }

    Ok(pools)
}