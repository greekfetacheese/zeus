//! Unshield execution paths: paymaster (ERC-4337) broadcast and emergency self-broadcast.

use std::{str::FromStr, time::Instant};

use alloy_signer_local::PrivateKeySigner;
use anyhow::anyhow;
use tracing::{error, info};
use userop_kit::{
   bundler::{Bundler, pimlico::PimlicoBundler},
   smart_account::simple_smart_account::SimpleSmartAccount,
};
use zeus_eth::{
   alloy_primitives::{Address, U256},
   alloy_provider::Provider,
   alloy_rpc_types::BlockId,
   currency::Currency,
   revm_utils::{ForkFactory, Host, new_evm},
   types::ChainId,
   utils::NumericValue,
};
use zeus_railgun::{
   RailgunSigner, caip::AssetId, rand::SeedableRng, rand_chacha::ChaCha12Rng,
   transact::TransactionBuilder,
};

use crate::{
   core::{DecodedEvent, TransactionAnalysis, UnshieldParams, ZeusCtx, send_transaction},
   gui::SHARED_GUI,
   utils::{
      RT,
      simulate::{fetch_accounts_info, simulate_transaction},
   },
};

/// Default public Pimlico bundler RPC for a chain.
pub fn default_bundler_url(chain_id: u64) -> String {
   format!("https://public.pimlico.io/v2/{}/rpc", chain_id)
}

