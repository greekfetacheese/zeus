use crate::core::clear_signing;
use crate::core::{
   TransactionAnalysis, TransactionRich, ZeusCtx, client::CLIENT_TIMEOUT_FOR_SENDING_TX,
};
use crate::utils::state::get_base_fee;
use alloy_eips::eip7702::{Authorization, SignedAuthorization};
use anyhow::anyhow;
use std::time::Duration;

use crate::core::tx::balance_diff::{
   BalanceDiff, collect_token_candidates, native_change, token_change,
};
use crate::gui::{SHARED_GUI, ui::NotificationType};
use crate::utils::{
   RT, TimeStamp, estimate_tx_cost,
   simulate::{fetch_accounts_info, simulate_transaction},
};
use zeus_eth::{
   alloy_contract::private::Provider,
   alloy_network::{
      Ethereum, NetworkTransactionBuilder, TransactionBuilder, TransactionBuilder7702,
   },
   alloy_primitives::{Address, Bytes, U256},
   alloy_rpc_types::{BlockId, TransactionReceipt, TransactionRequest},
   alloy_signer::SignerSync,
   revm_utils::{ForkFactory, Host, new_evm, simulate::erc20_balance},
   types::ChainId,
};
use zeus_wallet::SecureKey;

#[derive(Clone)]
pub struct TxParams {
   pub signer: SecureKey,
   pub transcact_to: Address,
   pub nonce: u64,
   pub value: U256,
   pub chain: ChainId,
   pub miner_tip: U256,
   pub base_fee: u64,
   pub call_data: Bytes,
   pub gas_used: u64,
   pub gas_limit: u64,
   pub authorization_list: Vec<SignedAuthorization>,
}

impl TxParams {
   pub fn new(
      signer: SecureKey,
      transcact_to: Address,
      nonce: u64,
      value: U256,
      chain: ChainId,
      miner_tip: U256,
      base_fee: u64,
      call_data: Bytes,
      gas_used: u64,
      gas_limit: u64,
      authorization_list: Vec<SignedAuthorization>,
   ) -> Self {
      Self {
         signer,
         transcact_to,
         nonce,
         value,
         chain,
         miner_tip,
         base_fee,
         call_data,
         gas_used,
         gas_limit,
         authorization_list,
      }
   }

   pub fn max_fee_per_gas(&self) -> U256 {
      let fee = self.miner_tip + U256::from(self.base_fee);
      // add a 10% tolerance
      fee * U256::from(110) / U256::from(100)
   }
}

async fn wait_tx_confirm() -> bool {
   loop {
      tokio::time::sleep(Duration::from_millis(50)).await;
      let confirmed = SHARED_GUI.read(|gui| gui.tx_confirmation_window.get_confirmed_or_rejected());
      if let Some(confirmed) = confirmed {
         SHARED_GUI.write(|gui| {
            gui.tx_confirmation_window.close();
         });
         return confirmed;
      }
   }
}

async fn wait_confirm_window() -> bool {
   loop {
      tokio::time::sleep(Duration::from_millis(50)).await;
      let confirmed = SHARED_GUI.read(|gui| gui.confirm_window.get_confirm());
      if let Some(confirmed) = confirmed {
         SHARED_GUI.write(|gui| {
            gui.confirm_window.reset();
         });
         return confirmed;
      }
   }
}

