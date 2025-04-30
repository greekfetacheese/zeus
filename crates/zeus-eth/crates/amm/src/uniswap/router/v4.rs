use super::{ExecuteParams, Route, SwapType, UniswapPool};
use abi::{
   permit::*,
   uniswap::{
      encode_v2_swap_exact_in, encode_v3_swap_exact_in,
      v4::{router::*, *},
   },
};
use alloy_contract::private::{Network, Provider};
use alloy_dyn_abi::TypedData;
use alloy_primitives::{Address, Bytes, U256, aliases::U48};
use alloy_sol_types::SolValue;
use anyhow::anyhow;
use currency::Currency as Currency2;
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
         Self::INCREASE_LIQUIDITY(params) => params.abi_encode(),
         Self::DECREASE_LIQUIDITY(params) => params.abi_encode(),
         Self::MINT_POSITION(params) => params.abi_encode(),
         Self::BURN_POSITION(params) => params.abi_encode(),
         Self::SWAP_EXACT_IN_SINGLE(params) => params.abi_encode(),
         Self::SWAP_EXACT_IN(params) => params.abi_encode(),
         Self::SWAP_EXACT_OUT_SINGLE(params) => params.abi_encode(),
         Self::SWAP_EXACT_OUT(params) => params.abi_encode(),
         Self::SETTLE(params) => params.abi_encode(),
         Self::SETTLE_ALL(params) => params.abi_encode(),
         Self::SETTLE_PAIR(params) => params.abi_encode(),
         Self::TAKE(params) => params.abi_encode(),
         Self::TAKE_ALL(params) => params.abi_encode(),
         Self::TAKE_PORTION(params) => params.abi_encode(),
         Self::TAKE_PAIR(params) => params.abi_encode(),
         Self::CLOSE_CURRENCY(params) => params.abi_encode(),
         Self::SWEEP(params) => params.abi_encode(),
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

fn encode_v4_swap_single(
   pool: &impl UniswapPool,
   swap_type: SwapType,
   currency_in: &Currency2,
   amount_in: U256,
   amount_out_min: U256,
) -> Result<Bytes, anyhow::Error> {
   if !pool.dex_kind().is_uniswap_v4() {
      return Err(anyhow!("Pool is not v4"));
   }

   let bytes = if swap_type.is_exact_input() {
      encode_exact_input_single(
         pool.get_pool_key()?,
         pool.zero_for_one_v4(currency_in),
         amount_in,
         amount_out_min,
         Bytes::default(),
      )?
   } else {
      encode_exact_output_single(
         pool.get_pool_key()?,
         pool.zero_for_one_v4(currency_in),
         amount_out_min,
         amount_in,
         Bytes::default(),
      )?
   };

   Ok(bytes)
}


