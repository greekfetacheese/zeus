//! ERC-20 approval helpers for the flows that need an allowance before their
//! real transaction.

use super::analysis::TransactionAnalysis;
use super::send::{SendTxOptions, SendTxRequest, send_transaction};
use crate::core::{DecodedEvent, TokenApproveParams, ZeusCtx};
use crate::gui::SHARED_GUI;
use crate::utils::RT;
use zeus_eth::{
   abi::{erc721, erc1155},
   alloy_primitives::{Address, Log, U256},
   alloy_rpc_types::TransactionReceipt,
   currency::ERC20Token,
   nft::NftStandard,
   types::ChainId,
   utils::NumericValue,
};

/// Result of simulating an approve call, used to build the confirm analysis.
pub struct ApproveSimulation {
   pub logs: Vec<Log>,
   pub gas_used: u64,
   pub eth_balance_before: U256,
   pub eth_balance_after: U256,
}

/// Approve `spender` to spend `amount` of `token`, confirming with a
/// [`TokenApproveParams`] main event.
///
/// `sim` is the caller's own simulation of the approve call. Pass it when the
/// simulation had to happen anyway — `swap_via_ur` commits the allowance into its
/// fork and would rather not have `send_transaction` simulate a second time. Pass
/// `None` to let `send_transaction` simulate.
///
/// Returns the receipt: callers that treat a failed approve as fatal check
/// `receipt.status()`.
pub async fn send_token_approve(
   ctx: ZeusCtx,
   chain: ChainId,
   owner: Address,
   token: &ERC20Token,
   spender: Address,
   amount: U256,
   sim: Option<ApproveSimulation>,
   dapp: &str,
   mev_protect: bool,
) -> Result<TransactionReceipt, anyhow::Error> {
   let interact_to = token.address;
   let call_data = token.encode_approve(spender, amount);
   let value = U256::ZERO;
   let auth_list = Vec::new();

   // Update its price, the UI will pick it up later from the ctx
   let price_manager = ctx.price_manager();
   let pool_manager = ctx.pool_manager();
   let tokens = vec![token.clone()];
   let ctx2 = ctx.clone();
   RT.spawn(async move {
      if let Err(e) = price_manager.calculate_prices(ctx2, chain.id(), pool_manager, tokens).await {
         tracing::error!("Error updating token price: {:?}", e);
      }
   });

   let tx_analysis = match sim {
      Some(sim) => {
         let params = TokenApproveParams {
            token: token.clone(),
            amount: NumericValue::format_wei(amount, token.decimals),
            amount_usd: None,
            owner,
            spender,
         };

         let mut analysis = TransactionAnalysis::new(
            ctx.clone(),
            chain.id(),
            owner,
            interact_to,
            Some(true),
            call_data.clone(),
            value,
            sim.logs,
            sim.gas_used,
            sim.eth_balance_before,
            sim.eth_balance_after,
            auth_list.clone(),
         )
         .await?;

         analysis.set_main_event(DecodedEvent::TokenApprove(params));

         Some(analysis)
      }
      None => None,
   };

   let mut req = SendTxRequest::new(chain, owner, interact_to)
      .call_data(call_data)
      .value(value)
      .authorization_list(auth_list);
   // `ensure_allowance` may have been handed a simulation to reuse, in which
   // case the analysis is already built and this stays `None`.
   req.analysis = tx_analysis;

   let (receipt, _) = send_transaction(
      ctx,
      true,
      req,
      SendTxOptions {
         dapp: dapp.to_string(),
         mev_protect,
         ..Default::default()
      },
   )
   .await?;

   Ok(receipt)
}

/// Send `setApprovalForAll(operator, true)` for an NFT collection.
///
/// One approval covers every token in the collection, so unlike an ERC-20 allowance there is no amount to
/// compare against — the operator is either approved or it is not.
///
/// No bespoke confirm params: the call emits `ApprovalForAll`, and the decoder already turns that into an
/// NFT approval event, which is a better description of what is happening than anything built by hand.
pub async fn send_nft_approve(
   ctx: ZeusCtx,
   chain: ChainId,
   owner: Address,
   collection: Address,
   standard: NftStandard,
   operator: Address,
   dapp: &str,
) -> Result<TransactionReceipt, anyhow::Error> {
   let interact_to = collection;
   let call_data = match standard {
      NftStandard::Erc721 => erc721::encode_set_approval_for_all(operator, true),
      NftStandard::Erc1155 => erc1155::encode_set_approval_for_all(operator, true),
   };

   let mut req = SendTxRequest::new(chain, owner, interact_to)
      .call_data(call_data)
      .value(U256::ZERO)
      .authorization_list(Vec::new());

   req.analysis = None;

   let (receipt, _) = send_transaction(
      ctx,
      true,
      req,
      SendTxOptions {
         dapp: dapp.to_string(),
         ..Default::default()
      },
   )
   .await?;

   Ok(receipt)
}

