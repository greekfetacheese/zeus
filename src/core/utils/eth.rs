use super::{RT, tx::TxParams, update};
use crate::core::{
   TransactionAnalysis, TransactionRich, ZeusCtx,
   db::V3Position,
   transaction::*,
   utils::{self, estimate_tx_cost, parse_typed_data, sign::SignMsgType, tx},
};
use crate::gui::{SHARED_GUI, ui::Step};
use alloy_signer::{Signature, Signer};
use anyhow::bail;
use anyhow::{Context, anyhow};
use serde_json::Value;
use std::future::IntoFuture;
use std::time::{Duration, Instant};
use zeus_eth::abi::uniswap::nft_position::encode_mint;
use zeus_eth::amm::uniswap::v3::{calculate_liquidity_amounts, calculate_liquidity_from_amount};
use zeus_eth::amm::{UniswapV3Pool, uniswap_v3_math};
use zeus_eth::{
   abi::{self, protocols::across::*, uniswap::nft_position::INonfungiblePositionManager},
   alloy_primitives::{Address, Bytes, Signed, TxKind, U256},
   alloy_provider::Provider,
   alloy_rpc_types::{BlockId, BlockNumberOrTag, Filter, Log, TransactionReceipt},
   amm::{
      DexKind, UniswapPool,
      uniswap::{
         router::{encode_swap, *},
         v3::get_tick_from_price,
      },
   },
   currency::{Currency, ERC20Token},
   dapps::{Dapp, across::spoke_pool_address},
   revm_utils::{
      Database, DatabaseCommit, Evm2, ExecuteCommitEvm, ExecutionResult, ForkDB, ForkFactory, Host,
      new_evm, revert_msg, simulate,
   },
   types::ChainId,
   utils::{
      NumericValue,
      address::{permit2_contract, uniswap_nft_position_manager, universal_router_v2},
   },
};