/// Build the params for the execute function
/// 
/// Currently only supports single-hop Uniswap V2 and V3 swaps
pub async fn build_execute_params<P, N>(
   client: P,
   chain_id: u64,
   route: &Route<impl UniswapPool + Clone>,
   swap_type: SwapType,
   amount_in: U256,
   amount_out_min: U256,
   signer: SecureSigner,
   recipient: Address,
) -> Result<ExecuteParams, anyhow::Error>
where
   P: Provider<N> + Clone + 'static,
   N: Network,
{
   // for now only support one single hop (swap)
   if route.pools.len() != 1 {
      return Err(anyhow!("Only support one single hop"));
   }

   if !swap_type.is_exact_input() {
      return Err(anyhow!("Only support exact input"));
   }

   let router_addr = utils::address::uniswap_v4_universal_router(chain_id)?;
   let pool = route.pools.first().cloned().unwrap();

   if !pool.dex_kind().is_uniswap() {
      return Err(anyhow!("Only support Uniswap"));
   }

   if pool.dex_kind().is_uniswap_v4() {
      return Err(anyhow!("Only support Uniswap V2 and V3"));
   }

   let currency_in = route.currency_in.clone();
   let currency_out = route.currency_out.clone();
   let need_to_wrap_eth = currency_in.is_native() && !pool.dex_kind().is_uniswap_v4();

   let owner = signer.borrow().address();
   let mut needs_permit2 = false;
   let mut commands = Vec::new();
   let mut inputs = Vec::new();
   let mut execute_params = ExecuteParams::new();

   if need_to_wrap_eth {
      let data = abi::uniswap::encode_wrap_eth(router_addr, amount_in);
      commands.push(Commands::WRAP_ETH as u8);
      inputs.push(data);

      execute_params.set_value(amount_in);
   }

   if currency_in.is_erc20() {
      let token_in = currency_in.to_erc20();

      let permit2_address = permit2_contract(chain_id)?;
      let data = abi::permit::allowance(
         client,
         permit2_address,
         owner,
         token_in.address,
         router_addr,
      )
      .await?;

      let current_time = std::time::SystemTime::now()
         .duration_since(std::time::UNIX_EPOCH)?
         .as_secs();

      let expired = u64::try_from(data.expiration)? < current_time;
      needs_permit2 = U256::from(data.amount) < amount_in || expired;

      if needs_permit2 {
         let expiration = U256::from(current_time + 30 * 24 * 60 * 60); // 30 days
         let sig_deadline = U256::from(current_time + 30 * 60); // 30 minutes

         let typed_data = generate_permit2_typed_data(
            chain_id,
            token_in.address,
            router_addr,
            amount_in,
            permit2_address,
            expiration,
            sig_deadline,
            data.nonce,
         )?;

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

         execute_params.set_message(Some(typed_data));
         execute_params.set_token_needs_approval(true);
      }
   }

   let (swap_command, input) = if pool.dex_kind().is_uniswap_v2() {
      let path = vec![currency_in.address(), currency_out.address()];
      (
         Commands::V2_SWAP_EXACT_IN as u8,
         encode_v2_swap_exact_in(recipient, amount_in, amount_out_min, path, needs_permit2)?,
      )
   } else if pool.dex_kind().is_uniswap_v3() {
      let path = vec![currency_in.address(), currency_out.address()];
      let fees = vec![pool.fee().fee_u24()];
      (
         Commands::V3_SWAP_EXACT_IN as u8,
         encode_v3_swap_exact_in(
            recipient,
            amount_in,
            amount_out_min,
            path,
            fees,
            needs_permit2,
         )?,
      )
   } else if pool.dex_kind().is_uniswap_v4() {
      (
         Commands::V4_SWAP as u8,
         encode_v4_swap_single(&pool, swap_type, &currency_in, amount_in, amount_out_min)?,
      )
   } else {
      return Err(anyhow::anyhow!("Unsupported DexKind"));
   };

   commands.push(swap_command);
   inputs.push(input);

   let command_bytes = Bytes::from(commands);
   let calldata = encode_execute(command_bytes, inputs);
   execute_params.set_call_data(calldata);

   Ok(execute_params)
}

fn generate_permit2_typed_data(
   chain_id: u64,
   token: Address,
   spender: Address,
   amount: U256,
   permit2: Address,
   expiration: U256,
   sig_deadline: U256,
   nonce: U48,
) -> Result<TypedData, anyhow::Error> {
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

   let typed = parse_typed_data(value)?;
   Ok(typed)
}

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

