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
   SecureSigner, address_book::permit2_contract, alloy_signer::Signer, erase_signer, generate_permit2_single_value,
   parse_typed_data,
};

use std::collections::HashSet;

/// Encode the calldata for a swap using the universal router
pub async fn encode_swap<P, N>(
   client: P,
   chain_id: u64,
   swap_steps: Vec<SwapStep<impl UniswapPool + Clone>>,
   swap_type: SwapType,
   amount_in: U256,
   amount_out_min: U256,
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

   let weth_currency = currency_in.to_weth_currency();

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
      let token_needs_permit2_approval = allowance < amount_in;

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
         execute_params.set_token_needs_approval(token_needs_permit2_approval);
      }
   }

   let intermediate_inputs: HashSet<Currency2> = swap_steps.iter().map(|s| s.currency_in.clone()).collect();

   // Amount of token out taken by the TAKE_ALL command in v4 swaps
   // We substract it at the end if needed to calculate the final expected output we get back
   let mut amount_taken = U256::ZERO;

   for swap in &swap_steps {
      /*
      eprintln!("|=== Swap Step ===|");
      eprintln!(
         "Swap Step: {} {} -> {} {} {} ({})",
         swap.amount_in.format_abbreviated(),
         swap.currency_in.symbol(),
         swap.amount_out.format_abbreviated(),
         swap.currency_out.symbol(),
         swap.pool.dex_kind().to_str(),
         swap.pool.fee().fee_percent()
      );
       */

      // A step uses initial funds if its input is the main currency OR its WETH equivalent.
      let uses_initial_funds = swap.currency_in == currency_in || swap.currency_in == weth_currency;

      let is_last_step_of_path = !intermediate_inputs.contains(&swap.currency_out);

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
         amount_taken += swap.amount_out.wei();

         let input = encode_v4_commands(
            &swap.pool,
            swap_type,
            &swap.currency_in,
            &swap.currency_out,
            swap.amount_in.wei(),
            step_amount_out_min,
            uses_initial_funds,
            is_last_step_of_path,
            recipient_addr,
         )?;
         commands.push(Commands::V4_SWAP as u8);
         inputs.push(input);
      }
   }

   // get the dex kind of the last pool
   let dex_kind = swap_steps.last().unwrap().pool.dex_kind();

   // If the last pool is V4 skip this part as the
   // TAKE_ALL command will handle sending the tokens or ETH back to the receipient

   if !dex_kind.is_v4() {
      // Handle ETH wrapping and final output
      let expected_amount_out = amount_out_min - amount_taken;
      if currency_out.is_native() {
         let data = abi::uniswap::encode_unwrap_weth(recipient, expected_amount_out);
         commands.push(Commands::UNWRAP_WETH as u8);
         inputs.push(data);
      } else {
         let sweep_params = Sweep {
            token: currency_out.address(),
            recipient,
            amountMin: expected_amount_out,
         };
         let data = sweep_params.abi_encode_params().into();
         commands.push(Commands::SWEEP as u8);
         inputs.push(data);
      }
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

fn encode_v4_commands(
   pool: &impl UniswapPool,
   swap_type: SwapType,
   currency_in: &Currency2,
   currency_out: &Currency2,
   amount_in: U256,
   amount_out: U256,
   is_first_step: bool,
   _is_last_step: bool,
   _recipient: Address,
) -> Result<Bytes, anyhow::Error> {
   let mut actions = Vec::new();
   let mut inputs = Vec::new();

   let address_in = if currency_in.is_native() {
      Address::ZERO
   } else {
      currency_in.address()
   };

   let (swap_action, swap_input) =
      encode_v4_swap_single_command_input(pool, swap_type, currency_in, amount_in, amount_out)?;

   actions.push(swap_action);
   inputs.push(swap_input);

   let mut payer = false;
   if is_first_step {
      payer = true;
   } // In any other case the token/funds should already be in UR

   let settle = SettleParams {
      currency: address_in,
      amount: amount_in,
      payerIsUser: payer,
   };

   let settle_action = Actions::SETTLE(settle);
   let settle_input = settle_action.abi_encode();
   actions.push(settle_action);
   inputs.push(settle_input);

   let address_out = if currency_out.is_native() {
      Address::ZERO
   } else {
      currency_out.address()
   };

   let take_all = TakeAllParams {
      currency: address_out,
      minAmount: amount_out,
   };

   let take_all_action = Actions::TAKE_ALL(take_all);
   let take_all_input = take_all_action.abi_encode();
   actions.push(take_all_action);
   inputs.push(take_all_input);

   encode_v4_router_command_input(actions, inputs)
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
