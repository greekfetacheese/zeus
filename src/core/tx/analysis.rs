use crate::core::{ZeusContext, ZeusCtx};
use alloy_eips::eip7702::SignedAuthorization;
use serde::{Deserialize, Serialize};
use zeus_eth::{
   abi::{erc20, protocols::across, uniswap, weth9},
   alloy_primitives::{Address, Bytes, Log, U256},
   alloy_provider::Provider,
   alloy_rpc_types::BlockId,
   currency::{Currency, NativeCurrency},
   utils::{
      NumericValue,
      address_book::{
         across_spoke_pool_v2, permit2_contract, uniswap_v3_nft_position_manager,
         universal_router_v2, vitalik, weth,
      },
   },
};

use super::events::decode::{DecodeCtx, decode_transaction};
use super::events::*;

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

   /// Exact output-token received from on-chain balances (Zeus-originated swaps).
   ///
   /// `None` for wallet-connector / inferred swaps — those keep the log heuristic.
   #[serde(default)]
   onchain_swap_received: Option<OnchainSwapReceived>,
}

/// Output-token amount the sender actually received, from `balanceOf` at
/// `tx_block - 1` vs latest. Survives buy/sell tax that logs cannot see.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OnchainSwapReceived {
   pub amount: NumericValue,
   pub amount_usd: Option<NumericValue>,
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

      analysis.decoded_selector = analysis.decode_selector(selector);

      let dctx = DecodeCtx::new(ctx, chain, from, interact_to, call_data, value);
      let (decoded_events, known_events) = decode_transaction(&dctx, &logs, auth_list).await;
      analysis.decoded_events = decoded_events;
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
      self
         .decoded_events
         .iter()
         .filter_map(|e| e.as_transfer().filter(|p| p.is_erc20_transfer()).cloned())
         .collect()
   }

   pub fn token_approvals_len(&self) -> usize {
      self.decoded_events.iter().filter(|t| t.is_token_approval()).count()
   }

   pub fn token_approvals(&self) -> Vec<TokenApproveParams> {
      self
         .decoded_events
         .iter()
         .filter_map(|e| e.as_token_approve().cloned())
         .collect()
   }

   pub fn eth_wraps_len(&self) -> usize {
      self.decoded_events.iter().filter(|t| t.is_wrap_eth()).count()
   }

   pub fn eth_wraps(&self) -> Vec<WrapETHParams> {
      self.decoded_events.iter().filter_map(|e| e.as_wrap_eth().cloned()).collect()
   }

   pub fn weth_unwraps_len(&self) -> usize {
      self.decoded_events.iter().filter(|t| t.is_unwrap_weth()).count()
   }

   pub fn weth_unwraps(&self) -> Vec<UnwrapWETHParams> {
      self.decoded_events.iter().filter_map(|e| e.as_unwrap_weth().cloned()).collect()
   }

   pub fn positions_ops_len(&self) -> usize {
      self.decoded_events.iter().filter(|t| t.is_uniswap_position_op()).count()
   }

   pub fn positions_ops(&self) -> Vec<UniswapPositionParams> {
      self
         .decoded_events
         .iter()
         .filter_map(|e| e.as_uniswap_position().cloned())
         .collect()
   }

   pub fn bridges_len(&self) -> usize {
      self.decoded_events.iter().filter(|t| t.is_bridge()).count()
   }

   pub fn bridges(&self) -> Vec<BridgeParams> {
      self.decoded_events.iter().filter_map(|e| e.as_bridge().cloned()).collect()
   }

   pub fn shields(&self) -> Vec<ShieldParams> {
      self.decoded_events.iter().filter_map(|e| e.as_shield().cloned()).collect()
   }

   pub fn unshields(&self) -> Vec<UnshieldParams> {
      self.decoded_events.iter().filter_map(|e| e.as_unshield().cloned()).collect()
   }

   pub fn swaps_len(&self) -> usize {
      self.decoded_events.iter().filter(|t| t.is_swap()).count()
   }

   pub fn swaps(&self) -> Vec<SwapParams> {
      self.decoded_events.iter().filter_map(|e| e.as_swap().cloned()).collect()
   }

   pub fn eoa_delegates_len(&self) -> usize {
      self.decoded_events.iter().filter(|t| t.is_eoa_delegate()).count()
   }

   pub fn eoa_delegates(&self) -> Vec<EOADelegateParams> {
      self
         .decoded_events
         .iter()
         .filter_map(|e| e.as_eoa_delegate().cloned())
         .collect()
   }

   pub fn permits_len(&self) -> usize {
      self.decoded_events.iter().filter(|t| t.is_permit()).count()
   }

   pub fn permits(&self) -> Vec<PermitParams> {
      self.decoded_events.iter().filter_map(|e| e.as_permit().cloned()).collect()
   }

   pub fn shield_len(&self) -> usize {
      self.decoded_events.iter().filter(|t| t.is_shield()).count()
   }

   pub fn unshield_len(&self) -> usize {
      self.decoded_events.iter().filter(|t| t.is_unshield()).count()
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

   /// Access the stored main-event override (used by ranking).
   pub(crate) fn main_event_opt(&self) -> Option<&DecodedEvent> {
      self.main_event.as_ref()
   }

   pub(crate) fn onchain_swap_received(&self) -> Option<&OnchainSwapReceived> {
      self.onchain_swap_received.as_ref()
   }

   #[cfg(test)]
   pub(crate) fn set_onchain_swap_received(
      &mut self,
      amount: NumericValue,
      amount_usd: Option<NumericValue>,
   ) {
      self.onchain_swap_received = Some(OnchainSwapReceived { amount, amount_usd });
   }

   /// Fetch the sender's output-token balance at `tx_block - 1` and latest,
   /// then store the delta as the exact swap received amount.
   ///
   /// Only used for Zeus-originated ERC-20 output swaps. No-op if there is
   /// no swap or the output is native (ETH already tracked via balances).
   pub async fn apply_onchain_swap_received(
      &mut self,
      ctx: ZeusCtx,
      tx_block: u64,
   ) -> Result<(), anyhow::Error> {
      if tx_block == 0 {
         return Ok(());
      }

      let swaps = self.swaps();
      let Some(last) = swaps.last() else {
         return Ok(());
      };

      let mut output = last.output_currency.clone();
      if output.is_native_wrapped() && self.weth_unwraps_len() == 1 {
         output = NativeCurrency::from(self.chain).into();
      }
      if !output.is_erc20() {
         return Ok(());
      }

      let token = output.to_erc20().into_owned();
      let owner = self.sender;
      let chain = self.chain;
      let before_block = BlockId::number(tx_block - 1);
      let z_client = ctx.get_zeus_client();

      let token_before = token.clone();
      let before_fut = z_client.request(chain, move |client| {
         let token = token_before.clone();
         async move { token.balance_of(client, owner, Some(before_block)).await }
      });
      let token_after = token.clone();
      let after_fut = z_client.request(chain, move |client| {
         let token = token_after.clone();
         async move { token.balance_of(client, owner, None).await }
      });

      let (balance_before, balance_after) = tokio::try_join!(before_fut, after_fut)?;

      if balance_after <= balance_before {
         return Ok(());
      }

      let amount = NumericValue::format_wei(balance_after - balance_before, output.decimals());
      let amount_usd = ctx.get_currency_value_for_amount(amount.f64(), &output);
      self.onchain_swap_received = Some(OnchainSwapReceived {
         amount,
         amount_usd: Some(amount_usd),
      });

      Ok(())
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

   pub fn is_shield(&self) -> bool {
      self.decoded_events() == 1 && self.shield_len() == 1
   }

   pub fn is_unshield(&self) -> bool {
      self.decoded_events() == 1 && self.unshield_len() == 1
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

   pub fn eth_received_usd(&self, ctx: &mut ZeusContext) -> NumericValue {
      let native = NativeCurrency::from(self.chain);
      ctx.get_currency_value_for_amount(self.eth_received().f64(), &Currency::from(native))
   }
}

impl TransactionAnalysis {
   pub fn dummy_token_approval() -> Self {
      let main_event = DecodedEvent::dummy_token_approve();
      let token = main_event.token_approval_params().token.clone();
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
         onchain_swap_received: None,
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
         onchain_swap_received: None,
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
         onchain_swap_received: None,
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
         onchain_swap_received: None,
      }
   }

   pub fn dummy_shield() -> Self {
      let main_event = DecodedEvent::dummy_shield();
      let calldata = Bytes::from_str("0x044a40c3000000000000000000000000000000000000000000000000000000000000002000000000000000000000000000000000000000000000000000000000000000011c7f67dc635a4b43233eee0404407aa83eb2e21cacb163ff9825bb166ba9d88a0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000fff9976782d46cc05630d1f6ebab18b2324d6b1400000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000de0b6b3a7640000a837c375aac12d1c426df9bd30f8471e1ac650c458fe2f07e257e8840552988ff4badb30337e4552bd9e1c47a9602a7d78105f21d8753a8dec2771c41bcb4808fc767b7a598165b1f19f0de719008db65ffb7b80c32b388ed38a806706d108f113ab7f7ca89acdd6cbd7d306f9766a40e4a57cfc4845fa710112753e5958505e").unwrap();
      Self {
         chain: 1,
         sender: vitalik(),
         interact_to: Address::ZERO,
         contract_interact: true,
         value: U256::ZERO,
         call_data: calldata,
         gas_used: 50_000,
         eth_balance_before: U256::ZERO,
         eth_balance_after: U256::ZERO,
         decoded_selector: "Shield".to_string(),
         logs_len: 1,
         known_events: 1,
         decoded_events: vec![main_event.clone()],
         main_event: Some(main_event),
         onchain_swap_received: None,
      }
   }

   pub fn dummy_unshield() -> Self {
      let main_event = DecodedEvent::dummy_unshield();
      Self {
         chain: 1,
         sender: vitalik(),
         interact_to: Address::ZERO,
         contract_interact: true,
         value: U256::ZERO,
         call_data: Bytes::from_str("0x").unwrap(),
         gas_used: 50_000,
         eth_balance_before: U256::ZERO,
         eth_balance_after: U256::ZERO,
         decoded_selector: "Unshield".to_string(),
         logs_len: 1,
         known_events: 1,
         decoded_events: vec![main_event.clone()],
         main_event: Some(main_event),
         onchain_swap_received: None,
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
         onchain_swap_received: None,
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
         onchain_swap_received: None,
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
         onchain_swap_received: None,
      }
   }

   pub fn dummy_uniswap_position_operation() -> Self {
      let main_event = DecodedEvent::dummy_uniswap_position_operation();
      Self {
         chain: 1,
         sender: vitalik(),
         interact_to: uniswap_v3_nft_position_manager(1).unwrap(),
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
         onchain_swap_received: None,
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
         onchain_swap_received: None,
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
         onchain_swap_received: None,
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
         onchain_swap_received: None,
      }
   }

   /// Unknown contract call so the confirm UI can show ERC-7730 rows.
   pub fn dummy_clear_signed() -> Self {
      let pool = Address::from_str("0xA238Dd80C259a72e81d7e4664a9801593F98d1c5").unwrap();
      let calldata = Bytes::from_str("0x617ba037000000000000000000000000833589fcd6edb6e08f4c7c32d4f71b54bda0291300000000000000000000000000000000000000000000000000000000000f424000000000000000000000000022222222222222222222222222222222222222220000000000000000000000000000000000000000000000000000000000000000").unwrap();
      Self {
         chain: 8453,
         sender: vitalik(),
         interact_to: pool,
         contract_interact: true,
         value: U256::ZERO,
         call_data: calldata,
         gas_used: 180_000,
         eth_balance_before: U256::ZERO,
         eth_balance_after: U256::ZERO,
         decoded_selector: "Supply".to_string(),
         logs_len: 0,
         known_events: 0,
         decoded_events: Vec::new(),
         main_event: None,
         onchain_swap_received: None,
      }
   }
}
