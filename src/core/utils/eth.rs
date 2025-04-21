use super::{RT, eth, tx::TxParams, update};
use crate::core::ZeusCtx;
use crate::core::utils::parse_typed_data;
use crate::core::utils::sign::SignMsgType;
use crate::core::utils::{
   action::OnChainAction,
   estimate_tx_cost,
   tx::{self, TxSummary},
};
use crate::gui::{SHARED_GUI, ui::Step};
use alloy_signer::{Signature, Signer};
use anyhow::bail;
use serde_json::Value;
use std::future::IntoFuture;
use std::time::Duration;
use zeus_eth::{
   abi::protocols::across::*,
   alloy_primitives::{Address, Bytes, TxKind, U256},
   alloy_provider::Provider,
   alloy_rpc_types::{BlockId, BlockNumberOrTag, Filter, Log, TransactionReceipt},
   amm::DexKind,
   currency::{Currency, ERC20Token, NativeCurrency},
   dapps::across::spoke_pool_address,
   revm_utils::{ExecuteCommitEvm, ForkFactory, Host, new_evm, revert_msg},
   types::ChainId,
   utils::NumericValue,
};

pub async fn sign_message(
   ctx: ZeusCtx,
   dapp: String,
   chain: ChainId,
   msg: Value,
) -> Result<Signature, anyhow::Error> {
   SHARED_GUI.write(|gui| {
      gui.loading_window.open("Loading...");
      gui.request_repaint();
   });

   let typed_data = parse_typed_data(msg)?;
   let msg_type = SignMsgType::new(ctx.clone(), chain.id(), typed_data.clone()).await;

   SHARED_GUI.write(|gui| {
      gui.loading_window.reset();
      gui.sign_msg_window.open(dapp, chain.id(), msg_type);
      gui.request_repaint();
   });

   // Wait for the user to sign or cancel
   let mut signed = None;
   loop {
      tokio::time::sleep(std::time::Duration::from_millis(100)).await;

      SHARED_GUI.read(|gui| {
         signed = gui.sign_msg_window.is_signed();
      });

      if signed.is_some() {
         SHARED_GUI.write(|gui| {
            gui.sign_msg_window.reset();
         });
         break;
      }
   }

   let signed = signed.unwrap();

   if !signed {
      return Err(anyhow::anyhow!("You cancelled the transaction"));
   }

   let wallet = ctx.current_wallet();
   let signer = ctx.get_wallet(wallet.address).key;
   let signature = signer.borrow().sign_dynamic_typed_data(&typed_data).await?;

   Ok(signature)
}