pub async fn send_transaction(
   ctx: ZeusCtx,
   dapp: String,
   tx_analysis: Option<TransactionAnalysis>,
   chain: ChainId,
   mev_protect: bool,
   from: Address,
   interact_to: Address,
   call_data: Bytes,
   value: U256,
   authorization_list: Vec<SignedAuthorization>,
) -> Result<(TransactionReceipt, TransactionRich), anyhow::Error> {
   let client = ctx.get_zeus_client();

   let base_fee_fut = get_base_fee(ctx.clone(), chain.id());
   let nonce_fut = client.request(chain.id(), |client| async move {
      client.get_transaction_count(from).await.map_err(|e| anyhow!("{:?}", e))
   });

   let balance_before = if let Some(analysis) = tx_analysis.as_ref() {
      analysis.eth_balance_before
   } else {
      client
         .request(chain.id(), |client| async move {
            client.get_balance(from).await.map_err(|e| anyhow!("{:?}", e))
         })
         .await?
   };

   // If no tx analysis is provided, simulate the transaction
   let tx_analysis = if let Some(analysis) = tx_analysis {
      analysis
   } else {
      SHARED_GUI.write(|gui| {
         gui.loading_window.open("Wait while magic happens");
         gui.request_repaint();
      });

      let block = client
         .request(chain.id(), |client| async move {
            client.get_block(BlockId::latest()).await.map_err(|e| anyhow!("{:?}", e))
         })
         .await?;

      let block =
         block.ok_or_else(|| anyhow!("No block found, this is usally a provider issue"))?;

      let block_id = BlockId::number(block.header.number);

      let accounts = vec![from, interact_to, block.header.beneficiary];

      let accounts_info = fetch_accounts_info(ctx.clone(), chain.id(), block_id, accounts).await;
      let fork_client = ctx.get_client(chain.id()).await?;
      let mut factory =
         ForkFactory::new_sandbox_factory(fork_client, chain.id(), None, Some(block_id));

      for info in accounts_info {
         factory.insert_account_info(info.address, info.info);
      }

      let fork_before = factory.new_sandbox_fork();
      let fork_db = factory.new_sandbox_fork();

      let bytecode_fut = client.request(chain.id(), |client| async move {
         client.get_code_at(interact_to).await.map_err(|e| anyhow!("{:?}", e))
      });

      let portfolio_tokens = ctx.get_portfolio(chain.id(), from);

      let (sim_res, balance_after, logs, measured_tokens) = {
         let mut evm = new_evm(chain, Some(&block), fork_db);

         let time = std::time::Instant::now();
         let sim_res = simulate_transaction(
            &mut evm,
            from,
            interact_to,
            call_data.clone(),
            value,
            authorization_list.clone(),
         )?;

         tracing::info!(
            "Simulate Transaction took {} ms",
            time.elapsed().as_millis()
         );

         let balance_after = evm.balance(from).map(|state| state.data).unwrap_or(U256::ZERO);
         let logs = sim_res.clone().into_logs();

         let candidates = collect_token_candidates(
            portfolio_tokens.tokens().iter().map(|t| t.address),
            interact_to,
            logs.iter().map(|log| log.address),
         );

         let mut before_evm = new_evm(chain, Some(&block), fork_before);
         before_evm.tx.caller = from;
         let mut measured_tokens = Vec::new();
         for token_addr in candidates {
            let Ok(token_before) = erc20_balance(&mut before_evm, token_addr, from) else {
               continue;
            };
            let Ok(token_after) = erc20_balance(&mut evm, token_addr, from) else {
               continue;
            };
            if token_before != token_after {
               measured_tokens.push((token_addr, token_before, token_after));
            }
         }

         (sim_res, balance_after, logs, measured_tokens)
      };

      let mut token_changes = Vec::new();
      for (token_addr, token_before, token_after) in measured_tokens {
         let Ok(token) = ctx.get_token(chain.id(), token_addr).await else {
            continue;
         };
         if let Some(change) = token_change(token, token_before, token_after) {
            token_changes.push(change);
         }
      }

      let bytecode = bytecode_fut.await?;
      let contract_interact = Some(!bytecode.is_empty());

      let mut analysis = TransactionAnalysis::new(
         ctx.clone(),
         chain.id(),
         from,
         interact_to,
         contract_interact,
         call_data.clone(),
         value,
         logs,
         sim_res.tx_gas_used(),
         balance_before,
         balance_after,
         authorization_list.clone(),
      )
      .await?;

      analysis.balance_diff = BalanceDiff {
         native: native_change(chain.id(), balance_before, balance_after),
         tokens: token_changes,
      };
      analysis
   };

   let priority_fee = ctx.get_priority_fee(chain.id()).unwrap_or_default();
   let sponsored = false;

   SHARED_GUI.write(|gui| {
      gui.tx_confirmation_window.open(
         ctx.clone(),
         dapp,
         chain,
         tx_analysis.clone(),
         priority_fee.f64().to_string(),
         mev_protect,
         sponsored,
      );
      gui.loading_window.reset();
      gui.request_repaint();
   });

   if !wait_tx_confirm().await {
      return Err(anyhow!("Transaction rejected"));
   }

   let main_event = tx_analysis.infer_main_event(ctx.clone(), chain.id());
   let main_event_name = if main_event.is_known() {
      main_event.name()
   } else {
      "Transaction in progress".to_string()
   };

   let nofitification = NotificationType::from_main_event(main_event);

   SHARED_GUI.write(|gui| {
      gui.notification.open_with_spinner(main_event_name, nofitification);
      gui.request_repaint();
   });

   let (fee, gas_limit, confirm_clear) = SHARED_GUI.read(|gui| {
      (
         gui.tx_confirmation_window.get_priority_fee(),
         gui.tx_confirmation_window.get_gas_limit(),
         gui.tx_confirmation_window.get_clear_display(),
      )
   });

   let priority_fee = if fee.is_zero() {
      ctx.get_priority_fee(chain.id()).unwrap_or_default()
   } else {
      fee
   };

   let base_fee = base_fee_fut.await?;
   let nonce = nonce_fut.await?;
   let signer = ctx.get_wallet(from).ok_or(anyhow!("Wallet not found"))?.key;
   let gas_used = tx_analysis.gas_used;

   let tx_params = TxParams::new(
      signer,
      interact_to,
      nonce,
      value,
      chain,
      priority_fee.wei(),
      base_fee.next,
      call_data.clone(),
      gas_used,
      gas_limit,
      authorization_list.clone(),
   );

   let rpc = client.get_best_rpc(chain.id()).ok_or(anyhow!("No available RPC found"))?;
   let tx_client = client.connect_with_timeout(&rpc, CLIENT_TIMEOUT_FOR_SENDING_TX).await?;

   // If needed use MEV protect client, if not found prompt the user to continue
   let send_client = if mev_protect {
      match ctx.get_mev_protect_client(chain.id()).await {
         Ok(mev_client) => mev_client,
         Err(_) => {
            SHARED_GUI.write(|gui| {
               let msg2 = "Continue without MEV protection?";
               gui.confirm_window.open("No available MEV protect RPC found");
               gui.confirm_window.set_msg2(msg2);
               gui.request_repaint();
            });

            if !wait_confirm_window().await {
               return Err(anyhow!("Transaction rejected"));
            }

            tx_client
         }
      }
   } else {
      tx_client
   };

   let receipt = send_tx(send_client, tx_params).await?;
   let tx_block = receipt.block_number.ok_or(anyhow!("No block number from tx receipt"))?;

   let logs: Vec<_> = receipt.logs().iter().cloned().map(|l| l.into_inner()).collect();

   let timestamp = TimeStamp::now_as_secs()?;

   let block_id = BlockId::number(tx_block);
   let balance_after = client
      .request(chain.id(), |client| async move {
         client
            .get_balance(from)
            .block_id(block_id)
            .await
            .map_err(|e| anyhow!("{:?}", e))
      })
      .await?;

   let contract_interact = Some(tx_analysis.contract_interact);

   let mut new_tx_analysis = TransactionAnalysis::new(
      ctx.clone(),
      chain.id(),
      from,
      interact_to,
      contract_interact,
      tx_analysis.call_data.clone(),
      tx_analysis.value,
      logs,
      receipt.gas_used,
      balance_before,
      balance_after,
      authorization_list,
   )
   .await?;

   // Zeus-originated swaps already have a SwapToken main-event override.
   // Connector / inferred swaps do not — those keep the log heuristic.
   if tx_analysis.main_event_opt().is_some_and(|e| e.is_swap()) {
      if let Err(e) = new_tx_analysis.apply_onchain_swap_received(ctx.clone(), tx_block).await {
         tracing::warn!("Failed to apply on-chain swap received: {:?}", e);
      }
   }

   let main_event = new_tx_analysis.infer_main_event(ctx.clone(), chain.id());

   let clear_display = if main_event.is_other() {
      if confirm_clear.is_some() {
         confirm_clear
      } else if new_tx_analysis.contract_interact && new_tx_analysis.call_data.len() >= 4 {
         clear_signing::try_clear_sign_calldata(
            ctx.clone(),
            chain.id(),
            from,
            interact_to,
            new_tx_analysis.value,
            &new_tx_analysis.call_data,
         )
         .await
      } else {
         None
      }
   } else {
      None
   };

   let main_event_name = if main_event.is_known() {
      main_event.name()
   } else if let Some(display) = &clear_display {
      display.heading.clone()
   } else {
      "Transaction successful".to_string()
   };

   let nofitification = NotificationType::from_main_event(main_event.clone());

   let (tx_cost, tx_cost_usd) = ctx.write(|ctx| {
      estimate_tx_cost(
         ctx,
         chain.id(),
         receipt.gas_used,
         priority_fee.wei(),
      )
   });

   // Remove the redunant main event
   new_tx_analysis.remove_main_event();

   let eth_received_usd = ctx.write(|ctx| new_tx_analysis.eth_received_usd(ctx));

   let tx_rich = TransactionRich {
      tx_type: receipt.transaction_type(),
      success: receipt.status(),
      chain: chain.id(),
      block: receipt.block_number.unwrap_or_default(),
      timestamp,
      value_sent: new_tx_analysis.value_sent(),
      value_sent_usd: new_tx_analysis.value_sent_usd(ctx.clone()),
      eth_received: new_tx_analysis.eth_received(),
      eth_received_usd,
      tx_cost,
      tx_cost_usd,
      hash: receipt.transaction_hash,
      contract_interact: new_tx_analysis.contract_interact,
      analysis: new_tx_analysis,
      main_event,
      clear_display,
   };

   let ctx_clone = ctx.clone();
   let tx = tx_rich.clone();
   RT.spawn_blocking(move || {
      ctx_clone.add_transaction(chain.id(), from, tx);
   });

   if !receipt.status() {
      return Err(anyhow!("Transaction Failed"));
   }

   let now = TimeStamp::now_as_millis()?.timestamp();
   let finish = now + 6000;

   SHARED_GUI.write(|gui| {
      gui.notification.open_with_progress_bar(
         now,
         finish,
         main_event_name,
         nofitification,
         Some(tx_rich.clone()),
      );
      gui.loading_window.reset();
      gui.request_repaint();
   });

   Ok((receipt, tx_rich))
}

