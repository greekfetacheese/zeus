use crate::utils::state::get_base_fee;
use crate::core::{TransactionAnalysis, TransactionRich, ZeusCtx, client::TIMEOUT_FOR_SENDING_TX};
use alloy_eips::eip7702::{Authorization, SignedAuthorization};
use anyhow::anyhow;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::gui::{SHARED_GUI, ui::NotificationType};
use crate::utils::{
   RT, TimeStamp, estimate_tx_cost,
   simulate::{fetch_accounts_info, simulate_transaction},
};
use zeus_eth::{
   alloy_contract::private::Provider,
   alloy_network::{Ethereum, TransactionBuilder, TransactionBuilder7702},
   alloy_primitives::{Address, Bytes, U256},
   alloy_rpc_types::{BlockId, Log, TransactionReceipt, TransactionRequest},
   alloy_signer::SignerSync,
   revm_utils::{ForkFactory, Host, new_evm},
   types::ChainId,
   utils::{NumericValue, SecureSigner},
};

#[derive(Clone)]
pub struct TxParams {
   pub signer: SecureSigner,
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
      signer: SecureSigner,
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

   pub fn gas_cost(&self) -> U256 {
      if self.chain.is_ethereum() || self.chain.is_optimism() || self.chain.is_base() {
         U256::from(U256::from(self.gas_used) * self.max_fee_per_gas())
      } else {
         U256::from(self.gas_used * self.base_fee)
      }
   }

