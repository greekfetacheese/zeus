//! Unshield execution paths: paymaster (ERC-4337) broadcast and emergency self-broadcast.

use std::str::FromStr;

use alloy_signer_local::PrivateKeySigner;
use anyhow::anyhow;
use tracing::{error, info};
use userop_kit::{
   bundler::{Bundler, pimlico::PimlicoBundler},
   smart_account::simple_smart_account::SimpleSmartAccount,
};
use zeus_eth::{
   alloy_primitives::Address, currency::Currency, types::ChainId, utils::NumericValue,
};
use zeus_railgun::{
   RailgunSigner, caip::AssetId, rand::SeedableRng, rand_chacha::ChaCha12Rng,
   transact::TransactionBuilder,
};

use crate::{
   core::{ZeusCtx, send_transaction},
   gui::SHARED_GUI,
   utils::RT,
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
   // Unshield recipient is independent of this key.
   let sa_key = PrivateKeySigner::random();
   let smart_account = SimpleSmartAccount::new(sa_key.address(), chain.id(), client);

   SHARED_GUI.write(|gui| {
      gui.loading_window.open("Proving unshield + estimating paymaster fee…");
      gui.request_repaint();
   });

   // ChaCha12Rng is Send; ThreadRng is not. prepare_userop still holds &dyn Bundler
   // across awaits, so the outer unshield future is not Send — spawn path uses
   // RT.spawn_blocking + RT.block_on (multi-thread RT; see shield.rs).
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
      .map_err(|e| anyhow!("Failed to prepare UserOperation: {}", e))?;

   SHARED_GUI.write(|gui| {
      gui.loading_window.open("Submitting unshield via bundler…");
      gui.request_repaint();
   });

   let signed = signable
      .sign(&sa_key)
      .await
      .map_err(|e| anyhow!("Failed to sign UserOperation: {}", e))?;

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