/// Send `approve(operator, token_id)` for one ERC-721.
///
/// The scoped counterpart of [`send_nft_approve`]: `setApprovalForAll` hands the operator every token the
/// owner holds in that collection and keeps that right until it is revoked, while a shield moves **one**
/// token. The protocol takes the scoped form — `RailgunLogic.transferTokenIn` calls
/// `IERC721.transferFrom(msg.sender, address(this), tokenSubID)`, which accepts either an `approve` for
/// that id or a collection-wide `setApprovalForAll` — so there is nothing to pay for the broader grant.
///
/// Exists only for ERC-721: ERC-1155 has no per-token approval to give.
///
/// No bespoke confirm params: the call emits `Approval(owner, to, tokenId)`, and the decoder already
/// turns that into an NFT approval event, which is a better description of what is happening than
/// anything built by hand.
pub async fn send_erc721_approve(
   ctx: ZeusCtx,
   chain: ChainId,
   owner: Address,
   collection: Address,
   token_id: U256,
   operator: Address,
   dapp: &str,
) -> Result<TransactionReceipt, anyhow::Error> {
   let interact_to = collection;
   let call_data = erc721::encode_approve(operator, token_id);

   let mut req = SendTxRequest::new(chain, owner, interact_to)
      .call_data(call_data)
      .value(U256::ZERO)
      .authorization_list(Vec::new());

   req.analysis = None;

   let (receipt, _) = send_transaction(
      ctx,
      true,
      req,
      SendTxOptions {
         dapp: dapp.to_string(),
         ..Default::default()
      },
   )
   .await?;

   Ok(receipt)
}

/// Ensure `operator` may move `token_id` of `collection`, sending a per-token `approve` when it may not.
///
/// A revoke reads back as the zero address, so "may move it" is exactly "the operator is the approved
/// address" — there is no amount to compare against.
pub async fn ensure_erc721_approve(
   ctx: ZeusCtx,
   chain: ChainId,
   owner: Address,
   collection: Address,
   token_id: U256,
   operator: Address,
   dapp: &str,
   loading_msg: &str,
) -> Result<bool, anyhow::Error> {
   let client = ctx.get_client(chain.id()).await?;

   // `getApproved` answers the per-token address, which is zero when only a collection-wide grant exists —
   // so the collection case has to be asked about separately, or we would send a transaction that grants
   // nothing new. This honours a *broader* existing grant; it never creates one.
   let approved = erc721::get_approved(collection, token_id, client.clone()).await? == operator
      || erc721::is_approved_for_all(collection, owner, operator, client).await?;

   if approved {
      return Ok(false);
   }

   SHARED_GUI.write(|gui| {
      gui.loading_window.open(loading_msg);
      gui.request_repaint();
   });

   send_erc721_approve(
      ctx, chain, owner, collection, token_id, operator, dapp,
   )
   .await?;

   Ok(true)
}

/// Ensure `operator` may move every token of `collection` held by `owner`, sending a
/// `setApprovalForAll` when it may not.
///
/// Returns whether a transaction had to be sent.
pub async fn ensure_approval_for_all(
   ctx: ZeusCtx,
   chain: ChainId,
   owner: Address,
   collection: Address,
   standard: NftStandard,
   operator: Address,
   dapp: &str,
   loading_msg: &str,
) -> Result<bool, anyhow::Error> {
   let client = ctx.get_client(chain.id()).await?;

   let approved = match standard {
      NftStandard::Erc721 => {
         erc721::is_approved_for_all(collection, owner, operator, client).await?
      }
      NftStandard::Erc1155 => {
         erc1155::is_approved_for_all(collection, owner, operator, client).await?
      }
   };

   if approved {
      return Ok(false);
   }

   SHARED_GUI.write(|gui| {
      gui.loading_window.open(loading_msg);
      gui.request_repaint();
   });

   send_nft_approve(
      ctx, chain, owner, collection, standard, operator, dapp,
   )
   .await?;

   Ok(true)
}

/// Ensure `owner` has at least `required` allowance of `token` for `spender`,
/// sending an approve transaction when it does not.
///
/// `loading_msg` is shown only when an approve is actually needed, so flows can
/// explain why the user is being asked to approve. Returns whether an approve had
/// to be sent.
pub async fn ensure_allowance(
   ctx: ZeusCtx,
   chain: ChainId,
   owner: Address,
   token: &ERC20Token,
   spender: Address,
   required: U256,
   dapp: &str,
   loading_msg: &str,
) -> Result<bool, anyhow::Error> {
   let client = ctx.get_client(chain.id()).await?;

   if token.allowance(client, owner, spender).await? >= required {
      return Ok(false);
   }

   SHARED_GUI.write(|gui| {
      gui.loading_window.open(loading_msg);
      gui.request_repaint();
   });

   send_token_approve(
      ctx, chain, owner, token, spender, required, None, dapp, false,
   )
   .await?;

   Ok(true)
}
