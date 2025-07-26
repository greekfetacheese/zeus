use super::{Commands, SwapExecuteParams, SwapStep, SwapType, UniswapPool};
use crate::uniswap::v4::Actions;
use abi::{
   permit::*,
   uniswap::{
      encode_v2_swap_exact_in, encode_v3_swap_exact_in,
      universal_router_v2::{IV4Router::*, encode_execute, encode_execute_with_deadline},
   },
};
use alloy_contract::private::{Network, Provider};
use alloy_primitives::{Address, Bytes, U256};
use alloy_sol_types::SolValue;
use anyhow::anyhow;
use currency::Currency as Currency2;
use utils::{
   NumericValue, SecureSigner, address_book::permit2_contract, alloy_signer::Signer, erase_signer,
   generate_permit2_single_value, parse_typed_data,
};

/// Encode the calldata for a swap using the universal router
pub async fn encode_swap<P, N>(
   client: P,
   chain_id: u64,
   swap_steps: Vec<SwapStep<impl UniswapPool + Clone>>,
   swap_type: SwapType,
   amount_in: U256,
   amount_out_min: U256,
   slippage: f64,
   currency_in: Currency2,
   currency_out: Currency2,
   secure_signer: SecureSigner,
   recipient: Address,
   deadline: Option<U256>,
) -> Result<SwapExecuteParams, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   if swap_steps.is_empty() {
      return Err(anyhow!("No swap steps provided"));
   }
   if !swap_type.is_exact_input() {
      return Err(anyhow!("Only support exact input"));
   }

   let owner = secure_signer.address();
   let router_addr = utils::address_book::universal_router_v2(chain_id)?;
   let mut commands = Vec::new();
   let mut inputs = Vec::new();
   let mut execute_params = SwapExecuteParams::new();

   let weth_currency = currency_in.to_weth();

   if currency_in.is_native() {
      // Always set the tx value to the total input amount when dealing with native ETH.
      execute_params.set_value(amount_in);

      // Calculate how much ETH needs to be wrapped for V2/V3 pools.
      let amount_to_wrap: U256 = swap_steps
         .iter()
         // Compare against `weth_currency`, not the native `currency_in`.
         .filter(|s| s.currency_in == weth_currency && !s.pool.dex_kind().is_uniswap_v4())
         .map(|s| s.amount_in.wei())
         .sum();

      if amount_to_wrap > U256::ZERO {
         let data = abi::uniswap::encode_wrap_eth(router_addr, amount_to_wrap);
         commands.push(Commands::WRAP_ETH as u8);
         inputs.push(data);
      }
   }

   // Handle Permit2 approvals
   let mut first_step_uses_permit2 = false;
   if currency_in.is_erc20() {
      let token_in = currency_in.to_erc20();

      let permit2_address = permit2_contract(chain_id)?;
      let data_fut = abi::permit::allowance(
         client.clone(),
         permit2_address,
         owner,
         token_in.address,
         router_addr,
      );

      let allowance_fut = token_in.allowance(client.clone(), owner, permit2_address);

      let (data, allowance) = tokio::try_join!(data_fut, allowance_fut)?;
      let permit2_contract_need_approval = allowance < amount_in;

      let current_time = std::time::SystemTime::now()
         .duration_since(std::time::UNIX_EPOCH)?
         .as_secs();

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

         let permit_input = encode_permit2_permit_ur_input(
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
         execute_params.set_token_needs_approval(permit2_contract_need_approval);
      }
   }

   // Router ETH and WETH balances after the swaps
   let mut router_eth_balance = U256::ZERO;
   let mut router_weth_balance = U256::ZERO;

   for swap in &swap_steps {
      if swap.currency_in.is_native() {
         if router_eth_balance >= swap.amount_in.wei() {
            router_eth_balance -= swap.amount_in.wei();
         }
      }

      if swap.currency_in.is_native_wrapped() {
         if router_weth_balance >= swap.amount_in.wei() {
            router_weth_balance -= swap.amount_in.wei();
         }
      }

      if swap.currency_out.is_native() {
         router_eth_balance += swap.amount_out.wei();
      }

      if swap.currency_out.is_native_wrapped() {
         router_weth_balance += swap.amount_out.wei();
      }

      /* 
      eprintln!("|=== Swap Step ===|");
      eprintln!(
         "Swap Step: {} {} -> {} {} {} ({})",
         swap.amount_in.format_abbreviated(),
         swap.currency_in.symbol(),
         swap.amount_out.format_abbreviated(),
         swap.currency_out.symbol(),
         swap.pool.dex_kind().as_str(),
         swap.pool.fee().fee_percent()
      );
      */

      // A step uses initial funds ONLY if its input is the main currency_in for the entire trade.
      let uses_initial_funds = swap.currency_in == currency_in;

      // All intermediate swaps send funds back to the router.
      // The final output is handled by SWEEP or UNWRAP_WETH at the end.
      let recipient_addr = router_addr;

      // Slippage is only enforced at the very end.
      let step_amount_out_min = U256::ZERO;

      // For V2/V3, the input currency should always be the WETH address, even if the user starts with ETH.
      // The WRAP_ETH command ensures the router has the WETH.
      let step_currency_in = if swap.currency_in.is_native() {
         &weth_currency
      } else {
         &swap.currency_in
      };

      if swap.pool.dex_kind().is_uniswap_v2() {
         let path = vec![step_currency_in.address(), swap.currency_out.address()];
         let input = encode_v2_swap_exact_in(
            recipient_addr,
            swap.amount_in.wei(),
            step_amount_out_min,
            path,
            uses_initial_funds && first_step_uses_permit2,
         )?;
         commands.push(Commands::V2_SWAP_EXACT_IN as u8);
         inputs.push(input);
      }

      if swap.pool.dex_kind().is_uniswap_v3() {
         let path = vec![step_currency_in.address(), swap.currency_out.address()];
         let fees = vec![swap.pool.fee().fee_u24()];
         let input = encode_v3_swap_exact_in(
            recipient_addr,
            swap.amount_in.wei(),
            step_amount_out_min,
            path,
            fees,
            uses_initial_funds && first_step_uses_permit2,
         )?;
         commands.push(Commands::V3_SWAP_EXACT_IN as u8);
         inputs.push(input);
      }

      // V4 logic can use the original `swap.currency_in` as it handles native ETH directly.
      if swap.pool.dex_kind().is_uniswap_v4() {
         let input = encode_v4_internal_actions(
            &swap.pool,
            swap_type,
            &swap.currency_in,
            &swap.currency_out,
            swap.amount_in.wei(),
            step_amount_out_min,
            router_addr,
            uses_initial_funds,
         )?;
         commands.push(Commands::V4_SWAP as u8);
         inputs.push(input);
      }
   }

   let ur_has_weth_balance = router_weth_balance > U256::ZERO;
   let ur_has_eth_balance = router_eth_balance > U256::ZERO;

   let mut should_sweep = true;
   let amount_to_sweep = amount_out_min;

   // Handle native ETH output

   // UR has just WETH, in that case we just unwrap WETH and send it to the recipient
   if currency_out.is_native() && ur_has_weth_balance && !ur_has_eth_balance {
      let data = abi::uniswap::encode_unwrap_weth(recipient, amount_out_min);
      commands.push(Commands::UNWRAP_WETH as u8);
      inputs.push(data);

      should_sweep = false;
   }

   // UR has both WETH and ETH, We need to UNWRAP WETH and then let the SWEEP to send all the ETH
   if currency_out.is_native() && ur_has_weth_balance && ur_has_eth_balance {
      let mut weth_amount = NumericValue::format_wei(router_weth_balance, currency_out.decimals());
      weth_amount.calc_slippage(slippage, currency_out.decimals());

      let data = abi::uniswap::encode_unwrap_weth(router_addr, weth_amount.wei());
      commands.push(Commands::UNWRAP_WETH as u8);
      inputs.push(data);
   }

   if should_sweep {
      let sweep_params = Sweep {
         token: currency_out.address(),
         recipient,
         amountMin: amount_to_sweep,
      };

     // eprintln!("Sweep Params: {:?}", sweep_params);

      let data = sweep_params.abi_encode_params().into();
      commands.push(Commands::SWEEP as u8);
      inputs.push(data);
   }

   let command_bytes = Bytes::from(commands);
  // eprintln!("Command Bytes: {:?}", command_bytes);
   tracing::info!(target: "zeus_eth::amm::uniswap::router", "Command Bytes: {:?}", command_bytes);
   let calldata = if let Some(deadline_val) = deadline {
      encode_execute_with_deadline(command_bytes, inputs, deadline_val)
   } else {
      encode_execute(command_bytes, inputs)
   };
   execute_params.set_call_data(calldata);

   Ok(execute_params)
}