/// Eth balances for sender and interact_to are updated and the tx summary is added to the ZeusCtx
pub async fn send_transaction(
   ctx: ZeusCtx,
   dapp: String,
   fork_db: Option<ForkDB>,
   tx_analysis: Option<TransactionAnalysis>,
   chain: ChainId,
   mev_protect: bool,
   from: Address,
   interact_to: Address,
   call_data: Bytes,
   value: U256,
) -> Result<(TransactionReceipt, TransactionRich), anyhow::Error> {
   let client = ctx.get_client(chain.id()).await?;
   let base_fee_fut = update::get_base_fee(ctx.clone(), chain.id());
   let nonce_fut = client.get_transaction_count(from).into_future();

   // If no tx analysis is provided, simulate the transaction
   let tx_analysis = if let Some(analysis) = tx_analysis {
      analysis
   } else {
      SHARED_GUI.write(|gui| {
         gui.loading_window.open("Wait while magic happens");
         gui.request_repaint();
      });

      let fork_db = if let Some(fork_db) = fork_db {
         fork_db
      } else {
         let factory = ForkFactory::new_sandbox_factory(client.clone(), chain.id(), None, None);
         factory.new_sandbox_fork()
      };

      let bytecode_fut = client.get_code_at(interact_to).into_future();
      let block = client.get_block(BlockId::latest()).await?;

      let balance_before = ctx.get_eth_balance(chain.id(), from);

      let balance_after;
      let sim_res;

      {
         let mut evm = new_evm(chain, block.as_ref(), fork_db);

         let time = std::time::Instant::now();
         sim_res = simulate_transaction(
            &mut evm,
            from,
            interact_to,
            call_data.clone(),
            value,
         )?;

         tracing::info!(
            "Simulate Transaction took {} ms",
            time.elapsed().as_millis()
         );

         let state = evm.balance(from);
         balance_after = if let Some(state) = state {
            state.data
         } else {
            U256::ZERO
         };
      }

      let logs = sim_res.clone().into_logs();

      let bytecode = bytecode_fut.await?;
      let contract_interact = Some(bytecode.len() > 0);

      let tx_analysis = TransactionAnalysis::new(
         ctx.clone(),
         chain.id(),
         from,
         interact_to,
         contract_interact,
         call_data.clone(),
         value,
         logs,
         sim_res.gas_used(),
         balance_before.wei2(),
         balance_after,
      )
      .await?;

      tx_analysis
   };

   let priority_fee = ctx.get_priority_fee(chain.id()).unwrap_or_default();

   SHARED_GUI.write(|gui| {
      gui.tx_confirmation_window.open(
         ctx.clone(),
         dapp,
         chain,
         tx_analysis.clone(),
         priority_fee.formatted().clone(),
      );
      gui.loading_window.reset();
      gui.request_repaint();
   });

   // wait for the user to confirm or reject the transaction
   let mut confirmed = None;
   loop {
      tokio::time::sleep(std::time::Duration::from_millis(100)).await;

      SHARED_GUI.read(|gui| {
         confirmed = gui.tx_confirmation_window.get_confirmed_or_rejected();
      });

      if confirmed.is_some() {
         SHARED_GUI.write(|gui| {
            gui.tx_confirmation_window.close();
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

   let fee = SHARED_GUI.read(|gui| gui.tx_confirmation_window.get_priority_fee());
   let gas_limit = SHARED_GUI.read(|gui| gui.tx_confirmation_window.get_gas_limit());

   let priority_fee = if fee.is_zero() {
      ctx.get_priority_fee(chain.id()).unwrap_or_default()
   } else {
      fee
   };

   let base_fee = base_fee_fut.await?;
   let nonce = nonce_fut.await?;
   let signer = ctx.get_wallet(from)?.key;
   let gas_used = tx_analysis.gas_used;

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

   // If needed use MEV protect client, if not found prompt the user to continue
   let new_client = if chain.is_ethereum() && mev_protect {
      let mev_client_res = ctx.get_mev_protect_client(chain.id()).await;

      if mev_client_res.is_err() {
         SHARED_GUI.write(|gui| {
            let msg2 = "Continue without MEV protection?";
            gui.confirm_window
               .open("Error while connecting to MEV protect RPC");
            gui.confirm_window.set_msg2(msg2);
            gui.request_repaint();
         });

         // wait for the user to confirm or reject the transaction
         let mut confirmed = None;
         loop {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;

            SHARED_GUI.read(|gui| {
               confirmed = gui.confirm_window.get_confirm();
            });

            if confirmed.is_some() {
               SHARED_GUI.write(|gui| {
                  gui.confirm_window.reset();
               });
               break;
            }
         }

         let confirmed = confirmed.unwrap();
         if !confirmed {
            return Err(anyhow!("Transaction Rejected"));
         }

         // keep the old client
         client.clone()
      } else {
         mev_client_res.unwrap()
      }
   } else {
      client.clone()
   };

   let receipt = tx::send_tx(new_client, tx_params).await?;

   let logs: Vec<Log> = receipt.logs().to_vec();
   let logs = logs
      .iter()
      .map(|l| l.clone().into_inner())
      .collect::<Vec<_>>();

   let timestamp = std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .unwrap()
      .as_secs();

   let balance_before = ctx.get_eth_balance(chain.id(), from);
   let balance_after = client.get_balance(from).await?;
   let contract_interact = Some(tx_analysis.contract_interact);

   let new_tx_analysis = TransactionAnalysis::new(
      ctx.clone(),
      chain.id(),
      from,
      interact_to,
      contract_interact,
      tx_analysis.call_data.clone(),
      tx_analysis.value,
      logs,
      receipt.gas_used,
      balance_before.wei2(),
      balance_after,
   )
   .await?;

   let action = new_tx_analysis.infer_action(ctx.clone(), chain.id());
   let (tx_cost, tx_cost_usd) = estimate_tx_cost(
      ctx.clone(),
      chain.id(),
      receipt.gas_used,
      priority_fee.wei2(),
   );

   let transaction_rich = TransactionRich {
      success: receipt.status(),
      chain: chain.id(),
      block: receipt.block_number.unwrap_or_default(),
      timestamp,
      value_sent: new_tx_analysis.value_sent(),
      value_sent_usd: new_tx_analysis.value_sent_usd(ctx.clone()),
      eth_received: new_tx_analysis.eth_received(),
      eth_received_usd: new_tx_analysis.eth_received_usd(ctx.clone()),
      tx_cost,
      tx_cost_usd,
      hash: receipt.transaction_hash,
      contract_interact: new_tx_analysis.contract_interact,
      analysis: new_tx_analysis,
      action,
   };

   let ctx_clone = ctx.clone();
   let tx = transaction_rich.clone();
   RT.spawn_blocking(move || {
      ctx_clone.write(|ctx| ctx.tx_db.add_tx(chain.id(), from, tx));
      ctx_clone.save_tx_db();
   });

   // update wallet balances
   let ctx_clone = ctx.clone();
   RT.spawn(async move {
      let manager = ctx_clone.balance_manager();
      manager
         .update_eth_balance(ctx_clone.clone(), chain.id(), from)
         .await
         .unwrap();
      ctx_clone.save_balance_manager();
   });

   let exists = ctx.wallet_exists(interact_to);
   if exists {
      RT.spawn(async move {
         let manager = ctx.balance_manager();
         manager
            .update_eth_balance(ctx.clone(), chain.id(), interact_to)
            .await
            .unwrap();
         ctx.save_balance_manager();
      });
   }

   if !receipt.status() {
      bail!("Transaction Failed");
   }

   SHARED_GUI.write(|gui| {
      gui.loading_window.reset();
   });

   Ok((receipt, transaction_rich))
}

pub fn simulate_transaction<DB>(
   evm: &mut Evm2<DB>,
   from: Address,
   interact_to: Address,
   call_data: Bytes,
   value: U256,
) -> Result<ExecutionResult, anyhow::Error>
where
   DB: Database + DatabaseCommit,
{
   evm.tx.caller = from;
   evm.tx.kind = TxKind::Call(interact_to);
   evm.tx.data = call_data.clone();
   evm.tx.value = value;

   let sim_res = evm
      .transact_commit(evm.tx.clone())
      .map_err(|e| anyhow!("Simulation failed: {:?}", e))?;
   let output = sim_res.output().unwrap_or_default();
   let gas_used = sim_res.gas_used();

   if !sim_res.is_success() {
      let err = revert_msg(&output);
      tracing::error!(
         "Simulation failed: {} \n Gas Used {}",
         err,
         gas_used
      );
      return Err(anyhow!("Failed to simulate transaction: {}", err));
   }

   Ok(sim_res)
}

/// Prompt the user to sign a message
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
   let signer = ctx.get_wallet(wallet.address)?.key;
   let signature = signer
      .to_signer()
      .sign_dynamic_typed_data(&typed_data)
      .await?;

   Ok(signature)
}

pub async fn unwrap_weth(
   ctx: ZeusCtx,
   from: Address,
   chain: ChainId,
   amount: NumericValue,
) -> Result<(), anyhow::Error> {
   let client = ctx.get_client(chain.id()).await?;
   let block = client.get_block(BlockId::latest()).await?;
   let wrapped = ERC20Token::wrapped_native_token(chain.id());

   let call_data = wrapped.encode_withdraw(amount.wei2());
   let interact_to = wrapped.address;
   let value = U256::ZERO;

   let factory = ForkFactory::new_sandbox_factory(client.clone(), chain.id(), None, None);
   let fork_db = factory.new_sandbox_fork();

   let eth_balance_after;
   let sim_res;
   {
      let mut evm = new_evm(chain, block.as_ref(), fork_db.clone());

      let time = Instant::now();
      sim_res = simulate_transaction(
         &mut evm,
         from,
         interact_to,
         call_data.clone(),
         value,
      )?;

      tracing::info!(
         "Simulate Transaction took {} ms",
         time.elapsed().as_millis()
      );

      let state = evm.balance(from);
      eth_balance_after = if let Some(state) = state {
         state.data
      } else {
         U256::ZERO
      };
   }

   let logs = sim_res.clone().into_logs();

   let eth_balance_before = ctx.get_eth_balance(chain.id(), from);
   let mut eth_received = None;

   for log in &logs {
      if let Ok(decoded) = abi::weth9::decode_withdraw_log(log) {
         eth_received = Some(decoded.wad);
         break;
      }
   }

   if eth_received.is_none() {
      return Err(anyhow::anyhow!(
         "Failed to decode weth withdraw log"
      ));
   }

   let eth_received = NumericValue::format_wei(eth_received.unwrap(), wrapped.decimals);
   let wrapped_c: Currency = wrapped.into();
   let eth_received_usd = ctx.get_currency_value_for_amount(eth_received.f64(), &wrapped_c);

   let contract_interact = true;
   let params = UnwrapWETHParams {
      from,
      weth_unwrapped: amount,
      weth_unwrapped_usd: Some(eth_received_usd.clone()),
      eth_received,
      eth_received_usd: Some(eth_received_usd),
   };

   let tx_analysis = TransactionAnalysis {
      chain: chain.id(),
      sender: from,
      interact_to,
      contract_interact,
      value,
      call_data: call_data.clone(),
      gas_used: sim_res.gas_used(),
      eth_balance_before: eth_balance_before.wei2(),
      eth_balance_after,
      decoded_selector: "Withdraw".to_string(),
      weth_unwraps: vec![params],
      ..Default::default()
   };

   let mev_protect = false;
   let (_, tx_rich) = send_transaction(
      ctx.clone(),
      "".to_string(),
      None,
      Some(tx_analysis),
      chain,
      mev_protect,
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

   SHARED_GUI.write(|gui| {
      gui.progress_window
         .open_with(vec![step1], "Success!".to_string());
      gui.progress_window.set_tx(tx_rich);
      gui.request_repaint();
   });

   // update balances
   RT.spawn(async move {
      let manager = ctx.balance_manager();
      manager
         .update_tokens_balance(
            ctx.clone(),
            chain.id(),
            from,
            vec![wrapped_c.to_wrapped_native()],
         )
         .await
         .unwrap();
      ctx.save_balance_manager();
   });

   Ok(())
}

pub async fn wrap_eth(
   ctx: ZeusCtx,
   from: Address,
   chain: ChainId,
   amount: NumericValue,
) -> Result<(), anyhow::Error> {
   let client = ctx.get_client(chain.id()).await?;
   let block = client.get_block(BlockId::latest()).await?;
   let wrapped = ERC20Token::wrapped_native_token(chain.id());

   let call_data = wrapped.encode_deposit();
   let interact_to = wrapped.address;
   let value = amount.wei2();

   let factory = ForkFactory::new_sandbox_factory(client.clone(), chain.id(), None, None);
   let fork_db = factory.new_sandbox_fork();

   let eth_balance_after;
   let sim_res;
   {
      let mut evm = new_evm(chain, block.as_ref(), fork_db.clone());

      let time = Instant::now();
      sim_res = simulate_transaction(
         &mut evm,
         from,
         interact_to,
         call_data.clone(),
         value,
      )?;
      tracing::info!(
         "Simulate Transaction took {} ms",
         time.elapsed().as_millis()
      );

      let state = evm.balance(from);
      eth_balance_after = if let Some(state) = state {
         state.data
      } else {
         U256::ZERO
      };
   }

   let logs = sim_res.clone().into_logs();

   let wrapped: Currency = wrapped.into();
   let eth_balance_before = ctx.get_eth_balance(chain.id(), from);
   let eth_wrapped_usd = ctx.get_currency_value_for_amount(amount.f64(), &wrapped);
   let mut weth_received = None;

   for log in &logs {
      if let Ok(decoded) = abi::weth9::decode_deposit_log(log) {
         weth_received = Some(decoded.wad);
         break;
      }
   }

   if weth_received.is_none() {
      return Err(anyhow::anyhow!(
         "Failed to decode weth deposit log"
      ));
   }

   let weth_received = NumericValue::format_wei(weth_received.unwrap(), wrapped.decimals());
   let weth_received_usd = ctx.get_currency_value_for_amount(weth_received.f64(), &wrapped);

   let contract_interact = true;
   let params = WrapETHParams {
      from,
      eth_wrapped: amount,
      eth_wrapped_usd: Some(eth_wrapped_usd),
      weth_received: weth_received,
      weth_received_usd: Some(weth_received_usd),
   };

   let tx_analysis = TransactionAnalysis {
      chain: chain.id(),
      sender: from,
      interact_to,
      contract_interact,
      value,
      call_data: call_data.clone(),
      gas_used: sim_res.gas_used(),
      eth_balance_before: eth_balance_before.wei2(),
      eth_balance_after,
      decoded_selector: "Deposit".to_string(),
      eth_wraps: vec![params],
      ..Default::default()
   };

   let mev_protect = false;
   let (_, tx_rich) = send_transaction(
      ctx.clone(),
      "".to_string(),
      None,
      Some(tx_analysis),
      chain,
      mev_protect,
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

   SHARED_GUI.write(|gui| {
      gui.progress_window
         .open_with(vec![step1], "Success!".to_string());
      gui.progress_window.set_tx(tx_rich);
      gui.request_repaint();
   });

   // update balances
   RT.spawn(async move {
      let manager = ctx.balance_manager();
      manager
         .update_tokens_balance(
            ctx.clone(),
            chain.id(),
            from,
            vec![wrapped.to_wrapped_native()],
         )
         .await
         .unwrap();
      ctx.save_balance_manager();
   });

   Ok(())
}

pub async fn swap(
   ctx: ZeusCtx,
   chain: ChainId,
   slippage: f64,
   mev_protect: bool,
   from: Address,
   swap_type: SwapType,
   amount_in: NumericValue,
   _amount_out: NumericValue,
   currency_in: Currency,
   currency_out: Currency,
   swap_steps: Vec<SwapStep<impl UniswapPool + Clone>>,
) -> Result<(), anyhow::Error> {
   let client = ctx.get_client(chain.id()).await?;
   let interact_to = universal_router_v2(chain.id())?;
   let block_fut = client.get_block(BlockId::latest());
   let signer = ctx.get_wallet(from)?.key;

   // Simulate the swap to find out the real amount of tokens received in case of a tax or any malicious contract
   let time = std::time::Instant::now();
   let execute_params = encode_swap(
      client.clone(),
      chain.id(),
      swap_steps.clone(),
      swap_type,
      amount_in.wei2(),
      U256::ZERO, // No slippage so the simulation goes through
      currency_in.clone(),
      currency_out.clone(),
      signer.clone(),
      from,
      None,
   )
   .await?;

   tracing::info!(
      "Build Execute Params took {} ms",
      time.elapsed().as_millis()
   );

   let block = block_fut.await?;
   let factory = ForkFactory::new_sandbox_factory(client.clone(), chain.id(), None, None);
   let fork_db = factory.new_sandbox_fork();

   let token_out_balance_after;
   let eth_balance_after;
   let sim_res;
   let mut approval_gas_used = 50_000;
   {
      let mut evm = new_evm(chain, block.as_ref(), fork_db.clone());
      if execute_params.token_needs_approval {
         // make sure token in is approved
         let permit2 = permit2_contract(chain.id())?;
         let time = std::time::Instant::now();
         let res = simulate::approve_token(
            &mut evm,
            currency_in.address(),
            from,
            permit2,
            U256::MAX,
         )?;
         tracing::info!(
            "Approve Token sim took {} ms",
            time.elapsed().as_millis()
         );

         approval_gas_used = res.gas_used();
      }

      // simulate the swap
      let time = std::time::Instant::now();
      sim_res = simulate_transaction(
         &mut evm,
         from,
         interact_to,
         execute_params.call_data.clone(),
         execute_params.value,
      )?;
      tracing::info!(
         "Swap Simulation took {} ms",
         time.elapsed().as_millis()
      );
      tracing::info!("Gas Used: {}", sim_res.gas_used());

      let state = evm.balance(from);
      eth_balance_after = if let Some(state) = state {
         state.data
      } else {
         U256::ZERO
      };

      // get the balance after
      let time = std::time::Instant::now();
      token_out_balance_after = if currency_out.is_native() {
         eth_balance_after
      } else {
         let b = simulate::erc20_balance(&mut evm, currency_out.address(), from)?;
         tracing::info!(
            "Balance after sim took {} ms",
            time.elapsed().as_millis()
         );
         b
      };
   }

   let balance_before = if currency_out.is_native() {
      client.get_balance(from).await?
   } else {
      currency_out
         .to_erc20()
         .balance_of(client.clone(), from, None)
         .await?
   };

   // calculate the real amount out
   let real_amount_out = if token_out_balance_after > balance_before {
      let amount_out = token_out_balance_after - balance_before;
      NumericValue::format_wei(amount_out, currency_out.decimals())
   } else {
      return Err(anyhow::anyhow!(
         "No tokens received from the swap"
      ));
   };

   // apply the slippage
   let mut amount_out_with_slippage = real_amount_out.clone();
   amount_out_with_slippage.calc_slippage(slippage, currency_out.decimals());
   tracing::info!(
      "Amount Out with slippage: {} {}",
      amount_out_with_slippage.formatted(),
      currency_out.symbol()
   );

   // build the call data again with the real_amount_out and slippage applied
   // TODO: Avoid this step again or at least avoid making calls to the provider
   let time = std::time::Instant::now();
   let execute_params = encode_swap(
      client.clone(),
      chain.id(),
      swap_steps.clone(),
      swap_type,
      amount_in.wei2(),
      amount_out_with_slippage.wei2(),
      currency_in.clone(),
      currency_out.clone(),
      signer,
      from,
      None,
   )
   .await?;
   tracing::info!(
      "Build Execute Params took {} ms",
      time.elapsed().as_millis()
   );

   let amount_in_usd = ctx.get_currency_value_for_amount(amount_in.f64(), &currency_in);
   let received_usd = ctx.get_currency_value_for_amount(real_amount_out.f64(), &currency_out);
   let min_received_usd =
      ctx.get_currency_value_for_amount(amount_out_with_slippage.f64(), &currency_out);

   let swap_params = SwapParams {
      dapp: Dapp::Uniswap,
      input_currency: currency_in.clone(),
      output_currency: currency_out.clone(),
      amount_in: amount_in.clone(),
      amount_in_usd: Some(amount_in_usd),
      received: real_amount_out,
      received_usd: Some(received_usd),
      min_received: Some(amount_out_with_slippage),
      min_received_usd: Some(min_received_usd),
      sender: from,
      recipient: Some(from),
   };

   let contract_interact = true;
   let eth_balance_before = ctx.get_eth_balance(chain.id(), from);

   let swap_tx_analysis = TransactionAnalysis {
      chain: chain.id(),
      sender: from,
      interact_to,
      contract_interact,
      value: execute_params.value,
      call_data: execute_params.call_data.clone(),
      gas_used: sim_res.gas_used(),
      eth_balance_before: eth_balance_before.wei2(),
      eth_balance_after,
      decoded_selector: "Execute".to_string(),
      swaps: vec![swap_params],
      ..Default::default()
   };

   SHARED_GUI.reset_loading();
   SHARED_GUI.request_repaint();

   if execute_params.token_needs_approval {
      let msg_value = execute_params.message.clone();
      if msg_value.is_none() {
         return Err(anyhow!("Missing message"));
      }

      let msg_value = msg_value.unwrap();
      let _ = sign_message(ctx.clone(), "".to_string(), chain, msg_value).await?;

      let permit2 = permit2_contract(chain.id())?;
      let token = currency_in.to_erc20();

      let call_data = token.encode_approve(permit2, U256::MAX);
      let dapp = "".to_string();
      let interact_to = token.address;
      let value = U256::ZERO;
      let amount = NumericValue::format_wei(U256::MAX, token.decimals);

      let params = TokenApproveParams {
         token: vec![token.into_owned()],
         amount: vec![amount],
         amount_usd: vec![None],
         owner: from,
         spender: permit2,
      };

      let analysis = TransactionAnalysis {
         chain: chain.id(),
         sender: from,
         interact_to,
         contract_interact: true,
         value,
         call_data: call_data.clone(),
         gas_used: approval_gas_used,
         eth_balance_before: eth_balance_before.wei2(),
         eth_balance_after,
         decoded_selector: "Approve".to_string(),
         token_approvals: vec![params],
         ..Default::default()
      };

      let (receipt, _) = send_transaction(
         ctx.clone(),
         dapp,
         None,
         Some(analysis),
         chain,
         mev_protect,
         from,
         interact_to,
         call_data,
         value,
      )
      .await?;

      if !receipt.status() {
         return Err(anyhow!("Token Approval Failed"));
      }
   }

   // now we can proceed with the swap
   let call_data = execute_params.call_data.clone();
   let value = execute_params.value;
   let dapp = "".to_string();

   let (_, tx_rich) = send_transaction(
      ctx.clone(),
      dapp,
      None,
      Some(swap_tx_analysis),
      chain,
      mev_protect,
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

   SHARED_GUI.write(|gui| {
      gui.progress_window
         .open_with(vec![step1], "Success!".to_string());
      gui.progress_window.set_tx(tx_rich);
      gui.request_repaint();
   });

   let tokens = vec![
      currency_in.to_erc20().into_owned(),
      currency_out.to_erc20().into_owned(),
   ];
   RT.spawn(async move {
      let manager = ctx.balance_manager();
      manager
         .update_tokens_balance(ctx.clone(), chain.id(), from, tokens)
         .await
         .unwrap();
      ctx.save_balance_manager();
   });

   Ok(())
}

/// Collect fees on a V3 position
pub async fn collect_fees_position_v3(
   ctx: ZeusCtx,
   chain: ChainId,
   from: Address,
   mut position: V3Position,
   token0: Currency,
   token1: Currency,
) -> Result<(), anyhow::Error> {
   let client = ctx.get_client(chain.id()).await?;
   let nft_contract = uniswap_nft_position_manager(chain.id())?;

   let collect_fee_params =
      abi::uniswap::nft_position::INonfungiblePositionManager::CollectParams {
         tokenId: position.id,
         recipient: from,
         amount0Max: u128::MAX,
         amount1Max: u128::MAX,
      };

   let call_data = abi::uniswap::nft_position::encode_collect(collect_fee_params);
   let interact_to = nft_contract;
   let value = U256::ZERO;
   let mev_protect = false;

   let (receipt, tx_rich) = send_transaction(
      ctx.clone(),
      "".to_string(),
      None,
      None,
      chain,
      mev_protect,
      from,
      interact_to,
      call_data,
      value,
   )
   .await?;

   if !receipt.status() {
      return Err(anyhow!("Transaction failed"));
   }

   let step1 = Step {
      id: "step1",
      in_progress: false,
      finished: true,
      msg: "Transaction Sent".to_string(),
   };

   SHARED_GUI.write(|gui| {
      gui.progress_window
         .open_with(vec![step1], "Success!".to_string());
      gui.progress_window.set_tx(tx_rich);
      gui.request_repaint();
   });

   let tokens = vec![
      token0.to_erc20().into_owned(),
      token1.to_erc20().into_owned(),
   ];

   let ctx2 = ctx.clone();
   RT.spawn(async move {
      let manager = ctx2.balance_manager();
      manager
         .update_tokens_balance(ctx2.clone(), chain.id(), from, tokens)
         .await
         .unwrap();
      ctx2.save_balance_manager();
   });

   let updated_position =
      abi::uniswap::nft_position::positions(client.clone(), nft_contract, position.id).await?;

   let tokens_owed0 = NumericValue::format_wei(updated_position.tokens_owed0, token0.decimals());
   let tokens_owed1 = NumericValue::format_wei(updated_position.tokens_owed1, token1.decimals());

   position.tokens_owed0 = tokens_owed0;
   position.tokens_owed1 = tokens_owed1;
   position.fee_growth_inside0_last_x128 = updated_position.fee_growth_inside0_last_x128;
   position.fee_growth_inside1_last_x128 = updated_position.fee_growth_inside1_last_x128;

   ctx.write(|ctx| {
      ctx.v3_positions_db
         .insert(chain.id(), position.owner, position);
   });

   ctx.save_v3_positions_db();

   Ok(())
}

/// Decrease liquidity on a V3 position
pub async fn decrease_liquidity_position_v3(
   ctx: ZeusCtx,
   chain: ChainId,
   from: Address,
   mut position: V3Position,
   mut pool: UniswapV3Pool,
   liquidity_to_remove: u128,
   slippage: String,
   mev_protect: bool,
) -> Result<(), anyhow::Error> {
   let client = ctx.get_client(chain.id()).await?;
   let nft_contract = uniswap_nft_position_manager(chain.id())?;

   let block_fut = client.get_block(BlockId::latest()).into_future();

   pool.update_state(client.clone(), None).await?;

   let sqrt_price_lower = uniswap_v3_math::tick_math::get_sqrt_ratio_at_tick(position.tick_lower)?;
   let sqrt_price_upper = uniswap_v3_math::tick_math::get_sqrt_ratio_at_tick(position.tick_upper)?;

   let state = pool.state().v3_state().unwrap();

   let (amount0_to_remove, amount1_to_remove) = calculate_liquidity_amounts(
      state.sqrt_price,
      sqrt_price_lower,
      sqrt_price_upper,
      liquidity_to_remove,
   )?;

   let amount0_to_remove = NumericValue::format_wei(amount0_to_remove, pool.token0().decimals);
   let amount1_to_remove = NumericValue::format_wei(amount1_to_remove, pool.token1().decimals);

   let mut amount0_min_to_remove = amount0_to_remove.clone();
   let mut amount1_min_to_remove = amount1_to_remove.clone();

   let slippage: f64 = slippage.parse().unwrap_or(0.5);

   amount0_min_to_remove.calc_slippage(slippage, pool.token0().decimals);
   amount1_min_to_remove.calc_slippage(slippage, pool.token1().decimals);

   let deadline = utils::get_unix_time_in_minutes(1)?;
   let mut decrease_liquidity_params = INonfungiblePositionManager::DecreaseLiquidityParams {
      tokenId: position.id,
      liquidity: liquidity_to_remove,
      amount0Min: amount0_min_to_remove.wei2(),
      amount1Min: amount1_min_to_remove.wei2(),
      deadline: U256::from(deadline),
   };

   let owner = from;

   let factory = ForkFactory::new_sandbox_factory(client.clone(), chain.id(), None, None);
   let fork_db = factory.new_sandbox_fork();

   let block = block_fut.await?;

   let eth_balance_after;
   let sim_res;
   let amount0_removed;
   let amount1_removed;
   {
      let mut evm = new_evm(chain, block.as_ref(), fork_db.clone());

      let now = Instant::now();
      let (res, amount0, amount1) = simulate::decrease_liquidity(
         &mut evm,
         decrease_liquidity_params.clone(),
         owner,
         nft_contract,
         true,
      )?;

      tracing::info!(
         "Simulated Decrease Liquidity in {} ms",
         now.elapsed().as_millis()
      );

      amount0_removed = NumericValue::format_wei(amount0, pool.token0().decimals);
      amount1_removed = NumericValue::format_wei(amount1, pool.token1().decimals);
      sim_res = res;

      let state = evm.balance(from);
      eth_balance_after = if let Some(state) = state {
         state.data
      } else {
         U256::ZERO
      };
   }

   let mut minimum_amount0_to_be_removed = amount0_removed.clone();
   let mut minimum_amount1_to_be_removed = amount1_removed.clone();

   minimum_amount0_to_be_removed.calc_slippage(slippage, pool.token0().decimals);
   minimum_amount1_to_be_removed.calc_slippage(slippage, pool.token1().decimals);

   let amount0_usd_to_be_removed =
      ctx.get_currency_value_for_amount(amount0_removed.f64(), pool.currency0());
   let amount1_usd_to_be_removed =
      ctx.get_currency_value_for_amount(amount1_removed.f64(), pool.currency1());
   let minimum_amount0_usd_to_be_removed = ctx.get_currency_value_for_amount(
      minimum_amount0_to_be_removed.f64(),
      pool.currency0(),
   );
   let minimum_amount1_usd_to_be_removed = ctx.get_currency_value_for_amount(
      minimum_amount1_to_be_removed.f64(),
      pool.currency1(),
   );

   decrease_liquidity_params.amount0Min = minimum_amount0_to_be_removed.wei2();
   decrease_liquidity_params.amount1Min = minimum_amount1_to_be_removed.wei2();

   let call_data = abi::uniswap::nft_position::encode_decrease_liquidity(decrease_liquidity_params);

   let position_params = UniswapPositionParams {
      position_operation: PositionOperation::DecreaseLiquidity,
      currency0: pool.currency0.clone(),
      currency1: pool.currency1.clone(),
      amount0: amount0_removed.clone(),
      amount1: amount1_removed.clone(),
      amount0_usd: Some(amount0_usd_to_be_removed),
      amount1_usd: Some(amount1_usd_to_be_removed),
      min_amount0: Some(minimum_amount0_to_be_removed),
      min_amount0_usd: Some(minimum_amount0_usd_to_be_removed),
      min_amount1: Some(minimum_amount1_to_be_removed),
      min_amount1_usd: Some(minimum_amount1_usd_to_be_removed),
      sender: from,
      recipient: None,
   };

   let eth_balance_before = ctx.get_eth_balance(chain.id(), from);

   let contract_interact = true;
   let interact_to = nft_contract;
   let value = U256::ZERO;

   let tx_analysis = TransactionAnalysis {
      chain: chain.id(),
      sender: from,
      interact_to,
      contract_interact,
      value,
      call_data: call_data.clone(),
      gas_used: sim_res.gas_used(),
      eth_balance_before: eth_balance_before.wei2(),
      eth_balance_after,
      decoded_selector: "Decrease Liquidity".to_string(),
      positions_ops: vec![position_params],
      ..Default::default()
   };

   let (receipt, tx_rich) = send_transaction(
      ctx.clone(),
      "".to_string(),
      None,
      Some(tx_analysis),
      chain,
      mev_protect,
      from,
      interact_to,
      call_data,
      value,
   )
   .await?;

   if !receipt.status() {
      return Err(anyhow!("Transaction failed"));
   }

   let step1 = Step {
      id: "step1",
      in_progress: false,
      finished: true,
      msg: "Transaction Sent".to_string(),
   };

   SHARED_GUI.write(|gui| {
      gui.progress_window
         .open_with(vec![step1], "Success!".to_string());
      gui.progress_window.set_tx(tx_rich);
      gui.request_repaint();
   });

   let tokens = vec![pool.token0().into_owned(), pool.token1().into_owned()];

   let ctx2 = ctx.clone();
   RT.spawn(async move {
      let manager = ctx2.balance_manager();
      manager
         .update_tokens_balance(ctx2.clone(), chain.id(), from, tokens)
         .await
         .unwrap();
      ctx2.save_balance_manager();
   });

   let updated_position =
      abi::uniswap::nft_position::positions(client.clone(), nft_contract, position.id).await?;

   // calculate the new amounts
   let (new_amount0, new_amount1) = calculate_liquidity_amounts(
      state.sqrt_price,
      sqrt_price_lower,
      sqrt_price_upper,
      updated_position.liquidity,
   )?;

   let new_amount0 = NumericValue::format_wei(new_amount0, pool.token0().decimals);
   let new_amount1 = NumericValue::format_wei(new_amount1, pool.token1().decimals);

   let tokens_owed0 = NumericValue::format_wei(
      updated_position.tokens_owed0,
      pool.token0().decimals,
   );
   let tokens_owed1 = NumericValue::format_wei(
      updated_position.tokens_owed1,
      pool.token1().decimals,
   );

   // update the position
   position.amount0 = new_amount0;
   position.amount1 = new_amount1;
   position.tokens_owed0 = tokens_owed0;
   position.tokens_owed1 = tokens_owed1;
   position.fee_growth_inside0_last_x128 = updated_position.fee_growth_inside0_last_x128;
   position.fee_growth_inside1_last_x128 = updated_position.fee_growth_inside1_last_x128;
   position.liquidity = updated_position.liquidity;

   ctx.write(|ctx| {
      ctx.v3_positions_db.insert(chain.id(), owner, position);
   });

   ctx.save_v3_positions_db();

   Ok(())
}

/// Increase liquidity on a V3 position
pub async fn increase_liquidity_position_v3(
   ctx: ZeusCtx,
   chain: ChainId,
   from: Address,
   mut position: V3Position,
   mut pool: UniswapV3Pool,
   token0_deposit_amount: NumericValue,
   slippage: String,
   mev_protect: bool,
) -> Result<(), anyhow::Error> {
   let client = ctx.get_client(chain.id()).await?;
   let nft_contract = uniswap_nft_position_manager(chain.id())?;

   let token0 = pool.token0().into_owned();
   let token1 = pool.token1().into_owned();

   let block_fut = client.get_block(BlockId::latest()).into_future();
   let token0_allowance_fut = token0.allowance(client.clone(), from, nft_contract);
   let token1_allowance_fut = token1.allowance(client.clone(), from, nft_contract);

   pool.update_state(client.clone(), None).await?;

   let sqrt_price_lower = uniswap_v3_math::tick_math::get_sqrt_ratio_at_tick(position.tick_lower)?;
   let sqrt_price_upper = uniswap_v3_math::tick_math::get_sqrt_ratio_at_tick(position.tick_upper)?;

   let state = pool.state().v3_state().unwrap();

   let liquidity = calculate_liquidity_from_amount(
      state.sqrt_price,
      sqrt_price_lower,
      sqrt_price_upper,
      token0_deposit_amount.wei2(),
      true,
   )?;

   let (amount0, amount1) = calculate_liquidity_amounts(
      state.sqrt_price,
      sqrt_price_lower,
      sqrt_price_upper,
      liquidity,
   )?;

   let amount0 = NumericValue::format_wei(amount0, pool.currency0().decimals());
   let amount1 = NumericValue::format_wei(amount1, pool.currency0().decimals());

   let mut amount0_min = amount0.clone();
   let mut amount1_min = amount1.clone();

   let slippage: f64 = slippage.parse().unwrap_or(0.5);

   amount0_min.calc_slippage(slippage, pool.token0().decimals);
   amount1_min.calc_slippage(slippage, pool.token1().decimals);

   let deadline = utils::get_unix_time_in_minutes(1)?;
   let mut increase_liquidity_params = INonfungiblePositionManager::IncreaseLiquidityParams {
      tokenId: position.id,
      amount0Desired: amount0.wei2(),
      amount1Desired: amount1.wei2(),
      amount0Min: amount0_min.wei2(),
      amount1Min: amount1_min.wei2(),
      deadline: U256::from(deadline),
   };

   let owner = from;

   let factory = ForkFactory::new_sandbox_factory(client.clone(), chain.id(), None, None);
   let fork_db = factory.new_sandbox_fork();

   let block = block_fut.await?;
   let token0_allowance = token0_allowance_fut.await?;
   let token1_allowance = token1_allowance_fut.await?;

   // Simulate the transaction
   let eth_balance_after;
   let increase_liquidity_sim_res;
   let amount0_minted;
   let amount1_minted;
   let mut approve_sim_res_a = None;
   let mut approve_sim_res_b = None;

   {
      let mut evm = new_evm(chain, block.as_ref(), fork_db.clone());
      if token0_allowance < amount0.wei2() {
         let res = simulate::approve_token(
            &mut evm,
            pool.token0().address,
            owner,
            nft_contract,
            U256::MAX,
         )?;

         approve_sim_res_a = Some(res);
      }

      if token1_allowance < amount1.wei2() {
         let res = simulate::approve_token(
            &mut evm,
            pool.token1().address,
            owner,
            nft_contract,
            U256::MAX,
         )?;

         approve_sim_res_b = Some(res);
      }

      let now = Instant::now();
      let (res, _liquidity, amount0, amount1) = simulate::increase_liquidity(
         &mut evm,
         increase_liquidity_params.clone(),
         owner,
         nft_contract,
         true,
      )?;

      tracing::info!(
         "Simulated Increase Liquidity in {} ms",
         now.elapsed().as_millis()
      );

      amount0_minted = NumericValue::format_wei(amount0, pool.token0().decimals);
      amount1_minted = NumericValue::format_wei(amount1, pool.token1().decimals);
      increase_liquidity_sim_res = res;

      let state = evm.balance(from);
      eth_balance_after = if let Some(state) = state {
         state.data
      } else {
         U256::ZERO
      };
   }

   let mut min_amount0_minted = amount0_minted.clone();
   let mut min_amount1_minted = amount1_minted.clone();

   min_amount0_minted.calc_slippage(slippage, pool.token0().decimals);
   min_amount1_minted.calc_slippage(slippage, pool.token1().decimals);

   increase_liquidity_params.amount0Desired = amount0_minted.wei2();
   increase_liquidity_params.amount1Desired = amount1_minted.wei2();
   increase_liquidity_params.amount0Min = min_amount0_minted.wei2();
   increase_liquidity_params.amount1Min = min_amount1_minted.wei2();

   let currency0 = Currency::from(pool.token0().into_owned());
   let currency1 = Currency::from(pool.token1().into_owned());

   let amount0_usd = ctx.get_currency_value_for_amount(amount0_minted.f64(), &currency0);
   let amount1_usd = ctx.get_currency_value_for_amount(amount1_minted.f64(), &currency1);
   let min_amount0_usd = ctx.get_currency_value_for_amount(min_amount0_minted.f64(), &currency0);
   let min_amount1_usd = ctx.get_currency_value_for_amount(min_amount1_minted.f64(), &currency1);

   let position_params = UniswapPositionParams {
      position_operation: PositionOperation::AddLiquidity,
      currency0: currency0.clone(),
      currency1: currency1.clone(),
      amount0: amount0_minted.clone(),
      amount1: amount1_minted.clone(),
      amount0_usd: Some(amount0_usd),
      amount1_usd: Some(amount1_usd),
      min_amount0: Some(amount0_min.clone()),
      min_amount0_usd: Some(min_amount0_usd),
      min_amount1: Some(amount1_min.clone()),
      min_amount1_usd: Some(min_amount1_usd),
      sender: from,
      recipient: None,
   };

   let eth_balance_before = ctx.get_eth_balance(chain.id(), from);

   // Handle one-time token approvals for the nft contract if needed
   if token0_allowance < amount0.wei2() {
      let sim_res = approve_sim_res_a.unwrap();

      let call_data = pool.token0().encode_approve(nft_contract, U256::MAX);
      let interact_to = pool.token0().address;
      let value = U256::ZERO;
      let contract_interact = true;
      let amount = NumericValue::format_wei(U256::MAX, pool.token0().decimals);

      let params = TokenApproveParams {
         token: vec![pool.token0().into_owned()],
         amount: vec![amount],
         amount_usd: vec![None],
         owner: from,
         spender: nft_contract,
      };

      let analysis = TransactionAnalysis {
         chain: chain.id(),
         sender: from,
         interact_to,
         contract_interact,
         value,
         call_data: call_data.clone(),
         gas_used: sim_res.gas_used(),
         eth_balance_before: eth_balance_before.wei2(),
         eth_balance_after,
         decoded_selector: "Approve".to_string(),
         token_approvals: vec![params],
         ..Default::default()
      };

      let (receipt, _) = send_transaction(
         ctx.clone(),
         "".to_string(),
         None,
         Some(analysis),
         chain,
         mev_protect,
         from,
         interact_to,
         call_data,
         value,
      )
      .await?;

      if !receipt.status() {
         return Err(anyhow!("Token Approval Failed"));
      }
   }

   if token1_allowance < amount1.wei2() {
      let sim_res = approve_sim_res_b.unwrap();

      let call_data = pool.token1().encode_approve(nft_contract, U256::MAX);
      let interact_to = pool.token1().address;
      let value = U256::ZERO;
      let contract_interact = true;
      let amount = NumericValue::format_wei(U256::MAX, pool.token1().decimals);

      let params = TokenApproveParams {
         token: vec![pool.token1().into_owned()],
         amount: vec![amount],
         amount_usd: vec![None],
         owner: from,
         spender: nft_contract,
      };

      let analysis = TransactionAnalysis {
         chain: chain.id(),
         sender: from,
         interact_to,
         contract_interact,
         value,
         call_data: call_data.clone(),
         gas_used: sim_res.gas_used(),
         eth_balance_before: eth_balance_before.wei2(),
         eth_balance_after,
         decoded_selector: "Approve".to_string(),
         token_approvals: vec![params],
         ..Default::default()
      };

      let (receipt, _) = send_transaction(
         ctx.clone(),
         "".to_string(),
         None,
         Some(analysis),
         chain,
         mev_protect,
         from,
         interact_to,
         call_data.into(),
         value,
      )
      .await?;

      if !receipt.status() {
         return Err(anyhow!("Token Approval Failed"));
      }
   }

   // Now proceed with the call

   let call_data = abi::uniswap::nft_position::encode_increase_liquidity(increase_liquidity_params);
   let contract_interact = true;
   let interact_to = nft_contract;
   let value = U256::ZERO;

   let tx_analysis = TransactionAnalysis {
      chain: chain.id(),
      sender: from,
      interact_to,
      contract_interact,
      value,
      call_data: call_data.clone(),
      gas_used: increase_liquidity_sim_res.gas_used(),
      eth_balance_before: eth_balance_before.wei2(),
      eth_balance_after,
      decoded_selector: "Increase Liquidity".to_string(),
      positions_ops: vec![position_params],
      ..Default::default()
   };

   let (receipt, tx_rich) = send_transaction(
      ctx.clone(),
      "".to_string(),
      None,
      Some(tx_analysis),
      chain,
      mev_protect,
      from,
      interact_to,
      call_data,
      value,
   )
   .await?;

   if !receipt.status() {
      return Err(anyhow!("Transaction failed"));
   }

   let step1 = Step {
      id: "step1",
      in_progress: false,
      finished: true,
      msg: "Transaction Sent".to_string(),
   };

   SHARED_GUI.write(|gui| {
      gui.progress_window
         .open_with(vec![step1], "Success!".to_string());
      gui.progress_window.set_tx(tx_rich);
      gui.request_repaint();
   });

   let tokens = vec![pool.token0().into_owned(), pool.token1().into_owned()];

   let ctx2 = ctx.clone();
   RT.spawn(async move {
      let manager = ctx2.balance_manager();
      manager
         .update_tokens_balance(ctx2.clone(), chain.id(), from, tokens)
         .await
         .unwrap();
      ctx2.save_balance_manager();
   });

   let updated_position =
      abi::uniswap::nft_position::positions(client.clone(), nft_contract, position.id).await?;

   // calculate the new amounts
   let (new_amount0, new_amount1) = calculate_liquidity_amounts(
      state.sqrt_price,
      sqrt_price_lower,
      sqrt_price_upper,
      updated_position.liquidity,
   )?;

   let amount0_minted = NumericValue::format_wei(new_amount0, pool.token0().decimals);
   let amount1_minted = NumericValue::format_wei(new_amount1, pool.token1().decimals);
   let tokens_owed0 = NumericValue::format_wei(
      updated_position.tokens_owed0,
      pool.token0().decimals,
   );
   let tokens_owed1 = NumericValue::format_wei(
      updated_position.tokens_owed1,
      pool.token1().decimals,
   );

   // update the position
   position.amount0 = amount0_minted;
   position.amount1 = amount1_minted;
   position.tokens_owed0 = tokens_owed0;
   position.tokens_owed1 = tokens_owed1;
   position.fee_growth_inside0_last_x128 = updated_position.fee_growth_inside0_last_x128;
   position.fee_growth_inside1_last_x128 = updated_position.fee_growth_inside1_last_x128;
   position.liquidity = updated_position.liquidity;

   ctx.write(|ctx| {
      ctx.v3_positions_db.insert(chain.id(), owner, position);
   });
   ctx.save_v3_positions_db();

   Ok(())
}

/// Open a new position on a V3 pool
pub async fn mint_new_liquidity_position_v3(
   ctx: ZeusCtx,
   chain: ChainId,
   from: Address,
   mut pool: UniswapV3Pool,
   lower_range: f64,
   upper_range: f64,
   token0_deposit_amount: NumericValue,
   slippage: String,
   mev_protect: bool,
) -> Result<(), anyhow::Error> {
   let client = ctx.get_client(chain.id()).await?;
   let nft_contract = uniswap_nft_position_manager(chain.id())?;

   let token0 = pool.token0().into_owned();
   let token1 = pool.token1().into_owned();

   let block_fut = client.get_block(BlockId::latest()).into_future();
   let token0_allowance_fut = token0.allowance(client.clone(), from, nft_contract);
   let token1_allowance_fut = token1.allowance(client.clone(), from, nft_contract);

   pool.update_state(client.clone(), None).await?;

   let lower_tick = get_tick_from_price(lower_range);
   let upper_tick = get_tick_from_price(upper_range);

   let sqrt_price_lower = uniswap_v3_math::tick_math::get_sqrt_ratio_at_tick(lower_tick)?;
   let sqrt_price_upper = uniswap_v3_math::tick_math::get_sqrt_ratio_at_tick(upper_tick)?;

   let state = pool.state().v3_state().unwrap();

   let liquidity = calculate_liquidity_from_amount(
      state.sqrt_price,
      sqrt_price_lower,
      sqrt_price_upper,
      token0_deposit_amount.wei2(),
      true,
   )?;

   let (final_amount0, final_amount1) = calculate_liquidity_amounts(
      state.sqrt_price,
      sqrt_price_lower,
      sqrt_price_upper,
      liquidity,
   )?;

   let amount0 = NumericValue::format_wei(final_amount0, pool.currency0().decimals());
   let amount1 = NumericValue::format_wei(final_amount1, pool.currency0().decimals());

   let lower_tick: Signed<24, 1> = lower_tick
      .to_string()
      .parse()
      .context("Failed to parse lower tick")?;
   let upper_tick: Signed<24, 1> = upper_tick
      .to_string()
      .parse()
      .context("Failed to parse upper tick")?;

   let mut amount0_min = amount0.clone();
   let mut amount1_min = amount1.clone();

   let slippage: f64 = slippage.parse().unwrap_or(0.5);
   amount0_min.calc_slippage(slippage, pool.token0().decimals);
   amount1_min.calc_slippage(slippage, pool.token1().decimals);

   let deadline = utils::get_unix_time_in_minutes(1)?;
   let mint_params = INonfungiblePositionManager::MintParams {
      token0: pool.token0().address,
      token1: pool.token1().address,
      fee: pool.fee().fee_u24(),
      tickLower: lower_tick,
      tickUpper: upper_tick,
      amount0Desired: amount0.wei2(),
      amount1Desired: amount1.wei2(),
      amount0Min: amount0_min.wei2(),
      amount1Min: amount1_min.wei2(),
      recipient: from, // owner of the position
      deadline: U256::from(deadline),
   };

   tracing::info!("Mint Params {:?}", mint_params);

   let owner = from;

   let factory = ForkFactory::new_sandbox_factory(client.clone(), chain.id(), None, None);
   let fork_db = factory.new_sandbox_fork();

   let block = block_fut.await?;
   let token0_allowance = token0_allowance_fut.await?;
   let token1_allowance = token1_allowance_fut.await?;

   let mint_call_data = encode_mint(mint_params.clone());

   // Simulate the transaction
   let eth_balance_after;
   let mint_sim_res;
   let amount0_minted;
   let amount1_minted;
   let mut approve_sim_res_a = None;
   let mut approve_sim_res_b = None;
   {
      let mut evm = new_evm(chain, block.as_ref(), fork_db.clone());

      if token0_allowance < amount0.wei2() {
         let res = simulate::approve_token(
            &mut evm,
            pool.token0().address,
            owner,
            nft_contract,
            U256::MAX,
         )?;

         approve_sim_res_a = Some(res);
      }

      if token1_allowance < amount1.wei2() {
         let res = simulate::approve_token(
            &mut evm,
            pool.token1().address,
            owner,
            nft_contract,
            U256::MAX,
         )?;
         approve_sim_res_b = Some(res);
      }

      let now = Instant::now();
      let (res, mint_res) =
         simulate::mint_position(&mut evm, mint_params, from, nft_contract, true)?;

      tracing::info!(
         "Simulated Mint position in {} ms",
         now.elapsed().as_millis()
      );

      amount0_minted = NumericValue::format_wei(mint_res.amount0, pool.token0().decimals);
      amount1_minted = NumericValue::format_wei(mint_res.amount1, pool.token1().decimals);
      mint_sim_res = res;

      let state = evm.balance(from);
      eth_balance_after = if let Some(state) = state {
         state.data
      } else {
         U256::ZERO
      };
   }

   let mut min_amount0_minted = amount0_minted.clone();
   let mut min_amount1_minted = amount1_minted.clone();

   min_amount0_minted.calc_slippage(slippage, pool.token0().decimals);
   min_amount1_minted.calc_slippage(slippage, pool.token1().decimals);

   let currency0 = Currency::from(pool.token0().into_owned());
   let currency1 = Currency::from(pool.token1().into_owned());

   let amount0_usd = ctx.get_currency_value_for_amount(amount0_minted.f64(), &currency0);
   let amount1_usd = ctx.get_currency_value_for_amount(amount1_minted.f64(), &currency1);
   let min_amount0_usd = ctx.get_currency_value_for_amount(min_amount0_minted.f64(), &currency0);
   let min_amount1_usd = ctx.get_currency_value_for_amount(min_amount1_minted.f64(), &currency1);

   let position_params = UniswapPositionParams {
      position_operation: PositionOperation::AddLiquidity,
      currency0: currency0.clone(),
      currency1: currency1.clone(),
      amount0: amount0_minted.clone(),
      amount1: amount1_minted.clone(),
      amount0_usd: Some(amount0_usd),
      amount1_usd: Some(amount1_usd),
      min_amount0: Some(amount0_min.clone()),
      min_amount0_usd: Some(min_amount0_usd),
      min_amount1: Some(amount1_min.clone()),
      min_amount1_usd: Some(min_amount1_usd),
      sender: from,
      recipient: Some(from),
   };

   let eth_balance_before = ctx.get_eth_balance(chain.id(), from);

   // Handle one-time token approvals for the nft contract if needed
   if token0_allowance < amount0.wei2() {
      let sim_res = approve_sim_res_a.unwrap();

      let call_data = pool.token0().encode_approve(nft_contract, U256::MAX);
      let interact_to = pool.token0().address;
      let value = U256::ZERO;
      let contract_interact = true;
      let amount = NumericValue::format_wei(U256::MAX, pool.token0().decimals);

      let params = TokenApproveParams {
         token: vec![pool.token0().into_owned()],
         amount: vec![amount],
         amount_usd: vec![None],
         owner: from,
         spender: nft_contract,
      };

      let tx_analysis = TransactionAnalysis {
         chain: chain.id(),
         sender: from,
         interact_to,
         contract_interact,
         value,
         call_data: call_data.clone(),
         gas_used: sim_res.gas_used(),
         eth_balance_before: eth_balance_before.wei2(),
         eth_balance_after,
         decoded_selector: "Approve".to_string(),
         token_approvals: vec![params],
         ..Default::default()
      };

      let (receipt, _) = send_transaction(
         ctx.clone(),
         "".to_string(),
         None,
         Some(tx_analysis),
         chain,
         mev_protect,
         from,
         interact_to,
         call_data.into(),
         value,
      )
      .await?;

      if !receipt.status() {
         return Err(anyhow!("Token Approval Failed"));
      }
   }

   if token1_allowance < amount1.wei2() {
      let sim_res = approve_sim_res_b.unwrap();

      let call_data = pool.token1().encode_approve(nft_contract, U256::MAX);
      let interact_to = pool.token1().address;
      let value = U256::ZERO;
      let contract_interact = true;
      let amount = NumericValue::format_wei(U256::MAX, pool.token1().decimals);

      let params = TokenApproveParams {
         token: vec![pool.token1().into_owned()],
         amount: vec![amount],
         amount_usd: vec![None],
         owner: from,
         spender: nft_contract,
      };

      let tx_analysis = TransactionAnalysis {
         chain: chain.id(),
         sender: from,
         interact_to,
         contract_interact,
         value,
         call_data: call_data.clone(),
         gas_used: sim_res.gas_used(),
         eth_balance_before: eth_balance_before.wei2(),
         eth_balance_after,
         decoded_selector: "Approve".to_string(),
         token_approvals: vec![params],
         ..Default::default()
      };

      let (receipt, _) = send_transaction(
         ctx.clone(),
         "".to_string(),
         None,
         Some(tx_analysis),
         chain,
         mev_protect,
         from,
         interact_to,
         call_data.into(),
         value,
      )
      .await?;

      if !receipt.status() {
         return Err(anyhow!("Token Approval Failed"));
      }
   }

   // Now proceed with the mint call

   let contract_interact = true;
   let interact_to = nft_contract;
   let value = U256::ZERO;

   let tx_analysis = TransactionAnalysis {
      chain: chain.id(),
      sender: from,
      interact_to,
      contract_interact,
      value,
      call_data: mint_call_data.clone(),
      gas_used: mint_sim_res.gas_used(),
      eth_balance_before: eth_balance_before.wei2(),
      eth_balance_after,
      decoded_selector: "Mint".to_string(),
      positions_ops: vec![position_params],
      ..Default::default()
   };

   let (receipt, tx_rich) = send_transaction(
      ctx.clone(),
      "".to_string(),
      None,
      Some(tx_analysis),
      chain,
      mev_protect,
      from,
      interact_to,
      mint_call_data,
      value,
   )
   .await?;

   if !receipt.status() {
      return Err(anyhow!("Transaction failed"));
   }

   let logs: Vec<Log> = receipt.logs().to_vec();
   let log_data = logs
      .iter()
      .map(|l| l.clone().into_inner())
      .collect::<Vec<_>>();

   let mut position_info = None;

   for log in &log_data {
      if let Ok(decoded) = abi::uniswap::nft_position::decode_increase_liquidity_log(log) {
         position_info = Some(decoded);
         break;
      }
   }

   let hash = receipt.transaction_hash;

   if position_info.is_none() {
      tracing::error!(
         "Tx with hash {} was success but not No position info found",
         hash
      );
   }

   let step1 = Step {
      id: "step1",
      in_progress: false,
      finished: true,
      msg: "Transaction Sent".to_string(),
   };

   SHARED_GUI.write(|gui| {
      gui.progress_window
         .open_with(vec![step1], "Success!".to_string());
      gui.progress_window.set_tx(tx_rich);
      gui.request_repaint();
   });

   let tokens = vec![pool.token0().into_owned(), pool.token1().into_owned()];

   let ctx2 = ctx.clone();
   RT.spawn(async move {
      let manager = ctx2.balance_manager();
      manager
         .update_tokens_balance(ctx2.clone(), chain.id(), from, tokens)
         .await
         .unwrap();
      ctx2.save_balance_manager();
   });

   if position_info.is_some() {
      let position_info = position_info.unwrap();

      let position = abi::uniswap::nft_position::positions(
         client.clone(),
         nft_contract,
         position_info.tokenId,
      )
      .await?;

      let receipt_block = receipt.block_number.unwrap_or_default();
      let amount0 = NumericValue::format_wei(position_info.amount0, pool.currency0().decimals());
      let amount1 = NumericValue::format_wei(position_info.amount1, pool.currency1().decimals());

      let timestamp = std::time::SystemTime::now()
         .duration_since(std::time::UNIX_EPOCH)?
         .as_secs();

      let v3_position = V3Position {
         chain_id: chain.id(),
         owner,
         dex: pool.dex,
         block: receipt_block,
         timestamp,
         id: position_info.tokenId,
         nonce: position.nonce,
         operator: position.operator,
         token0: pool.currency0().clone(),
         token1: pool.currency1().clone(),
         fee: pool.fee(),
         pool_address: pool.address(),
         tick_lower: position.tick_lower,
         tick_upper: position.tick_upper,
         liquidity: position.liquidity,
         fee_growth_inside0_last_x128: position.fee_growth_inside0_last_x128,
         fee_growth_inside1_last_x128: position.fee_growth_inside1_last_x128,
         amount0,
         amount1,
         tokens_owed0: NumericValue::default(),
         tokens_owed1: NumericValue::default(),
         apr: 0.0,
      };

      ctx.write(|ctx| {
         ctx.v3_positions_db.insert(chain.id(), owner, v3_position);
      });
      ctx.save_v3_positions_db();
   } else {
      tracing::error!("Position Info not added because the position was not found");
   }

   Ok(())
}

pub async fn across_bridge(
   ctx: ZeusCtx,
   chain: ChainId,
   dest_chain: ChainId,
   deadline: u32,
   from: Address,
   recipient: Address,
   interact_to: Address,
   call_data: Bytes,
   value: U256,
) -> Result<(), anyhow::Error> {
   // Across protocol is very fast on filling the orders
   // So we get the latest block from the destination chain now so we dont miss it and the progress window stucks
   let dest_client = ctx.get_client(dest_chain.id()).await?;
   let from_block_fut = dest_client.get_block_number().into_future();

   let (_, tx_rich) = send_transaction(
      ctx,
      "".to_string(),
      None,
      None,
      chain,
      false,
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
   let from_block = from_block_fut.await?;

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
      gui.progress_window.set_tx(tx_rich);
      gui.request_repaint();
   });

   Ok(())
}

pub async fn send_crypto(
   ctx: ZeusCtx,
   chain: ChainId,
   from: Address,
   interact_to: Address,
   call_data: Bytes,
   value: U256,
) -> Result<(), anyhow::Error> {
   let (_, tx_rich) = send_transaction(
      ctx,
      "".to_string(),
      None,
      None,
      chain,
      false,
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

   SHARED_GUI.write(|gui| {
      gui.progress_window
         .open_with(vec![step1], "Success!".to_string());
      gui.progress_window.set_tx(tx_rich);
      gui.request_repaint();
   });

   Ok(())
}

pub async fn sync_pools_for_tokens(
   ctx: ZeusCtx,
   chain: u64,
   tokens: Vec<ERC20Token>,
   sync_v4: bool,
) -> Result<(), anyhow::Error> {
   let pool_manager = ctx.pool_manager();
   let dex_kind = DexKind::main_dexes(chain);

   let client = if let Ok(client) = ctx.get_archive_client(chain).await {
      client
   } else {
      ctx.get_client(chain).await?
   };

   pool_manager
      .sync_pools_for_tokens(client, chain, tokens, dex_kind, sync_v4)
      .await?;

   Ok(())
}
