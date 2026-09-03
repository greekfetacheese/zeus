//! Private (zk → zk) Railgun transfer execution

use std::time::{Duration, Instant};
use tokio::time::sleep;

use anyhow::anyhow;
use tracing::{error, info};

use zeus_eth::{
   alloy_primitives::{Address, U256},
   alloy_provider::Provider,
   alloy_rpc_types::{BlockId, Log},
   currency::{Currency, ERC20Token},
   revm_utils::{ForkFactory, Host, new_evm},
   types::ChainId,
   utils::NumericValue,
};
use zeus_railgun::{
   PrivateHistoryKind, RailgunAddress, RailgunSigner,
   caip::AssetId,
   encode_history_memo,
   rand::SeedableRng,
   rand_chacha::ChaCha12Rng,
   transact::{NoteSelectionMode, TransactionBuilder},
};

use crate::{
   core::{
      DecodedEvent, PrivateTransferParams, TransactionAnalysis, TransactionRich, TxParams, ZeusCtx,
      send_tx,
   },
   gui::{SHARED_GUI, ui::NotificationType},
   utils::{
      RT, TimeStamp, estimate_tx_cost,
      simulate::{
         fetch_accounts_info, fetch_storage_for_railgun, railgun_common_accounts,
         simulate_transaction,
      },
      state::get_base_fee,
   },
};

/// Private transfer of notes from the current wallet's 0zk address to another 0zk address.
pub async fn private_transfer(
   ctx: ZeusCtx,
   chain: ChainId,
   currency: Currency,
   amount: NumericValue,
   from: Address,
   recipient_zk: String,
   memo: String,
) -> Result<(), anyhow::Error> {
   if !ctx.railgun_is_supported(chain) {
      return Err(anyhow!(
         "Railgun is not supported for the {} network",
         chain.name()
      ));
   }

   if !ctx.is_railgun_enabled(chain.id()) {
      return Err(anyhow!(
         "Railgun is disabled. Enable it in Settings/Railgun."
      ));
   }

   if !currency.is_erc20() {
      return Err(anyhow!(
         "Private transfer requires an ERC-20 asset (use WETH for native-equivalent)"
      ));
   }

   let recipient = match RailgunAddress::from_zk_address(recipient_zk.trim()) {
      Ok(addr) => addr,
      Err(e) => return Err(anyhow!("Invalid Railgun Address {}", e)),
   };

   let wallet = ctx.get_current_wallet();
   if !wallet.can_derive_zk_address() {
      return Err(anyhow!(
         "Current wallet cannot derive a Railgun address (imported wallets without seedphrase are not supported)"
      ));
   }
   let seed = wallet.seed()?;
   let railgun_signer = RailgunSigner::from_seed(&seed, 0, chain.id())?;

   SHARED_GUI.write(|gui| {
      gui.loading_window.open("Preparing private transfer…");
      gui.request_repaint();
   });

   if let Err(e) = ctx.sync_railgun(chain.id(), false).await {
      let is_invalid_root = ctx.read(|ctx| ctx.railgun_status.is_error_invalid_root(chain.id()));
      if is_invalid_root {
         let ctx = ctx.clone();
         RT.spawn(async move {
            sleep(Duration::from_secs(1)).await;
            match ctx.resync_railgun(chain.id()).await {
               Ok(_) => {
                  info!(
                     "Railgun resynced to valid root for chain {}",
                     chain.id()
                  );
               }
               Err(e) => error!("Error syncing Railgun: {:?}", e),
            }
         });

         return Err(anyhow!(
            "Railgun state is corrupted (Invalid root), resync has started"
         ));
      }

      return Err(anyhow!("Error syncing Railgun: {:?}", e));
   }

   let token = currency.to_erc20().into_owned();
   let asset = AssetId::Erc20(token.address);
   let amount_u128: u128 = amount
      .wei()
      .try_into()
      .map_err(|_| anyhow!("Amount too large for private transfer"))?;

   let amount_usd = ctx.get_token_value_for_amount(amount.f64(), &token);
   let transfer_params = PrivateTransferParams {
      chain: chain.id(),
      recipient: recipient.address.clone(),
      asset,
      erc20: Some(token.clone()),
      amount_wei: amount.wei(),
      amount: Some(amount.clone()),
      amount_usd: Some(amount_usd),
   };

   let tx = TransactionBuilder::new()
      .transfer(
         railgun_signer.clone(),
         recipient,
         asset,
         amount_u128,
         memo.trim(),
      )
      .with_change_memo(&encode_history_memo(
         PrivateHistoryKind::Send,
         memo.trim(),
      ));

   exec_private_transfer(
      ctx,
      chain,
      railgun_signer,
      from,
      token,
      tx,
      transfer_params,
   )
   .await
}

