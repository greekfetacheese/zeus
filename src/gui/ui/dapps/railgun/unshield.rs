//! Unshield execution paths: paymaster (ERC-4337) broadcast and emergency self-broadcast.

use std::{
   str::FromStr,
   time::{Duration, Instant},
};
use tokio::time::sleep;

use alloy_consensus::TxType;
use alloy_signer_local::PrivateKeySigner;
use anyhow::anyhow;
use userop_kit::{
   bundler::PimlicoBundler,
   smart_account::simple_smart_account::{Call, SIMPLE_7702_ACCOUNT, SimpleSmartAccount},
};
use zeus_eth::{
   alloy_primitives::{Address, Bytes, KECCAK256_EMPTY, Log, U256, keccak256},
   alloy_provider::Provider,
   alloy_rpc_types::BlockId,
   currency::ERC20Token,
   revm_utils::{
      ForkFactory, Host, new_evm,
      revm::state::{AccountInfo, Bytecode},
   },
   types::ChainId,
   utils::{NumericValue, address_book},
};
use zeus_railgun::{
   PrivateHistoryKind, RailgunSigner, adapter_data::decode_fee_from_paymaster_data, caip::AssetId,
   encode_history_memo, rand::SeedableRng, rand_chacha::ChaCha12Rng, transact::TransactionBuilder,
};

use crate::{
   // The diffs for the sponsored unshield are probed off its own fork simulation.
   core::tx::DiffProbe,
   core::{
      ApprovalDiff, BalanceDiff, DecodedEvent, MainEvent, MinedTx, RecordPolicy, SendTxOptions,
      SendTxRequest, TransactionAnalysis, UnshieldParams, ZeusCtx, build_tx_outcome, confirm_tx,
      record_and_notify, send_transaction, tx::diffs_from_receipt,
   },
   gui::SHARED_GUI,
   utils::{
      RT, malloc_trim,
      simulate::{
         AccountPrefetch, ForkPrefetch, ForkSim, StoragePrefetch, fetch_accounts_info,
         fetch_storage_for_railgun, native_balance_at, pinned_head, railgun_common_accounts,
         simulate_transaction,
      },
   },
};

