use crate::core::ZeusCtx;

use super::{
   RT, eth,
   tx::{TxParams, legacy_or_eip1559},
   update,
};
use crate::core::utils::tx::TxDetails;
use zeus_eth::{
   alloy_network::TransactionBuilder,
   alloy_primitives::{Address, U256},
   alloy_provider::Provider,
   alloy_rpc_types::TransactionReceipt,
   amm::{DexKind, UniswapV2Pool, UniswapV3Pool},
   currency::{Currency, ERC20Token},
   utils::NumericValue,
   wallet::SecureWallet,
};

pub async fn send_crypto(
   ctx: ZeusCtx,
   currency: Currency,
   mut params: TxParams,
) -> Result<TransactionReceipt, anyhow::Error> {
   let client = ctx.get_client_with_id(params.chain.id())?;

   // override the base fee incase has been increased since the last update
   let base_fee = update::get_base_fee(ctx.clone(), params.chain.id()).await?;
   params.base_fee = base_fee.next;

   // check for sufficient balance again
   let balance = ctx.get_currency_balance(
      params.chain.id(),
      params.signer.borrow().address(),
      &currency,
   );
   params.sufficient_balance(balance)?;

   let signer_address = params.signer.borrow().address();
   let nonce = client.get_transaction_count(signer_address).await?;

   let mut tx = legacy_or_eip1559(params.clone());
   tx.set_nonce(nonce);
   let gas_limit = params.gas_used * 15 / 10; // +50%
   tx.set_gas_limit(gas_limit);

   let wallet = SecureWallet::from(params.signer.clone());
   let tx_envelope = tx.clone().build(wallet.borrow()).await?;

   tracing::info!("Sending Transaction...");
   let time = std::time::Instant::now();
   let receipt = client
      .send_tx_envelope(tx_envelope)
      .await?
      .get_receipt()
      .await?;
   tracing::info!(
      "Time take to send tx: {:?}secs",
      time.elapsed().as_secs_f32()
   );

   let native_token = ERC20Token::native_wrapped_token(params.chain.id());

   let tx_type = receipt.inner.tx_type();
   let value = NumericValue::format_wei(params.value, 18);
   let eth_price = ctx.get_token_price(&native_token).unwrap_or_default();
   let base_fee = NumericValue::format_to_gwei(U256::from(params.base_fee));
   let priority_fee = NumericValue::format_to_gwei(params.miner_tip);
   
   let tx_details = TxDetails::new(
      receipt.status(),
      params.signer.borrow().address(),
      params.recipient,
      value,
      eth_price,
      params.call_data,
      receipt.transaction_hash,
      receipt.block_number.unwrap_or_default(),
      receipt.transaction_index.unwrap_or_default(),
      params.tx_method,
      nonce,
      receipt.gas_used,
      gas_limit,
      base_fee,
      priority_fee,
      tx_type
   );

   ctx.write(|ctx| {
      ctx.tx_db.add_tx(params.chain.id(), params.signer.borrow().address(), tx_details);
      let _ = ctx.tx_db.save();
   });

   Ok(receipt)
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

   // If there is a balance add the token to the portfolio
   if !balance.is_zero() {
      ctx.write(|ctx| {
         ctx.portfolio_db
            .add_currency(chain, owner, currency.clone());
      });
   }

   // Sync the pools for the token
   let ctx_clone = ctx.clone();
   let token_clone = token.clone();
   RT.spawn(async move {
      ctx_clone.write(|ctx| {
         ctx.data_syncing = true;
      });

      match eth::sync_pools_for_token(ctx_clone.clone(), token_clone.clone(), true, true).await {
         Ok(_) => {
            tracing::info!("Synced Pools for {}", token_clone.symbol);
         }
         Err(e) => tracing::error!("Error syncing pools for {}: {:?}", token_clone.symbol, e),
      }

      let pool_manager = ctx_clone.pool_manager();
      match pool_manager.update(client, chain).await {
         Ok(_) => {
            tracing::info!("Updated pool state for {}", token_clone.symbol);
         }
         Err(e) => {
            tracing::error!(
               "Error updating pool state for {}: {:?}",
               token_clone.symbol,
               e
            );
         }
      }
      ctx_clone.update_portfolio_value(chain, owner);
      ctx_clone.write(|ctx| ctx.data_syncing = false);
      match ctx_clone.save_all() {
         Ok(_) => tracing::info!("Saved all"),
         Err(e) => tracing::error!("Error saving all: {:?}", e),
      }
   });

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
   let pools = ctx.get_v2_pools(&token);

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
   let pools = ctx.get_v3_pools(&token);

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
