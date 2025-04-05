use super::{
   RT, eth,
   tx::{TxParams, legacy_or_eip1559},
   update,
};
use crate::core::ZeusCtx;
use crate::core::utils::tx::TxDetails;
use crate::gui::SHARED_GUI;
use anyhow::bail;
use std::time::Duration;
use zeus_eth::{
   alloy_network::TransactionBuilder,
   alloy_primitives::{Address, U256},
   alloy_provider::Provider,
   alloy_rpc_types::{BlockId, BlockNumberOrTag, Filter, TransactionReceipt},
   amm::DexKind,
   currency::{Currency, ERC20Token, NativeCurrency},
   dapps::across::{self, decode_funds_deposited},
   revm_utils::{ForkFactory, new_evm, simulate},
   types::ChainId,
   utils::NumericValue,
   wallet::SecureWallet,
};

/// Bridges the given currency using the Across protocol
pub async fn across_bridge(
   ctx: ZeusCtx,
   currency: Currency,
   deadline: u32,
   expected_output_amount: NumericValue,
   dest_chain: u64,
   recipient: Address,
   mut params: TxParams,
) -> Result<TransactionReceipt, anyhow::Error> {
   let client = ctx.get_client_with_id(params.chain.id())?;

   SHARED_GUI.write(|gui| {
      gui.across_bridge.progress_window.open = true;
   });

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

   // Simulate the deposit
   let fork_block = client.get_block(BlockId::latest()).await?;
   let factory = ForkFactory::new_sandbox_factory(client.clone(), None, None);
   let fork_db = factory.new_sandbox_fork();
   let mut evm = new_evm(params.chain.id(), fork_block, fork_db);

   let time = std::time::Instant::now();
   let res = simulate::across_deposit_v3(
      &mut evm,
      params.call_data.clone(),
      params.value,
      params.signer.borrow().address(),
      params.transcact_to,
      false,
   )?;
   tracing::info!(
      "Simulated DepositV3 in {:?} seconds",
      time.elapsed().as_secs_f32()
   );

   let logs = res.into_logs();
   tracing::debug!("Logs: {:#?}", logs);
   let mut minimum_received = NumericValue::default();
   for log in logs {
      if let Ok(decoded) = decode_funds_deposited(&log) {
         tracing::debug!("Log Decoded: {:#?}", decoded);
         // make sure the output amount is between an 1% tolerance
         minimum_received = NumericValue::format_wei(decoded.output_amount, currency.decimals());
         if minimum_received.f64() < expected_output_amount.f64() * 0.99 {
            let err = format!(
               "Expected output amount of {} but received {}",
               expected_output_amount.formatted(),
               minimum_received.formatted()
            );
            bail!(err);
         } else {
            tracing::info!("Output amount is ok");
         }
      }
   }

   // this actually should not happen
   if minimum_received.is_zero() {
      bail!("Minimum received is zero");
   }

   SHARED_GUI.write(|gui| {
      gui.across_bridge.progress_window.done_simulating();
      gui.across_bridge.progress_window.sending();
   });

   let signer = params.signer.clone();
   let nonce = client
      .get_transaction_count(signer.borrow().address())
      .await?;

   let mut tx = legacy_or_eip1559(params.clone());
   tx.set_nonce(nonce);
   let gas_limit = params.gas_used * 15 / 10; // +50%
   tx.set_gas_limit(gas_limit);

   let wallet = SecureWallet::from(params.signer.clone());
   let tx_envelope = tx.clone().build(wallet.borrow()).await?;
   let timeout = Duration::from_secs(60);

   // Across protocol is very fast on filling the orders
   // So we get the latest block from the destination chain now so we dont miss it and the progress window stucks
   let dest_chain_id = ChainId::new(dest_chain)?;
   let dest_client = ctx.get_client_with_id(dest_chain)?;
   let from_block = dest_client.get_block_number().await?;
   tracing::info!(
      "Will Query the destination {} chain from block {}",
      dest_chain_id.name(),
      from_block
   );

   tracing::info!("Sending Transaction...");
   let time = std::time::Instant::now();
   let receipt = client
      .send_tx_envelope(tx_envelope)
      .await?
      .with_timeout(Some(timeout))
      .get_receipt()
      .await?;
   tracing::info!(
      "Time take to send tx: {:?}secs",
      time.elapsed().as_secs_f32()
   );

   let native_token = ERC20Token::wrapped_native_token(params.chain.id());
   let tx_type = receipt.inner.tx_type();
   let value = NumericValue::format_wei(params.value, native_token.decimals);
   let eth_price = ctx.get_token_price(&native_token).unwrap_or_default();
   let base_fee = NumericValue::format_to_gwei(U256::from(params.base_fee));
   let priority_fee = NumericValue::format_to_gwei(params.miner_tip);

   let tx_details = TxDetails::new(
      receipt.status(),
      params.signer.borrow().address(),
      params.transcact_to,
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
      tx_type,
   );

   SHARED_GUI.write(|gui| {
      gui.across_bridge.progress_window.done_sending();
      gui.across_bridge.progress_window.order_filling();
   });

   ctx.write(|ctx| {
      ctx.tx_db.add_tx(
         params.chain.id(),
         params.signer.borrow().address(),
         tx_details,
      );
   });
   ctx.save_tx_db();

   let chain = params.chain.id();
   let owner = params.signer.borrow().address();
   let ctx_clone = ctx.clone();
   RT.spawn(async move {
      get_eth_balance(ctx_clone.clone(), chain, owner)
         .await
         .unwrap();
      ctx_clone.update_portfolio_value(chain, owner);
   });

   // TODO: Add revert reason
   if !receipt.status() {
      bail!("Deposit Failed");
   }

   let mut block_time_ms = dest_chain_id.block_time();
   if dest_chain_id.is_arbitrum() {
      // give more time so we dont spam the rpc
      block_time_ms *= 3;
   }

   let now = std::time::Instant::now();
   let mut funds_received = false;

   // Wait for the order to be filled at the destination chain
   while now.elapsed().as_secs() < deadline as u64 {
      let filter = Filter::new()
         .from_block(BlockNumberOrTag::Number(from_block))
         .address(across::spoke_pool_address(dest_chain)?)
         .event(across::filled_relay_signature());
      let logs = dest_client.get_logs(&filter).await?;
      tracing::info!("Found {} Filled Relay Logs", logs.len());
      for log in logs {
         if let Ok(decoded) = across::decode_filled_relay(log.data()) {
            tracing::debug!("Filled Relay Log Decoded: {:#?}", decoded);
            if decoded.recipient == recipient {
               tracing::info!("Funds received");
               funds_received = true;
               break;
            }
         }
      }

      if funds_received {
         break;
      }

      tokio::time::sleep(Duration::from_millis(block_time_ms)).await;
   }

   // I dont expect this to happen
   if !funds_received {
      let err = format!(
         "Deadline exceeded\n
      No funds received on the {} chain\n
      Your deposit should be refunded shortly",
         dest_chain_id.name(),
      );
      bail!(err);
   }

   SHARED_GUI.write(|gui| {
      gui.across_bridge.progress_window.done_order_filling();
      gui.across_bridge.progress_window.funds_received = true;
      gui.across_bridge.progress_window.currency_received = currency;
      gui.across_bridge.progress_window.amount_received = minimum_received;
      gui.across_bridge.progress_window.dest_chain = dest_chain_id;
   });

   // if recipient is a wallet owned by this account, update its balance
   let exists = ctx.account().wallet_address_exists(recipient);
   if exists {
      RT.spawn(async move {
         let _ = get_eth_balance(ctx.clone(), dest_chain, recipient).await;
         ctx.update_portfolio_value(dest_chain, recipient);
      });
   }

   Ok(receipt)
}

