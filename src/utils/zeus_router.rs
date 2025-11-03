use zeus_eth::{
   abi::zeus::{ZeusRouter, encode_permit2_permit, encode_z_swap},
   alloy_primitives::{Address, Bytes, U256},
   alloy_sol_types::SolValue,
   amm::uniswap::UniswapPool,
   currency::Currency,
   utils::{address_book, secure_signer::SecureSigner},
};

use crate::{
   core::ZeusCtx,
   utils::{Permit2Details, TimeStamp},
};

use super::swap_quoter::SwapStep;
use anyhow::anyhow;
use serde_json::Value;

#[allow(non_camel_case_types)]
#[derive(Clone, Debug, PartialEq)]
#[repr(u8)]
pub enum Commands {
   PERMIT2_PERMIT = 0x00,
   V2_SWAP = 0x01,
   V3_SWAP = 0x02,
   V4_SWAP = 0x03,
   WRAP_ETH = 0x04,
   WRAP_ALL_ETH = 0x05,
   UNWRAP_WETH = 0x06,
   SWEEP = 0x07,
}

pub struct ZeusExecuteParams {
   pub call_data: Bytes,
   /// The eth to be sent along with the transaction
   pub value: U256,

   pub permit2_details: Option<Permit2Details>,
}

impl Default for ZeusExecuteParams {
   fn default() -> Self {
      Self::new()
   }
}

impl ZeusExecuteParams {
   pub fn new() -> Self {
      Self {
         call_data: Bytes::default(),
         value: U256::ZERO,
         permit2_details: None,
      }
   }

   pub fn set_call_data(&mut self, call_data: Bytes) {
      self.call_data = call_data;
   }

   pub fn set_value(&mut self, value: U256) {
      self.value = value;
   }

   pub fn set_permit2_details(&mut self, permit2_details: Option<Permit2Details>) {
      self.permit2_details = permit2_details;
   }

   pub fn permit2_needs_approval(&self) -> bool {
      if let Some(permit2_details) = &self.permit2_details {
         permit2_details.permit2_needs_approval
      } else {
         false
      }
   }

   pub fn needs_new_signature(&self) -> bool {
      if let Some(permit2_details) = &self.permit2_details {
         permit2_details.needs_new_signature
      } else {
         false
      }
   }

   pub fn message(&self) -> Result<Value, anyhow::Error> {
      if let Some(permit2_details) = &self.permit2_details {
         if let Some(msg) = &permit2_details.msg {
            Ok(msg.clone())
         } else {
            Err(anyhow!("Permit2 Details found but no message"))
         }
      } else {
         Err(anyhow!("No Permit2 Details found"))
      }
   }
}