fn encode_v4_internal_actions(
   pool: &impl UniswapPool,
   swap_type: SwapType,
   currency_in: &Currency2,
   currency_out: &Currency2,
   amount_in: U256,
   amount_out_min: U256,
   router_addr: Address,
   uses_initial_funds: bool,
) -> Result<Bytes, anyhow::Error> {
   let (swap_action, swap_input) =
      encode_v4_swap_single_command_input(pool, swap_type, currency_in, amount_in, amount_out_min)?;

   // Settle tells the V4 contract how to receive the input tokens
   let settle = SettleParams {
      currency: currency_in.address(),
      amount: amount_in,
      payerIsUser: uses_initial_funds, // True if funds come from user, false if from UR's balance in V4
   };

   let settle_action = Actions::SETTLE(settle);
   let settle_input = settle_action.abi_encode();

   let take_params = TakeParams {
      currency: currency_out.address(),
      recipient: router_addr,
      amount: amount_out_min,
   };

   let take_action = Actions::TAKE(take_params);
   let take_input = take_action.abi_encode();

   let v4_actions = vec![settle_action, swap_action, take_action];
   let v4_action_params = vec![settle_input, swap_input, take_input];

   encode_v4_router_command_input(v4_actions, v4_action_params)
}

fn encode_v4_swap_single_command_input(
   pool: &impl UniswapPool,
   swap_type: SwapType,
   currency_in: &Currency2,
   amount_in: U256,
   amount_out: U256,
) -> Result<(Actions, Bytes), anyhow::Error> {
   let (action, action_params_bytes) = if swap_type.is_exact_input() {
      let params = ExactInputSingleParams {
         poolKey: pool.get_pool_key()?,
         zeroForOne: pool.zero_for_one_v4(currency_in),
         amountIn: amount_in.try_into()?,
         amountOutMinimum: amount_out.try_into()?,
         hookData: Bytes::default(),
      };
      let action = Actions::SWAP_EXACT_IN_SINGLE(params);
      let params_bytes = action.abi_encode();
      (action, params_bytes)
   } else {
      let params = ExactOutputSingleParams {
         poolKey: pool.get_pool_key()?,
         zeroForOne: pool.zero_for_one_v4(currency_in),
         amountOut: amount_out.try_into()?,
         amountInMaximum: amount_in.try_into()?,
         hookData: Bytes::default(),
      };
      let action = Actions::SWAP_EXACT_OUT_SINGLE(params);
      let params_bytes = action.abi_encode();
      (action, params_bytes)
   };

   Ok((action, action_params_bytes))
}