/// Unshield private notes to a public address.
///
/// - `self_broadcast = false` (default): Kohaku-style privacy paymaster + bundler UserOp.
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
         "Current wallet cannot derive a Railgun address (imported keys without seed are not supported yet)"
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
      unshield_self_broadcast(ctx, chain, from, &mut railgun_provider, tx).await
   } else {
      unshield_via_paymaster(
         ctx,
         chain,
         from,
         &mut railgun_provider,
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
   railgun_provider: &mut zeus_railgun::RailgunProvider<zeus_eth::utils::client::RpcClient>,
   tx: TransactionBuilder,
) -> Result<(), anyhow::Error> {
   SHARED_GUI.write(|gui| {
      gui.loading_window.open("Proving unshield (self-broadcast)…");
      gui.request_repaint();
   });

   // ChaCha12Rng is Send (+ Sync); ThreadRng is not and breaks RT.spawn futures.
   let proved = {
      let mut rng = ChaCha12Rng::from_os_rng();
      railgun_provider.build(tx, &mut rng).await?
   };

   let calldata = proved.tx_data.data.clone();
   let interact_to = proved.tx_data.to;
   let value = proved.tx_data.value;

   let dapp = "Railgun Unshield (self-broadcast)".to_string();
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

   post_unshield_sync(ctx, chain, from, railgun_provider);
   Ok(())
}

async fn unshield_via_paymaster(
   ctx: ZeusCtx,
   chain: ChainId,
   from: Address,
   railgun_provider: &mut zeus_railgun::RailgunProvider<zeus_eth::utils::client::RpcClient>,
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

   let fee_asset = AssetId::Erc20(chain_config.wrapped_base_token);
   let rg_addr = railgun_signer.address().clone();

   // Live spendable notes (not UI portfolio cache). Paymaster always charges a private
   // fee note in wrapped base token on top of the unshield amount when unshielding WETH.
   let notes = railgun_provider.notes(rg_addr.clone()).await;
   let fee_token_balance = railgun_provider.balance_erc20(rg_addr.clone(), fee_asset).await;

   let note_summary: Vec<String> = notes
      .iter()
      .map(|n| {
         format!(
            "{{asset={}, value={}, tree={}, leaf={}}}",
            n.asset, n.amount, n.tree_number, n.leaf_index
         )
      })
      .collect();
   info!(
      "Paymaster unshield preflight: notes={} fee_token_balance_wei={} fee_token={:?} note_summary={:?}",
      notes.len(),
      fee_token_balance,
      chain_config.wrapped_base_token,
      note_summary
   );

   // Initial fee seed inside prepare_userop (Kohaku). Real fee after gas estimate is often higher.
   const INITIAL_FEE_WEI: u128 = 100_000_000;
   if fee_token_balance < INITIAL_FEE_WEI {
      let note_assets: Vec<String> =
         notes.iter().map(|n| format!("{} ({} wei)", n.asset, n.amount)).collect();
      let fee_token = chain_config.wrapped_base_token;
      return Err(anyhow!(
         "Not enough private balance of the Railgun fee token ({fee_token:?}) for the paymaster fee note.\n\
          Fee-token spendable balance: {fee_token_balance} wei (need ≥ {INITIAL_FEE_WEI} wei fee seed, plus unshield amount if same asset).\n\
          Notes loaded ({}) : {note_assets:?}\n\n\
          Common Sepolia issue: Zeus default WETH is 0x7b7999… while Railgun/Kohaku paymaster fee uses 0xfff997….\n\
          Private notes in a different WETH cannot pay the fee. Self-broadcast still works for those notes.\n\
          Fix going forward: native Railgun shield now wraps to ChainConfig.wrapped_base_token ({fee_token:?}).\n\
          To use private broadcast, shield more of that fee token (or swap/shield into it).",
         notes.len()
      ));
   }

   let bundler_url = if bundler_url.trim().is_empty() {
      default_bundler_url(chain.id())
   } else {
      bundler_url.trim().to_string()
   };

   let parsed_url = bundler_url
      .parse()
      .map_err(|e| anyhow!("Invalid bundler URL '{}': {}", bundler_url, e))?;

   let client = ctx.get_client(chain.id()).await?;
   let bundler = PimlicoBundler::new(parsed_url);

   // Ephemeral smart-account owner for the UserOp.
   // Unshield recipient is independent of this key — does NOT affect private note selection.
   let sa_key = PrivateKeySigner::random();
   let smart_account = SimpleSmartAccount::new(sa_key.address(), chain.id(), client);

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
         chain_config.wrapped_base_token,
         Vec::new(), // no post-unshield calls for v1
         &mut rng,
      )
      .await
      .map_err(|e| {
         anyhow!(
            "Failed to prepare UserOperation: {e}\n\
             (paymaster fee is a private transfer of wrapped base token to the Railgun privacy paymaster; \
             initial fee seed is {INITIAL_FEE_WEI} wei, then converges to gas*maxFee. \
             Spendable fee-token balance was {fee_token_balance} wei across {} note(s). \
             Random smart-account key is only the 4337 submitter and does not spend your notes.)",
            notes.len()
         )
      })?;

   let signed = signable
      .sign(&sa_key)
      .await
      .map_err(|e| anyhow!("Failed to sign UserOperation: {}", e))?;

   SHARED_GUI.write(|gui| {
      gui.loading_window.open("Simulating Transaction…");
      gui.request_repaint();
   });

   // Simulate the tx
   let fork_block_id = BlockId::number(fork_block.header.number);

   let mut accounts = Vec::new();
   accounts.push(from);
   accounts.push(signed.entry_point);
   accounts.push(fork_block.header.beneficiary);

   let accounts_info = fetch_accounts_info(ctx.clone(), chain.id(), fork_block_id, accounts).await;

   let fork_client = ctx.get_client(chain.id()).await?;
   let mut factory =
      ForkFactory::new_sandbox_factory(fork_client, chain.id(), None, Some(fork_block_id));

   for info in accounts_info {
      factory.insert_account_info(info.address, info.info);
   }

   let fork_db = factory.new_sandbox_fork();

   let eth_balance_after;
   let sim_res;
   {
      let mut evm = new_evm(chain, Some(&fork_block), fork_db.clone());

      let from = signed.user_op.sender;
      let to = signed.entry_point;
      let data = signed.user_op.call_data.clone();
      let value = U256::ZERO;

      let time = Instant::now();
      sim_res = simulate_transaction(&mut evm, from, to, data, value, vec![])?;
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
   info!("Logs Len {}", logs.len());

   let mut unshield_events = Vec::new();

   for log in &logs {
      if let Ok(params) = UnshieldParams::from_log(ctx.clone(), chain.id(), log).await {
         unshield_events.push(params);
      }
   }

   // Should not happen
   if unshield_events.len() > 1 {
      return Err(anyhow!("More than one Unshield event found"));
   }

   if unshield_events.is_empty() {
      return Err(anyhow!("No Unshield event found"));
   }

   let mut unshield_params = unshield_events[0].clone();
   // TODO: Add the broadcaster fee to the unshield params

   let eth_balance_before = eth_balance_before_fut.await?;

   let value = U256::ZERO;
   let contract_interact = Some(true);
   let interact_to = signed.entry_point;
   let calldata = signed.user_op.call_data.clone();
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

   let main_event = DecodedEvent::Unshield(unshield_params.clone());
   tx_analysis.set_main_event(main_event);

   let priority_fee = ctx.get_priority_fee(chain.id()).unwrap_or_default();
   let dapp = "Railgun Unshield".to_string();
   let mev_protect = false;

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

   info!("Unshield UserOp submitted: {:?}", hash);

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
      return Err(anyhow!(
         "Unshield UserOperation failed on-chain: {:?}",
         receipt
      ));
   }

   info!("Unshield UserOp succeeded: {:?}", receipt);

   post_unshield_sync(ctx, chain, from, railgun_provider);

   SHARED_GUI.write(|gui| {
      gui.loading_window.reset();
      gui.msg_window.open(
         "Unshield submitted",
         format!(
            "Private broadcast succeeded via paymaster/bundler.\nUserOp hash: {:?}",
            hash
         ),
      );
      gui.request_repaint();
   });

   Ok(())
}

fn post_unshield_sync(
   ctx: ZeusCtx,
   chain: ChainId,
   from: Address,
   railgun_provider: &zeus_railgun::RailgunProvider<zeus_eth::utils::client::RpcClient>,
) {
   let ctx2 = ctx.clone();
   let chain_id = chain.id();
   let mut provider = railgun_provider.clone();

   RT.spawn(async move {
      match provider.sync().await {
         Ok(_) => info!("Railgun provider synced after unshield"),
         Err(e) => error!(
            "Error syncing Railgun provider after unshield: {:?}",
            e
         ),
      }

      ctx2.update_private_data(chain_id, from).await;

      let manager = ctx2.balance_manager();
      if let Err(e) = manager.update_eth_balance(ctx2.clone(), chain_id, vec![from], true).await {
         error!(
            "Error updating ETH balance after unshield: {:?}",
            e
         );
      }
   });
}
