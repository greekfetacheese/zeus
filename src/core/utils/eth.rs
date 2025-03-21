use crate::core::ZeusCtx;

use super::{tx::{send_tx, TxParams}, update};
use zeus_eth::{
   alloy_primitives::{Address, U256},
   alloy_provider::Provider,
   alloy_rpc_types::TransactionReceipt,
   amm::{DexKind, UniswapV2Pool, UniswapV3Pool},
   currency::{Currency, ERC20Token},
   utils::NumericValue,
};


pub async fn send_crypto(
   ctx: ZeusCtx,
   currency: Currency,
   mut params: TxParams
) -> Result<TransactionReceipt, anyhow::Error> {
   let client = ctx.get_client_with_id(params.chain.id())?;

   // override the base fee incase has been increased since the last update
   let base_fee = update::get_base_fee(ctx.clone(), params.chain.id()).await?;
   params.base_fee = base_fee.next;

   // check for sufficient balance again
   let balance = ctx.get_currency_balance(params.chain.id(), params.signer.borrow().address(), &currency);
   params.sufficient_balance(balance)?;
  
   let tx = send_tx(client.clone(), params).await?;

   Ok(tx)
}

pub async fn get_eth_balance(ctx: ZeusCtx, chain: u64, owner: Address) -> Result<NumericValue, anyhow::Error> {
   let client = ctx.get_client_with_id(chain)?;
   let balance = client.get_balance(owner).await?;
   Ok(NumericValue::currency_balance(balance, 18))
}

pub async fn get_token_balance(ctx: ZeusCtx, owner: Address, token: ERC20Token) -> Result<NumericValue, anyhow::Error> {
   let client = ctx.get_client_with_id(token.chain_id)?;
   let balance = token.balance_of(client, owner, None).await?;
   Ok(NumericValue::currency_balance(balance, token.decimals))
}

pub async fn get_currency_balance(
   ctx: ZeusCtx,
   owner: Address,
   currency: Currency,
) -> Result<NumericValue, anyhow::Error> {
   if currency.is_native() {
      get_eth_balance(ctx, currency.chain_id(), owner).await
   } else {
      get_token_balance(ctx, owner, currency.erc20().cloned().unwrap()).await
   }
}

/// Get the ERC20 Token from the blockchain and update the db
pub async fn get_erc20_token(
   ctx: ZeusCtx,
   chain: u64,
   owner: Address,
   token_address: Address,
) -> Result<ERC20Token, anyhow::Error> {
   let client = ctx.get_client_with_id(chain)?;
   let token = ERC20Token::new(client.clone(), token_address, chain).await?;

   let balance = if owner != Address::ZERO {
      token.balance_of(client.clone(), owner, None).await?
   } else {
      U256::ZERO
   };

   let currency = Currency::from_erc20(token.clone());

   // Update the db
   ctx.write(|ctx| {
      ctx.currency_db.insert_currency(chain, currency.clone());
      ctx.balance_db
         .insert_token_balance(chain, owner, balance, &token);
   });

   ctx.update_portfolio_value(chain, owner);

   match ctx.save_currency_db() {
      Ok(_) => tracing::info!("CurrencyDB saved"),
      Err(e) => tracing::error!("Error saving DB: {:?}", e),
   }

   match ctx.save_balance_db() {
      Ok(_) => tracing::info!("BalanceDB saved"),
      Err(e) => tracing::error!("Error saving DB: {:?}", e),
   }

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
   let dex_kind = DexKind::main_dexes(chain);

   pool_manager
      .get_v2_pools_for_token(client, token.clone(), dex_kind)
      .await?;
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
   let dex_kind = DexKind::main_dexes(chain);

   pool_manager
      .get_v3_pools_for_token(client, token.clone(), dex_kind)
      .await?;
   let pools = ctx.get_v3_pools(token);

   Ok(pools)
}


pub async fn sync_pools_for_token(ctx: ZeusCtx, token: ERC20Token, v2: bool, v3: bool) -> Result<(), anyhow::Error> {
   let chain = token.chain_id;
   let client = ctx.get_client_with_id(chain)?;
   let pool_manager = ctx.pool_manager();
   let dex_kind = DexKind::main_dexes(chain);

   if v2 {
      pool_manager
         .get_v2_pools_for_token(client.clone(), token.clone(), dex_kind.clone())
         .await?;
   }

   if v3 {
      pool_manager
         .get_v3_pools_for_token(client, token.clone(), dex_kind)
         .await?;
   }

   Ok(())
}