pub async fn delegate_to(
   ctx: ZeusCtx,
   chain: ChainId,
   from: Address,
   delegate_to: Address,
) -> Result<(), anyhow::Error> {
   let wallet = ctx.get_wallet(from).ok_or(anyhow!("Wallet not found"))?.key;
   let client = ctx.get_zeus_client();

   if !delegate_to.is_zero() {
      let code = client
         .request(chain.id(), |client| async move {
            client.get_code_at(delegate_to).await.map_err(|e| anyhow!("{:?}", e))
         })
         .await?;

      if code.is_empty() {
         return Err(anyhow!(
            "Code is empty, you can only delegate to a smart contract address"
         ));
      }
   }

   let address = wallet.address();

   let nonce = client
      .request(chain.id(), |client| async move {
         client.get_transaction_count(address).await.map_err(|e| anyhow!("{:?}", e))
      })
      .await?;

   let auth_nonce = nonce + 1;

   let auth = Authorization {
      chain_id: U256::from(chain.id()),
      address: delegate_to,
      nonce: auth_nonce,
   };

   let signature = wallet.to_signer().sign_hash_sync(&auth.signature_hash())?;
   let signed_authorization = auth.into_signed(signature);

   send_transaction(
      ctx.clone(),
      String::new(),
      None,
      chain,
      false,
      from,
      from,
      Bytes::default(),
      U256::ZERO,
      vec![signed_authorization],
   )
   .await?;

   if delegate_to.is_zero() {
      ctx.write(|ctx| {
         ctx.delegated_wallets.remove(chain.id(), from);
      });
   } else {
      ctx.write(|ctx| {
         ctx.delegated_wallets.add(chain.id(), from, delegate_to);
      });
   }

   Ok(())
}

