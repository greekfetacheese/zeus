//! Private (zk → zk) Railgun transfer execution

use anyhow::anyhow;

use zeus_eth::{
   alloy_primitives::{Address, U256},
   alloy_rpc_types::BlockId,
   currency::Currency,
   types::ChainId,
   utils::NumericValue,
};
use zeus_railgun::{
   PrivateHistoryKind, RailgunAddress, RailgunSigner,
   caip::AssetId,
   encode_history_memo,
   transact::{NoteSelectionMode, TransactionBuilder},
};

use crate::{
   core::{
      DecodedEvent, PrivateTransferParams, SendTxOptions, SendTxRequest, TransactionAnalysis,
      ZeusCtx, send_transaction,
   },
   gui::SHARED_GUI,
   utils::{
      RT,
      simulate::{
         AccountPrefetch, ForkPrefetch, ForkSim, StoragePrefetch, native_balance_at, pinned_head,
         railgun_common_accounts,
      },
   },
};

use super::{
   ProvedCall, RailgunAsset, SettledOp, prove, railgun_ready, settle_railgun_op, simulate_proved,
};

/// Private transfer of notes from the current wallet's 0zk address to another 0zk address.
///
/// Takes a [`RailgunAsset`] for the same reason the shield does: the builders are asset-agnostic, and the
/// difference between a fungible amount, an ERC-1155 quantity and an ERC-721's single token is already
/// answered by `asset_id()` and `value()` in one place.
pub async fn private_transfer(
   ctx: ZeusCtx,
   chain: ChainId,
   asset: RailgunAsset,
   amount: NumericValue,
   from: Address,
   recipient_zk: String,
   memo: String,
) -> Result<(), anyhow::Error> {
   // A fungible transfer has to move an ERC-20 — native has no note of its own to spend, so WETH stands
   // in. An NFT note *is* the asset, so there is nothing to check for it.
   if let RailgunAsset::Fungible(currency) = &asset {
      if !currency.is_erc20() {
         return Err(anyhow!(
            "Private transfer requires an ERC-20 asset (use WETH for native-equivalent)"
         ));
      }
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

   railgun_ready(ctx.clone(), chain).await?;

   let asset_id = asset.asset_id();

   // What actually moves: an ERC-20 its amount, an ERC-1155 a count of an id, an ERC-721 exactly one
   // whatever the field says. `value` is where that difference lives, and the number it answers is what
   // the receipt below reports too — so a receipt cannot claim something other than what moved.
   let amount_u128: u128 = asset
      .value(amount.wei())
      .try_into()
      .map_err(|_| anyhow!("Amount too large for private transfer"))?;

   // What the confirmation and the notification describe, and the contract the fork prefetches: the token
   // for an ERC-20, the collection for an NFT.
   let (contract, erc20, nft, shown_amount, amount_usd) = match &asset {
      RailgunAsset::Fungible(currency) => {
         let token = currency.to_erc20().into_owned();
         let amount_usd = ctx.get_token_value_for_amount(amount.f64(), &token);

         (
            token.address,
            Some(token),
            None,
            amount.clone(),
            Some(amount_usd),
         )
      }
      RailgunAsset::Nft(nft) => (
         nft.collection,
         None,
         Some(nft.clone()),
         // Whole units, with no decimals to scale a count by — and no price to go beside it.
         NumericValue::format_wei(U256::from(amount_u128), 0),
         None,
      ),
   };

   let transfer_params = PrivateTransferParams {
      chain: chain.id(),
      recipient: recipient.address.clone(),
      asset: asset_id,
      erc20,
      nft,
      amount_wei: U256::from(amount_u128),
      amount: Some(shown_amount),
      amount_usd,
   };

   let tx = TransactionBuilder::new()
      .transfer(
         railgun_signer.clone(),
         recipient,
         asset_id,
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
      contract,
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
   SHARED_GUI.write(|gui| {
      gui.loading_window.open("Preparing note merge…");
      gui.request_repaint();
   });

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

   railgun_ready(ctx.clone(), chain).await?;

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
      nft: None,
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
      token.address,
      tx,
      transfer_params,
   )
   .await
}

/// Runs a proved private transfer: pin the synced block, prove, fork-simulate, then send.
///
/// `contract` is what the fork prefetches — the token for an ERC-20, the collection for an NFT.
async fn exec_private_transfer(
   ctx: ZeusCtx,
   chain: ChainId,
   railgun_signer: RailgunSigner,
   from: Address,
   contract: Address,
   tx: TransactionBuilder,
   transfer_params: PrivateTransferParams,
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

   let mut accounts = Vec::new();
   accounts.push(AccountPrefetch::eoa(from));
   accounts.push(AccountPrefetch::contract(contract));
   accounts.push(AccountPrefetch::eoa(
      fork_block.header.beneficiary,
   ));
   accounts.push(AccountPrefetch::contract(railgun_address));

   let common_accounts = railgun_common_accounts(chain.id());
   accounts.extend(common_accounts.into_iter().map(AccountPrefetch::contract));

   let call = prove(&mut railgun_provider, tx).await?;

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

   // Transact logs are not public ERC-20 transfers, so record the intent.
   tx_analysis.set_main_event(DecodedEvent::PrivateTransfer(transfer_params));

   let tx_opt = SendTxOptions {
      dapp: "Railgun".to_string(),
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
      SettledOp::Private,
   ));

   Ok(())
}