#[cfg(test)]
mod tests {
   use super::*;
   use crate::{UniswapV2Pool, UniswapV3Pool, uniswap::v4::pool::UniswapV4Pool};
   use alloy_primitives::{TxKind, U256};
   use alloy_provider::ProviderBuilder;
   use alloy_rpc_types::BlockId;
   use currency::{ERC20Token, NativeCurrency};
   use revm_utils::{
      AccountType, DummyAccount, ExecuteCommitEvm, ExecuteEvm, ForkFactory, new_evm, revert_msg, simulate,
   };
   use url::Url;
   use utils::NumericValue;

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn can_call_permit2() {
      let url = Url::parse("https://eth.merkle.io").unwrap();
      let client = ProviderBuilder::new().on_http(url);
      let chain_id = 1;

      let weth_balance = NumericValue::parse_to_wei("10", 18);
      let amount_in = NumericValue::parse_to_wei("1", 18);
      let weth = ERC20Token::weth();

      let alice = DummyAccount::new(AccountType::EOA, weth_balance.wei2());
      let signer = SecureSigner::new(alice.key.clone());

      let mut fork_factory = ForkFactory::new_sandbox_factory(client.clone(), chain_id, None, None);
      // insert Alice into the fork factory
      fork_factory.insert_dummy_account(alice.clone());

      // prepare the calldata
      let mut commands = Vec::new();
      let mut inputs = Vec::new();

      let current_time = std::time::SystemTime::now()
         .duration_since(std::time::UNIX_EPOCH)
         .unwrap()
         .as_secs();
      let expiration = U256::from(current_time + 30 * 24 * 60 * 60); // 30 days
      let sig_deadline = U256::from(current_time + 30 * 60); // 30 minutes

      let permit2_address = permit2_contract(chain_id).unwrap();
      let router_addr = utils::address::uniswap_v4_universal_router(chain_id).unwrap();

      let data = abi::permit::allowance(
         client.clone(),
         permit2_address,
         alice.address,
         weth.address,
         router_addr,
      )
      .await
      .unwrap();

      let typed_data = generate_permit2_typed_data(
         chain_id,
         weth.address,
         router_addr,
         amount_in.wei2(),
         permit2_address,
         expiration,
         sig_deadline,
         data.nonce,
      )
      .unwrap();

      let signature = signer
         .borrow()
         .sign_dynamic_typed_data(&typed_data)
         .await
         .unwrap();

      let permit_input = encode_permit2_permit_ur_input(
         weth.address,
         amount_in.wei2(),
         expiration,
         data.nonce,
         router_addr,
         sig_deadline,
         signature,
      );
      commands.push(Commands::PERMIT2_PERMIT as u8);
      inputs.push(permit_input);

      let deadline = U256::from(current_time + 30 * 60);
      let command_bytes = Bytes::from(commands);
      println!("Command Bytes: {:?}", command_bytes);
      let call_data = encode_execute_with_deadline(command_bytes, inputs, deadline);
      println!("Calldata: {:?}", call_data);

      let block = client.get_block(BlockId::latest()).await.unwrap();
      let fork_db = fork_factory.new_sandbox_fork();
      let mut evm = new_evm(chain_id, block, fork_db);

      // make sure alice has enough balance
      let balance = simulate::erc20_balance(&mut evm, weth.address, alice.address).unwrap();
      assert!(balance == weth_balance.wei2());
      println!("Alice WETH Balance: {}", weth_balance.formatted());

      // simulate the call to the router
      evm.tx.caller = alice.address;
      evm.tx.data = call_data;
      evm.tx.value = U256::ZERO;
      evm.tx.kind = TxKind::Call(router_addr);

      let res = evm.transact(evm.tx.clone()).unwrap().result;
      let output = res.output().unwrap();

      if !res.is_success() {
         let err = revert_msg(&output);
         println!("Call Reverted: {}", err);
         println!("Output: {:?}", output);
         println!("Gas Used: {}", res.gas_used());
      } else {
         println!("Call Successful");
         println!("Gas Used: {}", res.gas_used());
      }
   }

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn can_swap_on_v2() {
      let url = Url::parse("https://eth.merkle.io").unwrap();
      let client = ProviderBuilder::new().on_http(url);
      let chain_id = 1;

      let mut pool = UniswapV2Pool::weth_uni();
      pool.update_state(client.clone(), None).await.unwrap();

      let weth = pool.base_currency().clone();
      let uni = pool.quote_currency().clone();

      let eth_balance = NumericValue::parse_to_wei("1", 18);
      let weth_balance = NumericValue::parse_to_wei("10", 18);
      // Create Alice with 1 ETH balance
      let alice = DummyAccount::new(AccountType::EOA, eth_balance.wei2());
      let signer = SecureSigner::new(alice.key.clone());

      let mut factory = ForkFactory::new_sandbox_factory(client.clone(), chain_id, None, None);
      factory.insert_dummy_account(alice.clone());
      // give Alice 10 WETH
      factory
         .give_token(alice.address, weth.address(), weth_balance.wei2())
         .unwrap();

      // Get the amount of UNI received for 1 WETH
      let amount_in = NumericValue::parse_to_wei("1", 18);
      let amount_out = pool.simulate_swap(&weth, amount_in.wei2()).unwrap();
      let amount_out = NumericValue::format_wei(amount_out, uni.decimals());
      println!("Amount out: {}", amount_out.formatted());

      // calculate 1% slippage tolerance
      let amount_with_slip = amount_out.f64() * 0.99;
      let amount_out_min = NumericValue::parse_to_wei(&amount_with_slip.to_string(), uni.decimals());
      println!("Amount out with slippage: {}", amount_out_min.formatted());

      println!(
         "Swapped {} {} For {} {}",
         amount_in.formatted(),
         weth.symbol(),
         amount_out.formatted(),
         uni.symbol()
      );

      let router_addr = utils::address::uniswap_v4_universal_router(1).unwrap();
      let route = Route::new(vec![pool], weth.clone(), uni.clone());

      // Build the calldata
      let exec_params = build_execute_params(
         client.clone(),
         chain_id,
         &route,
         SwapType::ExactInput,
         amount_in.wei2(),
         amount_out_min.wei2(),
         signer.clone(),
         signer.borrow().address(),
      )
      .await
      .unwrap();

      let block = client.get_block(BlockId::latest()).full().await.unwrap();

      // Prepare the fork enviroment
      let fork_db = factory.new_sandbox_fork();
      let mut evm = new_evm(1, block, fork_db);

      let balance = simulate::erc20_balance(&mut evm, weth.address(), alice.address).unwrap();
      assert!(balance == weth_balance.wei2());
      println!("Alice WETH Balance: {}", weth_balance.formatted());

      let permit2 = permit2_contract(chain_id).unwrap();

      if exec_params.token_needs_approval {
         // Approve the Permit2 contract to spend the tokens
         // it commit changes
         simulate::approve_token(
            &mut evm,
            weth.address(),
            alice.address,
            permit2,
            amount_in.wei2(),
         )
         .unwrap();
      }

      // simulate the call to the router

      evm.tx.caller = alice.address;
      evm.tx.data = exec_params.call_data;
      evm.tx.value = exec_params.value;
      evm.tx.kind = TxKind::Call(router_addr);

      let res = evm.transact_commit(evm.tx.clone()).unwrap();
      let output = res.output().unwrap();
      if !res.is_success() {
         let err = revert_msg(output);
         println!("Call Reverted: {}", err);
         println!("Output: {:?}", output);
         println!("Gas Used: {}", res.gas_used());
         panic!("Call Failed");
      }

      println!("Call Successful");
      println!("Gas Used: {}", res.gas_used());

      let uni_balance = simulate::erc20_balance(&mut evm, uni.address(), alice.address).unwrap();
      let uni_balance = NumericValue::format_wei(uni_balance, uni.decimals());
      println!("Alice UNI Balance: {}", uni_balance.formatted());
   }

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn can_swap_on_v3() {
      let url = "https://eth.merkle.io".parse().unwrap();
      let client = ProviderBuilder::new().on_http(url);
      let chain_id = 1;

      let mut pool = UniswapV3Pool::usdt_uni();
      pool.update_state(client.clone(), None).await.unwrap();

      let usdt = pool.base_currency().clone();
      let uni = pool.quote_currency().clone();

      let amount_in = NumericValue::parse_to_wei("1000", usdt.decimals());
      let amount_out = pool.simulate_swap(&usdt, amount_in.wei2()).unwrap();
      let amount_out = NumericValue::format_wei(amount_out, uni.decimals());

      println!("Amount out: {}", amount_out.formatted());

      // calculate 1% slippage tolerance
      let amount_with_slip = amount_out.f64() * 0.99;
      let amount_out_min = NumericValue::parse_to_wei(&amount_with_slip.to_string(), uni.decimals());
      println!("Amount out with slippage: {}", amount_out_min.formatted());

      println!(
         "Swapped {} {} For {} {}",
         amount_in.formatted(),
         usdt.symbol(),
         amount_out.formatted(),
         uni.symbol()
      );

      let usdt_balance = NumericValue::parse_to_wei("10000", usdt.decimals());
      // Create Alice
      let alice = DummyAccount::new(AccountType::EOA, U256::ZERO);
      let signer = SecureSigner::new(alice.key.clone());

      let mut factory = ForkFactory::new_sandbox_factory(client.clone(), chain_id, None, None);
      // give Alice 10k USDT
      factory.insert_dummy_account(alice.clone());
      factory
         .give_token(alice.address, usdt.address(), usdt_balance.wei2())
         .unwrap();

      let router_addr = utils::address::uniswap_v4_universal_router(chain_id).unwrap();
      let route = Route::new(vec![pool], usdt.clone(), uni.clone());

      // Build the calldata
      let exec_params = build_execute_params(
         client.clone(),
         chain_id,
         &route,
         SwapType::ExactInput,
         amount_in.wei2(),
         amount_out_min.wei2(),
         signer.clone(),
         signer.borrow().address(),
      )
      .await
      .unwrap();

      let block = client.get_block(BlockId::latest()).full().await.unwrap();

      // Prepare the fork enviroment
      let fork_db = factory.new_sandbox_fork();
      let mut evm = new_evm(chain_id, block, fork_db);

      let balance = simulate::erc20_balance(&mut evm, usdt.address(), alice.address).unwrap();
      assert!(balance == usdt_balance.wei2());
      println!("Alice USDT Balance: {}", usdt_balance.formatted());

      let permit2 = permit2_contract(chain_id).unwrap();

      if exec_params.token_needs_approval {
         // Approve the Permit2 contract to spend the tokens
         // it commit changes
         simulate::approve_token(
            &mut evm,
            usdt.address(),
            alice.address,
            permit2,
            amount_in.wei2(),
         )
         .unwrap();
      }

      // simulate the call to the router

      evm.tx.caller = alice.address;
      evm.tx.data = exec_params.call_data;
      evm.tx.value = exec_params.value;
      evm.tx.kind = TxKind::Call(router_addr);

      let res = evm.transact_commit(evm.tx.clone()).unwrap();
      let output = res.output().unwrap();
      if !res.is_success() {
         let err = revert_msg(output);
         println!("Call Reverted: {}", err);
         println!("Output: {:?}", output);
         println!("Gas Used: {}", res.gas_used());
         panic!("Call Failed");
      }

      println!("Call Successful");
      println!("Gas Used: {}", res.gas_used());

      let uni_balance = simulate::erc20_balance(&mut evm, uni.address(), alice.address).unwrap();
      let uni_balance = NumericValue::format_wei(uni_balance, uni.decimals());
      println!("Alice UNI Balance: {}", uni_balance.formatted());
   }

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn can_swap_from_eth_on_v2_v3() {
      let url = "https://eth.merkle.io".parse().unwrap();
      let client = ProviderBuilder::new().on_http(url);
      let chain_id = 1;

      let mut weth_uni = UniswapV2Pool::weth_uni();
      let mut weth_usdc = UniswapV3Pool::weth_usdc();

      weth_usdc.update_state(client.clone(), None).await.unwrap();
      weth_uni.update_state(client.clone(), None).await.unwrap();

      let weth = weth_uni.base_currency().clone();
      let uni = weth_uni.quote_currency().clone();
      let usdc = Currency2::from(ERC20Token::usdc());
      let eth = Currency2::from(NativeCurrency::from(chain_id));

      let amount_in = NumericValue::parse_to_wei("1", eth.decimals());
      let amount_out = weth_uni.simulate_swap(&weth, amount_in.wei2()).unwrap();
      let amount_out = NumericValue::format_wei(amount_out, uni.decimals());

      println!("Amount out: {}", amount_out.formatted());

      // calculate 1% slippage tolerance
      let amount_with_slip = amount_out.f64() * 0.99;
      let amount_out_min = NumericValue::parse_to_wei(&amount_with_slip.to_string(), uni.decimals());
      println!("Min Amount out {}", amount_out_min.formatted());

      println!(
         "Swapped {} {} For {} {}",
         amount_in.formatted(),
         weth.symbol(),
         amount_out.formatted(),
         uni.symbol()
      );

      let eth_balance = NumericValue::parse_to_wei("10", eth.decimals());
      // Create Alice
      let alice = DummyAccount::new(AccountType::EOA, eth_balance.wei2());
      let signer = SecureSigner::new(alice.key.clone());

      let mut factory = ForkFactory::new_sandbox_factory(client.clone(), chain_id, None, None);
      factory.insert_dummy_account(alice.clone());

      let router_addr = utils::address::uniswap_v4_universal_router(chain_id).unwrap();
      let route = Route::new(vec![weth_uni.clone()], eth.clone(), uni.clone());

      // Build the calldata
      let exec_params = build_execute_params(
         client.clone(),
         chain_id,
         &route,
         SwapType::ExactInput,
         amount_in.wei2(),
         amount_out_min.wei2(),
         signer.clone(),
         signer.borrow().address(),
      )
      .await
      .unwrap();

      let block = client.get_block(BlockId::latest()).full().await.unwrap();

      // Prepare the fork enviroment
      let fork_db = factory.new_sandbox_fork();
      let mut evm = new_evm(chain_id, block, fork_db);

      // simulate the call to the router
      evm.tx.caller = alice.address;
      evm.tx.data = exec_params.call_data;
      evm.tx.value = exec_params.value;
      evm.tx.kind = TxKind::Call(router_addr);

      let res = evm.transact_commit(evm.tx.clone()).unwrap();
      let output = res.output().unwrap();
      if !res.is_success() {
         let err = revert_msg(output);
         println!("Call Reverted: {}", err);
         println!("Output: {:?}", output);
         println!("Gas Used: {}", res.gas_used());
         panic!("Call Failed");
      }

      println!("Call Successful");
      println!("Gas Used: {}", res.gas_used());

      let uni_balance = simulate::erc20_balance(&mut evm, uni.address(), alice.address).unwrap();
      let uni_balance = NumericValue::format_wei(uni_balance, uni.decimals());
      println!("Alice UNI Balance: {}", uni_balance.formatted());

      // V3 swap
      let amount_out = weth_usdc.simulate_swap(&weth, amount_in.wei2()).unwrap();
      let amount_out = NumericValue::format_wei(amount_out, usdc.decimals());

      println!("Amount out: {}", amount_out.formatted());

      // calculate 1% slippage tolerance
      let amount_with_slip = amount_out.f64() * 0.99;
      let amount_out_min = NumericValue::parse_to_wei(&amount_with_slip.to_string(), usdc.decimals());
      println!("Min Amount out {}", amount_out_min.formatted());

      println!(
         "Swapped {} {} For {} {}",
         amount_in.formatted(),
         weth.symbol(),
         amount_out.formatted(),
         usdc.symbol()
      );

      let route = Route::new(vec![weth_usdc.clone()], eth.clone(), usdc.clone());

      // Build the calldata
      let exec_params = build_execute_params(
         client.clone(),
         chain_id,
         &route,
         SwapType::ExactInput,
         amount_in.wei2(),
         amount_out_min.wei2(),
         signer.clone(),
         signer.borrow().address(),
      )
      .await
      .unwrap();

      evm.tx.caller = alice.address;
      evm.tx.data = exec_params.call_data;
      evm.tx.value = exec_params.value;
      evm.tx.kind = TxKind::Call(router_addr);

      let res = evm.transact_commit(evm.tx.clone()).unwrap();
      let output = res.output().unwrap();
      if !res.is_success() {
         let err = revert_msg(output);
         println!("Call Reverted: {}", err);
         println!("Output: {:?}", output);
         println!("Gas Used: {}", res.gas_used());
         panic!("Call Failed");
      }

      println!("Call Successful");
      println!("Gas Used: {}", res.gas_used());
      let usdc_balance = simulate::erc20_balance(&mut evm, usdc.address(), alice.address).unwrap();
      let usdc_balance = NumericValue::format_wei(usdc_balance, usdc.decimals());
      println!("Alice USDC Balance: {}", usdc_balance.formatted());
   }

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn can_swap_on_v4() {
      let url = "https://eth.merkle.io".parse().unwrap();
      let client = ProviderBuilder::new().on_http(url);
      let chain_id = 1;

      let mut pool = UniswapV4Pool::eth_uni();
      pool.update_state(client.clone(), None).await.unwrap();

      let eth = pool.base_currency().clone();
      let uni = pool.quote_currency().clone();

      println!("Base Currency: {}", eth.symbol());
      println!("Quote Currency: {}", uni.symbol());

      let amount_in = NumericValue::parse_to_wei("1", uni.decimals());
      let amount_out = pool.simulate_swap(&eth, amount_in.wei2()).unwrap();
      let amount_out = NumericValue::format_wei(amount_out, uni.decimals());

      println!("Amount out: {}", amount_out.formatted());

      // calculate 1% slippage tolerance
      let amount_with_slip = amount_out.f64() * 0.99;
      let amount_out_min = NumericValue::parse_to_wei(&amount_with_slip.to_string(), uni.decimals());
      println!("Amount out with slippage: {}", amount_out_min.formatted());

      println!(
         "Swapped {} {} For {} {}",
         amount_in.formatted(),
         eth.symbol(),
         amount_out.formatted(),
         uni.symbol()
      );

      let eth_balance = NumericValue::parse_to_wei("10", eth.decimals());
      // Create Alice
      let alice = DummyAccount::new(AccountType::EOA, eth_balance.wei2());
      let signer = SecureSigner::new(alice.key.clone());

      let mut factory = ForkFactory::new_sandbox_factory(client.clone(), chain_id, None, None);
      factory.insert_dummy_account(alice.clone());

      let router_addr = utils::address::uniswap_v4_universal_router(chain_id).unwrap();
      let route = Route::new(vec![pool], eth.clone(), uni.clone());

      // Build the calldata
      let exec_params = build_execute_params(
         client.clone(),
         chain_id,
         &route,
         SwapType::ExactInput,
         amount_in.wei2(),
         amount_out_min.wei2(),
         signer.clone(),
         signer.borrow().address(),
      )
      .await
      .unwrap();

      let block = client.get_block(BlockId::latest()).full().await.unwrap();

      // Prepare the fork enviroment
      let fork_db = factory.new_sandbox_fork();
      let mut evm = new_evm(chain_id, block, fork_db);


      // simulate the call to the router
      evm.tx.caller = alice.address;
      evm.tx.data = exec_params.call_data;
      evm.tx.value = exec_params.value;
      evm.tx.kind = TxKind::Call(router_addr);

      let res = evm.transact_commit(evm.tx.clone()).unwrap();
      let output = res.output().unwrap();
      if !res.is_success() {
         let err = revert_msg(output);
         println!("Call Reverted: {}", err);
         println!("Output: {:?}", output);
         println!("Gas Used: {}", res.gas_used());
         panic!("Call Failed");
      }

      println!("Call Successful");
      println!("Gas Used: {}", res.gas_used());

      let uni_balance = simulate::erc20_balance(&mut evm, uni.address(), alice.address).unwrap();
      let uni_balance = NumericValue::format_wei(uni_balance, uni.decimals());
      println!("Alice UNI Balance: {}", uni_balance.formatted());
   }
}