   pub fn sufficient_balance(&self, balance: NumericValue) -> Result<(), anyhow::Error> {
      let coin = self.chain.coin_symbol();
      let cost_in_eth = self.gas_cost();
      let cost = NumericValue::format_wei(cost_in_eth, 18);

      if balance.wei() < cost.wei() {
         return Err(anyhow!(
            "Insufficient balance to cover gas fees, need at least {} {} but you have {} {}",
            cost.formatted(),
            coin,
            balance.formatted(),
            coin
         ));
      }

      Ok(())
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

      let block = if let Some(block) = block {
         block
      } else {
         return Err(anyhow!(
            "No block found, this is usally a provider issue"
         ));
      };

      let block_id = BlockId::number(block.header.number);

      let mut accounts = Vec::new();
      accounts.push(from);
      accounts.push(interact_to);
      accounts.push(block.header.beneficiary);

      let accounts_info = fetch_accounts_info(ctx.clone(), chain.id(), block_id, accounts).await;
      let fork_client = ctx.get_client(chain.id()).await?;
      let mut factory =
         ForkFactory::new_sandbox_factory(fork_client, chain.id(), None, Some(block_id));

      for info in accounts_info {
         factory.insert_account_info(info.address, info.info);
      }

      let fork_db = factory.new_sandbox_fork();

      let bytecode_fut = client.request(chain.id(), |client| async move {
         client.get_code_at(interact_to).await.map_err(|e| anyhow!("{:?}", e))
      });

      let balance_after;
      let sim_res;

      {
         let mut evm = new_evm(chain, Some(&block), fork_db);

         let time = std::time::Instant::now();
         sim_res = simulate_transaction(
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

      TransactionAnalysis::new(
         ctx.clone(),
         chain.id(),
         from,
         interact_to,
         contract_interact,
         call_data.clone(),
         value,
         logs,
         sim_res.gas_used(),
         balance_before,
         balance_after,
         authorization_list.clone(),
      )
      .await?
   };

   let priority_fee = ctx.get_priority_fee(chain.id()).unwrap_or_default();

   SHARED_GUI.write(|gui| {
      gui.tx_confirmation_window.open(
         ctx.clone(),
         dapp,
         chain,
         tx_analysis.clone(),
         priority_fee.f64().to_string(),
         mev_protect,
      );
      gui.loading_window.reset();
      gui.request_repaint();
   });

   // wait for the user to confirm or reject the transaction
   let mut confirmed = None;
   loop {
      tokio::time::sleep(std::time::Duration::from_millis(50)).await;

      SHARED_GUI.read(|gui| {
         confirmed = gui.tx_confirmation_window.get_confirmed_or_rejected();
      });

      if confirmed.is_some() {
         SHARED_GUI.write(|gui| {
            gui.tx_confirmation_window.close(ctx.clone());
         });
         break;
      }
   }

   let confirmed = confirmed.unwrap();
   if !confirmed {
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

   let fee = SHARED_GUI.read(|gui| gui.tx_confirmation_window.get_priority_fee());
   let gas_limit = SHARED_GUI.read(|gui| gui.tx_confirmation_window.get_gas_limit());

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

   let z_client = ctx.get_zeus_client();
   let rpc = z_client.get_best_rpc(chain.id()).ok_or(anyhow!("No available RPC found"))?;
   let tx_client = z_client.connect_with_timeout(&rpc, TIMEOUT_FOR_SENDING_TX).await?;

   // If needed use MEV protect client, if not found prompt the user to continue
   let new_client = if chain.is_ethereum() && mev_protect {
      let mev_client_res = ctx.get_mev_protect_client(chain.id()).await;

      if mev_client_res.is_err() {
         SHARED_GUI.write(|gui| {
            let msg2 = "Continue without MEV protection?";
            gui.confirm_window.open("Error while connecting to MEV protect RPC");
            gui.confirm_window.set_msg2(msg2);
            gui.request_repaint();
         });

         // wait for the user to confirm or reject the transaction
         let mut confirmed = None;
         loop {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;

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
         tx_client
      } else {
         mev_client_res.unwrap()
      }
   } else {
      tx_client.clone()
   };

   let receipt = send_tx(new_client, tx_params).await?;

   let logs: Vec<Log> = receipt.logs().to_vec();
   let logs = logs.iter().map(|l| l.clone().into_inner()).collect::<Vec<_>>();

   let timestamp = TimeStamp::now_as_secs();

   let balance_after = client
      .request(chain.id(), |client| async move {
         client.get_balance(from).await.map_err(|e| anyhow!("{:?}", e))
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

   let main_event = new_tx_analysis.infer_main_event(ctx.clone(), chain.id());
   let main_event_name = if main_event.is_known() {
      main_event.name()
   } else {
      "Transaction successful".to_string()
   };

   let nofitification = NotificationType::from_main_event(main_event.clone());

   let (tx_cost, tx_cost_usd) = estimate_tx_cost(
      ctx.clone(),
      chain.id(),
      receipt.gas_used,
      priority_fee.wei(),
   );

   // Remove the redunant main event
   new_tx_analysis.remove_main_event();

   let tx_rich = TransactionRich {
      tx_type: receipt.transaction_type(),
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
      main_event,
   };

   let ctx_clone = ctx.clone();
   let tx = tx_rich.clone();
   RT.spawn_blocking(move || {
      ctx_clone.write(|ctx| ctx.tx_db.add_tx(chain.id(), from, tx));
      ctx_clone.save_tx_db();
   });

   if !receipt.status() {
      return Err(anyhow!("Transaction Failed"));
   }

   let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
   let finish = now + 6;

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
         let nonce = client.get_transaction_count(address).await.map_err(|e| anyhow!("{:?}", e));
         Ok(nonce?)
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

   let dapp = String::new();
   let tx_analysis = None;
   let mev_protect = false;
   let call_data = Bytes::default();
   let value = U256::ZERO;

   let (receipt, _) = send_transaction(
      ctx.clone(),
      dapp,
      tx_analysis,
      chain,
      mev_protect,
      from,
      from,
      call_data,
      value,
      vec![signed_authorization],
   )
   .await?;

   if !receipt.status() {
      return Err(anyhow!("Transaction Failed"));
   }

   if delegate_to.is_zero() {
      ctx.write(|ctx| {
         ctx.smart_accounts.remove(chain.id(), from);
      });
   } else {
      ctx.write(|ctx| {
         ctx.smart_accounts.add(chain.id(), from, delegate_to);
      });
   }

   ctx.save_smart_accounts();

   Ok(())
}

async fn send_tx<P>(client: P, params: TxParams) -> Result<TransactionReceipt, anyhow::Error>
where
   P: Provider<Ethereum> + Clone + 'static,
{
   let tx = make_tx_request(params.clone());
   let wallet = params.signer.to_wallet();
   let tx_envelope = tx.clone().build(&wallet).await?;
   drop(wallet);

   let time = std::time::Instant::now();
   let receipt = client
      .send_tx_envelope(tx_envelope)
      .await?
      .with_timeout(Some(Duration::from_secs(TIMEOUT_FOR_SENDING_TX)))
      .get_receipt()
      .await?;
   tracing::info!(
      "Time take to send tx: {:?}secs",
      time.elapsed().as_secs_f32()
   );

   Ok(receipt)
}

fn make_tx_request(params: TxParams) -> TransactionRequest {
   if params.chain.is_ethereum() || params.chain.is_optimism() || params.chain.is_base() {
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
         tx.set_authorization_list(params.authorization_list);
      }

      tx
   } else {
      // Legacy
      TransactionRequest::default()
         .with_from(params.signer.address())
         .with_to(params.transcact_to)
         .with_value(params.value)
         .with_nonce(params.nonce)
         .with_input(params.call_data)
         .with_gas_limit(params.gas_limit)
         .with_gas_price(params.base_fee.into())
   }
}