pub async fn send_crypto(
   ctx: ZeusCtx,
   currency: Currency,
   recipient: Address,
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
      .with_timeout(Some(Duration::from_secs(60)))
      .get_receipt()
      .await?;
   tracing::info!(
      "Time take to send tx: {:?}secs",
      time.elapsed().as_secs_f32()
   );

   let native_token = ERC20Token::wrapped_native_token(params.chain.id());

   let tx_type = receipt.inner.tx_type();
   let value = NumericValue::format_wei(params.value, 18);
   let eth_price = ctx.get_token_price(&native_token).unwrap_or_default();
   let base_fee = NumericValue::format_to_gwei(U256::from(params.base_fee));
   let priority_fee = NumericValue::format_to_gwei(params.miner_tip);

   let tx_details = TxDetails::new(
      receipt.status(),
      params.signer.borrow().address(),
      params.transcact_to,
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
      tx_type,
   );

   ctx.write(|ctx| {
      ctx.tx_db.add_tx(
         params.chain.id(),
         params.signer.borrow().address(),
         tx_details,
      );
   });
   ctx.save_tx_db();

   // update owner balance
   let ctx_clone = ctx.clone();
   RT.spawn(async move {
      let _ = get_eth_balance(ctx_clone.clone(), params.chain.id(), signer_address).await;
      ctx_clone.update_portfolio_value(params.chain.id(), signer_address);
   });

   // if recipient is a wallet owned by this account, update its balance
   let exists = ctx.account().wallet_address_exists(recipient);
   if exists {
      RT.spawn(async move {
         let _ = get_eth_balance(ctx.clone(), params.chain.id(), recipient).await;
         ctx.update_portfolio_value(params.chain.id(), recipient);
      });
   }

   Ok(receipt)
}

