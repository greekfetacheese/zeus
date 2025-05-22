use super::{ExecuteParams, SwapStep, SwapType, UniswapPool};
use abi::{
   permit::*,
   uniswap::{
      encode_v2_swap_exact_in, encode_v3_swap_exact_in,
      v4::{router::*, *},
   },
};
use alloy_contract::private::{Network, Provider};
use alloy_primitives::{Address, Bytes, U256, aliases::U48};
use alloy_sol_types::SolValue;
use anyhow::anyhow;
use currency::Currency as Currency2;
use serde_json::Value;
use utils::{address::permit2_contract, parse_typed_data};
use wallet::{SecureSigner, alloy_signer::Signer};

#[allow(non_camel_case_types)]
#[derive(Clone, Debug, PartialEq)]
#[repr(u8)]
pub enum Actions {
   // Pool actions
   // Liquidity actions
   INCREASE_LIQUIDITY(IncreaseLiquidityParams) = 0x00,
   DECREASE_LIQUIDITY(DecreaseLiquidityParams) = 0x01,
   MINT_POSITION(MintPositionParams) = 0x02,
   BURN_POSITION(BurnPositionParams) = 0x03,
   // Swapping
   SWAP_EXACT_IN_SINGLE(ExactInputSingleParams) = 0x06,
   SWAP_EXACT_IN(ExactInputParams) = 0x07,
   SWAP_EXACT_OUT_SINGLE(ExactOutputSingleParams) = 0x08,
   SWAP_EXACT_OUT(ExactOutputParams) = 0x09,

   // Closing deltas on the pool manager
   // Settling
   SETTLE(SettleParams) = 0x0b,
   SETTLE_ALL(SettleAllParams) = 0x0c,
   SETTLE_PAIR(SettlePairParams) = 0x0d,
   // Taking
   TAKE(TakeParams) = 0x0e,
   TAKE_ALL(TakeAllParams) = 0x0f,
   TAKE_PORTION(TakePortionParams) = 0x10,
   TAKE_PAIR(TakePairParams) = 0x11,

   CLOSE_CURRENCY(CloseCurrencyParams) = 0x12,
   SWEEP(SweepParams) = 0x14,
}

/// https://doc.rust-lang.org/error_codes/E0732.html
#[inline]
const fn discriminant(v: &Actions) -> u8 {
   unsafe { *(v as *const Actions as *const u8) }
}

impl Actions {
   #[inline]
   pub const fn command(&self) -> u8 {
      discriminant(self)
   }

   #[inline]
   pub fn abi_encode(&self) -> Bytes {
      match self {
         Self::INCREASE_LIQUIDITY(params) => params.abi_encode_params(),
         Self::DECREASE_LIQUIDITY(params) => params.abi_encode_params(),
         Self::MINT_POSITION(params) => params.abi_encode_params(),
         Self::BURN_POSITION(params) => params.abi_encode_params(),
         Self::SWAP_EXACT_IN_SINGLE(params) => params.abi_encode_params(),
         Self::SWAP_EXACT_IN(params) => params.abi_encode_params(),
         Self::SWAP_EXACT_OUT_SINGLE(params) => params.abi_encode_params(),
         Self::SWAP_EXACT_OUT(params) => params.abi_encode_params(),
         Self::SETTLE(params) => params.abi_encode_params(),
         Self::SETTLE_ALL(params) => params.abi_encode_params(),
         Self::SETTLE_PAIR(params) => params.abi_encode_params(),
         Self::TAKE(params) => params.abi_encode_params(),
         Self::TAKE_ALL(params) => params.abi_encode_params(),
         Self::TAKE_PORTION(params) => params.abi_encode_params(),
         Self::TAKE_PAIR(params) => params.abi_encode_params(),
         Self::CLOSE_CURRENCY(params) => params.abi_encode_params(),
         Self::SWEEP(params) => params.abi_encode_params(),
      }
      .into()
   }

