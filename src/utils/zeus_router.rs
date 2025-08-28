use zeus_eth::{
   abi::{permit::allowance, zeus_router as zeus_router_abi},
   alloy_contract::private::{Network, Provider},
   alloy_primitives::{Address, Bytes, U256},
   alloy_signer::Signer,
   alloy_sol_types::SolValue,
   amm::uniswap::UniswapPool,
   currency::Currency,
   utils::{
      NumericValue, address_book, generate_permit2_single_value, parse_typed_data,
      secure_signer::{SecureSigner, erase_signer},
   },
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
   UNWRAP_WETH = 0x05,
   SWEEP = 0x06,
}

pub struct ZeusExecuteParams {
   pub call_data: Bytes,
   /// The eth to be sent along with the transaction
   pub value: U256,
   /// Whether permit2 contract needs to be approved to spend our tokens
   pub permit2_needs_approval: bool,
   /// The Permi2 message to be signed if needed
   ///
   /// This is just to show it in a UI, the message if any already signed internally
   pub message: Option<Value>,

   /// For sanity check, make sure we dont fuck up
   pub calculated_min_amount_out: U256,
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
         permit2_needs_approval: false,
         message: None,
         calculated_min_amount_out: U256::ZERO,
      }
   }

   pub fn set_call_data(&mut self, call_data: Bytes) {
      self.call_data = call_data;
   }

   pub fn set_value(&mut self, value: U256) {
      self.value = value;
   }

   pub fn set_permit2_needs_approval(&mut self, permit2_needs_approval: bool) {
      self.permit2_needs_approval = permit2_needs_approval;
   }

   pub fn set_message(&mut self, message: Option<Value>) {
      self.message = message;
   }

   pub fn set_calculated_min_amount_out(&mut self, calculated_min_amount_out: U256) {
      self.calculated_min_amount_out = calculated_min_amount_out;
   }
}