/// Get the balance of the given owner in the given chain
///
/// And update the balance db
pub async fn get_eth_balance(ctx: ZeusCtx, chain: u64, owner: Address) -> Result<NumericValue, anyhow::Error> {
   let client = ctx.get_client_with_id(chain)?;
   let balance = client.get_balance(owner).await?;
   let value = NumericValue::currency_balance(balance, 18);

   ctx.write(|ctx| {
      ctx.balance_db.insert_eth_balance(
         chain,
         owner,
         value.wei().unwrap(),
         &NativeCurrency::from_chain_id(chain).unwrap(),
      );
   });
   ctx.update_portfolio_value(chain, owner);
   let _ = ctx.save_balance_db();
   let _ = ctx.save_portfolio_db();

   Ok(value)
}

/// Get the balance of the given owner in the given token
///
/// And update the balance db
pub async fn get_token_balance(ctx: ZeusCtx, owner: Address, token: ERC20Token) -> Result<NumericValue, anyhow::Error> {
   let client = ctx.get_client_with_id(token.chain_id)?;
   let balance = token.balance_of(client, owner, None).await?;
   let value = NumericValue::currency_balance(balance, token.decimals);

   ctx.write(|ctx| {
      ctx.balance_db
         .insert_token_balance(token.chain_id, owner, balance, &token);
   });

   ctx.update_portfolio_value(token.chain_id, owner);
   let _ = ctx.save_balance_db();
   let _ = ctx.save_portfolio_db();

   Ok(value)
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
      ctx_clone.save_all();
   });

   Ok(token)
}

/// Sync all the possible v2 pools for the given token based on:
///
/// - The token's chain id
/// - All the possible [DexKind] for the chain
/// - Base Tokens [base_tokens]
pub async fn sync_v2_pools_for_token(ctx: ZeusCtx, token: ERC20Token) -> Result<(), anyhow::Error> {
   let chain = token.chain_id;
   let client = ctx.get_client_with_id(chain)?;
   let pool_manager = ctx.pool_manager();
   let dex_kind = DexKind::main_dexes(chain);

   pool_manager
      .sync_v2_pools_for_token(client, token.clone(), dex_kind)
      .await?;

   Ok(())
}

/// Sync all the possible v3 pools for the given token based on:
///
/// - The token's chain id
/// - All the possible [DexKind] for the chain
/// - Base Tokens [base_tokens]
pub async fn sync_v3_pools_for_token(ctx: ZeusCtx, token: ERC20Token) -> Result<(), anyhow::Error> {
   let chain = token.chain_id;
   let client = ctx.get_client_with_id(chain)?;
   let pool_manager = ctx.pool_manager();
   let dex_kind = DexKind::main_dexes(chain);

   pool_manager
      .sync_v3_pools_for_token(client, token.clone(), dex_kind)
      .await?;

   Ok(())
}

pub async fn sync_pools_for_token(ctx: ZeusCtx, token: ERC20Token, v2: bool, v3: bool) -> Result<(), anyhow::Error> {
   let chain = token.chain_id;
   let client = ctx.get_client_with_id(chain)?;
   let pool_manager = ctx.pool_manager();
   let dex_kind = DexKind::main_dexes(chain);

   if v2 {
      pool_manager
         .sync_v2_pools_for_token(client.clone(), token.clone(), dex_kind.clone())
         .await?;
   }

   if v3 {
      pool_manager
         .sync_v3_pools_for_token(client, token.clone(), dex_kind)
         .await?;
   }

   Ok(())
}