   #[inline]
   pub fn abi_decode(command: u8, data: &Bytes) -> Result<Self, anyhow::Error> {
      let data = data.iter().as_slice();
      Ok(match command {
         0x00 => Self::INCREASE_LIQUIDITY(IncreaseLiquidityParams::abi_decode(data)?),
         0x01 => Self::DECREASE_LIQUIDITY(DecreaseLiquidityParams::abi_decode(data)?),
         0x02 => Self::MINT_POSITION(MintPositionParams::abi_decode(data)?),
         0x03 => Self::BURN_POSITION(BurnPositionParams::abi_decode(data)?),
         0x06 => Self::SWAP_EXACT_IN_SINGLE(ExactInputSingleParams::abi_decode(data)?),
         0x07 => Self::SWAP_EXACT_IN(ExactInputParams::abi_decode(data)?),
         0x08 => Self::SWAP_EXACT_OUT_SINGLE(ExactOutputSingleParams::abi_decode(data)?),
         0x09 => Self::SWAP_EXACT_OUT(ExactOutputParams::abi_decode(data)?),
         0x0b => Self::SETTLE(SettleParams::abi_decode(data)?),
         0x0c => Self::SETTLE_ALL(SettleAllParams::abi_decode(data)?),
         0x0d => Self::SETTLE_PAIR(SettlePairParams::abi_decode(data)?),
         0x0e => Self::TAKE(TakeParams::abi_decode(data)?),
         0x0f => Self::TAKE_ALL(TakeAllParams::abi_decode(data)?),
         0x10 => Self::TAKE_PORTION(TakePortionParams::abi_decode(data)?),
         0x11 => Self::TAKE_PAIR(TakePairParams::abi_decode(data)?),
         0x12 => Self::CLOSE_CURRENCY(CloseCurrencyParams::abi_decode(data)?),
         0x14 => Self::SWEEP(SweepParams::abi_decode(data)?),
         _ => return Err(anyhow::anyhow!("Invalid action")),
      })
   }
}

// https://docs.uniswap.org/contracts/universal-router/technical-reference
#[allow(non_camel_case_types)]
#[derive(Clone, Debug, PartialEq)]
#[repr(u8)]
pub enum Commands {
   V3_SWAP_EXACT_IN = 0x00,
   V3_SWAP_EXACT_OUT = 0x01,
   PERMIT2_TRANSFER_FROM = 0x02,
   PERMIT2_PERMIT_BATCH = 0x03,
   SWEEP = 0x04,
   TRANSFER = 0x05,
   PAY_PORTION = 0x06,
   V2_SWAP_EXACT_IN = 0x08,
   V2_SWAP_EXACT_OUT = 0x09,
   PERMIT2_PERMIT = 0x0a,
   WRAP_ETH = 0x0b,
   UNWRAP_WETH = 0x0c,
   PERMIT2_TRANSFER_FROM_BATCH = 0x0d,
   BALANCE_CHECK_ERC20 = 0x0e,
   V4_SWAP = 0x10,
   V3_POSITION_MANAGER_PERMIT = 0x11,
   V3_POSITION_MANAGER_CALL = 0x12,
   V4_INITIALIZE_POOL = 0x13,
   V4_POSITION_MANAGER_CALL = 0x14,
   EXECUTE_SUB_PLAN = 0x21,
}

