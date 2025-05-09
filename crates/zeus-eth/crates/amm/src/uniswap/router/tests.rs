#[cfg(test)]
mod tests {
   use crate::uniswap::router::{SwapStep, SwapType, v4::*};
   use crate::AnyUniswapPool;
use crate::{UniswapPool, UniswapV2Pool, UniswapV3Pool, uniswap::v4::pool::UniswapV4Pool};
   use abi::uniswap::v4::router::*;
   use alloy_primitives::Bytes;
   use alloy_primitives::{TxKind, U256};
   use alloy_provider::{Provider, ProviderBuilder};
   use alloy_rpc_types::BlockId;
   use currency::{Currency, ERC20Token, NativeCurrency};
   use revm_utils::{
      AccountType, DummyAccount, ExecuteCommitEvm, ExecuteEvm, ForkFactory, Host, new_evm, op_revm::OpSpecId,
      revert_msg, revm::context::result::ExecutionResult, simulate,
   };
   use revm_utils::{
      op_revm::{DefaultOp, OpBuilder, OpContext},
      revm::handler::EvmTr,
   };
   use url::Url;
   use utils::{NumericValue, address::permit2_contract, parse_typed_data};
   use wallet::{SecureSigner, alloy_signer::Signer};

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn can_swap_on_multiple_pools() {
      let url = "https://eth.merkle.io".parse().unwrap();
      let client = ProviderBuilder::new().connect_http(url);
      let chain_id = 1;

      let mut weth_uni = UniswapV2Pool::weth_uni();
      let mut weth_usdc = UniswapV3Pool::weth_usdc();

      weth_uni.update_state(client.clone(), None).await.unwrap();
      weth_usdc.update_state(client.clone(), None).await.unwrap();

      // Buy WETH on the USDC pool and sell WETH on the UNI pool
      let usdc = Currency::from(ERC20Token::usdc());
      let weth = Currency::from(ERC20Token::weth());
      let uni = weth_uni.quote_currency().clone();

      let usdc_amount_in = NumericValue::parse_to_wei("1800", usdc.decimals());
      let weth_amount_out = weth_usdc.simulate_swap(&usdc, usdc_amount_in.wei2()).unwrap();
      let mut weth_amount_out = NumericValue::format_wei(weth_amount_out, weth.decimals());
      weth_amount_out.calc_slippage(0.5, weth.decimals());

      println!(
         "Sell {} {} For {} {}",
         usdc_amount_in.formatted(),
         usdc.symbol(),
         weth_amount_out.formatted(),
         weth.symbol()
      );

      let uni_amount_out = weth_uni.simulate_swap(&weth, weth_amount_out.wei2()).unwrap();
      let mut uni_amount_out = NumericValue::format_wei(uni_amount_out, uni.decimals());
      uni_amount_out.calc_slippage(0.5, uni.decimals());

      println!(
         "Sell {} {} For {} {}",
         weth_amount_out.formatted(),
         weth.symbol(),
         uni_amount_out.formatted(),
         uni.symbol()
      );

      let alice = DummyAccount::new(AccountType::EOA, U256::ZERO);
      println!("Alice address: {:?}", alice.address);
      let signer = SecureSigner::new(alice.key.clone());
      let usdc_balance = NumericValue::parse_to_wei("10000", usdc.decimals());

      let mut factory = ForkFactory::new_sandbox_factory(client.clone(), chain_id, None, None);
      factory.insert_dummy_account(alice.clone());
      factory
         .give_token(alice.address, usdc.address(), usdc_balance.wei2())
         .unwrap();

      let router_addr = utils::address::uniswap_v4_universal_router(chain_id).unwrap();
      println!("Router address: {:?}", router_addr);
      let swap_step = SwapStep::new(
         AnyUniswapPool::from_pool(weth_usdc),
         usdc_amount_in.clone(),
         weth_amount_out.clone(),
         usdc.clone(),
         weth.clone(),
      );

      let swap_step2 = SwapStep::new(
         AnyUniswapPool::from_pool(weth_uni),
         weth_amount_out,
         uni_amount_out.clone(),
         weth.clone(),
         uni.clone(),
      );


      // Build the calldata
      let exec_params = build_execute_params(
         client.clone(),
         chain_id,
         vec![swap_step, swap_step2],
         SwapType::ExactInput,
         usdc_amount_in.wei2(),
         uni_amount_out.wei2(),
         usdc.clone(),
         uni.clone(),
         signer,
         alice.address,
      )
      .await
      .unwrap();

      let block = client.get_block(BlockId::latest()).full().await.unwrap();

      // Prepare the fork enviroment
      let fork_db = factory.new_sandbox_fork();
      let mut evm = new_evm(chain_id.into(), block.as_ref(), fork_db);

      let balance = simulate::erc20_balance(&mut evm, usdc.address(), alice.address).unwrap();
      assert!(balance == usdc_balance.wei2());
      println!("Alice USDC Balance: {}", usdc_balance.formatted());

      let permit2 = permit2_contract(chain_id).unwrap();

      if exec_params.token_needs_approval {
         // Approve the Permit2 contract to spend the tokens
         // it commit changes
         simulate::approve_token(
            &mut evm,
            usdc.address(),
            alice.address,
            permit2,
            usdc_amount_in.wei2(),
         )
         .unwrap();
      }

      // simulate the call to the router
      evm.tx.caller = alice.address;
      evm.tx.data = exec_params.call_data;
      evm.tx.value = exec_params.value;
      evm.tx.kind = TxKind::Call(router_addr);

      let res = evm.replay_commit().unwrap();
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
   async fn can_swap_on_v3_on_base() {
      let url = "https://base-rpc.publicnode.com".parse().unwrap();
      let client = ProviderBuilder::new().connect_http(url);
      let chain_id = 8453;

      let mut pool = UniswapV3Pool::weth_usdc_base();
      pool.update_state(client.clone(), None).await.unwrap();

      let currency_in = Currency::from(ERC20Token::weth_base());
      let currency_out = Currency::from(ERC20Token::usdc_base());

      let amount_in = NumericValue::parse_to_wei("1", currency_in.decimals());
      let amount_out = pool.simulate_swap(&currency_in, amount_in.wei2()).unwrap();
      let amount_out = NumericValue::format_wei(amount_out, currency_out.decimals());

      println!("Amount out: {}", amount_out.formatted());

      // calculate 1% slippage tolerance
      let amount_with_slip = amount_out.f64() * 0.99;
      let amount_out_min = NumericValue::parse_to_wei(&amount_with_slip.to_string(), currency_out.decimals());
      println!("Amount out with slippage: {}", amount_out_min.formatted());

      println!(
         "Swapped {} {} For {} {}",
         amount_in.formatted(),
         currency_in.symbol(),
         amount_out.formatted(),
         currency_out.symbol()
      );

      // Create Alice
      let alice = DummyAccount::new(AccountType::EOA, U256::ZERO);
      let signer = SecureSigner::new(alice.key.clone());
      let weth_balance = NumericValue::parse_to_wei("10", currency_in.decimals());

      let mut factory = ForkFactory::new_sandbox_factory(client.clone(), chain_id, None, None);
      // give Alice 10 WETH
      factory.insert_dummy_account(alice.clone());
      factory
         .give_token(alice.address, currency_in.address(), weth_balance.wei2())
         .unwrap();

      let router_addr = utils::address::uniswap_v4_universal_router(chain_id).unwrap();
      let swap_step = SwapStep::new(
         pool,
         amount_in.clone(),
         amount_out_min.clone(),
         currency_in.clone(),
         currency_out.clone(),
      );

      // Build the calldata
      let exec_params = build_execute_params(
         client.clone(),
         chain_id,
         vec![swap_step],
         SwapType::ExactInput,
         amount_in.wei2(),
         amount_out_min.wei2(),
         currency_in.clone(),
         currency_out.clone(),
         signer.clone(),
         signer.borrow().address(),
      )
      .await
      .unwrap();

      let block = client.get_block(BlockId::latest()).await.unwrap();

      // Prepare the fork enviroment
      let fork_db = factory.new_sandbox_fork();
      let mut evm = new_evm(chain_id.into(), block.as_ref(), fork_db);

      let balance = simulate::erc20_balance(&mut evm, currency_in.address(), alice.address).unwrap();
      assert!(balance == weth_balance.wei2());
      println!("Alice WETH Balance: {}", weth_balance.formatted());

      let permit2 = permit2_contract(chain_id).unwrap();

      if exec_params.token_needs_approval {
         // Approve the Permit2 contract to spend the tokens
         // it commit changes

         simulate::approve_token(
            &mut evm,
            currency_in.address(),
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

      let res = evm.replay_commit().unwrap();
      let output = res.output().unwrap_or_default();

      let res2 = res.clone();
      match res2 {
         ExecutionResult::Halt { reason, .. } => {
            println!("Halt Reason: {:?}", reason);
         }
         _ => {}
      }

      if !res.is_success() {
         let err = revert_msg(output);
         println!("Call Reverted: {}", err);
         println!("Output: {:?}", output);
         println!("Gas Used: {}", res.gas_used());
         panic!("Call Failed");
      }

      println!("Call Successful");
      println!("Gas Used: {}", res.gas_used());

      let balance = simulate::erc20_balance(&mut evm, currency_out.address(), alice.address).unwrap();
      let balance = NumericValue::format_wei(balance, currency_out.decimals());
      println!(
         "Alice {} Balance: {}",
         currency_out.symbol(),
         balance.formatted()
      );
   }

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn can_swap_on_v3_on_base2() {
      let url = "https://base-rpc.publicnode.com".parse().unwrap();
      let client = ProviderBuilder::new().connect_http(url);
      let chain_id = 8453;

      let mut pool = UniswapV3Pool::weth_usdc_base();
      pool.update_state(client.clone(), None).await.unwrap();

      let currency_in = Currency::from(ERC20Token::weth_base());
      let currency_out = Currency::from(ERC20Token::usdc_base());

      let amount_in = NumericValue::parse_to_wei("1", currency_in.decimals());
      let amount_out = pool.simulate_swap(&currency_in, amount_in.wei2()).unwrap();
      let amount_out = NumericValue::format_wei(amount_out, currency_out.decimals());

      println!("Amount out: {}", amount_out.formatted());

      // calculate 1% slippage tolerance
      let amount_with_slip = amount_out.f64() * 0.99;
      let amount_out_min = NumericValue::parse_to_wei(&amount_with_slip.to_string(), currency_out.decimals());
      println!("Amount out with slippage: {}", amount_out_min.formatted());

      println!(
         "Swapped {} {} For {} {}",
         amount_in.formatted(),
         currency_in.symbol(),
         amount_out.formatted(),
         currency_out.symbol()
      );

      // Create Alice
      let alice = DummyAccount::new(AccountType::EOA, U256::ZERO);
      let signer = SecureSigner::new(alice.key.clone());
      let weth_balance = NumericValue::parse_to_wei("10", currency_in.decimals());

      let mut factory = ForkFactory::new_sandbox_factory(client.clone(), chain_id, None, None);
      // give Alice 10 WETH
      factory.insert_dummy_account(alice.clone());
      factory
         .give_token(alice.address, currency_in.address(), weth_balance.wei2())
         .unwrap();

      let router_addr = utils::address::uniswap_v4_universal_router(chain_id).unwrap();
      let swap_step = SwapStep::new(
         pool,
         amount_in.clone(),
         amount_out_min.clone(),
         currency_in.clone(),
         currency_out.clone(),
      );

      // Build the calldata
      let exec_params = build_execute_params(
         client.clone(),
         chain_id,
         vec![swap_step],
         SwapType::ExactInput,
         amount_in.wei2(),
         amount_out_min.wei2(),
         currency_in.clone(),
         currency_out.clone(),
         signer.clone(),
         signer.borrow().address(),
      )
      .await
      .unwrap();

      let block = client
         .get_block(BlockId::latest())
         .await
         .unwrap()
         .expect("Block not found");

      // Prepare the fork enviroment
      let fork_db = factory.new_sandbox_fork();
      // let mut evm = new_evm(chain_id, block, fork_db);
      let mut evm = OpContext::op().with_db(fork_db).build_op();

      evm.ctx().cfg.spec = OpSpecId::HOLOCENE;
      // evm.ctx().cfg.chain_id = chain_id;
      evm.ctx().block.number = block.header.number;
      evm.ctx().block.timestamp = block.header.timestamp;
      evm.ctx().block.beneficiary = block.header.beneficiary;

      evm.ctx().cfg.disable_balance_check = true;
      evm.ctx().cfg.disable_base_fee = true;
      evm.ctx().cfg.disable_block_gas_limit = true;
      evm.ctx().cfg.disable_nonce_check = true;

      // let balance = simulate::erc20_balance(&mut evm, currency_in.address(), alice.address).unwrap();
      // assert!(balance == weth_balance.wei2());
      // println!("Alice WETH Balance: {}", weth_balance.formatted());

      let permit2 = permit2_contract(chain_id).unwrap();

      if exec_params.token_needs_approval {
         // Approve the Permit2 contract to spend the tokens
         // it commit changes
         /*
          simulate::approve_token(
             &mut evm,
             currency_in.address(),
             alice.address,
             permit2,
             amount_in.wei2(),
          )
          .unwrap();
         */

         let data = abi::erc20::encode_approve(permit2, amount_in.wei2());
         evm.ctx().modify_tx(|tx| {
            tx.base.caller = alice.address;
            tx.base.data = data;
            tx.base.value = U256::ZERO;
            tx.base.kind = TxKind::Call(currency_in.address());
         });

         let res = evm.replay_commit().unwrap();

         if !res.is_success() {
            let err = revert_msg(&res.output().unwrap());
            println!("Call Reverted: {}", err);
            println!("Output: {:?}", res.output().unwrap());
            println!("Gas Used: {}", res.gas_used());
            panic!("Call Failed");
         }

         println!("Approve Call Successful");
         println!("Gas Used: {}", res.gas_used());
      }

      // simulate the call to the router

      evm.ctx().modify_tx(|tx| {
         tx.base.caller = alice.address;
         tx.base.data = exec_params.call_data;
         tx.base.value = exec_params.value;
         tx.base.kind = TxKind::Call(router_addr);
      });

      let res = evm.replay_commit().unwrap();
      let output = res.output().unwrap_or_default();

      let res2 = res.clone();
      match res2 {
         ExecutionResult::Halt { reason, .. } => {
            println!("Halt Reason: {:?}", reason);
         }
         _ => {}
      }

      if !res.is_success() {
         let err = revert_msg(output);
         println!("Call Reverted: {}", err);
         println!("Output: {:?}", output);
         println!("Gas Used: {}", res.gas_used());
         panic!("Call Failed");
      }

      println!("Call Successful");
      println!("Gas Used: {}", res.gas_used());

      /*
      let balance = simulate::erc20_balance(&mut evm, currency_out.address(), alice.address).unwrap();
      let balance = NumericValue::format_wei(balance, currency_out.decimals());
      println!(
         "Alice {} Balance: {}",
         currency_out.symbol(),
         balance.formatted()
      );
      */
   }

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn can_call_permit2() {
      let url = Url::parse("https://eth.merkle.io").unwrap();
      let client = ProviderBuilder::new().connect_http(url);
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

      let value = generate_permit2_typed_data(
         chain_id,
         weth.address,
         router_addr,
         amount_in.wei2(),
         permit2_address,
         expiration,
         sig_deadline,
         data.nonce,
      );
      let typed_data = parse_typed_data(value).unwrap();

      let signature = signer
         .borrow()
         .sign_dynamic_typed_data(&typed_data)
         .await
         .unwrap();

      let permit_input = abi::permit::encode_permit2_permit_ur_input(
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
      let mut evm = new_evm(chain_id.into(), block.as_ref(), fork_db);

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
   async fn can_swap_from_erc20_to_eth() {
      let url = Url::parse("https://eth.merkle.io").unwrap();
      let client = ProviderBuilder::new().connect_http(url);
      let chain_id = 1;

      let mut pool = UniswapV3Pool::weth_usdc();
      pool.update_state(client.clone(), None).await.unwrap();

      let eth = Currency::from(NativeCurrency::from(1));
      let usdc = Currency::from(ERC20Token::usdc());

      let eth_balance = NumericValue::parse_to_wei("1", 18);
      let usdc_balance = NumericValue::parse_to_wei("10000", usdc.decimals());

      let currency_in = usdc;
      let currency_out = eth;

      // Create Alice with 1 ETH balance
      let alice = DummyAccount::new(AccountType::EOA, eth_balance.wei2());
      let signer = SecureSigner::new(alice.key.clone());

      let mut factory = ForkFactory::new_sandbox_factory(client.clone(), chain_id, None, None);
      factory.insert_dummy_account(alice.clone());
      // give Alice 10k USDC
      factory
         .give_token(alice.address, currency_in.address(), usdc_balance.wei2())
         .unwrap();

      let amount_in = usdc_balance;
      let amount_out = pool.simulate_swap(&currency_in, amount_in.wei2()).unwrap();
      let amount_out = NumericValue::format_wei(amount_out, currency_out.decimals());
      println!("Amount out: {}", amount_out.formatted());

      // calculate 1% slippage tolerance
      let amount_with_slip = amount_out.f64() * 0.99;
      let amount_out_min = NumericValue::parse_to_wei(&amount_with_slip.to_string(), currency_out.decimals());
      println!("Amount out with slippage: {}", amount_out_min.formatted());

      let router_addr = utils::address::uniswap_v4_universal_router(1).unwrap();
      let swap_step = SwapStep::new(
         pool,
         amount_in.clone(),
         amount_out_min.clone(),
         currency_in.clone(),
         currency_out.clone(), // ETH as output
      );

      // Build the calldata
      let exec_params = build_execute_params(
         client.clone(),
         chain_id,
         vec![swap_step],
         SwapType::ExactInput,
         amount_in.wei2(),
         amount_out_min.wei2(),
         currency_in.clone(),
         currency_out.clone(),
         signer.clone(),
         signer.borrow().address(),
      )
      .await
      .unwrap();

      let block = client.get_block(BlockId::latest()).full().await.unwrap();

      // Prepare the fork enviroment
      let fork_db = factory.new_sandbox_fork();
      let mut evm = new_evm(1.into(), block.as_ref(), fork_db);

      let permit2 = permit2_contract(chain_id).unwrap();

      if exec_params.token_needs_approval {
         // Approve the Permit2 contract to spend the tokens
         // it commit changes
         simulate::approve_token(
            &mut evm,
            currency_in.address(),
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

      let state_load = evm.balance(alice.address).unwrap();
      let balance = NumericValue::format_wei(state_load.data, currency_out.decimals());
      println!("Alice ETH Balance: {}", balance.formatted());
   }

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn can_swap_on_v2() {
      let url = Url::parse("https://eth.merkle.io").unwrap();
      let client = ProviderBuilder::new().connect_http(url);
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
      let swap_step = SwapStep::new(
         pool,
         amount_in.clone(),
         amount_out_min.clone(),
         weth.clone(),
         uni.clone(),
      );

      // Build the calldata
      let exec_params = build_execute_params(
         client.clone(),
         chain_id,
         vec![swap_step],
         SwapType::ExactInput,
         amount_in.wei2(),
         amount_out_min.wei2(),
         weth.clone(),
         uni.clone(),
         signer.clone(),
         signer.borrow().address(),
      )
      .await
      .unwrap();

      let block = client.get_block(BlockId::latest()).full().await.unwrap();

      // Prepare the fork enviroment
      let fork_db = factory.new_sandbox_fork();
      let mut evm = new_evm(1.into(), block.as_ref(), fork_db);

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
      let client = ProviderBuilder::new().connect_http(url);
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
      let swap_step = SwapStep::new(
         pool,
         amount_in.clone(),
         amount_out_min.clone(),
         usdt.clone(),
         uni.clone(),
      );

      // Build the calldata
      let exec_params = build_execute_params(
         client.clone(),
         chain_id,
         vec![swap_step],
         SwapType::ExactInput,
         amount_in.wei2(),
         amount_out_min.wei2(),
         usdt.clone(),
         uni.clone(),
         signer.clone(),
         signer.borrow().address(),
      )
      .await
      .unwrap();

      let block = client.get_block(BlockId::latest()).full().await.unwrap();

      // Prepare the fork enviroment
      let fork_db = factory.new_sandbox_fork();
      let mut evm = new_evm(chain_id.into(), block.as_ref(), fork_db);

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
      let client = ProviderBuilder::new().connect_http(url);
      let chain_id = 1;

      let mut weth_uni = UniswapV2Pool::weth_uni();
      let mut weth_usdc = UniswapV3Pool::weth_usdc();

      weth_usdc.update_state(client.clone(), None).await.unwrap();
      weth_uni.update_state(client.clone(), None).await.unwrap();

      let weth = weth_uni.base_currency().clone();
      let uni = weth_uni.quote_currency().clone();
      let usdc = Currency::from(ERC20Token::usdc());
      let eth = Currency::from(NativeCurrency::from(chain_id));

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
      let swap_step = SwapStep::new(
         weth_uni.clone(),
         amount_in.clone(),
         amount_out_min.clone(),
         eth.clone(),
         uni.clone(),
      );

      // Build the calldata
      let exec_params = build_execute_params(
         client.clone(),
         chain_id,
         vec![swap_step],
         SwapType::ExactInput,
         amount_in.wei2(),
         amount_out_min.wei2(),
         eth.clone(),
         uni.clone(),
         signer.clone(),
         signer.borrow().address(),
      )
      .await
      .unwrap();

      let block = client.get_block(BlockId::latest()).full().await.unwrap();

      // Prepare the fork enviroment
      let fork_db = factory.new_sandbox_fork();
      let mut evm = new_evm(chain_id.into(), block.as_ref(), fork_db);

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

      let swap_step = SwapStep::new(
         weth_usdc.clone(),
         amount_in.clone(),
         amount_out_min.clone(),
         eth.clone(),
         usdc.clone(),
      );

      // Build the calldata
      let exec_params = build_execute_params(
         client.clone(),
         chain_id,
         vec![swap_step],
         SwapType::ExactInput,
         amount_in.wei2(),
         amount_out_min.wei2(),
         eth.clone(),
         usdc.clone(),
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
      let client = ProviderBuilder::new().connect_http(url);
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
      let swap_step = SwapStep::new(
         pool,
         amount_in.clone(),
         amount_out_min.clone(),
         eth.clone(),
         uni.clone(),
      );
      // Build the calldata
      let exec_params = build_execute_params(
         client.clone(),
         chain_id,
         vec![swap_step],
         SwapType::ExactInput,
         amount_in.wei2(),
         amount_out_min.wei2(),
         eth.clone(),
         uni.clone(),
         signer.clone(),
         signer.borrow().address(),
      )
      .await
      .unwrap();

      let block = client.get_block(BlockId::latest()).full().await.unwrap();

      // Prepare the fork enviroment
      let fork_db = factory.new_sandbox_fork();
      let mut evm = new_evm(chain_id.into(), block.as_ref(), fork_db);

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
