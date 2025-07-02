use crate::core::ZeusCtx;
use zeus_eth::{
   abi::{erc20, protocols::across, uniswap, weth9},
   alloy_primitives::{Address, Bytes, Log, U256},
   alloy_provider::Provider,
   currency::{Currency, NativeCurrency},
   dapps::Dapp,
   utils::NumericValue,
};

use super::transaction::*;
use serde::{Deserialize, Serialize};

/// An analysis of all recognizable events and data within a single transaction.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TransactionAnalysis {
   pub chain: u64,
   /// Who initiated the transaction
   pub sender: Address,
   /// The address the sender interacted with
   pub interact_to: Address,
   pub contract_interact: bool,
   pub value: U256,
   pub call_data: Bytes,
   pub gas_used: u64,
   /// Native balance before the transaction
   pub eth_balance_before: U256,
   /// Native balance after the transaction
   pub eth_balance_after: U256,

   /// Decoded function selector
   /// If not known we keep the selector's keccak256 hash
   pub decoded_selector: String,

   // All decoded events
   pub erc20_transfers: Vec<ERC20TransferParams>,
   pub token_approvals: Vec<TokenApproveParams>,
   pub eth_wraps: Vec<WrapETHParams>,
   pub weth_unwraps: Vec<UnwrapWETHParams>,
   pub positions_ops: Vec<UniswapPositionParams>,
   pub bridge: Vec<BridgeParams>,
   pub swaps: Vec<SwapParams>,
}

impl TransactionAnalysis {
   pub async fn new(
      ctx: ZeusCtx,
      chain: u64,
      from: Address,
      interact_to: Address,
      contract_interact: Option<bool>,
      call_data: Bytes,
      value: U256,
      logs: Vec<Log>,
      gas_used: u64,
      eth_balance_before: U256,
      eth_balance_after: U256,
   ) -> Result<Self, anyhow::Error> {
      let contract_interact = if let Some(contract_interact) = contract_interact {
         contract_interact
      } else {
         let client = ctx.get_client(chain).await?;
         let bytecode = client.get_code_at(interact_to).await?;
         bytecode.len() > 0
      };

      let selector = call_data.get(0..4).unwrap_or_default();

      let mut analysis = TransactionAnalysis {
         chain,
         sender: from,
         interact_to,
         contract_interact,
         value,
         call_data: call_data.clone(),
         eth_balance_before,
         eth_balance_after,
         gas_used,
         ..Default::default()
      };

      let decoded_selector = analysis.decode_selector(&selector);
      analysis.decoded_selector = decoded_selector;

      let log_slice = logs.as_slice();
      for log in &logs {
         if let Ok(params) = WrapETHParams::from_log(ctx.clone(), chain, log) {
            analysis.eth_wraps.push(params);
            continue;
         }

         if let Ok(params) = UnwrapWETHParams::from_log(ctx.clone(), chain, log) {
            analysis.weth_unwraps.push(params);
            continue;
         }

         if let Ok(params) = ERC20TransferParams::from_log(ctx.clone(), chain, log).await {
            analysis.erc20_transfers.push(params);
            continue;
         }

         if let Ok(params) = TokenApproveParams::from_log(ctx.clone(), chain, log).await {
            analysis.token_approvals.push(params);
            continue;
         }

         if let Ok(params) = BridgeParams::from_log(ctx.clone(), chain, log).await {
            analysis.bridge.push(params);
            continue;
         }

         if let Ok(params) = SwapParams::from_uniswap_v2(ctx.clone(), chain, from, log).await {
            analysis.swaps.push(params);
            continue;
         }

         if let Ok(params) = SwapParams::from_uniswap_v3(ctx.clone(), chain, from, log).await {
            analysis.swaps.push(params);
            continue;
         }

         if let Ok(params) = SwapParams::from_uniswap_v4(ctx.clone(), chain, from, log).await {
            analysis.swaps.push(params);
            continue;
         }

         if let Ok(params) =
            UniswapPositionParams::collect_fees_for_v3_from_log(ctx.clone(), chain, from, log).await
         {
            analysis.positions_ops.push(params);
            continue;
         }

         if let Ok(params) = UniswapPositionParams::add_liquidity_for_v3_from_logs(
            ctx.clone(),
            chain,
            from,
            log_slice,
         )
         .await
         {
            analysis.positions_ops.push(params);
            continue;
         }

         if let Ok(params) = UniswapPositionParams::decrease_liquidity_for_v3_from_logs(
            ctx.clone(),
            chain,
            from,
            log_slice,
         )
         .await
         {
            analysis.positions_ops.push(params);
            continue;
         }
      }

      Ok(analysis)
   }

   fn decode_selector(&self, selector: &[u8]) -> String {
      // convert the selector to a string
      let mut selector_str = format!("{:?}", selector);

      if selector == weth9::deposit_selector() {
         selector_str = "Deposit".to_string();
      }

      if selector == weth9::withdraw_selector() {
         selector_str = "Withdraw".to_string();
      }

      if selector == erc20::transfer_selector() {
         selector_str = "Transfer".to_string();
      }

      if selector == erc20::approve_selector() {
         selector_str = "Approve".to_string();
      }

      if selector == uniswap::universal_router_v2::execute_call_selector() {
         selector_str = "Execute".to_string();
      }

      if selector == uniswap::universal_router_v2::execute_with_deadline_call_selector() {
         selector_str = "Execute".to_string();
      }

      if selector == uniswap::nft_position::collect_call_selector() {
         selector_str = "Collect".to_string();
      }

      if selector == uniswap::nft_position::decrease_liquidity_call_selector() {
         selector_str = "Decrease Liquidity".to_string();
      }

      if selector == uniswap::nft_position::increase_liquidity_call_selector() {
         selector_str = "Increase Liquidity".to_string();
      }

      if selector == uniswap::nft_position::mint_call_selector() {
         selector_str = "Mint".to_string();
      }

      if selector == across::deposit_v3_selector() {
         selector_str = "Deposit V3".to_string();
      }

      selector_str
   }