pub async fn encode_swap<P, N>(
   client: P,
   chain_id: u64,
   router_address: Address,
   swap_steps: Vec<SwapStep<impl UniswapPool + Clone>>,
   amount_in: U256,
   amount_out_min: U256,
   slippage: f64,
   currency_in: Currency,
   currency_out: Currency,
   secure_signer: SecureSigner,
   recipient: Address,
) -> Result<ZeusExecuteParams, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   if swap_steps.is_empty() {
      return Err(anyhow!("No swap steps provided"));
   }

   let router_addr = router_address;
   let owner = secure_signer.address();
   let mut commands = Vec::new();
   let mut inputs = Vec::new();
   let mut execute_params = ZeusExecuteParams::new();

   let weth_currency = currency_in.to_weth();

   if currency_in.is_native() {
      // Always set the tx value to the total input amount when dealing with native ETH.
      execute_params.set_value(amount_in);

      // Calculate how much ETH needs to be wrapped for V2/V3 pools.
      let amount_to_wrap: U256 = swap_steps
         .iter()
         .filter(|s| s.currency_in == weth_currency && !s.pool.dex_kind().is_uniswap_v4())
         .map(|s| s.amount_in.wei())
         .sum();

      if amount_to_wrap > U256::ZERO {
         let data = zeus_router_abi::WrapETH {
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
   let mut first_step_uses_permit2 = false;
   if currency_in.is_erc20() {
      let token_in = currency_in.to_erc20();

      let permit2_address = address_book::permit2_contract(chain_id)?;
      let data_fut = allowance(
         client.clone(),
         permit2_address,
         owner,
         token_in.address,
         router_addr,
      );

      let allowance_fut = token_in.allowance(client.clone(), owner, permit2_address);

      let (data, allowance) = tokio::try_join!(data_fut, allowance_fut)?;
      let permit2_contract_need_approval = allowance < amount_in;

      let current_time =
         std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();

      let expired = u64::try_from(data.expiration)? < current_time;
      let needs_permit2 = U256::from(data.amount) < amount_in || expired;

      if needs_permit2 {
         first_step_uses_permit2 = true;
         let expiration = U256::from(current_time + 30 * 24 * 60 * 60); // 30 days
         let sig_deadline = U256::from(current_time + 30 * 60); // 30 minutes

         let value = generate_permit2_single_value(
            chain_id,
            token_in.address,
            router_addr,
            amount_in,
            permit2_address,
            expiration,
            sig_deadline,
            data.nonce,
         );

         let typed_data = parse_typed_data(value.clone())?;

         let signer = secure_signer.to_signer();
         let signature = signer.sign_dynamic_typed_data(&typed_data).await?;
         erase_signer(signer);

         let permit_input = zeus_router_abi::encode_permit2_permit(
            token_in.address,
            amount_in,
            expiration,
            data.nonce,
            router_addr,
            sig_deadline,
            signature,
         );

         commands.push(Commands::PERMIT2_PERMIT as u8);
         inputs.push(permit_input);

         execute_params.set_message(Some(value));
         execute_params.set_permit2_needs_approval(permit2_contract_need_approval);
      }
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
         recipient
      } else {
         router_addr
      };

      // If its a single swap, set the slippage now
      // otherwise is enforced at the end by the SWEEP or UNWRAP_WETH commands
      let min_amount_out = if swaps_len == 1 {
         execute_params.set_calculated_min_amount_out(amount_out_min);
         amount_out_min
      } else {
         U256::ZERO
      };

      // A step uses initial funds ONLY if its input is the main currency_in for the entire trade.
      let uses_initial_funds = swap.currency_in == currency_in;

      // For V2/V3, the input currency should always be the WETH address, even if the user starts with ETH.
      // The WRAP_ETH command ensures the router has the WETH.
      let step_currency_in = if swap.currency_in.is_native() {
         &weth_currency
      } else {
         &swap.currency_in
      };

      if swap.pool.dex_kind().is_v2() {
         let input = zeus_router_abi::V2V3SwapParams {
            amountIn: swap.amount_in.wei(),
            amountOutMin: min_amount_out,
            tokenIn: step_currency_in.address(),
            tokenOut: swap.currency_out.address(),
            pool: swap.pool.address(),
            poolVariant: U256::from(0),
            recipient: recipient,
            fee: swap.pool.fee().fee_u24(),
            permit2: uses_initial_funds && first_step_uses_permit2,
         }
         .abi_encode()
         .into();

         commands.push(Commands::V2_SWAP as u8);
         inputs.push(input);
      }

      if swap.pool.dex_kind().is_v3() {
         let input = zeus_router_abi::V2V3SwapParams {
            amountIn: swap.amount_in.wei(),
            amountOutMin: min_amount_out,
            tokenIn: step_currency_in.address(),
            tokenOut: swap.currency_out.address(),
            pool: swap.pool.address(),
            poolVariant: U256::from(1),
            recipient: recipient,
            fee: swap.pool.fee().fee_u24(),
            permit2: uses_initial_funds && first_step_uses_permit2,
         }
         .abi_encode()
         .into();
         commands.push(Commands::V3_SWAP as u8);
         inputs.push(input);
      }

      if swap.pool.dex_kind().is_uniswap_v4() {
         let input = zeus_router_abi::V4SwapParams {
            currencyIn: swap.currency_in.address(),
            currencyOut: swap.currency_out.address(),
            amountIn: swap.amount_in.wei(),
            amountOutMin: min_amount_out,
            fee: swap.pool.fee().fee_u24(),
            tickSpacing: swap.pool.fee().tick_spacing(),
            zeroForOne: swap.pool.zero_for_one_v4(&swap.currency_in),
            hooks: Address::ZERO,
            hookData: Bytes::default(),
            recipient: recipient,
            permit2: uses_initial_funds && first_step_uses_permit2,
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
      recipient,
      amount_out_min,
      router_addr,
      router_eth_balance,
      router_weth_balance,
      slippage,
      &mut execute_params,
      &mut commands,
      &mut inputs,
   );

   // Sanity check
   if execute_params.calculated_min_amount_out != amount_out_min {
      let calculated = NumericValue::format_wei(
         execute_params.calculated_min_amount_out,
         currency_out.decimals(),
      );
      let actual = NumericValue::format_wei(amount_out_min, currency_out.decimals());
      return Err(anyhow!(
         "Calculated min amount out in Params does not match the actual amount out min, Calculated {} {}, Actual {} {}",
         currency_out.symbol(),
         calculated.format_abbreviated(),
         currency_out.symbol(),
         actual.format_abbreviated(),
      ));
   }

   let command_bytes = Bytes::from(commands);
   eprintln!("Command Bytes: {:?}", command_bytes);
   let calldata = zeus_router_abi::encode_execute(command_bytes, inputs);

   execute_params.set_call_data(calldata);

   Ok(execute_params)
}

fn handle_recipient_payment(
   swap_steps: Vec<SwapStep<impl UniswapPool + Clone>>,
   currency_out: &Currency,
   recipient: Address,
   amount_out_min: U256,
   router_addr: Address,
   router_eth_balance: U256,
   router_weth_balance: U256,
   slippage: f64,
   execute_params: &mut ZeusExecuteParams,
   commands: &mut Vec<u8>,
   inputs: &mut Vec<Bytes>,
) {
   let v4_pools: usize = swap_steps.iter().filter(|s| s.pool.dex_kind().is_v4()).count();

   // Pool will send directly to recipient
   if swap_steps.len() == 1 && currency_out.is_erc20() {
      return;
   }

   // V4 Pool will send directly to recipient
   if swap_steps.len() == 1 && v4_pools > 0 {
      return;
   }

   let router_has_weth_balance = router_weth_balance > U256::ZERO;
   let router_has_eth_balance = router_eth_balance > U256::ZERO;

   let mut should_sweep = true;
   let amount_to_sweep = amount_out_min;
   execute_params.set_calculated_min_amount_out(amount_to_sweep);

   // Handle ETH output

   // Router has just WETH, in that case we just call unwrap WETH and send all the ETH to the recipient
   if currency_out.is_native() && router_has_weth_balance && !router_has_eth_balance {
      let data = zeus_router_abi::UnwrapWETH {
         recipient,
         amountMin: amount_out_min,
      }
      .abi_encode()
      .into();
      commands.push(Commands::UNWRAP_WETH as u8);
      inputs.push(data);

      execute_params.set_calculated_min_amount_out(amount_out_min);
      should_sweep = false;
   }

   // Router has both WETH and ETH, We need to call UNWRAP WETH and then let the SWEEP to send all the ETH
   if currency_out.is_native() && router_has_weth_balance && router_has_eth_balance {
      let mut weth_amount = NumericValue::format_wei(router_weth_balance, currency_out.decimals());
      weth_amount.calc_slippage(slippage, currency_out.decimals());

      let data = zeus_router_abi::UnwrapWETH {
         recipient: router_addr,
         amountMin: weth_amount.wei(),
      }
      .abi_encode()
      .into();
      commands.push(Commands::UNWRAP_WETH as u8);
      inputs.push(data);
   }

   // Handles Both ETH and ERC20 output
   if should_sweep {
      let sweep_params = zeus_router_abi::Sweep {
         currency: currency_out.address(),
         recipient,
         amountMin: amount_to_sweep,
      };

      let data = sweep_params.abi_encode().into();
      commands.push(Commands::SWEEP as u8);
      inputs.push(data);
   }
}
