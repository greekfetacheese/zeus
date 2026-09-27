//! ERC-20 approval helpers for the flows that need an allowance before their
//! real transaction.

use super::analysis::TransactionAnalysis;
use super::send::{SendTxOptions, SendTxRequest, send_transaction};
use crate::core::{DecodedEvent, TokenApproveParams, ZeusCtx};
use crate::gui::SHARED_GUI;
use crate::utils::RT;
use zeus_eth::{
   alloy_primitives::{Address, Log, U256},
   alloy_rpc_types::TransactionReceipt,
   currency::ERC20Token,
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