/// Private self-transfer that merges small UTXO notes into one larger note.
///
/// Uses [`NoteSelectionMode::SmallestFirst`] so the transfer amount from
/// [`zeus_railgun::transact::suggest_merge`] spends the intended dust pack
/// even when larger notes exist.
pub async fn private_merge_notes(
   ctx: ZeusCtx,
   chain: ChainId,
   currency: Currency,
   amount: NumericValue,
   from: Address,
) -> Result<(), anyhow::Error> {
   if !ctx.railgun_is_supported(chain) {
      return Err(anyhow!(
         "Railgun is not supported for the {} network",
         chain.name()
      ));
   }

   if !ctx.is_railgun_enabled(chain.id()) {
      return Err(anyhow!(
         "Railgun is disabled. Enable it in Settings/Railgun."
      ));
   }

   if !currency.is_erc20() {
      return Err(anyhow!(
         "Merge notes requires an ERC-20 asset (use WETH for native-equivalent)"
      ));
   }

   let wallet = ctx.get_current_wallet();
   if !wallet.can_derive_zk_address() {
      return Err(anyhow!(
         "Current wallet cannot derive a Railgun address (imported wallets without seedphrase are not supported)"
      ));
   }
   let seed = wallet.seed()?;
   let railgun_signer = RailgunSigner::from_seed(&seed, 0, chain.id())?;
   let self_zk = railgun_signer.address().clone();

   SHARED_GUI.write(|gui| {
      gui.loading_window.open("Preparing note merge…");
      gui.request_repaint();
   });

   if let Err(e) = ctx.sync_railgun(chain.id(), false).await {
      let is_invalid_root = ctx.read(|ctx| ctx.railgun_status.is_error_invalid_root(chain.id()));
      if is_invalid_root {
         let ctx = ctx.clone();
         RT.spawn(async move {
            sleep(Duration::from_secs(1)).await;
            match ctx.resync_railgun(chain.id()).await {
               Ok(_) => {
                  info!(
                     "Railgun resynced to valid root for chain {}",
                     chain.id()
                  );
               }
               Err(e) => error!("Error syncing Railgun: {:?}", e),
            }
         });

         return Err(anyhow!(
            "Railgun state is corrupted (Invalid root), resync has started"
         ));
      }

      return Err(anyhow!("Error syncing Railgun: {:?}", e));
   }

   let token = currency.to_erc20().into_owned();
   let asset = AssetId::Erc20(token.address);
   let amount_u128: u128 = amount
      .wei()
      .try_into()
      .map_err(|_| anyhow!("Amount too large for note merge"))?;

   let amount_usd = ctx.get_token_value_for_amount(amount.f64(), &token);
   let transfer_params = PrivateTransferParams {
      chain: chain.id(),
      recipient: self_zk.address.clone(),
      asset,
      erc20: Some(token.clone()),
      amount_wei: amount.wei(),
      amount: Some(amount.clone()),
      amount_usd: Some(amount_usd),
   };

   let tx = TransactionBuilder::new()
      .with_selection_mode(NoteSelectionMode::SmallestFirst)
      .transfer(
         railgun_signer.clone(),
         self_zk,
         asset,
         amount_u128,
         "merge notes",
      );

   exec_private_transfer(
      ctx,
      chain,
      railgun_signer,
      from,
      token,
      tx,
      transfer_params,
   )
   .await
}

