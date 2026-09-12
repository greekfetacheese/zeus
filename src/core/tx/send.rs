use crate::core::clear_signing;
use crate::core::{
   TransactionAnalysis, TransactionRich, ZeusCtx, client::CLIENT_TIMEOUT_FOR_SENDING_TX,
};
use crate::utils::state::get_base_fee;
use alloy_eips::eip7702::{Authorization, SignedAuthorization};
use anyhow::anyhow;
use std::time::Duration;

use crate::core::tx::{diffs_from_receipt, simulate_and_diff};
use crate::gui::{SHARED_GUI, ui::NotificationType};
use crate::utils::{RT, TimeStamp, estimate_tx_cost};
use zeus_eth::{
   alloy_contract::private::Provider,
   alloy_network::{
      Ethereum, NetworkTransactionBuilder, TransactionBuilder, TransactionBuilder7702,
   },
   alloy_primitives::{Address, Bytes, U256},
   alloy_rpc_types::{BlockId, TransactionReceipt, TransactionRequest},
   alloy_signer::SignerSync,
   types::ChainId,
};
use zeus_wallet::SecureKey;

pub(crate) fn signed_7702_authorization(
   signer: &SecureKey,
   chain_id: u64,
   delegate_to: Address,
   auth_nonce: u64,
) -> Result<SignedAuthorization, anyhow::Error> {
   let auth = Authorization {
      chain_id: U256::from(chain_id),
      address: delegate_to,
      nonce: auth_nonce,
   };
   let signature = signer.to_signer().sign_hash_sync(&auth.signature_hash())?;
   Ok(auth.into_signed(signature))
}

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

   SHARED_GUI.write(|gui| {
      gui.loading_window.open("Wait while magic happens");
      gui.request_repaint();
   });

   let mut tx_analysis = if let Some(analysis) = tx_analysis {
      analysis
   } else {
      let simulated = simulate_and_diff(
         ctx.clone(),
         chain,
         from,
         interact_to,
         call_data.clone(),
         value,
         authorization_list.clone(),
      )
      .await?;

      let mut analysis = TransactionAnalysis::new(
         ctx.clone(),
         chain.id(),
         from,
         interact_to,
         Some(simulated.contract_interact),
         call_data.clone(),
         value,
         simulated.logs,
         simulated.sim_res.tx_gas_used(),
         simulated.balance_before,
         simulated.balance_after,
         authorization_list.clone(),
      )
      .await?;
      analysis.set_diffs(simulated.balance_diff, simulated.approval_diff);
      analysis
   };

   let involved_currencies = tx_analysis.involved_currencies();
   let price_manager = ctx.price_manager();
   let pool_manager = ctx.pool_manager();

   let tokens = involved_currencies
      .iter()
      .map(|c| c.to_erc20().into_owned())
      .collect::<Vec<_>>();

   let time = std::time::Instant::now();

   if let Err(e) = price_manager
      .calculate_prices(ctx.clone(), chain.id(), pool_manager, tokens)
      .await
   {
      tracing::error!("Error updating prices: {:?}", e);
   }

   tracing::info!(
      "Updated prices in {} ms",
      time.elapsed().as_millis()
   );

   tx_analysis.refresh_usd(&ctx);

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
      logs.clone(),
      receipt.gas_used,
      tx_analysis.eth_balance_before,
      balance_after,
      authorization_list,
   )
   .await?;

   match diffs_from_receipt(
      ctx.clone(),
      chain.id(),
      from,
      interact_to,
      &tx_analysis.call_data,
      &logs,
      tx_block,
      balance_after,
   )
   .await
   {
      Ok((balance, approval)) if !balance.is_empty() || !approval.is_empty() => {
         new_tx_analysis.set_diffs(balance, approval);
      }
      other => {
         if let Err(e) = other {
            tracing::warn!("receipt diffs failed: {:?}", e);
         }
         new_tx_analysis.set_diffs(
            tx_analysis.balance_diff.clone(),
            tx_analysis.approval_diff.clone(),
         );
      }
   }

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

   let signed_authorization =
      signed_7702_authorization(&wallet, chain.id(), delegate_to, auth_nonce)?;

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
