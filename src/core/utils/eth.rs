use super::{
   RT, eth,
   tx::{TxParams, TxParams2, legacy_or_eip1559},
   update,
};
use crate::core::ZeusCtx;
use crate::core::utils::{
   action::OnChainAction,
   estimate_tx_cost,
   tx::{self, TxDetails, TxSummary},
};
use crate::gui::SHARED_GUI;
use anyhow::bail;
use std::future::IntoFuture;
use std::time::Duration;
use zeus_eth::{
   abi::protocols::across::*,
   alloy_network::TransactionBuilder,
   alloy_primitives::{Address, Bytes, TxKind, U256},
   alloy_provider::Provider,
   alloy_rpc_types::{BlockId, BlockNumberOrTag, Filter, Log, TransactionReceipt},
   amm::DexKind,
   currency::{Currency, ERC20Token, NativeCurrency},
   dapps::across::spoke_pool_address,
   revm_utils::{ExecuteCommitEvm, ForkFactory, Host, new_evm, revert_msg, simulate},
   types::ChainId,
   utils::NumericValue,
   wallet::SecureWallet,
};

pub async fn send_transaction(
   ctx: ZeusCtx,
   dapp: String,
   action: Option<OnChainAction>,
   chain: ChainId,
   from: Address,
   interact_to: Address,
   call_data: Bytes,
   value: U256,
) -> Result<(), anyhow::Error> {
   SHARED_GUI.write(|gui| {
      gui.tx_confirm_window.open();
      gui.tx_confirm_window.simulating();
   });

   let client = ctx.get_client_with_id(chain.id()).unwrap();
   let base_fee_fut = update::get_base_fee(ctx.clone(), chain.id());
   let nonce_fut = client.get_transaction_count(from).into_future();
   let bytecode_fut = client.get_code_at(interact_to).into_future();
   let block = client.get_block(BlockId::latest()).await?;

   let factory = ForkFactory::new_sandbox_factory(client.clone(), None, None);
   let fork_db = factory.new_sandbox_fork();
   let mut evm = new_evm(chain.id(), block, fork_db);

   evm.tx.caller = from;
   evm.tx.kind = TxKind::Call(interact_to);
   evm.tx.data = call_data.clone();
   evm.tx.value = value;

   let sim_res = evm.transact_commit(evm.tx.clone()).unwrap();
   let output = sim_res.output().unwrap_or_default();
   let gas_used = sim_res.gas_used();

   if !sim_res.is_success() {
      let err = revert_msg(&output);
      tracing::error!("Simulation failed: {}", err);
      bail!("Simulation failed: {}", err);
   }

   let logs = sim_res.into_logs();
   let native_currency = NativeCurrency::from_chain_id(chain.id()).unwrap();
   let balance_before = ctx.get_eth_balance(chain.id(), from).unwrap_or_default();
   let state = evm.balance(from);
   let balance_after = if let Some(state) = state {
      NumericValue::format_wei(state.data, native_currency.decimals).wei2()
   } else {
      NumericValue::default().wei().unwrap_or_default()
   };

   let eth_spent = balance_before.wei2().checked_sub(balance_after);
   if eth_spent.is_none() {
      tracing::error!(
         "Error calculating eth spent, overflow occured, balance_before: {}, balance_after: {}",
         balance_before.wei2(),
         balance_after
      );
      bail!("Error calculating eth spent, overflow occured");
   }

   let base_fee = base_fee_fut.await?;
   let bytecode = bytecode_fut.await?;
   let eth_spent = NumericValue::format_wei(eth_spent.unwrap(), native_currency.decimals);
   let eth_price = ctx.get_currency_price(&Currency::from(native_currency.clone()));
   let eth_spent_value = NumericValue::value(eth_spent.f64(), eth_price.f64());
   let contract_interact = bytecode.len() > 0;
   let priority_fee = ctx.get_priority_fee(chain.id()).unwrap_or_default();
   let (tx_cost_wei, tx_cost_usd) = estimate_tx_cost(
      ctx.clone(),
      chain.id(),
      gas_used,
      priority_fee.wei2(),
   );

   let action = if let Some(action) = action {
      action
   } else {
      OnChainAction::new(
         ctx.clone(),
         chain.id(),
         from,
         interact_to,
         call_data.clone(),
         value,
         logs,
      )
      .await
   };

   SHARED_GUI.write(|gui| {
      gui.tx_confirm_window.done_simulating();
      gui.tx_confirm_window.open_with(
         dapp,
         chain,
         true, // confrim window
         eth_spent.clone(),
         eth_spent_value.clone(),
         tx_cost_wei.clone(),
         tx_cost_usd.clone(),
         gas_used,
         from,
         interact_to,
         contract_interact,
         action,
         priority_fee.formatted().clone(),
      );
   });

   // wait for the user to confirm or reject the transaction
   let mut confirmed = None;
   loop {
      tokio::time::sleep(std::time::Duration::from_millis(100)).await;

      SHARED_GUI.read(|gui| {
         confirmed = gui.tx_confirm_window.get_confirm();
      });

      if confirmed.is_some() {
         SHARED_GUI.write(|gui| {
            gui.tx_confirm_window.reset();
         });
         break;
      }
   }

   let confirmed = confirmed.unwrap();
   if !confirmed {
      bail!("Transaction rejected");
   }

   let fee = SHARED_GUI.read(|gui| gui.tx_confirm_window.get_priority_fee());
   let priority_fee = if fee.is_zero() {
      ctx.get_priority_fee(chain.id()).unwrap_or_default()
   } else {
      fee
   };

   let nonce = nonce_fut.await?;
   let signer = ctx.get_wallet(from).key;

   // give a 10% buffer to the gas limit
   let gas_limit = gas_used * 11 / 10;

   let tx_params = TxParams2::new(
      signer,
      interact_to,
      nonce,
      value,
      chain,
      priority_fee.wei2(),
      base_fee.next,
      call_data.clone(),
      gas_used,
      gas_limit,
   );

   let client = if chain.is_ethereum() {
      ctx.get_flashbots_fast_client().unwrap()
   } else {
      ctx.get_client_with_id(chain.id()).unwrap()
   };

   SHARED_GUI.write(|gui| {
      gui.loading_window.open("Sending Transaction...");
   });

   let receipt = tx::send_tx2(client, tx_params).await?;

   SHARED_GUI.write(|gui| {
      gui.loading_window.reset();
      gui.msg_window.open("Transaction Sent", "");
   });
   
   let logs: Vec<Log> = receipt.logs().to_vec();
   let log_data = logs.iter().map(|l| l.clone().into_inner()).collect::<Vec<_>>();

   let action = OnChainAction::new(
      ctx.clone(),
      chain.id(),
      from,
      interact_to,
      call_data.clone(),
      value,
      log_data,
   )
   .await;

   let tx_summary = TxSummary {
      success: receipt.status(),
      chain: chain.id(),
      block: receipt.block_number.unwrap_or_default(),
      from,
      to: interact_to,
      eth_spent,
      eth_spent_usd: eth_spent_value,
      tx_cost: tx_cost_wei,
      tx_cost_usd,
      gas_used,
      hash: receipt.transaction_hash,
      action,
      contract_interact
   };

   let ctx_clone = ctx.clone();
   RT.spawn_blocking(move || {
      ctx_clone.write(|ctx| ctx.tx_db.add_tx(chain.id(), from, tx_summary));
   });

   // update wallet balances
   let ctx_clone = ctx.clone();
   RT.spawn(async move {
      let _ = get_eth_balance(ctx_clone.clone(), chain.id(), from).await;
   });

   let exists = ctx.wallet_exists(interact_to);
   if exists {
      RT.spawn(async move {
         let _ = get_eth_balance(ctx.clone(), chain.id(), interact_to).await;
      });
   }

   if !receipt.status() {
      bail!("Transaction Failed");
   }

   Ok(())
}

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
      if let Ok(decoded) = decode_funds_deposited_log(&log) {
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

   let logs = receipt.logs();
   let log_data = logs.iter().map(|l| l.clone().into_inner()).collect::<Vec<_>>();
   let native = Currency::from(NativeCurrency::from(params.chain.id()));
   let tx_type = receipt.inner.tx_type();
   let value = NumericValue::format_wei(params.value, native.decimals());
   let value_usd = ctx.get_currency_value2(value.f64(), &native);
   let base_fee = NumericValue::format_to_gwei(U256::from(params.base_fee));
   let priority_fee = NumericValue::format_to_gwei(params.miner_tip);
   let tx_cost = NumericValue::format_wei(params.gas_cost(), native.decimals());
   let tx_cost_usd = ctx.get_currency_value2(tx_cost.f64(), &native);

   let action = OnChainAction::new(
      ctx.clone(),
      params.chain.id(),
      params.signer.borrow().address(),
      params.transcact_to,
      params.call_data.clone(),
      params.value,
      log_data,
   )
   .await;

   let tx_summary = TxSummary {
      success: receipt.status(),
      chain: params.chain.id(),
      block: receipt.block_number.unwrap_or_default(),
      from: params.signer.borrow().address(),
      to: params.transcact_to,
      eth_spent: value,
      eth_spent_usd: value_usd,
      tx_cost,
      tx_cost_usd,
      gas_used: params.gas_used,
      hash: receipt.transaction_hash,
      action,
      contract_interact: true
   };

   SHARED_GUI.write(|gui| {
      gui.across_bridge.progress_window.done_sending();
      gui.across_bridge.progress_window.order_filling();
   });

   ctx.write(|ctx| {
      ctx.tx_db.add_tx(
         params.chain.id(),
         params.signer.borrow().address(),
         tx_summary,
      );
   });

   let ctx_clone = ctx.clone();
   RT.spawn_blocking(move || {
      ctx_clone.save_tx_db();
   });

   let chain = params.chain.id();
   let owner = params.signer.borrow().address();
   let ctx_clone = ctx.clone();
   RT.spawn(async move {
      get_eth_balance(ctx_clone.clone(), chain, owner)
         .await
         .unwrap();
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
         .address(spoke_pool_address(dest_chain)?)
         .event(filled_relay_signature());
      let logs = dest_client.get_logs(&filter).await?;
      tracing::info!("Found {} Filled Relay Logs", logs.len());
      for log in logs {
         if let Ok(decoded) = decode_filled_relay_log(log.data()) {
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

   let currency_price = ctx.get_currency_price(&currency);
   let amount_usd = NumericValue::value(minimum_received.f64(), currency_price.f64());

   SHARED_GUI.write(|gui| {
      gui.across_bridge.progress_window.done_order_filling();
      gui.across_bridge.progress_window.set_funds_received(true);
      gui.across_bridge.progress_window.set_currency(currency);
      gui.across_bridge
         .progress_window
         .set_amount_sent(minimum_received);
      gui.across_bridge.progress_window.set_amount_usd(amount_usd);
      gui.across_bridge
         .progress_window
         .set_dest_chain(dest_chain_id);
   });

   // if recipient is a wallet owned by this account, update its balance
   let exists = ctx.wallet_exists(recipient);
   if exists {
      RT.spawn(async move {
         let _ = get_eth_balance(ctx.clone(), dest_chain, recipient).await;
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

   SHARED_GUI.write(|gui| {
      gui.send_crypto.progress_window.open();
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

   let logs = receipt.logs();
   let log_data = logs.iter().map(|l| l.clone().into_inner()).collect::<Vec<_>>();

   let currency_price = ctx.get_currency_price(&currency);
   let amount_sent = if params.tx_method.is_transfer() {
      NumericValue::format_wei(params.value, currency.decimals())
   } else {
      params.tx_method.erc20_transfer_info().unwrap().1.clone()
   };
   let amount_usd = NumericValue::value(amount_sent.f64(), currency_price.f64());

   SHARED_GUI.write(|gui| {
      gui.send_crypto.progress_window.done_sending();
      gui.send_crypto.progress_window.set_currency(currency);
      gui.send_crypto.progress_window.set_amount(amount_sent);
      gui.send_crypto.progress_window.set_amount_usd(amount_usd);
   });

   let native = Currency::from(NativeCurrency::from(params.chain.id()));
   let tx_type = receipt.inner.tx_type();
   let value = NumericValue::format_wei(params.value, 18);
   let value_usd = ctx.get_currency_value2(value.f64(), &native);
   let tx_cost = NumericValue::format_wei(params.gas_cost(), native.decimals());
   let tx_cost_usd = ctx.get_currency_value2(tx_cost.f64(), &native);
   let base_fee = NumericValue::format_to_gwei(U256::from(params.base_fee));
   let priority_fee = NumericValue::format_to_gwei(params.miner_tip);

   let action = OnChainAction::new(
      ctx.clone(),
      params.chain.id(),
      params.signer.borrow().address(),
      params.transcact_to,
      params.call_data.clone(),
      params.value,
      log_data,
   )
   .await;

   let tx_summary = TxSummary {
      success: receipt.status(),
      chain: params.chain.id(),
      block: receipt.block_number.unwrap_or_default(),
      from: params.signer.borrow().address(),
      to: params.transcact_to,
      eth_spent: value,
      eth_spent_usd: value_usd,
      tx_cost,
      tx_cost_usd,
      gas_used: params.gas_used,
      hash: receipt.transaction_hash,
      action,
      contract_interact: false
   };

   ctx.write(|ctx| {
      ctx.tx_db.add_tx(
         params.chain.id(),
         params.signer.borrow().address(),
         tx_summary,
      );
   });

   let ctx_clone = ctx.clone();
   RT.spawn_blocking(move || {
      ctx_clone.save_tx_db();
   });

   // update owner balance
   let ctx_clone = ctx.clone();
   RT.spawn(async move {
      let _ = get_eth_balance(
         ctx_clone.clone(),
         params.chain.id(),
         signer_address,
      )
      .await;
   });

   // if recipient is a wallet owned by this account, update its balance
   let exists = ctx.wallet_exists(recipient);
   if exists {
      RT.spawn(async move {
         let _ = get_eth_balance(ctx.clone(), params.chain.id(), recipient).await;
      });
   }

   Ok(receipt)
}

/// Get the balance of the given owner in the given chain
///
/// And update the balance db
pub async fn get_eth_balance(
   ctx: ZeusCtx,
   chain: u64,
   owner: Address,
) -> Result<NumericValue, anyhow::Error> {
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

   RT.spawn_blocking(move || {
      ctx.update_portfolio_value(chain, owner);
      let _ = ctx.save_balance_db();
      let _ = ctx.save_portfolio_db();
   });

   Ok(value)
}

/// Get the balance of the given owner in the given token
///
/// And update the balance db
pub async fn get_token_balance(
   ctx: ZeusCtx,
   owner: Address,
   token: ERC20Token,
) -> Result<NumericValue, anyhow::Error> {
   let client = ctx.get_client_with_id(token.chain_id)?;
   let balance = token.balance_of(client, owner, None).await?;
   let value = NumericValue::currency_balance(balance, token.decimals);

   ctx.write(|ctx| {
      ctx.balance_db
         .insert_token_balance(token.chain_id, owner, balance, &token);
   });

   RT.spawn_blocking(move || {
      ctx.update_portfolio_value(token.chain_id, owner);
      let _ = ctx.save_balance_db();
      let _ = ctx.save_portfolio_db();
   });

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

   let currency = Currency::from(token.clone());

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
         Err(e) => tracing::error!(
            "Error syncing pools for {}: {:?}",
            token_clone.symbol,
            e
         ),
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

      RT.spawn_blocking(move || {
         ctx_clone.update_portfolio_value(chain, owner);
         ctx_clone.write(|ctx| ctx.data_syncing = false);
         ctx_clone.save_all();
      });
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

pub async fn sync_pools_for_token(
   ctx: ZeusCtx,
   token: ERC20Token,
   v2: bool,
   v3: bool,
) -> Result<(), anyhow::Error> {
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