// ! V4 swaps from ETH to ERC and vice versa are working fine
// ! But from ERC to ERC they dont work
/// Build the params for the execute function
///
/// Currently does not support V4 swaps
pub async fn build_execute_params<P, N>(
   client: P,
   chain_id: u64,
   swap_steps: Vec<SwapStep<impl UniswapPool + Clone>>,
   swap_type: SwapType,
   amount_in: U256,
   amount_out_min: U256,
   currency_in: Currency2,
   currency_out: Currency2,
   signer: SecureSigner,
   recipient: Address,
) -> Result<ExecuteParams, anyhow::Error>
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

   for swap in &swap_steps {
      if !swap.pool.dex_kind().is_uniswap() {
         return Err(anyhow!("Only support Uniswap"));
      }

      if swap.pool.dex_kind().is_uniswap_v4() {
         if swap.pool.currency0().is_erc20() && swap.pool.currency1().is_erc20() {
            return Err(anyhow!("ERC20 to ERC20 swaps are not supported yet on V4"));
         }
      }
   }

   let router_addr = utils::address::uniswap_v4_universal_router(chain_id)?;
   let need_to_wrap_eth = currency_in.is_native() && !swap_steps[0].pool.dex_kind().is_uniswap_v4();
   let mut need_to_unwrap_weth = currency_out.is_native();

   let owner = signer.borrow().address();
   let mut commands = Vec::new();
   let mut inputs = Vec::new();
   let mut execute_params = ExecuteParams::new();

   if need_to_wrap_eth {
      let data = abi::uniswap::encode_wrap_eth(router_addr, amount_in);
      commands.push(Commands::WRAP_ETH as u8);
      inputs.push(data);

      execute_params.set_value(amount_in);
   }

   // Set the Tx Value to amount_in if the currency_in is native and the first step is v4 swap
   if currency_in.is_native() && swap_steps[0].pool.dex_kind().is_uniswap_v4() {
      execute_params.set_value(amount_in);
   }

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

         let value = generate_permit2_typed_data(
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

         let signature = signer.borrow().sign_dynamic_typed_data(&typed_data).await?;

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

   let steps_len = swap_steps.len();
   let multiple_steps = steps_len > 1;
   for (i, swap) in swap_steps.iter().enumerate() {
      let is_first_step = i == 0;
      let is_last_step = i == steps_len - 1;

      // last step is v4 with native as output we don't need to unwrap weth
      if is_last_step && swap.pool.dex_kind().is_uniswap_v4() && swap.currency_out.is_native() {
         need_to_unwrap_weth = false;
      }

      // keep weth in router so we can unwrap it
      let need_to_keep_weth = need_to_unwrap_weth && is_last_step;
      // Keep tokens in router if we are swapping on multiple pools
      let need_to_keep_tokens = multiple_steps && !is_last_step;

      let recipient_addr = if need_to_keep_weth || need_to_keep_tokens {
         router_addr
      } else {
         recipient
      };

      let current_step_uses_permit2 = is_first_step && first_step_uses_permit2;

      // For the last step, make sure to use the amount_out_min
      let step_amount_out_min = if is_last_step {
         amount_out_min
      } else {
         swap.amount_out.wei2()
      };

      if swap.pool.dex_kind().is_uniswap_v2() {
         let path = vec![swap.currency_in.address(), swap.currency_out.address()];

         let input = encode_v2_swap_exact_in(
            recipient_addr,
            swap.amount_in.wei2(),
            step_amount_out_min,
            path,
            current_step_uses_permit2,
         )?;

         commands.push(Commands::V2_SWAP_EXACT_IN as u8);
         inputs.push(input);
      }

      if swap.pool.dex_kind().is_uniswap_v3() {
         let path = vec![swap.currency_in.address(), swap.currency_out.address()];
         let fees = vec![swap.pool.fee().fee_u24()];

         let input = encode_v3_swap_exact_in(
            recipient_addr,
            swap.amount_in.wei2(),
            step_amount_out_min,
            path,
            fees,
            current_step_uses_permit2,
         )?;

         commands.push(Commands::V3_SWAP_EXACT_IN as u8);
         inputs.push(input);
      }

      if swap.pool.dex_kind().is_uniswap_v4() {
         let input = encode_v4_commands(
            &swap.pool,
            swap_type,
            &swap.currency_in,
            &swap.currency_out,
            swap.amount_in.wei2(),
            step_amount_out_min,
            is_first_step,
            is_last_step,
            recipient_addr,
         )?;

         commands.push(Commands::V4_SWAP as u8);
         inputs.push(input);
      }
   }

   if need_to_unwrap_weth {
      let data = abi::uniswap::encode_unwrap_weth(recipient, amount_out_min);
      commands.push(Commands::UNWRAP_WETH as u8);
      inputs.push(data);
   }

   let command_bytes = Bytes::from(commands);
   println!("Command Bytes: {:?}", command_bytes);
   tracing::info!(target: "zeus_eth::amm::uniswap::router", "Command Bytes: {:?}", command_bytes);
   let calldata = encode_execute(command_bytes, inputs);
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

   let encoded_data = (actions_bytes, v4_action_params).abi_encode_params();
   Ok(encoded_data.into())
}

pub fn generate_permit2_typed_data(
   chain_id: u64,
   token: Address,
   spender: Address,
   amount: U256,
   permit2: Address,
   expiration: U256,
   sig_deadline: U256,
   nonce: U48,
) -> Value {
   let value = serde_json::json!({
       "types": {
           "PermitSingle": [
               {"name": "details", "type": "PermitDetails"},
               {"name": "spender", "type": "address"},
               {"name": "sigDeadline", "type": "uint256"}
           ],
           "PermitDetails": [
               {"name": "token", "type": "address"},
               {"name": "amount", "type": "uint160"},
               {"name": "expiration", "type": "uint48"},
               {"name": "nonce", "type": "uint48"}
           ],
           "EIP712Domain": [
               {"name": "name", "type": "string"},
               {"name": "chainId", "type": "uint256"},
               {"name": "verifyingContract", "type": "address"}
           ]
       },
       "domain": {
           "name": "Permit2",
           "chainId": chain_id.to_string(),
           "verifyingContract": permit2.to_string()
       },
       "primaryType": "PermitSingle",
       "message": {
           "details": {
               "token": token.to_string(),
               "amount": amount.to_string(),
               "expiration": expiration.to_string(),
               "nonce": nonce.to_string()
           },
           "spender": spender.to_string(),
           "sigDeadline": sig_deadline.to_string()
       }
   });

   value
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