async fn exec_private_transfer(
   ctx: ZeusCtx,
   chain: ChainId,
   railgun_signer: RailgunSigner,
   from: Address,
   token: ERC20Token,
   tx: TransactionBuilder,
   transfer_params: PrivateTransferParams,
) -> Result<(), anyhow::Error> {
   let mut railgun_provider = ctx.get_railgun_provider(chain.id(), false).await?;

   let zeus_client = ctx.get_zeus_client();
   let last_synced_block_opt =
      railgun_provider.account_synced_block(railgun_signer.address()).await;
   let last_synced_block = match last_synced_block_opt {
      Some(block) => block,
      None => {
         return Err(anyhow!(
            "Account is not synced for signer {}",
            railgun_signer.address().address
         ));
      }
   };

   let eth_balance_before_fut = zeus_client.request(chain.id(), |client| async move {
      client
         .get_balance(from)
         .block_id(BlockId::latest())
         .await
         .map_err(|e| anyhow!("{:?}", e))
   });

   let fork_block_res = zeus_client
      .request(chain.id(), |client| async move {
         client
            .get_block(BlockId::number(last_synced_block))
            .await
            .map_err(|e| anyhow!("{:?}", e))
      })
      .await?;

   let fork_block = if let Some(fork_block) = fork_block_res {
      fork_block
   } else {
      return Err(anyhow!(
         "No block found, this is usally a provider issue"
      ));
   };

   let fork_block_id = BlockId::number(fork_block.header.number);
   let client = ctx.get_client(chain.id()).await?;
   let railgun_address = railgun_provider.railgun_address();

   let mut accounts = Vec::new();
   accounts.push(from);
   accounts.push(token.address);
   accounts.push(fork_block.header.beneficiary);
   accounts.push(railgun_address);

   let common_accounts = railgun_common_accounts(chain.id());
   accounts.extend(common_accounts);

   let accounts_info_fut = fetch_accounts_info(ctx.clone(), chain.id(), fork_block_id, accounts);

   let storage_info_fut = fetch_storage_for_railgun(
      ctx.clone(),
      chain.id(),
      fork_block_id,
      railgun_address,
   );

   let proved = {
      let mut rng = ChaCha12Rng::from_os_rng();
      railgun_provider.build(tx, &mut rng).await?
   };

   SHARED_GUI.write(|gui| {
      gui.loading_window.open("Simulating Transaction…");
      gui.request_repaint();
   });

   let calldata = proved.tx_data.data.clone();
   let interact_to = proved.tx_data.to;
   let value = proved.tx_data.value;

   let fork_client = ctx.get_client(chain.id()).await?;

   let mut factory =
      ForkFactory::new_sandbox_factory(fork_client, chain.id(), None, Some(fork_block_id));

   let accounts_info = accounts_info_fut.await;
   let storage_info = storage_info_fut.await;

   for info in accounts_info {
      factory.insert_account_info(info.address, info.info);
   }

   for info in storage_info {
      match factory.insert_account_storage(info.address, info.slot, info.value) {
         Ok(_) => {}
         Err(e) => tracing::error!("Failed to insert account storage: {:?}", e),
      }
   }

   let fork_db = factory.new_sandbox_fork();

   let eth_balance_after;
   let sim_res;
   {
      let mut evm = new_evm(chain, Some(&fork_block), fork_db.clone());
      evm.tx.gas_limit = 30_000_000;

      let time = Instant::now();

      sim_res = match simulate_transaction(
         &mut evm,
         from,
         interact_to,
         calldata.clone(),
         value,
         vec![],
      ) {
         Ok(res) => res,
         Err(e) => {
            let is_already_spent = e.to_string().contains("note already spent");
            if is_already_spent {
               let ctx_clone = ctx.clone();
               RT.spawn(async move {
                  sleep(Duration::from_secs(1)).await;
                  match ctx_clone.resync_railgun(chain.id()).await {
                     Ok(_) => {
                        tracing::info!(
                           "Railgun resynced to valid root for chain {}",
                           chain.id()
                        );
                     }
                     Err(e) => tracing::error!("Error syncing Railgun: {:?}", e),
                  }
               });
            }

            return Err(anyhow!("Simulation failed: {:?}", e));
         }
      };

      tracing::info!(
         "Simulate Private Transfer took {} ms, gas={}, logs={}",
         time.elapsed().as_millis(),
         sim_res.tx_gas_used(),
         sim_res.clone().into_logs().len()
      );

      let state = evm.balance(from);
      eth_balance_after = if let Some(state) = state {
         state.data
      } else {
         U256::ZERO
      };
   }

   let logs = sim_res.clone().into_logs();
   let eth_balance_before = eth_balance_before_fut.await?;
   let contract_interact = Some(true);
   let auth_list = Vec::new();

   let mut tx_analysis = TransactionAnalysis::new(
      ctx.clone(),
      chain.id(),
      from,
      interact_to,
      contract_interact,
      calldata.clone(),
      value,
      logs,
      sim_res.tx_gas_used(),
      eth_balance_before,
      eth_balance_after,
      auth_list.clone(),
   )
   .await?;

   let main_event = DecodedEvent::PrivateTransfer(transfer_params);
   tx_analysis.set_main_event(main_event.clone());

   let priority_fee = ctx.get_priority_fee(chain.id()).unwrap_or_default();
   let dapp = "Railgun".to_string();
   let mev_protect = false;
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

   let mut confirmed = None;
   loop {
      tokio::time::sleep(std::time::Duration::from_millis(50)).await;

      SHARED_GUI.read(|gui| {
         confirmed = gui.tx_confirmation_window.get_confirmed_or_rejected();
      });

      if confirmed.is_some() {
         SHARED_GUI.write(|gui| {
            ctx.write(|ctx| {
               gui.tx_confirmation_window.close(ctx);
            });
         });
         break;
      }
   }

   let confirmed = confirmed.unwrap();
   if !confirmed {
      return Err(anyhow!("Transaction rejected"));
   }

   SHARED_GUI.write(|gui| {
      gui.loading_window.open("Wait while magic happens");
      gui.request_repaint();
   });

   let z_client = ctx.get_zeus_client();
   let signer = ctx.get_wallet(from).ok_or(anyhow!("Wallet not found"))?.key;
   let gas_used = tx_analysis.gas_used;

   let fee = SHARED_GUI.read(|gui| gui.tx_confirmation_window.get_priority_fee());
   let gas_limit = SHARED_GUI.read(|gui| gui.tx_confirmation_window.get_gas_limit());

   let priority_fee = if fee.is_zero() {
      ctx.get_priority_fee(chain.id()).unwrap_or_default()
   } else {
      fee
   };

   let base_fee = get_base_fee(ctx.clone(), chain.id()).await?;
   let nonce = z_client
      .request(chain.id(), |client| async move {
         client.get_transaction_count(from).await.map_err(|e| anyhow!("{:?}", e))
      })
      .await?;

   let tx_params = TxParams::new(
      signer,
      interact_to,
      nonce,
      value,
      chain,
      priority_fee.wei(),
      base_fee.next,
      calldata.clone(),
      gas_used,
      gas_limit,
      vec![],
   );

   let event_name = main_event.name();
   let nofitification = NotificationType::from_main_event(main_event.clone());

   SHARED_GUI.write(|gui| {
      gui.notification.open_with_spinner(event_name, nofitification);
      gui.loading_window.reset();
      gui.request_repaint();
   });

   let receipt = send_tx(client, tx_params).await?;

   let logs: Vec<Log> = receipt.logs().to_vec();
   let logs = logs.iter().map(|l| l.clone().into_inner()).collect::<Vec<_>>();

   let eth_balance_after = z_client
      .request(chain.id(), |client| async move {
         client.get_balance(from).await.map_err(|e| anyhow!("{:?}", e))
      })
      .await?;

   let mut new_tx_analysis = TransactionAnalysis::new(
      ctx.clone(),
      chain.id(),
      from,
      interact_to,
      contract_interact,
      calldata.clone(),
      value,
      logs,
      receipt.gas_used,
      eth_balance_before,
      eth_balance_after,
      vec![],
   )
   .await?;

   // Keep intent-based main event, Transact logs are not public ERC-20 transfers.
   let new_main_event = main_event;
   new_tx_analysis.set_main_event(new_main_event.clone());

   let main_event_name = if new_main_event.is_known() {
      new_main_event.name()
   } else {
      "Transaction successful".to_string()
   };

   let nofitification = NotificationType::from_main_event(new_main_event.clone());

   let (tx_cost, tx_cost_usd) = ctx.write(|ctx| {
      estimate_tx_cost(
         ctx,
         chain.id(),
         receipt.gas_used,
         priority_fee.wei(),
      )
   });

   let eth_received_usd = ctx.write(|ctx| new_tx_analysis.eth_received_usd(ctx));
   let timestamp = TimeStamp::now_as_secs()?;

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
      main_event: new_main_event,
      clear_display: None,
   };

   let ctx_clone = ctx.clone();
   let tx = tx_rich.clone();
   RT.spawn_blocking(move || {
      ctx_clone.add_transaction(chain.id(), from, tx);
   });

   RT.spawn(async move {
      post_private_transfer_sync(ctx, chain).await;
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

   Ok(())
}

async fn post_private_transfer_sync(ctx: ZeusCtx, chain: ChainId) {
   ctx.write(|ctx| {
      ctx.railgun_status.set_op_in_progress(chain.id(), true);
   });

   let chain_id = chain.id();

   match ctx.sync_railgun(chain_id, false).await {
      Ok(_) => {}
      Err(e) => error!("Error syncing Railgun: {:?}", e),
   }

   let wallets = ctx.get_all_wallets_info();

   for wallet in wallets {
      ctx.update_private_data(chain_id, wallet.address).await;
   }

   ctx.write(|ctx| {
      ctx.railgun_status.set_op_in_progress(chain.id(), false);
   });
}
