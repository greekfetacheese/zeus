use super::transaction::*;
use crate::core::{Dapp, ZeusCtx};
use crate::utils::truncate_address;
use alloy_eips::eip7702::SignedAuthorization;
use serde::{Deserialize, Serialize};
use zeus_eth::{
   abi::{erc20, protocols::across, uniswap, weth9},
   alloy_primitives::{Address, Bytes, Log, U256},
   alloy_provider::Provider,
   currency::{Currency, NativeCurrency},
   utils::{
      NumericValue,
      address_book::{
         across_spoke_pool_v2, permit2_contract, uniswap_nft_position_manager, universal_router_v2,
         vitalik, weth,
      },
   },
};

use std::str::FromStr;

/// An analysis of all recognizable events and data within a single transaction.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TransactionAnalysis {
   pub chain: u64,
   /// Who initiated the transaction
   pub sender: Address,
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

   /// Events in total by how many logs were emitted
   pub logs_len: usize,

   /// Total decoded events by how many logs were decoded
   ///
   /// ETH transfers and EIP7702 Authorization events are not counted
   pub known_events: usize,

   // All decoded events
   pub decoded_events: Vec<DecodedEvent>,
   main_event: Option<DecodedEvent>,
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
      auth_list: Vec<SignedAuthorization>,
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
         logs_len: logs.len(),
         ..Default::default()
      };

      let decoded_selector = analysis.decode_selector(selector);
      analysis.decoded_selector = decoded_selector;

      let log_slice = logs.as_slice();
      let mut known_events = 0;

      for auth in auth_list {
         let params = EOADelegateParams::new(chain, from, auth);
         analysis.decoded_events.push(DecodedEvent::EOADelegate(params));
      }

      for log in &logs {
         if let Ok(params) = WrapETHParams::from_log(ctx.clone(), chain, log) {
            analysis.decoded_events.push(DecodedEvent::WrapETH(params));
            known_events += 1;
            continue;
         }

         if let Ok(params) = UnwrapWETHParams::from_log(ctx.clone(), chain, log) {
            analysis.decoded_events.push(DecodedEvent::UnwrapWETH(params));
            known_events += 1;
            continue;
         }

         if let Ok(params) = TransferParams::new(
            ctx.clone(),
            chain,
            from,
            interact_to,
            call_data.clone(),
            value,
            log,
         )
         .await
         {
            if params.is_erc20_transfer() {
               known_events += 1;
            }

            analysis.decoded_events.push(DecodedEvent::Transfer(params));
            continue;
         }

         if let Ok(params) = TokenApproveParams::from_log(ctx.clone(), chain, log).await {
            analysis.decoded_events.push(DecodedEvent::TokenApprove(params));
            known_events += 1;
            continue;
         }

         if let Ok(params) = PermitParams::from_log(ctx.clone(), chain, log).await {
            analysis.decoded_events.push(DecodedEvent::Permit(params));
            known_events += 1;
            continue;
         }

         if let Ok(params) = BridgeParams::from_log(ctx.clone(), chain, log).await {
            analysis.decoded_events.push(DecodedEvent::Bridge(params));
            known_events += 1;
            continue;
         }

         if let Ok(params) = SwapParams::from_uniswap_v2(ctx.clone(), chain, from, log).await {
            analysis.decoded_events.push(DecodedEvent::SwapToken(params));
            known_events += 1;
            continue;
         }

         if let Ok(params) = SwapParams::from_uniswap_v3(ctx.clone(), chain, from, log).await {
            analysis.decoded_events.push(DecodedEvent::SwapToken(params));
            known_events += 1;
            continue;
         }

         if let Ok(params) = SwapParams::from_uniswap_v4(ctx.clone(), chain, from, log).await {
            analysis.decoded_events.push(DecodedEvent::SwapToken(params));
            known_events += 1;
            continue;
         }

         if let Ok(params) =
            UniswapPositionParams::collect_fees_for_v3_from_log(ctx.clone(), chain, from, log).await
         {
            analysis.decoded_events.push(DecodedEvent::UniswapPositionOperation(params));
            known_events += 1;
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
            analysis.decoded_events.push(DecodedEvent::UniswapPositionOperation(params));
            known_events += 1;
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
            analysis.decoded_events.push(DecodedEvent::UniswapPositionOperation(params));
            known_events += 1;
            continue;
         }
      }

      analysis.known_events = known_events;

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

   pub fn erc20_transfers_len(&self) -> usize {
      self.decoded_events.iter().filter(|t| t.is_erc20_transfer()).count()
   }

   pub fn erc20_transfers(&self) -> Vec<TransferParams> {
      let mut params = Vec::new();
      for event in &self.decoded_events {
         if event.is_erc20_transfer() {
            params.push(event.transfer_params().clone());
         }
      }
      params
   }

   pub fn token_approvals_len(&self) -> usize {
      self.decoded_events.iter().filter(|t| t.is_token_approval()).count()
   }

   pub fn token_approvals(&self) -> Vec<TokenApproveParams> {
      let mut params = Vec::new();
      for event in &self.decoded_events {
         if event.is_token_approval() {
            params.push(event.token_approval_params().clone());
         }
      }
      params
   }

   pub fn eth_wraps_len(&self) -> usize {
      self.decoded_events.iter().filter(|t| t.is_wrap_eth()).count()
   }

   pub fn eth_wraps(&self) -> Vec<WrapETHParams> {
      let mut params = Vec::new();
      for event in &self.decoded_events {
         if event.is_wrap_eth() {
            params.push(event.wrap_eth_params().clone());
         }
      }
      params
   }

   pub fn weth_unwraps_len(&self) -> usize {
      self.decoded_events.iter().filter(|t| t.is_unwrap_weth()).count()
   }

   pub fn weth_unwraps(&self) -> Vec<UnwrapWETHParams> {
      let mut params = Vec::new();
      for event in &self.decoded_events {
         if event.is_unwrap_weth() {
            params.push(event.unwrap_weth_params().clone());
         }
      }
      params
   }

   pub fn positions_ops_len(&self) -> usize {
      self.decoded_events.iter().filter(|t| t.is_uniswap_position_op()).count()
   }

   pub fn positions_ops(&self) -> Vec<UniswapPositionParams> {
      let mut params = Vec::new();
      for event in &self.decoded_events {
         if event.is_uniswap_position_op() {
            params.push(event.uniswap_position_params().clone());
         }
      }
      params
   }

   pub fn bridges_len(&self) -> usize {
      self.decoded_events.iter().filter(|t| t.is_bridge()).count()
   }

   pub fn bridges(&self) -> Vec<BridgeParams> {
      let mut params = Vec::new();
      for event in &self.decoded_events {
         if event.is_bridge() {
            params.push(event.bridge_params().clone());
         }
      }
      params
   }

   pub fn swaps_len(&self) -> usize {
      self.decoded_events.iter().filter(|t| t.is_swap()).count()
   }

   pub fn swaps(&self) -> Vec<SwapParams> {
      let mut params = Vec::new();
      for event in &self.decoded_events {
         if event.is_swap() {
            params.push(event.swap_params().clone());
         }
      }
      params
   }

   pub fn eoa_delegates_len(&self) -> usize {
      self.decoded_events.iter().filter(|t| t.is_eoa_delegate()).count()
   }

   pub fn eoa_delegates(&self) -> Vec<EOADelegateParams> {
      let mut params = Vec::new();
      for event in &self.decoded_events {
         if event.is_eoa_delegate() {
            params.push(event.eoa_delegate_params().clone());
         }
      }
      params
   }

   pub fn permits_len(&self) -> usize {
      self.decoded_events.iter().filter(|t| t.is_permit()).count()
   }

   pub fn permits(&self) -> Vec<PermitParams> {
      let mut params = Vec::new();
      for event in &self.decoded_events {
         if event.is_permit() {
            params.push(event.permit_params().clone());
         }
      }
      params
   }

   pub fn total_events(&self) -> usize {
      self.logs_len
   }

   pub fn decoded_events(&self) -> usize {
      let mut total = 0;
      for event in &self.decoded_events {
         if event.is_native_transfer() || event.is_eoa_delegate() {
            continue;
         }

         total += 1;
      }
      total
   }

   pub fn set_main_event(&mut self, event: DecodedEvent) {
      self.main_event = Some(event);
   }

   pub fn remove_main_event(&mut self) {
      self.main_event = None;
   }

   /// Try to infer the main event from the analysis
   pub fn infer_main_event(&self, ctx: ZeusCtx, chain: u64) -> DecodedEvent {
      if self.main_event.is_some() {
         return self.main_event.clone().unwrap();
      }

      // ETH Transfer
      if self.is_native_transfer() {
         let native: Currency = NativeCurrency::from(chain).into();
         let amount = NumericValue::format_wei(self.value, native.decimals());
         let amount_usd = ctx.get_currency_value_for_amount(amount.f64(), &native);
         let sender = self.sender;
         let recipient = self.interact_to;

         let sender_name_opt = ctx.get_address_name(chain, sender);
         let recipient_name_opt = ctx.get_address_name(chain, recipient);

         let sender_str = if let Some(sender_name) = sender_name_opt {
            sender_name
         } else {
            truncate_address(sender.to_string())
         };

         let recipient_str = if let Some(recipient_name) = recipient_name_opt {
            recipient_name
         } else {
            truncate_address(recipient.to_string())
         };

         let params = TransferParams {
            currency: native,
            amount,
            amount_usd: Some(amount_usd),
            real_amount_sent: None,
            real_amount_sent_usd: None,
            sender,
            sender_str,
            recipient,
            recipient_str,
         };

         return DecodedEvent::Transfer(params);
      }

      // Single ERC20 Transfer
      if self.decoded_events() == 1 && self.erc20_transfers_len() == 1 {
         let params = self.erc20_transfers()[0].clone();
         return DecodedEvent::Transfer(params);
      }

      // Single Token Approval
      if self.decoded_events() == 1 && self.token_approvals_len() == 1 {
         let params = self.token_approvals()[0].clone();
         return DecodedEvent::TokenApprove(params);
      }

      // Single Wrap ETH
      if self.decoded_events() == 1 && self.eth_wraps_len() == 1 {
         let params = self.eth_wraps()[0].clone();
         return DecodedEvent::WrapETH(params);
      }

      // Single Unwrap WETH
      if self.decoded_events() == 1 && self.weth_unwraps_len() == 1 {
         let params = self.weth_unwraps()[0].clone();
         return DecodedEvent::UnwrapWETH(params);
      }

      // Single Uniswap Position Operation
      if self.decoded_events() == 1 && self.positions_ops_len() == 1 {
         let params = self.positions_ops()[0].clone();
         return DecodedEvent::UniswapPositionOperation(params);
      }

      // Bridge
      if self.bridges_len() == 1 {
         let params = self.bridges()[0].clone();
         return DecodedEvent::Bridge(params);
      }

      // Single EOA Delegate
      if self.eoa_delegates_len() == 1 {
         let params = self.eoa_delegates()[0].clone();
         return DecodedEvent::EOADelegate(params);
      }

      // Single Swap
      if self.swaps_len() == 1 {
         let mut params = self.swaps()[0].clone();

         // Handle ETH/WETH abstraction
         if params.input_currency.is_native_wrapped() {
            if self.value > U256::ZERO {
               params.input_currency = NativeCurrency::from(self.chain).into();
            }
         }

         if params.output_currency.is_native_wrapped() && self.weth_unwraps_len() == 1 {
            params.output_currency = NativeCurrency::from(self.chain).into();
         }

         return DecodedEvent::SwapToken(params);
      }

      // A lot of swaps go through multiple pools
      // Will try our best to figure out the input and output currencies but it's not perfect
      // Assuming that the recipient is the same address that sent the tx
      // If its not this will not return the recipient address
      let swaps_len = self.swaps_len();
      if swaps_len > 1 {
         let mut params = SwapParams {
            dapp: Dapp::Uniswap,
            sender: self.sender,
            ..Default::default()
         };

         let erc20_transfers = self.erc20_transfers();
         let swaps = self.swaps();

         for (i, swap) in swaps.iter().enumerate() {
            let is_first = i == 0;
            let is_last = i == swaps_len - 1;

            if is_first {
               let mut input = swap.input_currency.clone();

               // Handle ETH/WETH abstraction
               if input.is_native_wrapped() {
                  if self.value > U256::ZERO {
                     input = NativeCurrency::from(self.chain).into();
                  }
               }

               params.input_currency = input;
               params.amount_in = swap.amount_in.clone();
               params.amount_in_usd = swap.amount_in_usd.clone();
            }

            if is_last {
               let mut output = swap.output_currency.clone();

               // Handle ETH/WETH abstraction
               if output.is_native_wrapped() && self.weth_unwraps_len() == 1 {
                  output = NativeCurrency::from(self.chain).into();
               }

               params.output_currency = output;

               // find the actual amount received from the transfer logs
               if swap.output_currency.is_erc20() {
                  for transfer in erc20_transfers.iter() {
                     if transfer.currency.address() == swap.output_currency.address() {
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
                  params.received = swap.received.clone();
                  params.received_usd = swap.received_usd.clone();
               }
            }
         }

         return DecodedEvent::SwapToken(params);
      }

      DecodedEvent::Other
   }

   pub fn is_native_transfer(&self) -> bool {
      self.value > U256::ZERO && self.call_data.is_empty() && self.decoded_events() == 0
   }

   pub fn is_unwrap_weth(&self) -> bool {
      self.decoded_events() == 1 && self.weth_unwraps_len() == 1
   }

   pub fn is_swap(&self) -> bool {
      self.swaps_len() != 0
   }

   pub fn value_sent(&self) -> NumericValue {
      let native = NativeCurrency::from(self.chain);
      NumericValue::format_wei(self.value, native.decimals)
   }

   pub fn value_sent_usd(&self, ctx: ZeusCtx) -> NumericValue {
      let native = NativeCurrency::from(self.chain);
      ctx.get_currency_value_for_amount(
         self.value_sent().f64(),
         &Currency::from(native.clone()),
      )
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
      ctx.get_currency_value_for_amount(self.eth_received().f64(), &Currency::from(native))
   }
}

impl TransactionAnalysis {
   pub fn dummy_token_approval() -> Self {
      let main_event = DecodedEvent::dummy_token_approve();
      let token = main_event.token_approval_params().token[0].clone();
      Self {
         chain: 1,
         sender: vitalik(),
         interact_to: token.address,
         contract_interact: true,
         value: U256::ZERO,
         call_data: Bytes::from_str("0x").unwrap(),
         gas_used: 50_000,
         eth_balance_before: U256::ZERO,
         eth_balance_after: U256::ZERO,
         decoded_selector: "Approve".to_string(),
         logs_len: 1,
         known_events: 1,
         decoded_events: vec![main_event.clone()],
         main_event: Some(main_event),
      }
   }

   pub fn dummy_swap() -> Self {
      let main_event = DecodedEvent::dummy_swap();
      Self {
         chain: 1,
         sender: vitalik(),
         interact_to: universal_router_v2(1).unwrap(),
         contract_interact: true,
         value: U256::ZERO,
         call_data: Bytes::from_str("0x").unwrap(),
         gas_used: 150_000,
         eth_balance_before: U256::ZERO,
         eth_balance_after: U256::ZERO,
         decoded_selector: "Swap".to_string(),
         logs_len: 1,
         known_events: 1,
         decoded_events: vec![main_event.clone()],
         main_event: Some(main_event),
      }
   }

   pub fn dummy_bridge() -> Self {
      let main_event = DecodedEvent::dummy_bridge();
      Self {
         chain: 1,
         sender: vitalik(),
         interact_to: across_spoke_pool_v2(1).unwrap(),
         contract_interact: true,
         value: U256::ZERO,
         call_data: Bytes::from_str("0x").unwrap(),
         gas_used: 50_000,
         eth_balance_before: U256::ZERO,
         eth_balance_after: U256::ZERO,
         decoded_selector: "Bridge".to_string(),
         logs_len: 1,
         known_events: 1,
         decoded_events: vec![main_event.clone()],
         main_event: Some(main_event),
      }
   }

   pub fn dummy_transfer() -> Self {
      let main_event = DecodedEvent::dummy_transfer();
      Self {
         chain: 1,
         sender: vitalik(),
         interact_to: Address::from_str("0x0000000000000000000000000000000000000000").unwrap(),
         contract_interact: false,
         value: U256::ZERO,
         call_data: Bytes::from_str("0x").unwrap(),
         gas_used: 21_000,
         eth_balance_before: U256::ZERO,
         eth_balance_after: U256::ZERO,
         decoded_selector: "Transfer".to_string(),
         logs_len: 0,
         known_events: 0,
         decoded_events: vec![main_event.clone()],
         main_event: Some(main_event),
      }
   }

   pub fn dummy_erc20_transfer() -> Self {
      let main_event = DecodedEvent::dummy_erc20_transfer();
      let token = main_event.transfer_params().currency.address();
      Self {
         chain: 1,
         sender: vitalik(),
         interact_to: token,
         contract_interact: true,
         value: U256::ZERO,
         call_data: Bytes::from_str("0x").unwrap(),
         gas_used: 50_000,
         eth_balance_before: U256::ZERO,
         eth_balance_after: U256::ZERO,
         decoded_selector: "ERC20 Transfer".to_string(),
         logs_len: 1,
         known_events: 1,
         decoded_events: vec![main_event.clone()],
         main_event: Some(main_event),
      }
   }

   pub fn dummy_unwrap_weth() -> Self {
      let main_event = DecodedEvent::dummy_unwrap_weth();
      Self {
         chain: 1,
         sender: vitalik(),
         interact_to: weth(1).unwrap(),
         contract_interact: true,
         value: U256::ZERO,
         call_data: Bytes::from_str("0x").unwrap(),
         gas_used: 50_000,
         eth_balance_before: U256::ZERO,
         eth_balance_after: U256::ZERO,
         decoded_selector: "Withdraw".to_string(),
         logs_len: 1,
         known_events: 1,
         decoded_events: vec![main_event.clone()],
         main_event: Some(main_event),
      }
   }

   pub fn dummy_wrap_eth() -> Self {
      let main_event = DecodedEvent::dummy_wrap_eth();
      Self {
         chain: 1,
         sender: vitalik(),
         interact_to: weth(1).unwrap(),
         contract_interact: true,
         value: U256::ZERO,
         call_data: Bytes::from_str("0x").unwrap(),
         gas_used: 50_000,
         eth_balance_before: U256::ZERO,
         eth_balance_after: U256::ZERO,
         decoded_selector: "Deposit".to_string(),
         logs_len: 1,
         known_events: 1,
         decoded_events: vec![main_event.clone()],
         main_event: Some(main_event),
      }
   }

   pub fn dummy_uniswap_position_operation() -> Self {
      let main_event = DecodedEvent::dummy_uniswap_position_operation();
      Self {
         chain: 1,
         sender: vitalik(),
         interact_to: uniswap_nft_position_manager(1).unwrap(),
         contract_interact: true,
         value: U256::ZERO,
         call_data: Bytes::from_str("0x").unwrap(),
         gas_used: 100_000,
         eth_balance_before: U256::ZERO,
         eth_balance_after: U256::ZERO,
         decoded_selector: "AddLiquidity".to_string(),
         logs_len: 1,
         known_events: 1,
         decoded_events: vec![main_event.clone()],
         main_event: Some(main_event),
      }
   }

   pub fn dummy_permit() -> Self {
      let main_event = DecodedEvent::dummy_permit();
      Self {
         chain: 1,
         sender: vitalik(),
         interact_to: permit2_contract(1).unwrap(),
         contract_interact: true,
         value: U256::ZERO,
         call_data: Bytes::from_str("0x").unwrap(),
         gas_used: 50_000,
         eth_balance_before: U256::ZERO,
         eth_balance_after: U256::ZERO,
         decoded_selector: "Permit".to_string(),
         logs_len: 1,
         known_events: 1,
         decoded_events: vec![main_event.clone()],
         main_event: Some(main_event),
      }
   }

   pub fn dummy_eoa_delegate() -> Self {
      let main_event = DecodedEvent::dummy_eoa_delegate();
      Self {
         chain: 1,
         sender: vitalik(),
         interact_to: Address::from_str("0x0000000000000000000000000000000000000000").unwrap(),
         contract_interact: true,
         value: U256::ZERO,
         call_data: Bytes::from_str("0x").unwrap(),
         gas_used: 50_000,
         eth_balance_before: U256::ZERO,
         eth_balance_after: U256::ZERO,
         decoded_selector: "EOA Delegate".to_string(),
         logs_len: 0,
         known_events: 0,
         decoded_events: vec![main_event.clone()],
         main_event: Some(main_event),
      }
   }

   pub fn unknown_tx_1() -> Self {
      let erc20_transfer = DecodedEvent::dummy_erc20_transfer();
      let unwrap_weth = DecodedEvent::dummy_unwrap_weth();
      let balance_after = NumericValue::parse_to_wei("1", 18);

      Self {
         chain: 1,
         sender: vitalik(),
         interact_to: Address::from_str("0x0000000000000000000000000000000000000000").unwrap(),
         contract_interact: true,
         value: U256::ZERO,
         call_data: Bytes::from_str("0x").unwrap(),
         gas_used: 50_000,
         eth_balance_before: U256::ZERO,
         eth_balance_after: balance_after.wei(),
         decoded_selector: "Unknown".to_string(),
         logs_len: 2,
         known_events: 2,
         decoded_events: vec![erc20_transfer, unwrap_weth],
         main_event: None,
      }
   }
}
