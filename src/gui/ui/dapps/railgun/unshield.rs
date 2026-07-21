//! Unshield execution paths: paymaster (ERC-4337) broadcast and emergency self-broadcast.

use std::{
   str::FromStr,
   time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::time::sleep;

use alloy_consensus::TxType;
use alloy_signer_local::PrivateKeySigner;
use anyhow::anyhow;
use tracing::{error, info};
use userop_kit::{
   bundler::{Bundler, pimlico::PimlicoBundler},
   smart_account::simple_smart_account::{SIMPLE_7702_ACCOUNT, SimpleSmartAccount},
};
use zeus_eth::{
   alloy_primitives::{Address, Bytes, KECCAK256_EMPTY, U256, keccak256},
   alloy_provider::Provider,
   alloy_rpc_types::BlockId,
   currency::{Currency, ERC20Token},
   revm_utils::{
      ForkFactory, Host, new_evm,
      revm::state::{AccountInfo, Bytecode},
   },
   types::ChainId,
   utils::{NumericValue, address_book, client::RpcClient},
};
use zeus_railgun::{
   RailgunProvider, RailgunSigner, adapter_data::decode_fee_from_paymaster_data, caip::AssetId,
   rand::SeedableRng, rand_chacha::ChaCha12Rng, transact::TransactionBuilder,
};

use crate::{
   core::{
      DecodedEvent, TransactionAnalysis, TransactionRich, UnshieldParams, ZeusCtx, send_transaction,
   },
   gui::{SHARED_GUI, ui::NotificationType},
   utils::{
      RT, TimeStamp, estimate_tx_cost,
      simulate::{
         fetch_accounts_info, fetch_storage_for_railgun, railgun_common_accounts,
         simulate_transaction,
      },
   },
};

/// Default public Pimlico bundler RPC for a chain.
pub fn default_bundler_url(chain_id: u64) -> String {
   format!("https://public.pimlico.io/v2/{}/rpc", chain_id)
}

/// EIP-7702 designated delegated code: `0xef0100 || implementation`.
fn eip7702_delegated_code(implementation: Address) -> Bytes {
   let mut code = Vec::with_capacity(23);
   code.extend_from_slice(&[0xef, 0x01, 0x00]);
   code.extend_from_slice(implementation.as_slice());
   code.into()
}

/// Unshield private notes to a public address.
///
/// - `self_broadcast = false` (default): Railgun privacy paymaster + bundler UserOp.
/// - `self_broadcast = true`: emergency path — submit proved `transact` from the user's EOA
///   (links submitter to recipient; breaks anonymity).
pub async fn unshield(
   ctx: ZeusCtx,
   chain: ChainId,
   currency: Currency,
   amount: NumericValue,
   from: Address,
   recipient: String,
   self_broadcast: bool,
   bundler_url: String,
) -> Result<(), anyhow::Error> {
   if !ctx.railgun_is_supported(chain) {
      return Err(anyhow!(
         "Railgun is not supported for the {} network",
         chain.name()
      ));
   }

   if !currency.is_erc20() {
      return Err(anyhow!(
         "Unshield requires an ERC-20 asset (use WETH for native-equivalent)"
      ));
   }

   let recipient = Address::from_str(recipient.trim())
      .map_err(|e| anyhow!("Invalid recipient address: {}", e))?;

   let wallet = ctx.get_current_wallet();
   if !wallet.can_derive_zk_address() {
      return Err(anyhow!(
         "Current wallet cannot derive a Railgun address (imported wallets without seedphrease are not supported)"
      ));
   }
   let seed = wallet.seed()?;
   let railgun_signer = RailgunSigner::from_seed(&seed, 0, chain.id())?;

   let mut railgun_provider = ctx.get_railgun_provider(chain.id()).await?;
   if railgun_provider.chain_id() != chain.id() {
      return Err(anyhow!(
         "Railgun provider chain id {} does not match the current chain id {}",
         railgun_provider.chain_id(),
         chain.id()
      ));
   }

   SHARED_GUI.write(|gui| {
      gui.loading_window.open("Preparing unshield…");
      gui.request_repaint();
   });

   railgun_provider.register(railgun_signer.clone()).await?;

   // Ensure notes are current before proving.
   if let Err(e) = railgun_provider.sync().await {
      error!("Railgun sync before unshield failed: {:?}", e);
      // Continue — notes may already be available from a prior sync.
   }

   let token = currency.to_erc20().into_owned();
   let asset = AssetId::Erc20(token.address);
   let amount_u128: u128 =
      amount.wei().try_into().map_err(|_| anyhow!("Amount too large for unshield"))?;

   let tx = TransactionBuilder::new().unshield(
      railgun_signer.clone(),
      recipient,
      asset,
      amount_u128,
   )?;

   if self_broadcast {
      unshield_self_broadcast(ctx, chain, from, token, railgun_provider, tx).await
   } else {
      unshield_via_paymaster(
         ctx,
         chain,
         from,
         token,
         railgun_provider,
         railgun_signer,
         tx,
         bundler_url,
      )
      .await
   }
}

async fn unshield_self_broadcast(
   ctx: ZeusCtx,
   chain: ChainId,
   from: Address,
   token: ERC20Token,
   mut railgun_provider: RailgunProvider<RpcClient>,
   tx: TransactionBuilder,
) -> Result<(), anyhow::Error> {
   SHARED_GUI.write(|gui| {
      gui.loading_window.open("Generating proof…");
      gui.request_repaint();
   });

   let proved = {
      let mut rng = ChaCha12Rng::from_os_rng();
      railgun_provider.build(tx, &mut rng).await?
   };

   let calldata = proved.tx_data.data.clone();
   let interact_to = proved.tx_data.to;
   let value = proved.tx_data.value;

   let dapp = "Railgun Unshield".to_string();
   let mev_protect = false;
   let auth_list = Vec::new();

   let (_, _) = send_transaction(
      ctx.clone(),
      dapp,
      None,
      chain,
      mev_protect,
      from,
      interact_to,
      calldata,
      value,
      auth_list,
   )
   .await?;

   let railgun_provider = railgun_provider.clone();
   RT.spawn(async move {
      post_unshield_sync(ctx, chain, from, token, railgun_provider, true).await;
   });

   Ok(())
}

async fn unshield_via_paymaster(
   ctx: ZeusCtx,
   chain: ChainId,
   from: Address,
   token: ERC20Token,
   mut railgun_provider: RailgunProvider<RpcClient>,
   railgun_signer: RailgunSigner,
   tx: TransactionBuilder,
   bundler_url: String,
) -> Result<(), anyhow::Error> {
   let chain_config = railgun_provider.chain_config();

   if chain_config.privacy_paymaster.is_none() || chain_config.railgun_fee_adapter.is_none() {
      return Err(anyhow!(
         "Privacy paymaster is not configured for this chain"
      ));
   }

   let zeus_client = ctx.get_zeus_client();
   let last_synced_block = railgun_provider.global_synced_block().await;

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

   // Railgun privacy paymaster only accepts WETH
   let fee_token = ERC20Token::wrapped_native_token(chain.id());

   // handleOps caller = any funded EOA (bundler). Use a random wallet for the sim
   // since in the fork enviroment we dont check for gas
   let bundle_caller = PrivateKeySigner::random().address();

   // Ephemeral smart-account owner for the UserOp.
   // Unshield recipient is independent of this key — does NOT affect private note selection.
   let sa_key = PrivateKeySigner::random();
   let smart_account = SimpleSmartAccount::new(sa_key.address(), chain.id(), client);

   let entry_point = address_book::entry_point(chain.id())?;

   // Prefetch accounts and storage for the sim
   let mut accounts = Vec::new();
   accounts.push(from);
   accounts.push(entry_point);
   accounts.push(SIMPLE_7702_ACCOUNT);
   accounts.push(fork_block.header.beneficiary);

   if let Some(pm) = chain_config.privacy_paymaster {
      accounts.push(pm);
   }

   if let Some(adapter) = chain_config.railgun_fee_adapter {
      accounts.push(adapter);
   }

   accounts.push(railgun_provider.railgun_address());
   accounts.push(fee_token.address);

   let common_accounts = railgun_common_accounts(chain.id());
   accounts.extend(common_accounts);

   let accounts_info_fut = fetch_accounts_info(ctx.clone(), chain.id(), fork_block_id, accounts);

   let storage_info_fut = fetch_storage_for_railgun(
      ctx.clone(),
      chain.id(),
      fork_block_id,
      railgun_provider.railgun_address(),
   );

   const INITIAL_FEE_WEI: u128 = 100_000_000;

   let rg_addr = railgun_signer.address().clone();
   let bundler_url = if bundler_url.trim().is_empty() {
      default_bundler_url(chain.id())
   } else {
      bundler_url.trim().to_string()
   };

   let is_pimlico_bundler = bundler_url.contains("public.pimlico.io");
   let fee_token = if is_pimlico_bundler {
      fee_token
   } else {
      fee_token_selection(chain.id(), from).await?
   };

   let fee_asset = AssetId::Erc20(fee_token.address);
   let fee_token_balance = railgun_provider.balance_erc20(rg_addr.clone(), fee_asset).await;
   let fee_token_balance_fmt =
      NumericValue::format_wei(U256::from(fee_token_balance), fee_token.decimals);
   let min_fee_fmt = NumericValue::format_wei(U256::from(INITIAL_FEE_WEI), fee_token.decimals);

   if fee_token_balance < INITIAL_FEE_WEI {
      return Err(anyhow!(
         "Not enough private {} for the paymaster fee (have {} need at least {})",
         fee_token.symbol,
         fee_token_balance_fmt.abbreviated(),
         min_fee_fmt.abbreviated()
      ));
   }

   let parsed_url = bundler_url
      .parse()
      .map_err(|e| anyhow!("Invalid bundler URL '{}': {}", bundler_url, e))?;

   let bundler = PimlicoBundler::new(parsed_url);

   SHARED_GUI.write(|gui| {
      gui.loading_window.open("Generating proof…");
      gui.request_repaint();
   });

   let mut rng = ChaCha12Rng::from_os_rng();
   let signable = railgun_provider
      .prepare_userop(
         tx,
         &bundler,
         &smart_account,
         railgun_signer,
         fee_token.address,
         Vec::new(), // no post-unshield calls for v1
         &mut rng,
      )
      .await
      .map_err(|e| anyhow!("Failed to prepare UserOperation: {e}"))?;

   let signed = signable
      .sign(&sa_key)
      .await
      .map_err(|e| anyhow!("Failed to sign UserOperation: {}", e))?;

   SHARED_GUI.write(|gui| {
      gui.loading_window.open("Simulating Transaction…");
      gui.request_repaint();
   });

   let fork_client = ctx.get_client(chain.id()).await?;

   let mut factory =
      ForkFactory::new_sandbox_factory(fork_client, chain.id(), None, Some(fork_block_id));

   // Sa & bundler will always be empty so insert them here
   // to avoid rpc calls
   let sa_account = AccountInfo {
      balance: U256::ZERO,
      nonce: 0,
      code: None,
      account_id: None,
      code_hash: KECCAK256_EMPTY,
   };

   let bundler_account = AccountInfo {
      balance: U256::ZERO,
      nonce: 0,
      code: None,
      account_id: None,
      code_hash: KECCAK256_EMPTY,
   };

   factory.insert_account_info(bundle_caller, bundler_account);
   factory.insert_account_info(sa_key.address(), sa_account);

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

   // Force EIP-7702 delegation on the ephemeral smart-account sender.
   // Bundlers apply this via the outer type-4 authorization list before handleOps.
   {
      let code = eip7702_delegated_code(SIMPLE_7702_ACCOUNT);
      let code_hash = keccak256(&code);
      factory.insert_account_info(
         signed.user_op.sender,
         AccountInfo {
            balance: U256::ZERO,
            nonce: 0, // fresh random EOA
            code: Some(Bytecode::new_raw(code)),
            account_id: None,
            code_hash,
         },
      );
   }

   let fork_db = factory.new_sandbox_fork();

   let handle_ops_data = signed.encode_handle_ops(bundle_caller);
   let interact_to = signed.entry_point;
   let value = U256::ZERO;

   let eth_balance_after;
   let sim_res;
   {
      let mut evm = new_evm(chain, Some(&fork_block), fork_db.clone());
      evm.tx.gas_limit = 30_000_000;

      let time = Instant::now();
      sim_res = simulate_transaction(
         &mut evm,
         bundle_caller,
         interact_to,
         handle_ops_data.clone(),
         value,
         vec![],
      )?;
      tracing::info!(
         "Simulate handleOps took {} ms, gas={}, logs={}",
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

   let mut unshield_events = Vec::new();

   for log in &logs {
      if let Ok(params) = UnshieldParams::from_log(ctx.clone(), chain.id(), log).await {
         unshield_events.push(params);
      }
   }

   // Should not happen for a single unshield
   if unshield_events.len() > 1 {
      return Err(anyhow!("More than one Unshield event found"));
   }

   if unshield_events.is_empty() {
      return Err(anyhow!(
         "No Unshield event found in handleOps simulation ({} log(s))",
         logs.len()
      ));
   }

   let mut unshield_params = unshield_events[0].clone();

   // Broadcaster/paymaster fee = private fee note encoded in paymaster_data (wrapped base token).
   // Distinct from Unshield event `fee` (protocol 0.25% on the unshielded token).
   //
   // This is NOT a public ERC-20 transfer and is NOT deducted from the unshield amount
   // the recipient receives. It is a separate private transfer to the paymaster's 0zk,
   // funded from the user's private notes alongside the unshield spend.
   if let Some(pm_data) = signed.user_op.paymaster_data.as_ref() {
      match decode_fee_from_paymaster_data(pm_data.as_ref()) {
         Ok((fee_asset, fee_wei)) => {
            let fee_token = ctx.get_token(chain.id(), fee_asset).await.unwrap_or_else(|_| {
               let mut t = ERC20Token::wrapped_native_token(chain.id());
               t.address = fee_asset;
               t
            });
            let fee_amt = NumericValue::format_wei(U256::from(fee_wei), fee_token.decimals);
            let fee_usd = ctx.get_token_value_for_amount(fee_amt.f64(), &fee_token);
            unshield_params.broadcaster_fee = Some(fee_amt);
            unshield_params.broadcaster_fee_usd = Some(fee_usd);
         }
         Err(e) => {
            error!("Failed to decode paymaster fee from paymaster_data: {e}");
         }
      }
   }

   let eth_balance_before = eth_balance_before_fut.await?;

   let contract_interact = Some(true);
   let calldata = handle_ops_data;
   let auth_list = Vec::new();

   // TODO: Show the actual sender of the tx
   let sender = from;

   let mut tx_analysis = TransactionAnalysis::new(
      ctx.clone(),
      chain.id(),
      sender,
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

   let main_event = DecodedEvent::Unshield(unshield_params.clone());
   tx_analysis.set_main_event(main_event);

   // This tx is sponsored so the priority fee doesnt matter here
   let priority_fee = NumericValue::default();
   let dapp = "Railgun Unshield".to_string();
   let mev_protect = false;
   let sponsored = true;

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

   // wait for the user to confirm or reject the transaction
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
      gui.loading_window.open("Submitting unshield via bundler…");
      gui.request_repaint();
   });

   // Submit the UserOp tx
   let hash = bundler
      .send_user_operation(&signed)
      .await
      .map_err(|e| anyhow!("Bundler rejected UserOperation: {}", e))?;

   SHARED_GUI.write(|gui| {
      gui.loading_window.open("Waiting for bundler inclusion…");
      gui.request_repaint();
   });

   let receipt = bundler.wait_for_receipt(hash).await.map_err(|e| {
      anyhow!(
         "Timed out / failed waiting for UserOp receipt: {}",
         e
      )
   })?;

   if !receipt.success {
      return Err(anyhow!("Unshield UserOperation failed",));
   }

   let logs = receipt.logs.clone();
   let logs = logs.iter().map(|l| l.clone().into_inner()).collect::<Vec<_>>();
   let timestamp = TimeStamp::now_as_secs();

   let eth_balance_after = zeus_client
      .request(chain.id(), |client| async move {
         client.get_balance(from).await.map_err(|e| anyhow!("{:?}", e))
      })
      .await?;

   let sender = receipt.receipt.from;

   let mut new_tx_analysis = TransactionAnalysis::new(
      ctx.clone(),
      chain.id(),
      sender,
      interact_to,
      contract_interact,
      tx_analysis.call_data.clone(),
      tx_analysis.value,
      logs,
      receipt.receipt.gas_used,
      eth_balance_before,
      eth_balance_after,
      vec![],
   )
   .await?;

   let main_event = new_tx_analysis.infer_main_event(ctx.clone(), chain.id());
   let main_event_name = if main_event.is_known() {
      main_event.name()
   } else {
      "Transaction successful".to_string()
   };

   let nofitification = NotificationType::from_main_event(main_event.clone());

   let (tx_cost, tx_cost_usd) = ctx.write(|ctx| {
      estimate_tx_cost(
         ctx,
         chain.id(),
         receipt.receipt.gas_used,
         priority_fee.wei(),
      )
   });

   // Remove the redunant main event
   new_tx_analysis.remove_main_event();

   let eth_received_usd = ctx.write(|ctx| new_tx_analysis.eth_received_usd(ctx));

   let tx_rich = TransactionRich {
      tx_type: TxType::Eip7702,
      success: receipt.success,
      chain: chain.id(),
      block: receipt.receipt.block_number.unwrap_or_default(),
      timestamp,
      value_sent: new_tx_analysis.value_sent(),
      value_sent_usd: new_tx_analysis.value_sent_usd(ctx.clone()),
      eth_received: new_tx_analysis.eth_received(),
      eth_received_usd,
      tx_cost,
      tx_cost_usd,
      hash: receipt.receipt.transaction_hash,
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

   let railgun_provider = railgun_provider.clone();
   RT.spawn(async move {
      post_unshield_sync(ctx, chain, from, token, railgun_provider, false).await;
   });

   Ok(())
}

async fn post_unshield_sync(
   ctx: ZeusCtx,
   chain: ChainId,
   from: Address,
   token: ERC20Token,
   railgun_provider: RailgunProvider<RpcClient>,
   self_broadcast: bool,
) {
   let chain_id = chain.id();
   let mut provider = railgun_provider.clone();

   match provider.sync().await {
      Ok(_) => info!("Railgun provider synced after unshield"),
      Err(e) => error!(
         "Error syncing Railgun provider after unshield: {:?}",
         e
      ),
   }

   ctx.update_private_data(chain_id, from).await;

   let manager = ctx.balance_manager();
   if let Err(e) = manager
      .update_eth_balance(ctx.clone(), chain_id, vec![from], self_broadcast)
      .await
   {
      error!(
         "Error updating ETH balance after unshield: {:?}",
         e
      );
   }

   if let Err(e) = manager
      .update_tokens_balance(ctx.clone(), chain_id, from, vec![token], true)
      .await
   {
      error!(
         "Error updating token balance after unshield: {:?}",
         e
      );
   }
}

async fn fee_token_selection(chain: u64, from: Address) -> Result<ERC20Token, anyhow::Error> {
   // Open fee-token picker with only currently Railgun-supported fee tokens.
   SHARED_GUI.write(|gui| {
      gui.loading_window.reset();
      gui.token_selection.open(true, chain, from);
      gui.token_selection.set_title("Select Paymaster Fee Token".to_string());
      gui.request_repaint();
   });

   // Wait until private balances are processed (usually <100ms).
   let load_deadline = Instant::now() + Duration::from_secs(5);
   while SHARED_GUI.read(|gui| gui.token_selection.is_loading()) {
      if Instant::now() > load_deadline {
         SHARED_GUI.write(|gui| gui.token_selection.reset());
         return Err(anyhow!(
            "Timed out loading private fee-token balances"
         ));
      }
      sleep(Duration::from_millis(20)).await;
   }

   // Wait for selection or cancel (window closed).
   let selected_currency = loop {
      sleep(Duration::from_millis(50)).await;

      let (selected, open) = SHARED_GUI.read(|gui| {
         (
            gui.token_selection.get_selected_currency().cloned(),
            gui.token_selection.is_open(),
         )
      });

      if selected.is_none() && !open {
         return Err(anyhow!("No fee token selected"));
      }

      if let Some(currency) = selected {
         SHARED_GUI.write(|gui| {
            gui.token_selection.reset();
         });
         break currency;
      }
   };

   let fee_token = selected_currency.to_erc20().into_owned();

   Ok(fee_token)
}