use super::{
   ProvedCall, RailgunAsset, expect_single_event, prove, railgun_ready, resync_railgun_later,
   settle_railgun_op, simulate_proved,
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
   asset: RailgunAsset,
   amount: NumericValue,
   from: Address,
   recipient: String,
   self_broadcast: bool,
   unwrap_to_eth: bool,
   bundler_url: String,
   memo: String,
) -> Result<(), anyhow::Error> {
   if asset.is_native() {
      return Err(anyhow!(
         "Unshield requires a token or an NFT (use WETH for native-equivalent)"
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

   SHARED_GUI.write(|gui| {
      gui.loading_window.open("Preparing unshield…");
      gui.request_repaint();
   });

   railgun_ready(ctx.clone(), chain).await?;

   // An ERC-721 moves exactly one: the token id is the asset, so the amount the UI holds is not consulted.
   let amount_u128: u128 = asset
      .value(amount.wei())
      .try_into()
      .map_err(|_| anyhow!("Amount too large for unshield"))?;

   let tx = TransactionBuilder::new()
      .unshield(
         railgun_signer.clone(),
         recipient,
         asset.asset_id(),
         amount_u128,
      )?
      .with_change_memo(&encode_history_memo(
         PrivateHistoryKind::Unshield,
         memo.trim(),
      ));

   if self_broadcast {
      unshield_self_broadcast(
         ctx,
         chain,
         railgun_signer,
         from,
         recipient,
         &asset,
         tx,
      )
      .await
   } else {
      unshield_via_paymaster(
         ctx,
         chain,
         from,
         &asset,
         asset.value(amount.wei()),
         recipient,
         unwrap_to_eth,
         railgun_signer,
         tx,
         bundler_url,
         memo,
      )
      .await
   }
}

async fn unshield_self_broadcast(
   ctx: ZeusCtx,
   chain: ChainId,
   railgun_signer: RailgunSigner,
   from: Address,
   recipient: Address,
   asset: &RailgunAsset,
   tx: TransactionBuilder,
) -> Result<(), anyhow::Error> {
   SHARED_GUI.write(|gui| {
      gui.loading_window.open("Wait while magic happens");
      gui.request_repaint();
   });

   let mut railgun_provider = ctx.get_railgun_provider(chain.id(), false).await?;

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

   let (fork_block, fork_block_id) = pinned_head(
      ctx.clone(),
      chain,
      BlockId::number(last_synced_block),
   )
   .await?;

   let eth_balance_before_fut = native_balance_at(ctx.clone(), chain, from, fork_block_id);

   let railgun_address = railgun_provider.railgun_address();

   // Prefetch accounts and storage for the sim
   let mut accounts = Vec::new();
   accounts.push(AccountPrefetch::eoa(from));
   accounts.push(AccountPrefetch::eoa(recipient));
   accounts.push(AccountPrefetch::contract(
      asset.asset_id().address(),
   ));
   accounts.push(AccountPrefetch::eoa(
      fork_block.header.beneficiary,
   ));
   accounts.push(AccountPrefetch::contract(railgun_address));

   let common_accounts = railgun_common_accounts(chain.id());
   accounts.extend(common_accounts.into_iter().map(AccountPrefetch::contract));

   SHARED_GUI.write(|gui| {
      gui.loading_window.open("Generating proof…");
      gui.request_repaint();
   });

   let call = prove(&mut railgun_provider, tx).await?;

   railgun_provider.prover().artifact_loader().clear_mem_cache();
   malloc_trim();

   SHARED_GUI.write(|gui| {
      gui.loading_window.open("Simulating Transaction…");
      gui.request_repaint();
   });

   let gas_limit = 30_000_000;
   let prefetch = ForkPrefetch {
      block: fork_block,
      accounts,
      storage: StoragePrefetch::Railgun(railgun_address),
   };

   let sim = simulate_proved(
      ctx.clone(),
      chain,
      from,
      &call,
      prefetch,
      Some(gas_limit),
   )
   .await?;

   let ProvedCall {
      calldata,
      interact_to,
      value,
   } = call;

   let ForkSim {
      sim_res,
      logs,
      balance_after: eth_balance_after,
      ..
   } = sim;

   let mut unshield_events = Vec::new();

   for log in &logs {
      if let Ok(params) = UnshieldParams::from_log(ctx.clone(), chain.id(), log).await {
         unshield_events.push(params);
      }
   }

   let mut unshield_params = expect_single_event(
      unshield_events,
      "More than one Unshield event found",
      || {
         format!(
            "No Unshield event found in handleOps simulation ({} log(s))",
            logs.len()
         )
      },
   )?;
   unshield_params.is_self_broadcast = true;

   let eth_balance_before = eth_balance_before_fut.await?;

   let mut tx_analysis = TransactionAnalysis::new(
      ctx.clone(),
      chain.id(),
      from,
      interact_to,
      Some(true),
      calldata.clone(),
      value,
      logs,
      sim_res.tx_gas_used(),
      eth_balance_before,
      eth_balance_after,
      vec![],
   )
   .await?;

   tx_analysis.set_main_event(DecodedEvent::Unshield(unshield_params));

   let tx_opt = SendTxOptions {
      dapp: "Railgun".to_string(),
      // The Transact logs do not describe the unshield, so record the intent.
      keep_intent_event: true,
      ..Default::default()
   };

   let (_, _) = send_transaction(
      ctx.clone(),
      true,
      SendTxRequest::new(chain, from, interact_to)
         .call_data(calldata)
         .value(value)
         .analysis(tx_analysis),
      tx_opt,
   )
   .await?;

   RT.spawn(settle_railgun_op(
      ctx,
      chain,
      from,
      match asset {
         // An NFT has no ERC-20 balance to refresh; it shows up through the private scan.
         RailgunAsset::Fungible(currency) => Some(currency.to_erc20().into_owned()),
         RailgunAsset::Nft(_) => None,
      },
   ));

   Ok(())
}

async fn unshield_via_paymaster(
   ctx: ZeusCtx,
   chain: ChainId,
   from: Address,
   asset: &RailgunAsset,
   amount: U256,
   recipient: Address,
   unwrap_to_eth: bool,
   railgun_signer: RailgunSigner,
   tx: TransactionBuilder,
   bundler_url: String,
   memo: String,
) -> Result<(), anyhow::Error> {
   let mut railgun_provider = ctx.get_railgun_provider(chain.id(), false).await?;

   let chain_config = railgun_provider.chain_config();

   if chain_config.privacy_paymaster.is_none() || chain_config.railgun_fee_adapter.is_none() {
      return Err(anyhow!(
         "Privacy paymaster is not configured for this chain"
      ));
   }

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

   let (fork_block, fork_block_id) = pinned_head(
      ctx.clone(),
      chain,
      BlockId::number(last_synced_block),
   )
   .await?;

   let eth_balance_before_fut = native_balance_at(ctx.clone(), chain, from, fork_block_id);

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
   accounts.push(AccountPrefetch::eoa(from));
   accounts.push(AccountPrefetch::contract(entry_point));
   accounts.push(AccountPrefetch::contract(SIMPLE_7702_ACCOUNT));
   accounts.push(AccountPrefetch::eoa(
      fork_block.header.beneficiary,
   ));

   if let Some(pm) = chain_config.privacy_paymaster {
      accounts.push(AccountPrefetch::contract(pm));
   }

   if let Some(adapter) = chain_config.railgun_fee_adapter {
      accounts.push(AccountPrefetch::contract(adapter));
   }

   accounts.push(AccountPrefetch::contract(
      railgun_provider.railgun_address(),
   ));
   accounts.push(AccountPrefetch::contract(fee_token.address));

   let common_accounts = railgun_common_accounts(chain.id());
   accounts.extend(common_accounts.into_iter().map(AccountPrefetch::contract));

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

   // When unshielding the same asset used for the paymaster fee (usually WETH),
   // the fee note is spent from the same private notes as the unshield. Full-balance
   // unshields leave zero headroom for the fee and fail during UserOp prep.
   let amount_u128: u128 =
      amount.try_into().map_err(|_| anyhow!("Amount too large for unshield"))?;
   if let RailgunAsset::Fungible(currency) = asset {
      let token = currency.to_erc20();

      if token.address == fee_token.address {
         let need = amount_u128.saturating_add(INITIAL_FEE_WEI);
         if need > fee_token_balance {
            let amount_fmt = NumericValue::format_wei(amount, token.decimals);
            let max_unshield = fee_token_balance.saturating_sub(INITIAL_FEE_WEI);
            let max_fmt = NumericValue::format_wei(U256::from(max_unshield), token.decimals);
            return Err(anyhow!(
               "Not enough private {} for unshield + bundler fee. Unshielding {} leaves no room for the paymaster fee (private balance {}). Try unshielding at most {} or use self-broadcast.",
               token.symbol,
               amount_fmt.abbreviated(),
               fee_token_balance_fmt.abbreviated(),
               max_fmt.abbreviated()
            ));
         }
      }
   }

   let parsed_url = bundler_url
      .parse()
      .map_err(|e| anyhow!("Invalid bundler URL '{}': {}", bundler_url, e))?;

   let bundler = PimlicoBundler::new(parsed_url);

   SHARED_GUI.write(|gui| {
      gui.loading_window.open("Generating proof…");
      gui.request_repaint();
   });

   // Optional post-unshield calls run by the smart account after the paymaster
   //
   // Unwrap requires the unshield recipient to be the smart account so it holds
   // WETH, then: WETH.withdraw → (optional) send ETH to the real recipient.
   let sa_addr = smart_account.address();
   let (tx, post_calls) = if unwrap_to_eth {
      let is_wrapped_base = matches!(
         asset,
         RailgunAsset::Fungible(currency)
            if currency.to_erc20().address == chain_config.wrapped_base_token
      );

      if !is_wrapped_base {
         return Err(anyhow!(
            "Unwrap to ETH is only available when unshielding the chain wrapped base token (WETH)"
         ));
      }

      let amount_u128: u128 =
         amount.try_into().map_err(|_| anyhow!("Amount too large for unshield"))?;
      let fee_bps = u128::from(chain_config.unshield_fee_bps);
      // Recipient receives amount after protocol unshield fee (bps of amount).
      let protocol_fee = amount_u128.saturating_mul(fee_bps) / 10_000;
      let received = amount_u128.saturating_sub(protocol_fee);
      if received == 0 {
         return Err(anyhow!(
            "Unshield amount too small after protocol fee"
         ));
      }

      let tx = TransactionBuilder::new()
         .unshield(
            railgun_signer.clone(),
            sa_addr,
            AssetId::Erc20(chain_config.wrapped_base_token),
            amount_u128,
         )?
         .with_change_memo(&encode_history_memo(
            PrivateHistoryKind::Unshield,
            memo.trim(),
         ));

      let weth = ERC20Token::wrapped_native_token(chain.id());
      let received_u256 = U256::from(received);

      let mut calls = vec![Call {
         target: chain_config.wrapped_base_token,
         value: U256::ZERO,
         data: weth.encode_withdraw(received_u256),
      }];

      // Forward native ETH to the user-selected recipient (SA is ephemeral).
      if recipient != sa_addr {
         calls.push(Call {
            target: recipient,
            value: received_u256,
            data: Bytes::new(),
         });
      }

      #[cfg(feature = "dev")]
      tracing::info!(
         "Unwrap-to-ETH post-calls: withdraw {} wei WETH on SA {:?}, forward ETH to {:?}",
         received,
         sa_addr,
         recipient
      );

      (tx, calls)
   } else {
      let _ = memo;
      (tx, Vec::new())
   };

   let mut rng = ChaCha12Rng::from_os_rng();
   let signable = railgun_provider
      .prepare_userop(
         tx,
         &bundler,
         &smart_account,
         railgun_signer,
         fee_token.address,
         post_calls,
         &mut rng,
      )
      .await
      .map_err(|e| anyhow!("Failed to prepare UserOperation: {e}"))?;

   let signed = signable
      .sign(&sa_key)
      .await
      .map_err(|e| anyhow!("Failed to sign UserOperation: {}", e))?;

   railgun_provider.prover().artifact_loader().clear_mem_cache();
   malloc_trim();

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

   // Diffs come from the simulation, not the receipt: a bundler broadcasts this
   // one, so the confirm window is the only place the user sees what it does.
   let probe = DiffProbe::start(
      ctx.clone(),
      chain,
      fork_block_id,
      from,
      interact_to,
      handle_ops_data.clone(),
   );

   let eth_balance_after;
   let sim_res;
   let logs;
   let measured;
   {
      let mut evm = new_evm(chain, Some(&fork_block), fork_db.clone());
      evm.tx.gas_limit = 30_000_000;

      sim_res = match simulate_transaction(
         &mut evm,
         bundle_caller,
         interact_to,
         handle_ops_data.clone(),
         value,
         vec![],
      ) {
         Ok(res) => res,
         Err(e) => {
            // A "note already spent" revert means local Railgun state disagrees
            // with the chain.
            if e.to_string().contains("note already spent") {
               resync_railgun_later(ctx.clone(), chain);
            }

            return Err(anyhow!("Simulation failed: {:?}", e));
         }
      };

      let state = evm.balance(from);
      eth_balance_after = if let Some(state) = state {
         state.data
      } else {
         U256::ZERO
      };

      logs = sim_res.clone().into_logs();
      measured = probe.measure(ctx.clone(), chain, fork_block_id, &logs, &mut evm);
   }

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

   // Unwrap path unshields to the ephemeral SA then forwards ETH — show the
   // user-selected recipient in confirmation UX, not the SA address.
   if unwrap_to_eth {
      unshield_params.recipient = recipient;
   }

   // Broadcaster/paymaster fee = private fee note encoded in paymaster_data (wrapped base token).
   // Distinct from Unshield event `fee` (protocol 0.25% on the unshielded token).
   //
   // This is NOT a public ERC-20 transfer and is NOT deducted from the unshield amount
   // the recipient receives. It is a separate private transfer to the paymaster's 0zk,
   // funded from the user's private notes alongside the unshield spend.
   if let Some(pm_data) = signed.user_op.paymaster_data.as_ref() {
      let (fee_asset, fee_wei) = match decode_fee_from_paymaster_data(pm_data.as_ref()) {
         Ok((fee_asset, fee_wei)) => (fee_asset, fee_wei),
         Err(e) => {
            return Err(anyhow!("Failed to decode paymaster data: {e}"));
         }
      };

      if fee_asset != fee_token.address {
         return Err(anyhow!(
            "Fee token mismatch: paymaster fee asset {} != fee token {}",
            fee_asset,
            fee_token.address
         ));
      }

      let fee_amt = NumericValue::format_wei(U256::from(fee_wei), fee_token.decimals);
      let fee_usd = ctx.get_token_value_for_amount(fee_amt.f64(), &fee_token);
      unshield_params.fee_token = Some(fee_token);
      unshield_params.broadcaster_fee = Some(fee_amt);
      unshield_params.broadcaster_fee_usd = Some(fee_usd);
   }

   let eth_balance_before = eth_balance_before_fut.await?;

   // The confirm window's diffs come from the simulation: this tx is broadcast by
   // a bundler, so there is no receipt to diff yet, and the window is the only
   // place the user sees what it does.
   let (sim_balance_diff, sim_approval_diff) = measured
      .resolve(
         ctx.clone(),
         chain,
         eth_balance_before,
         eth_balance_after,
      )
      .await?;

   let contract_interact = Some(true);
   let calldata = handle_ops_data;
   let auth_list = Vec::new();

   // ? We can't know the actualy sender of the tx before hand
   // ? TxHistory will show the actual sender
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

   // The confirm window shows what the simulation found. The recorded tx keeps the
   // receipt-based diffs below, which are what actually happened.
   tx_analysis.set_diffs(sim_balance_diff, sim_approval_diff);

   let main_event = DecodedEvent::Unshield(unshield_params.clone());
   tx_analysis.set_main_event(main_event);

   // Replace the decoded unshield with the main so it can also show
   // the broadcaster fee
   for event in &mut tx_analysis.decoded_events {
      if let DecodedEvent::Unshield(params) = event {
         *params = unshield_params.clone();
      }
   }

   let tx_opt = SendTxOptions {
      dapp: "Railgun".to_string(),
      sponsored: true,
      ..Default::default()
   };

   // Sponsored: the priority fee is irrelevant, the paymaster pays it.
   confirm_tx(ctx.clone(), true, chain, &tx_analysis, &tx_opt).await?;

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

   // Prefer the included handleOps tx logs (same source self-broadcast uses).
   // `UserOperationReceipt.logs` is bundler-filtered and can drop the Railgun
   // Unshield emitted during paymaster validation.
   let logs: Vec<Log> = {
      let handle_ops: Vec<_> = receipt.receipt.logs().iter().cloned().map(Into::into).collect();
      if handle_ops.is_empty() {
         receipt.logs.iter().map(|l| l.clone().into_inner()).collect()
      } else {
         handle_ops
      }
   };

   let block = receipt.receipt.block_number.unwrap_or(0);
   let block_id = if block > 0 {
      BlockId::number(block)
   } else {
      BlockId::latest()
   };

   let eth_balance_after = zeus_client
      .request(chain.id(), |client| async move {
         client
            .get_balance(from)
            .block_id(block_id)
            .await
            .map_err(|e| anyhow!("{:?}", e))
      })
      .await?;

   let (balance_diff, approval_diff) = match diffs_from_receipt(
      ctx.clone(),
      chain.id(),
      from,
      interact_to,
      &tx_analysis.call_data,
      &logs,
      block,
      eth_balance_after,
   )
   .await
   {
      Ok(diffs) => diffs,
      Err(e) => {
         tracing::error!("Failed to diff logs: {:?}", e);
         (BalanceDiff::default(), ApprovalDiff::default())
      }
   };

   let mined_tx = MinedTx {
      // The handleOps transaction is sent by the bundler, not by the user.
      from: receipt.receipt.from,
      interact_to,
      call_data: tx_analysis.call_data.clone(),
      value: tx_analysis.value,
      logs,
      eth_balance_before,
      eth_balance_after,
      contract_interact,
      authorization_list: vec![],
      tx_type: TxType::Eip7702,
      block,
      gas_used: receipt.receipt.gas_used,
      hash: receipt.receipt.transaction_hash,
      success: receipt.success,
   };

   let main_event = MainEvent::Resolved(Box::new(move |analysis| {
      // Merge the broadcaster fee into the receipt-decoded unshield before
      // choosing it: the fee is a private note, not visible in public logs.
      let main_event = analysis.resolve_unshield_event(unshield_params);

      if let DecodedEvent::Unshield(params) = &main_event {
         let mut found = false;
         for event in &mut analysis.decoded_events {
            if let DecodedEvent::Unshield(existing) = event {
               *existing = params.clone();
               found = true;
            }
         }
         if !found {
            analysis.decoded_events.push(main_event.clone());
         }
      }

      main_event
   }));

   let policy = RecordPolicy {
      priority_fee: NumericValue::default(),
      diffs: Some((balance_diff, approval_diff)),
      ..Default::default()
   };

   let outcome = build_tx_outcome(ctx.clone(), chain, mined_tx, main_event, policy).await?;

   record_and_notify(ctx.clone(), chain, from, &outcome)?;

   // An NFT has no ERC-20 balance to refresh; it shows up through the private scan.
   let refresh_token = match asset {
      RailgunAsset::Fungible(currency) if !unwrap_to_eth => Some(currency.to_erc20().into_owned()),
      _ => None,
   };

   RT.spawn(settle_railgun_op(ctx, chain, from, refresh_token));

   Ok(())
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