pub async fn encode_swap(
   ctx: ZeusCtx,
   router_addr: Option<Address>,
   permit2_details: Option<Permit2Details>,
   chain_id: u64,
   swap_steps: Vec<SwapStep<impl UniswapPool + Clone>>,
   amount_in: U256,
   amount_out_min: U256,
   currency_in: Currency,
   currency_out: Currency,
   secure_signer: SecureSigner,
   deadline_in_minutes: u64,
) -> Result<ZeusExecuteParams, anyhow::Error> {
   if swap_steps.is_empty() {
      return Err(anyhow!("No swap steps provided"));
   }

   let router_addr = match router_addr {
      Some(addr) => addr,
      None => address_book::zeus_router(chain_id)?,
   };

   let mut commands = Vec::new();
   let mut inputs = Vec::new();
   let mut execute_params = ZeusExecuteParams::new();

   if currency_in.is_native() {
      // Always set the tx value to the total input amount when dealing with native ETH.
      execute_params.set_value(amount_in);

      // Calculate how much ETH needs to be wrapped for V2/V3 pools.
      let amount_to_wrap: U256 = swap_steps
         .iter()
         .filter(|s| s.currency_in.is_native_wrapped() && !s.pool.dex_kind().is_uniswap_v4())
         .map(|s| s.amount_in.wei())
         .sum();

      if amount_to_wrap > U256::ZERO {
         let data = ZeusRouter::WrapETH {
            recipient: router_addr,
            amount: amount_to_wrap,
         }
         .abi_encode()
         .into();
         commands.push(Commands::WRAP_ETH as u8);
         inputs.push(data);
      }
   }

   // Handle Permit2 approvals
   let first_step_uses_permit2 = currency_in.is_erc20();

   if currency_in.is_erc20() {
      let token_in = currency_in.to_erc20();

      let permit_details = if let Some(permit2_details) = permit2_details {
         permit2_details
      } else {
         let owner = secure_signer.address();

         let details = Permit2Details::new(
            ctx.clone(),
            chain_id,
            &token_in,
            amount_in,
            owner,
            router_addr,
         )
         .await?;
         details
      };

      if permit_details.needs_new_signature {
         let signature = permit_details.sign(&secure_signer).await?;

         let permit_input = encode_permit2_permit(
            token_in.address,
            amount_in,
            permit_details.expiration,
            permit_details.allowance.nonce,
            router_addr,
            permit_details.sig_deadline,
            signature,
         );

         commands.push(Commands::PERMIT2_PERMIT as u8);
         inputs.push(permit_input);
      }

      execute_params.set_permit2_details(Some(permit_details));
   }

   // Track Router ETH and WETH balances after the swaps
   let mut router_eth_balance = U256::ZERO;
   let mut router_weth_balance = U256::ZERO;

   let swaps_len = swap_steps.len();

   for swap in &swap_steps {
      if swap.currency_in.is_native() && router_eth_balance >= swap.amount_in.wei() {
         router_eth_balance -= swap.amount_in.wei();
      }

      if swap.currency_in.is_native_wrapped() && router_weth_balance >= swap.amount_in.wei() {
         router_weth_balance -= swap.amount_in.wei();
      }

      if swap.currency_out.is_native() {
         router_eth_balance += swap.amount_out.wei();
      }

      if swap.currency_out.is_native_wrapped() {
         router_weth_balance += swap.amount_out.wei();
      }

      // If its a single swap and currency_out is ERC20, set the recipient to the EOA
      // otherwise let the router to take the funds to continue the next Op
      let recipient = if swaps_len == 1 && currency_out.is_erc20() {
         secure_signer.address()
      } else {
         router_addr
      };

      // A step uses initial funds ONLY if its input is the main currency_in for the entire trade.
      let uses_initial_funds = swap.currency_in == currency_in;
      let permit2 = uses_initial_funds && first_step_uses_permit2;

      if swap.pool.dex_kind().is_v2() {
         let input = ZeusRouter::V2V3SwapParams {
            amountIn: swap.amount_in.wei(),
            tokenIn: swap.currency_in.address(),
            tokenOut: swap.currency_out.address(),
            pool: swap.pool.address(),
            poolVariant: U256::from(0),
            recipient: recipient,
            fee: swap.pool.fee().fee_u24(),
            permit2,
         }
         .abi_encode()
         .into();

         commands.push(Commands::V2_SWAP as u8);
         inputs.push(input);
      }

      if swap.pool.dex_kind().is_v3() {
         let input = ZeusRouter::V2V3SwapParams {
            amountIn: swap.amount_in.wei(),
            tokenIn: swap.currency_in.address(),
            tokenOut: swap.currency_out.address(),
            pool: swap.pool.address(),
            poolVariant: U256::from(1),
            recipient: recipient,
            fee: swap.pool.fee().fee_u24(),
            permit2,
         }
         .abi_encode()
         .into();
         commands.push(Commands::V3_SWAP as u8);
         inputs.push(input);
      }

      if swap.pool.dex_kind().is_uniswap_v4() {
         let input = ZeusRouter::V4SwapParams {
            currencyIn: swap.currency_in.address(),
            currencyOut: swap.currency_out.address(),
            amountIn: swap.amount_in.wei(),
            fee: swap.pool.fee().fee_u24(),
            tickSpacing: swap.pool.fee().tick_spacing(),
            zeroForOne: swap.pool.zero_for_one(&swap.currency_in),
            hooks: Address::ZERO,
            hookData: Bytes::default(),
            recipient: recipient,
            permit2,
         }
         .abi_encode()
         .into();
         commands.push(Commands::V4_SWAP as u8);
         inputs.push(input);
      }
   }

   handle_recipient_payment(
      swap_steps,
      &currency_out,
      secure_signer.address(),
      router_eth_balance,
      router_weth_balance,
      &mut commands,
      &mut inputs,
   );

   let command_bytes = Bytes::from(commands);
   eprintln!("Command Bytes: {:?}", command_bytes);

   let timestamp = TimeStamp::now_as_secs().add(deadline_in_minutes * 60);
   let deadline = U256::from(timestamp.timestamp());
   let calldata = encode_z_swap(
      command_bytes,
      inputs,
      currency_out.address(),
      amount_out_min,
      deadline,
   );

   execute_params.set_call_data(calldata);

   Ok(execute_params)
}

fn handle_recipient_payment(
   swap_steps: Vec<SwapStep<impl UniswapPool + Clone>>,
   currency_out: &Currency,
   recipient: Address,
   router_eth_balance: U256,
   router_weth_balance: U256,
   commands: &mut Vec<u8>,
   inputs: &mut Vec<Bytes>,
) {
   // Pool will send directly to recipient
   if swap_steps.len() == 1 && !currency_out.is_native() {
      return;
   }

   let router_has_weth_balance = router_weth_balance > U256::ZERO;
   let router_has_eth_balance = router_eth_balance > U256::ZERO;

   let mut should_sweep = true;

   // Handle ETH output

   // Router has WETH, call unwrap WETH and send all the ETH to the recipient
   if currency_out.is_native() && router_has_weth_balance {
      let data = ZeusRouter::UnwrapWETH { recipient }.abi_encode().into();
      commands.push(Commands::UNWRAP_WETH as u8);
      inputs.push(data);

      should_sweep = false;
   }

   // Handle WETH output

   // Router has ETH, call WrapAllETH and send all the WETH to the recipient
   if currency_out.is_native_wrapped() && router_has_eth_balance {
      let data = ZeusRouter::WrapAllETH { recipient }.abi_encode().into();
      commands.push(Commands::WRAP_ALL_ETH as u8);
      inputs.push(data);

      should_sweep = false;
   }

   // Handles Both ETH and ERC20 output
   if should_sweep {
      let sweep_params = ZeusRouter::Sweep {
         currency: currency_out.address(),
         recipient,
      };

      let data = sweep_params.abi_encode().into();
      commands.push(Commands::SWEEP as u8);
      inputs.push(data);
   }
}