// Wallet balances are updated and the tx summary is added to the ZeusCtx
pub async fn send_transaction(
   ctx: ZeusCtx,
   dapp: String,
   action: Option<OnChainAction>,
   chain: ChainId,
   from: Address,
   interact_to: Address,
   call_data: Bytes,
   value: U256,
) -> Result<(TransactionReceipt, TxSummary), anyhow::Error> {
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
   let mut evm = new_evm(chain.id(), block.clone(), fork_db);

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
      gui.request_repaint();
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

   SHARED_GUI.write(|gui| {
      gui.loading_window.open("Sending Transaction...");
      gui.request_repaint();
   });

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

   let tx_params = TxParams::new(
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

   let receipt = tx::send_tx(client, tx_params).await?;

   let logs: Vec<Log> = receipt.logs().to_vec();
   let log_data = logs
      .iter()
      .map(|l| l.clone().into_inner())
      .collect::<Vec<_>>();

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

   let timestamp = if let Some(block) = block {
      block.header.timestamp
   } else {
      std::time::SystemTime::now()
         .duration_since(std::time::UNIX_EPOCH)
         .unwrap()
         .as_secs()
   };

   let tx_summary = TxSummary {
      success: receipt.status(),
      chain: chain.id(),
      block: receipt.block_number.unwrap_or_default(),
      timestamp,
      from,
      to: interact_to,
      eth_spent,
      eth_spent_usd: eth_spent_value,
      tx_cost: tx_cost_wei,
      tx_cost_usd,
      gas_used,
      hash: receipt.transaction_hash,
      action,
      contract_interact,
   };

   let ctx_clone = ctx.clone();
   let summary = tx_summary.clone();
   RT.spawn_blocking(move || {
      ctx_clone.write(|ctx| ctx.tx_db.add_tx(chain.id(), from, summary));
      ctx_clone.save_tx_db();
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

   SHARED_GUI.write(|gui| {
      gui.loading_window.reset();
   });

   Ok((receipt, tx_summary))
}

pub async fn across_bridge(
   ctx: ZeusCtx,
   chain: ChainId,
   dest_chain: ChainId,
   deadline: u32,
   action: OnChainAction,
   from: Address,
   recipient: Address,
   interact_to: Address,
   call_data: Bytes,
   value: U256,
) -> Result<(), anyhow::Error> {
   SHARED_GUI.write(|gui| {
      gui.tx_confirm_window.open();
      gui.tx_confirm_window.simulating();
   });

   // Across protocol is very fast on filling the orders
   // So we get the latest block from the destination chain now so we dont miss it and the progress window stucks
   let dest_client = ctx.get_client_with_id(dest_chain.id())?;
   let from_block = dest_client.get_block_number().await?;

   let (_, tx_summary) = send_transaction(
      ctx,
      "".to_string(),
      Some(action),
      chain,
      from,
      interact_to,
      call_data,
      value,
   )
   .await?;

   let step1 = Step {
      id: "step1",
      in_progress: false,
      finished: true,
      msg: "Transaction Sent".to_string(),
   };

   let step2 = Step {
      id: "step2",
      in_progress: true,
      finished: false,
      msg: "Waiting for the order to be filled".to_string(),
   };

   SHARED_GUI.write(|gui| {
      gui.progress_window
         .open_with(vec![step1, step2], "Success!".to_string());
      gui.request_repaint();
   });

   let mut block_time_ms = dest_chain.block_time();
   if dest_chain.is_arbitrum() {
      // give more time so we dont spam the rpc
      block_time_ms *= 3;
   }

   let now = std::time::Instant::now();
   let mut funds_received = false;

   let target = spoke_pool_address(dest_chain.id())?;
   let filter = Filter::new()
      .from_block(BlockNumberOrTag::Number(from_block))
      .address(vec![target])
      .event(filled_relay_signature());

   // Wait for the order to be filled at the destination chain
   while now.elapsed().as_secs() < deadline as u64 {
      let logs = dest_client.get_logs(&filter).await?;
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
         dest_chain.name(),
      );
      bail!(err);
   }

   SHARED_GUI.write(|gui| {
      gui.progress_window.finish_last_step();
      gui.progress_window.set_tx_summary(tx_summary);
      gui.request_repaint();
   });

   Ok(())
}

pub async fn send_crypto(
   ctx: ZeusCtx,
   chain: ChainId,
   from: Address,
   recipient: Address,
   call_data: Bytes,
   value: U256,
   action: OnChainAction,
) -> Result<(), anyhow::Error> {
   SHARED_GUI.write(|gui| {
      gui.tx_confirm_window.open();
      gui.tx_confirm_window.simulating();
   });

   let (_, summary) = send_transaction(
      ctx,
      "".to_string(),
      Some(action),
      chain,
      from,
      recipient,
      call_data,
      value,
   )
   .await?;

   let step1 = Step {
      id: "step1",
      in_progress: false,
      finished: true,
      msg: "Transaction Sent".to_string(),
   };

   SHARED_GUI.write(|gui| {
      gui.progress_window
         .open_with(vec![step1], "Success!".to_string());
      gui.progress_window.set_tx_summary(summary);
      gui.request_repaint();
   });

   Ok(())
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