pub async fn send_tx<P>(client: P, params: TxParams) -> Result<TransactionReceipt, anyhow::Error>
where
   P: Provider<Ethereum> + Clone + 'static,
{
   let tx = make_tx_request(&params);
   let wallet = params.signer.to_wallet();
   let tx_envelope = tx.build(&wallet).await?;
   drop(wallet);

   let time = std::time::Instant::now();
   let receipt = client
      .send_tx_envelope(tx_envelope)
      .await?
      .with_timeout(Some(Duration::from_secs(
         CLIENT_TIMEOUT_FOR_SENDING_TX,
      )))
      .get_receipt()
      .await?;
   tracing::info!(
      "Time take to send tx: {:?}secs",
      time.elapsed().as_secs_f32()
   );

   Ok(receipt)
}

fn make_tx_request(params: &TxParams) -> TransactionRequest {
   if params.chain.supports_type_2_tx() {
      let mut tx = TransactionRequest::default()
         .with_from(params.signer.address())
         .with_to(params.transcact_to)
         .with_chain_id(params.chain.id())
         .with_value(params.value)
         .with_nonce(params.nonce)
         .with_input(params.call_data.clone())
         .with_gas_limit(params.gas_limit)
         .with_max_priority_fee_per_gas(params.miner_tip.to::<u128>())
         .max_fee_per_gas(params.max_fee_per_gas().to::<u128>());

      if !params.authorization_list.is_empty() {
         tx.set_authorization_list(params.authorization_list.clone());
      }

      tx
   } else {
      TransactionRequest::default()
         .with_from(params.signer.address())
         .with_to(params.transcact_to)
         .with_value(params.value)
         .with_nonce(params.nonce)
         .with_input(params.call_data.clone())
         .with_gas_limit(params.gas_limit)
         .with_gas_price(params.base_fee.into())
   }
}