   pub fn decoded_events(&self) -> usize {
      self.erc20_transfers.len()
         + self.token_approvals.len()
         + self.eth_wraps.len()
         + self.weth_unwraps.len()
         + self.positions_ops.len()
         + self.bridge.len()
         + self.swaps.len()
   }

   /// Try to infer a high-level action from the analysis
   pub fn infer_action(&self, ctx: ZeusCtx, chain: u64) -> TransactionAction {
      // ETH Transfer
      if self.is_native_transfer() {
         let native: Currency = NativeCurrency::from(chain).into();
         let amount = NumericValue::format_wei(self.value, native.decimals());
         let amount_usd = ctx.get_currency_value_for_amount(amount.f64(), &native);

         let params = TransferParams {
            currency: native,
            amount,
            amount_usd: Some(amount_usd),
            sender: self.sender,
            recipient: self.interact_to,
         };

         return TransactionAction::Transfer(params);
      }

      let transfers_len = self.erc20_transfers.len();
      let approvals_len = self.token_approvals.len();
      let positions_ops_len = self.positions_ops.len();
      let bridge_len = self.bridge.len();
      let swaps_len = self.swaps.len();

      // Single ERC20 Transfer
      if self.decoded_events() == 1 && transfers_len == 1 {
         let params = self.erc20_transfers[0].clone();
         return TransactionAction::ERC20Transfer(params);
      }

      // Single Token Approval
      if self.decoded_events() == 1 && approvals_len == 1 {
         let params = self.token_approvals[0].clone();
         return TransactionAction::TokenApprove(params);
      }

      // Single Wrap ETH
      if self.is_wrap_eth() {
         let params = self.eth_wraps[0].clone();
         return TransactionAction::WrapETH(params);
      }

      // Single Unwrap WETH
      if self.is_unwrap_weth() {
         let params = self.weth_unwraps[0].clone();
         return TransactionAction::UnwrapWETH(params);
      }

      // Single Uniswap Position Operation
      if positions_ops_len == 1 {
         let params = self.positions_ops[0].clone();
         return TransactionAction::UniswapPositionOperation(params);
      }

      // Single Bridge
      if bridge_len == 1 {
         let params = self.bridge[0].clone();
         return TransactionAction::Bridge(params);
      }

      // Single Swap
      if swaps_len == 1 {
         let params = self.swaps[0].clone();
         return TransactionAction::SwapToken(params);
      }

      // A lot of swaps go through multiple pools
      // Will try our best to figure out the input and output currencies but it's not perfect
      // Assuming that the recipient is the same address that sent the tx
      // If its not this will not return the recipient address
      if swaps_len > 1 {
         let mut params = SwapParams {
            dapp: Dapp::Uniswap,
            sender: self.sender,
            ..Default::default()
         };

         for (i, swap) in self.swaps.iter().enumerate() {
            let is_first = i == 0;
            let is_last = i == swaps_len - 1;

            if is_first {
               params.input_currency = swap.input_currency.clone();
               params.amount_in = swap.amount_in.clone();
               params.amount_in_usd = swap.amount_in_usd.clone();
            }

            if is_last {
               params.output_currency = swap.output_currency.clone();

               // find the actual amount received from the transfer logs
               if swap.output_currency.is_erc20() {
                  for transfer in self.erc20_transfers.iter() {
                     if transfer.token.address == swap.output_currency.address() {
                        if !transfer.recipient == self.sender {
                           continue;
                        }
                        params.received = transfer.amount.clone();
                        params.received_usd = transfer.amount_usd.clone();
                        params.recipient = Some(transfer.recipient);
                        break;
                     }
                  }
               } else {
                  // Output is native ETH
                  // For now we are going to trust the swap logs
                  params.received = swap.received.clone();
                  params.received_usd = swap.received_usd.clone();
               }
            }
         }

         return TransactionAction::SwapToken(params);
      }

      TransactionAction::Other
   }

   pub fn is_native_transfer(&self) -> bool {
      self.value > U256::ZERO && self.call_data.is_empty() && self.decoded_events() == 0
   }

   pub fn is_wrap_eth(&self) -> bool {
      self.decoded_events() == 1 && self.eth_wraps.len() == 1
   }

   pub fn is_unwrap_weth(&self) -> bool {
      self.decoded_events() == 1 && self.weth_unwraps.len() == 1
   }

   pub fn is_swap(&self) -> bool {
      self.swaps.len() >= 1
   }

   pub fn value_sent(&self) -> NumericValue {
      let native = NativeCurrency::from(self.chain);
      NumericValue::format_wei(self.value, native.decimals)
   }

   pub fn value_sent_usd(&self, ctx: ZeusCtx) -> NumericValue {
      let native = NativeCurrency::from(self.chain);
      let value = ctx.get_currency_value_for_amount(
         self.value_sent().f64(),
         &Currency::from(native.clone()),
      );
      value
   }

   pub fn eth_received(&self) -> NumericValue {
      let native = NativeCurrency::from(self.chain);
      if self.eth_balance_after > self.eth_balance_before {
         NumericValue::format_wei(
            self.eth_balance_after - self.eth_balance_before,
            native.decimals,
         )
      } else {
         NumericValue::default()
      }
   }

   pub fn eth_received_usd(&self, ctx: ZeusCtx) -> NumericValue {
      let native = NativeCurrency::from(self.chain);
      let value =
         ctx.get_currency_value_for_amount(self.eth_received().f64(), &Currency::from(native));
      value
   }
}