/// Encodes the input for the Universal Router's V4_SWAP command (0x10).
/// This input is itself an ABI-encoded tuple: (bytes actions, bytes[] params)
fn encode_v4_router_command_input(
   v4_actions: Vec<Actions>,
   v4_action_params: Vec<Bytes>,
) -> Result<Bytes, anyhow::Error> {
   if v4_actions.len() != v4_action_params.len() {
      return Err(anyhow::anyhow!(
         "V4 actions and params length mismatch: {} != {}",
         v4_actions.len(),
         v4_action_params.len()
      ));
   }

   let actions_bytes_vec: Vec<u8> = v4_actions.iter().map(|a| a.command()).collect();
   let actions_bytes = Bytes::from(actions_bytes_vec);

   let params = ActionsParams {
      actions: actions_bytes,
      params: v4_action_params,
   }
   .abi_encode_params();

   Ok(params.into())
}

/*
/// V4 specific
pub fn encode_route_to_path(
   route: &Route<impl UniswapPool>,
   exact_output: bool,
) -> Result<Vec<PathKey>, anyhow::Error> {
   let mut path_keys: Vec<PathKey> = Vec::with_capacity(route.pools.len());

   if exact_output {
      let mut currency_out = route.currency_out.clone();
      for pool in route.pools.iter().rev() {
         let (next_currency, path_key) = get_next_path_key(pool, &currency_out);
         path_keys.push(path_key);
         currency_out = next_currency;
      }
      path_keys.reverse();
   } else {
      let mut currency_in = route.currency_in.clone();
      for pool in route.pools.iter() {
         let (next_currency, path_key) = get_next_path_key(pool, &currency_in);
         path_keys.push(path_key);
         currency_in = next_currency;
      }
   }

   Ok(path_keys)
}

   */

/*
/// V4 specific
pub fn get_next_path_key(pool: &impl UniswapPool, currecy_in: &Currency2) -> (Currency2, PathKey) {
   let next_currency = if currecy_in == pool.currency0() {
      pool.currency1().clone()
   } else {
      pool.currency0().clone()
   };

   let intermediate_currency = if next_currency.is_native() {
      Address::ZERO
   } else {
      next_currency.to_erc20().address
   };

   let path_key = PathKey {
      intermediateCurrency: intermediate_currency,
      fee: pool.fee().fee_u24(),
      tickSpacing: pool.fee().tick_spacing(),
      hooks: Address::ZERO,
      hookData: Bytes::default(),
   };

   (next_currency, path_key)
}

*/
