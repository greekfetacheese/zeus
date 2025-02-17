use crate::core::ZeusCtx;
use zeus_eth::alloy_primitives::{ Address, U256 };
use zeus_eth::{currency::{Currency, ERC20Token}, amm::{UniswapV2Pool, UniswapV3Pool}};



/// Get the ERC20 Token from the blockchain and update the db
pub async fn get_erc20_token(ctx: ZeusCtx, token_address: Address, chain_id: u64) -> Result<ERC20Token, anyhow::Error> {
    let client = ctx.get_client()?;
    let owner = ctx.wallet().key.address();

    let token = ERC20Token::new(client.clone(), token_address, chain_id).await?;

    let balance = if owner != Address::ZERO {
        token.balance_of(client.clone(), owner, None).await?
    } else {
        U256::ZERO
    };

    // Update the db
    ctx.write(|ctx| {
        let currency = Currency::from_erc20(token.clone());

        ctx.db.insert_currency(chain_id, currency);
        ctx.db.insert_token_balance(chain_id, owner, token.address, balance);
        ctx.db.save_to_file().unwrap();
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
    let pool_manager = ctx.pool_manager();

    pool_manager.get_v2_pools_for_token(client, token.clone()).await?;
    let pools = ctx.get_v2_pools(token);

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
    let pool_manager = ctx.pool_manager();

    pool_manager.get_v3_pools_for_token(client, token.clone()).await?;
    let pools = ctx.get_v3_pools(token);

    Ok(pools)
}