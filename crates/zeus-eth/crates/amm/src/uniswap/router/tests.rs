#[cfg(test)]
mod tests {
   use crate::AnyUniswapPool;
   use crate::uniswap::router::{Commands, SwapStep, SwapType, encode_swap};
   use crate::{UniswapPool, UniswapV2Pool, UniswapV3Pool, uniswap::v4::pool::UniswapV4Pool};
   use abi::uniswap::universal_router_v2::*;
   use alloy_primitives::{
      Bytes, TxKind, U256, address,
      aliases::{U48, U160},
   };
   use alloy_provider::{Provider, ProviderBuilder};
   use alloy_rpc_types::BlockId;
   use alloy_sol_types::SolValue;
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
   use utils::generate_permit2_batch_value;
   use utils::{NumericValue, address::permit2_contract, generate_permit2_single_value, parse_typed_data};
   use wallet::{SecureSigner, alloy_signer::Signer};

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn can_call_permit2_batch() {
      let url = "https://reth-ethereum.ithaca.xyz/rpc".parse().unwrap();
      let client = ProviderBuilder::new().connect_http(url);
      let chain_id = 1;

      let block = client.get_block(BlockId::latest()).full().await.unwrap();

      let weth = ERC20Token::weth();
      let usdc = ERC20Token::usdc();

      let alice = DummyAccount::new(AccountType::EOA, U256::ZERO);
      let signer = SecureSigner::from(alice.key.clone());

      let mut factory = ForkFactory::new_sandbox_factory(client.clone(), chain_id, None, None);
      factory.insert_dummy_account(alice.clone());

      let weth_amount = NumericValue::parse_to_wei("1", weth.decimals);
      let usdc_amount = NumericValue::parse_to_wei("10000", usdc.decimals);

      factory
         .give_token(alice.address, weth.address, weth_amount.wei2())
         .unwrap();
      factory
         .give_token(alice.address, usdc.address, usdc_amount.wei2())
         .unwrap();

      let permit2_address = utils::address::permit2_contract(chain_id).unwrap();
      let router_addr = utils::address::universal_router_v2(chain_id).unwrap();

      let weth_nonce_fut = abi::permit::allowance(
         client.clone(),
         permit2_address,
         alice.address,
         weth.address,
         router_addr,
      );
      let usdc_nonce_fut = abi::permit::allowance(
         client.clone(),
         permit2_address,
         alice.address,
         usdc.address,
         router_addr,
      );
      let (weth_allowance, usdc_allowance) = tokio::try_join!(weth_nonce_fut, usdc_nonce_fut).unwrap();

      let now = std::time::SystemTime::now()
         .duration_since(std::time::UNIX_EPOCH)
         .unwrap()
         .as_secs();

      let expiration = now + 86400; // 1 day
      let expiration = U48::from(expiration);
      let sig_deadline = now + 60;
      let sig_deadline = U256::from(sig_deadline);

      let permit_detail_a = abi::permit::Permit2::PermitDetails {
         token: weth.address,
         amount: U160::from(weth_amount.wei2()),
         expiration,
         nonce: weth_allowance.nonce,
      };

      let permit_detail_b = abi::permit::Permit2::PermitDetails {
         token: usdc.address,
         amount: U160::from(usdc_amount.wei2()),
         expiration,
         nonce: usdc_allowance.nonce,
      };

      let details = vec![permit_detail_a, permit_detail_b];

      let permit_batch = abi::permit::Permit2::PermitBatch {
         details: details.clone(),
         spender: router_addr,
         sigDeadline: sig_deadline,
      };

      let msg_value = generate_permit2_batch_value(
         chain_id,
         details,
         router_addr,
         permit2_address,
         sig_deadline,
      );

      let typed_data = utils::parse_typed_data(msg_value).unwrap();
      let sig = signer
         .to_signer()
         .sign_dynamic_typed_data(&typed_data)
         .await
         .unwrap();

      let mut commands = Vec::new();
      let mut inputs = Vec::new();

      commands.push(Commands::PERMIT2_PERMIT_BATCH as u8);
      let input = abi::permit::encode_permit_batch_ur_input(permit_batch.clone(), sig);
      inputs.push(input);

      commands.push(Commands::PERMIT2_TRANSFER_FROM as u8);
      let input = abi::uniswap::Permit2TransferFrom {
         token: weth.address,
         recipient: router_addr,
         amount: U160::from(weth_amount.wei2()),
      };
      inputs.push(input.abi_encode_params().into());

      commands.push(Commands::PERMIT2_TRANSFER_FROM as u8);
      let input = abi::uniswap::Permit2TransferFrom {
         token: usdc.address,
         recipient: router_addr,
         amount: U160::from(usdc_amount.wei2()),
      };
      inputs.push(input.abi_encode_params().into());

      let fork_db = factory.new_sandbox_fork();
      let mut evm = new_evm(chain_id.into(), block.as_ref(), fork_db);

      simulate::approve_token(
         &mut evm,
         weth.address,
         alice.address,
         permit2_address,
         weth_amount.wei2(),
      ).unwrap();

      simulate::approve_token(
         &mut evm,
         usdc.address,
         alice.address,
         permit2_address,
         usdc_amount.wei2(),
      ).unwrap();

      let execute_call_data = abi::uniswap::universal_router_v2::encode_execute(commands.into(), inputs);

      evm.tx.caller = alice.address;
      evm.tx.data = execute_call_data;
      evm.tx.kind = TxKind::Call(router_addr);
      evm.tx.value = U256::ZERO;

      let res = evm.transact_commit(evm.tx.clone()).unwrap();
      let output = res.output().unwrap();

      if !res.is_success() {
         let err = revert_msg(&output);
         eprintln!("Call Reverted: {}", err);
         eprintln!("Output: {:?}", output);
         eprintln!("Gas Used: {}", res.gas_used());
         panic!("Call Failed");
      }

      eprintln!("Router Call Successful");
      eprintln!("Gas Used: {}", res.gas_used());
   }

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn can_swap_on_multiple_pools() {
      let url = "https://reth-ethereum.ithaca.xyz/rpc".parse().unwrap();
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
      let weth_amount_out = weth_usdc
         .simulate_swap(&usdc, usdc_amount_in.wei2())
         .unwrap();
      let mut weth_amount_out = NumericValue::format_wei(weth_amount_out, weth.decimals());
      weth_amount_out.calc_slippage(0.5, weth.decimals());

      println!(
         "Sell {} {} For {} {}",
         usdc_amount_in.formatted(),
         usdc.symbol(),
         weth_amount_out.formatted(),
         weth.symbol()
      );

      let uni_amount_out = weth_uni
         .simulate_swap(&weth, weth_amount_out.wei2())
         .unwrap();
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
      let signer = SecureSigner::from(alice.key.clone());
      let usdc_balance = NumericValue::parse_to_wei("10000", usdc.decimals());

      let mut factory = ForkFactory::new_sandbox_factory(client.clone(), chain_id, None, None);
      factory.insert_dummy_account(alice.clone());
      factory
         .give_token(alice.address, usdc.address(), usdc_balance.wei2())
         .unwrap();

      let router_addr = utils::address::universal_router_v2(chain_id).unwrap();
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
      let exec_params = encode_swap(
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
         None,
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
      let signer = SecureSigner::from(alice.key.clone());
      let weth_balance = NumericValue::parse_to_wei("10", currency_in.decimals());

      let mut factory = ForkFactory::new_sandbox_factory(client.clone(), chain_id, None, None);
      // give Alice 10 WETH
      factory.insert_dummy_account(alice.clone());
      factory
         .give_token(alice.address, currency_in.address(), weth_balance.wei2())
         .unwrap();

      let router_addr = utils::address::universal_router_v2(chain_id).unwrap();
      let swap_step = SwapStep::new(
         pool,
         amount_in.clone(),
         amount_out_min.clone(),
         currency_in.clone(),
         currency_out.clone(),
      );

      // Build the calldata
      let exec_params = encode_swap(
         client.clone(),
         chain_id,
         vec![swap_step],
         SwapType::ExactInput,
         amount_in.wei2(),
         amount_out_min.wei2(),
         currency_in.clone(),
         currency_out.clone(),
         signer.clone(),
         signer.address(),
         None,
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
      let signer = SecureSigner::from(alice.key.clone());
      let weth_balance = NumericValue::parse_to_wei("10", currency_in.decimals());

      let mut factory = ForkFactory::new_sandbox_factory(client.clone(), chain_id, None, None);
      // give Alice 10 WETH
      factory.insert_dummy_account(alice.clone());
      factory
         .give_token(alice.address, currency_in.address(), weth_balance.wei2())
         .unwrap();

      let router_addr = utils::address::universal_router_v2(chain_id).unwrap();
      let swap_step = SwapStep::new(
         pool,
         amount_in.clone(),
         amount_out_min.clone(),
         currency_in.clone(),
         currency_out.clone(),
      );

      // Build the calldata
      let exec_params = encode_swap(
         client.clone(),
         chain_id,
         vec![swap_step],
         SwapType::ExactInput,
         amount_in.wei2(),
         amount_out_min.wei2(),
         currency_in.clone(),
         currency_out.clone(),
         signer.clone(),
         signer.address(),
         None,
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
      let url = Url::parse("https://reth-ethereum.ithaca.xyz/rpc").unwrap();
      let client = ProviderBuilder::new().connect_http(url);
      let chain_id = 1;

      let weth_balance = NumericValue::parse_to_wei("10", 18);
      let amount_in = NumericValue::parse_to_wei("1", 18);
      let weth = ERC20Token::weth();

      let alice = DummyAccount::new(AccountType::EOA, weth_balance.wei2());
      let signer = SecureSigner::from(alice.key.clone());

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
      let router_addr = utils::address::universal_router_v2(chain_id).unwrap();

      let data = abi::permit::allowance(
         client.clone(),
         permit2_address,
         alice.address,
         weth.address,
         router_addr,
      )
      .await
      .unwrap();

      let value = generate_permit2_single_value(
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

      let signer2 = signer.to_signer();
      let signature = signer2.sign_dynamic_typed_data(&typed_data).await.unwrap();

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
      let url = Url::parse("https://reth-ethereum.ithaca.xyz/rpc").unwrap();
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
      let signer = SecureSigner::from(alice.key.clone());

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

      let router_addr = utils::address::universal_router_v2(1).unwrap();
      let swap_step = SwapStep::new(
         pool,
         amount_in.clone(),
         amount_out_min.clone(),
         currency_in.clone(),
         currency_out.clone(), // ETH as output
      );

      // Build the calldata
      let exec_params = encode_swap(
         client.clone(),
         chain_id,
         vec![swap_step],
         SwapType::ExactInput,
         amount_in.wei2(),
         amount_out_min.wei2(),
         currency_in.clone(),
         currency_out.clone(),
         signer.clone(),
         signer.address(),
         None,
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
      let url = Url::parse("https://reth-ethereum.ithaca.xyz/rpc").unwrap();
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
      let signer = SecureSigner::from(alice.key.clone());

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

      let router_addr = utils::address::universal_router_v2(1).unwrap();
      let swap_step = SwapStep::new(
         pool,
         amount_in.clone(),
         amount_out_min.clone(),
         weth.clone(),
         uni.clone(),
      );

      // Build the calldata
      let exec_params = encode_swap(
         client.clone(),
         chain_id,
         vec![swap_step],
         SwapType::ExactInput,
         amount_in.wei2(),
         amount_out_min.wei2(),
         weth.clone(),
         uni.clone(),
         signer.clone(),
         signer.address(),
         None,
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
      let url = "https://reth-ethereum.ithaca.xyz/rpc".parse().unwrap();
      let client = ProviderBuilder::new().connect_http(url);
      let chain_id = 1;

      let mut pool = UniswapV3Pool::weth_usdc();
      pool.update_state(client.clone(), None).await.unwrap();

      let weth = Currency::from(ERC20Token::weth());
      let usdc = Currency::from(ERC20Token::usdc());

      let amount_in = NumericValue::parse_to_wei("1", weth.decimals());
      let amount_out = pool.simulate_swap(&weth, amount_in.wei2()).unwrap();
      let amount_out = NumericValue::format_wei(amount_out, usdc.decimals());

      println!(
         "Quote {} {} For {} {}",
         amount_in.formatted(),
         weth.symbol(),
         amount_out.formatted(),
         usdc.symbol()
      );

      // Create Alice
      let alice = DummyAccount::new(AccountType::EOA, U256::ZERO);
      let signer = SecureSigner::from(alice.key.clone());

      let mut factory = ForkFactory::new_sandbox_factory(client.clone(), chain_id, None, None);
      factory.insert_dummy_account(alice.clone());
      factory
         .give_token(alice.address, weth.address(), amount_in.wei2())
         .unwrap();

      let router_addr = utils::address::universal_router_v2(chain_id).unwrap();
      let swap_step = SwapStep::new(
         pool,
         amount_in.clone(),
         amount_out.clone(),
         weth.clone(),
         usdc.clone(),
      );

      // Build the calldata
      let exec_params = encode_swap(
         client.clone(),
         chain_id,
         vec![swap_step],
         SwapType::ExactInput,
         amount_in.wei2(),
         amount_out.wei2(),
         weth.clone(),
         usdc.clone(),
         signer.clone(),
         signer.address(),
         None,
      )
      .await
      .unwrap();

      let block = client.get_block(BlockId::latest()).full().await.unwrap();

      // Prepare the fork enviroment
      let fork_db = factory.new_sandbox_fork();
      let mut evm = new_evm(chain_id.into(), block.as_ref(), fork_db);
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

      let usdc_balance = simulate::erc20_balance(&mut evm, usdc.address(), alice.address).unwrap();
      let usdc_balance = NumericValue::format_wei(usdc_balance, usdc.decimals());
      println!("Alice USDC Balance: {}", usdc_balance.formatted());
   }

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn can_swap_from_eth_on_v2_v3() {
      let url = "https://reth-ethereum.ithaca.xyz/rpc".parse().unwrap();
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
      let signer = SecureSigner::from(alice.key.clone());

      let mut factory = ForkFactory::new_sandbox_factory(client.clone(), chain_id, None, None);
      factory.insert_dummy_account(alice.clone());

      let router_addr = utils::address::universal_router_v2(chain_id).unwrap();
      let swap_step = SwapStep::new(
         weth_uni.clone(),
         amount_in.clone(),
         amount_out_min.clone(),
         eth.clone(),
         uni.clone(),
      );

      // Build the calldata
      let exec_params = encode_swap(
         client.clone(),
         chain_id,
         vec![swap_step],
         SwapType::ExactInput,
         amount_in.wei2(),
         amount_out_min.wei2(),
         eth.clone(),
         uni.clone(),
         signer.clone(),
         signer.address(),
         None,
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
      let exec_params = encode_swap(
         client.clone(),
         chain_id,
         vec![swap_step],
         SwapType::ExactInput,
         amount_in.wei2(),
         amount_out_min.wei2(),
         eth.clone(),
         usdc.clone(),
         signer.clone(),
         signer.address(),
         None,
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
   async fn can_swap_on_v4_from_eth_to_erc() {
      let url = "https://reth-ethereum.ithaca.xyz/rpc".parse().unwrap();
      let client = ProviderBuilder::new().connect_http(url);
      let chain_id = 1;

      let mut pool = UniswapV4Pool::eth_uni();
      pool.update_state(client.clone(), None).await.unwrap();

      let eth = pool.base_currency().clone();
      let uni = pool.quote_currency().clone();

      println!("Base Currency: {}", eth.symbol());
      println!("Quote Currency: {}", uni.symbol());

      let amount_in = NumericValue::parse_to_wei("1", eth.decimals());
      let amount_out = pool.simulate_swap(&eth, amount_in.wei2()).unwrap();
      let amount_out = NumericValue::format_wei(amount_out, uni.decimals());

      println!("Amount out: {}", amount_out.formatted());

      let mut amount_out_min = amount_out.clone();
      amount_out_min.calc_slippage(1.0, uni.decimals());
      println!("Amount out with slippage: {}", amount_out_min.formatted());

      println!(
         "Quote {} {} For {} {}",
         amount_in.formatted(),
         eth.symbol(),
         amount_out.formatted(),
         uni.symbol()
      );

      let eth_balance = NumericValue::parse_to_wei("10", eth.decimals());
      // Create Alice
      let alice = DummyAccount::new(AccountType::EOA, eth_balance.wei2());
      let signer = SecureSigner::from(alice.key.clone());

      let mut factory = ForkFactory::new_sandbox_factory(client.clone(), chain_id, None, None);
      factory.insert_dummy_account(alice.clone());

      let router_addr = utils::address::universal_router_v2(chain_id).unwrap();
      let swap_step = SwapStep::new(
         pool,
         amount_in.clone(),
         amount_out_min.clone(),
         eth.clone(),
         uni.clone(),
      );
      // Build the calldata
      let exec_params = encode_swap(
         client.clone(),
         chain_id,
         vec![swap_step],
         SwapType::ExactInput,
         amount_in.wei2(),
         amount_out_min.wei2(),
         eth.clone(),
         uni.clone(),
         signer.clone(),
         signer.address(),
         None,
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

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn can_swap_on_v4_from_erc_to_eth() {
      let url = "https://reth-ethereum.ithaca.xyz/rpc".parse().unwrap();
      let client = ProviderBuilder::new().connect_http(url);
      let chain_id = 1;

      let mut pool = UniswapV4Pool::eth_uni();
      pool.update_state(client.clone(), None).await.unwrap();

      let eth = pool.base_currency().clone();
      let uni = pool.quote_currency().clone();

      println!("Base Currency: {}", eth.symbol());
      println!("Quote Currency: {}", uni.symbol());

      let amount_in = NumericValue::parse_to_wei("500", uni.decimals());
      let amount_out = pool.simulate_swap(&uni, amount_in.wei2()).unwrap();
      let amount_out = NumericValue::format_wei(amount_out, eth.decimals());

      println!("Amount out: {}", amount_out.formatted());

      let mut amount_out_min = amount_out.clone();
      amount_out_min.calc_slippage(1.0, eth.decimals());
      println!("Amount out with slippage: {}", amount_out_min.formatted());

      println!(
         "Quote {} {} For {} {}",
         amount_in.formatted(),
         uni.symbol(),
         amount_out.formatted(),
         eth.symbol()
      );

      // Create Alice
      let alice = DummyAccount::new(AccountType::EOA, U256::ZERO);
      let signer = SecureSigner::from(alice.key.clone());

      let mut factory = ForkFactory::new_sandbox_factory(client.clone(), chain_id, None, None);
      factory.insert_dummy_account(alice.clone());
      factory
         .give_token(alice.address, uni.address(), amount_in.wei2())
         .unwrap();

      let router_addr = utils::address::universal_router_v2(chain_id).unwrap();
      let swap_step = SwapStep::new(
         pool,
         amount_in.clone(),
         amount_out_min.clone(),
         uni.clone(),
         eth.clone(),
      );
      // Build the calldata
      let exec_params = encode_swap(
         client.clone(),
         chain_id,
         vec![swap_step],
         SwapType::ExactInput,
         amount_in.wei2(),
         amount_out_min.wei2(),
         uni.clone(),
         eth.clone(),
         signer.clone(),
         signer.address(),
         None,
      )
      .await
      .unwrap();

      let block = client.get_block(BlockId::latest()).full().await.unwrap();

      // Prepare the fork enviroment
      let fork_db = factory.new_sandbox_fork();
      let mut evm = new_evm(chain_id.into(), block.as_ref(), fork_db);

      let permit2 = permit2_contract(chain_id).unwrap();
      if exec_params.token_needs_approval {
         // Approve the Permit2 contract to spend the tokens
         // it commit changes
         simulate::approve_token(
            &mut evm,
            uni.address(),
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

      let data = evm.balance(alice.address).unwrap();
      let eth_balance = NumericValue::format_wei(data.data, eth.decimals());
      println!("Alice's ETH balance: {}", eth_balance.formatted());
   }

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn can_swap_on_v4_usdc_wbtc() {
      let url = "https://reth-ethereum.ithaca.xyz/rpc".parse().unwrap();
      let client = ProviderBuilder::new().connect_http(url);
      let chain_id = 1;

      let mut pool = UniswapV4Pool::usdc_wbtc();
      pool.update_state(client.clone(), None).await.unwrap();

      let usdc = Currency::from(ERC20Token::usdc());
      let wbtc = address!("0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599");
      let wbtc = ERC20Token {
         chain_id: 1,
         address: wbtc,
         decimals: 8,
         symbol: "WBTC".to_string(),
         name: "Wrapped BTC".to_string(),
         total_supply: U256::ZERO,
      };
      let wbtc = Currency::from(wbtc);

      let amount_in = NumericValue::parse_to_wei("5000", usdc.decimals());
      let amount_out = pool.simulate_swap(&usdc, amount_in.wei2()).unwrap();
      let amount_out = NumericValue::format_wei(amount_out, wbtc.decimals());

      println!(
         "Quote {} {} For {} {}",
         amount_in.formatted(),
         usdc.symbol(),
         amount_out.formatted(),
         wbtc.symbol()
      );

      // Create Alice
      let alice = DummyAccount::new(AccountType::EOA, U256::ZERO);
      let signer = SecureSigner::from(alice.key.clone());

      let mut factory = ForkFactory::new_sandbox_factory(client.clone(), chain_id, None, None);
      factory.insert_dummy_account(alice.clone());
      factory
         .give_token(alice.address, usdc.address(), amount_in.wei2())
         .unwrap();

      let router_addr = utils::address::universal_router_v2(chain_id).unwrap();
      let swap_step = SwapStep::new(
         pool,
         amount_in.clone(),
         amount_out.clone(),
         usdc.clone(),
         wbtc.clone(),
      );
      // Build the calldata
      let exec_params = encode_swap(
         client.clone(),
         chain_id,
         vec![swap_step],
         SwapType::ExactInput,
         amount_in.wei2(),
         amount_out.wei2(),
         usdc.clone(),
         wbtc.clone(),
         signer.clone(),
         signer.address(),
         None,
      )
      .await
      .unwrap();

      let block = client.get_block(BlockId::latest()).full().await.unwrap();

      // Prepare the fork enviroment
      let fork_db = factory.new_sandbox_fork();
      let mut evm = new_evm(chain_id.into(), block.as_ref(), fork_db);

      let permit2 = permit2_contract(chain_id).unwrap();
      if exec_params.token_needs_approval {
         // Approve the Permit2 contract to spend the tokens
         // it commit changes
         simulate::approve_token(
            &mut evm,
            usdc.address(),
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

      let wbtc_balance = simulate::erc20_balance(&mut evm, wbtc.address(), alice.address).unwrap();
      let wbtc_balance = NumericValue::format_wei(wbtc_balance, wbtc.decimals());
      println!("Alice WBTC Balance: {}", wbtc_balance.formatted());
   }

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn can_swap_on_v4_eth_usdt() {
      let url = "https://reth-ethereum.ithaca.xyz/rpc".parse().unwrap();
      let client = ProviderBuilder::new().connect_http(url);
      let chain_id = 1;

      let mut pool = UniswapV4Pool::eth_usdt();
      pool.update_state(client.clone(), None).await.unwrap();

      let usdt = Currency::from(ERC20Token::usdt());
      let eth = Currency::from(NativeCurrency::from(1));

      let currency_in = usdt;
      let currency_out = eth;

      let amount_in = NumericValue::parse_to_wei("3000", currency_in.decimals());
      let amount_out = pool.simulate_swap(&currency_in, amount_in.wei2()).unwrap();
      let amount_out = NumericValue::format_wei(amount_out, currency_out.decimals());

      println!(
         "Quote {} {} For {} {}",
         amount_in.formatted(),
         currency_in.symbol(),
         amount_out.formatted(),
         currency_out.symbol()
      );

      // Create Alice
      let alice = DummyAccount::new(AccountType::EOA, U256::ZERO);
      let signer = SecureSigner::from(alice.key.clone());

      let mut factory = ForkFactory::new_sandbox_factory(client.clone(), chain_id, None, None);
      factory.insert_dummy_account(alice.clone());
      factory
         .give_token(alice.address, currency_in.address(), amount_in.wei2())
         .unwrap();

      let router_addr = utils::address::universal_router_v2(chain_id).unwrap();
      let swap_step = SwapStep::new(
         pool,
         amount_in.clone(),
         amount_out.clone(),
         currency_in.clone(),
         currency_out.clone(),
      );
      // Build the calldata
      let exec_params = encode_swap(
         client.clone(),
         chain_id,
         vec![swap_step],
         SwapType::ExactInput,
         amount_in.wei2(),
         amount_out.wei2(),
         currency_in.clone(),
         currency_out.clone(),
         signer.clone(),
         signer.address(),
         None,
      )
      .await
      .unwrap();

      let block = client.get_block(BlockId::latest()).full().await.unwrap();

      // Prepare the fork enviroment
      let fork_db = factory.new_sandbox_fork();
      let mut evm = new_evm(chain_id.into(), block.as_ref(), fork_db);

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

      let state = evm.balance(alice.address).unwrap();
      let balance = NumericValue::format_wei(state.data, currency_out.decimals());
      println!(
         "Alice {} Balance: {}",
         currency_out.symbol(),
         balance.formatted()
      );
      assert!(balance.wei2() >= amount_out.wei2());
   }

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn can_swap_on_v4_usdc_usdt() {
      let url = "https://reth-ethereum.ithaca.xyz/rpc".parse().unwrap();
      let client = ProviderBuilder::new().connect_http(url);
      let chain_id = 1;

      let mut pool = UniswapV4Pool::usdc_usdt();
      pool.update_state(client.clone(), None).await.unwrap();

      let usdc = Currency::from(ERC20Token::usdc());
      let usdt = Currency::from(ERC20Token::usdt());

      let currency_in = usdt;
      let currency_out = usdc;

      let amount_in = NumericValue::parse_to_wei("10000", currency_in.decimals());
      let amount_out = pool.simulate_swap(&currency_in, amount_in.wei2()).unwrap();
      let amount_out = NumericValue::format_wei(amount_out, currency_out.decimals());

      println!(
         "Quote {} {} For {} {}",
         amount_in.formatted(),
         currency_in.symbol(),
         amount_out.formatted(),
         currency_out.symbol()
      );

      // Create Alice
      let alice = DummyAccount::new(AccountType::EOA, U256::ZERO);
      let signer = SecureSigner::from(alice.key.clone());

      let mut factory = ForkFactory::new_sandbox_factory(client.clone(), chain_id, None, None);
      factory.insert_dummy_account(alice.clone());
      factory
         .give_token(alice.address, currency_in.address(), amount_in.wei2())
         .unwrap();

      let router_addr = utils::address::universal_router_v2(chain_id).unwrap();
      let swap_step = SwapStep::new(
         pool,
         amount_in.clone(),
         amount_out.clone(),
         currency_in.clone(),
         currency_out.clone(),
      );
      // Build the calldata
      let exec_params = encode_swap(
         client.clone(),
         chain_id,
         vec![swap_step],
         SwapType::ExactInput,
         amount_in.wei2(),
         amount_out.wei2(),
         currency_in.clone(),
         currency_out.clone(),
         signer.clone(),
         signer.address(),
         None,
      )
      .await
      .unwrap();

      let block = client.get_block(BlockId::latest()).full().await.unwrap();

      // Prepare the fork enviroment
      let fork_db = factory.new_sandbox_fork();
      let mut evm = new_evm(chain_id.into(), block.as_ref(), fork_db);

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

      let balance = simulate::erc20_balance(&mut evm, currency_out.address(), alice.address).unwrap();
      let balance = NumericValue::format_wei(balance, currency_out.decimals());
      println!(
         "Alice {} Balance: {}",
         currency_out.symbol(),
         balance.formatted()
      );
   }

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn can_swap_on_multiple_pools_with_v4_last_pool() {
      let url = "https://reth-ethereum.ithaca.xyz/rpc".parse().unwrap();
      let client = ProviderBuilder::new().connect_http(url);
      let chain_id = 1;

      // Swap USDC for WETH on the WETH/USDC pool
      // Then Swap WETH for UNI on the WETH/UNI pool
      // Finally Swap UNI for ETH on the UNI/ETH pool
      let mut weth_usdc = UniswapV3Pool::weth_usdc();
      let mut weth_uni = UniswapV2Pool::weth_uni();
      let mut eth_uni = UniswapV4Pool::eth_uni();

      weth_usdc.update_state(client.clone(), None).await.unwrap();
      weth_uni.update_state(client.clone(), None).await.unwrap();
      eth_uni.update_state(client.clone(), None).await.unwrap();

      let usdc = Currency::from(ERC20Token::usdc());
      let weth = Currency::from(ERC20Token::weth());
      let eth = Currency::from(NativeCurrency::from(1));
      let uni = weth_uni.quote_currency().clone();

      let usdc_amount_in = NumericValue::parse_to_wei("2500", usdc.decimals());
      let weth_amount_out = weth_usdc
         .simulate_swap(&usdc, usdc_amount_in.wei2())
         .unwrap();
      let weth_amount_out = NumericValue::format_wei(weth_amount_out, weth.decimals());

      println!(
         "Quote {} {} For {} {}",
         usdc_amount_in.formatted(),
         usdc.symbol(),
         weth_amount_out.formatted(),
         weth.symbol()
      );

      let uni_amount_out = weth_uni
         .simulate_swap(&weth, weth_amount_out.wei2())
         .unwrap();
      let uni_amount_out = NumericValue::format_wei(uni_amount_out, uni.decimals());

      println!(
         "Quote {} {} For {} {}",
         weth_amount_out.formatted(),
         weth.symbol(),
         uni_amount_out.formatted(),
         uni.symbol()
      );

      let eth_amount_out = eth_uni.simulate_swap(&uni, uni_amount_out.wei2()).unwrap();
      let eth_amount_out = NumericValue::format_wei(eth_amount_out, eth.decimals());

      println!(
         "Quote {} {} For {} {}",
         uni_amount_out.formatted(),
         uni.symbol(),
         eth_amount_out.formatted(),
         eth.symbol()
      );

      let alice = DummyAccount::new(AccountType::EOA, U256::ZERO);
      let signer = SecureSigner::from(alice.key.clone());
      let usdc_balance = NumericValue::parse_to_wei("10000", usdc.decimals());

      let mut factory = ForkFactory::new_sandbox_factory(client.clone(), chain_id, None, None);
      factory.insert_dummy_account(alice.clone());
      factory
         .give_token(alice.address, usdc.address(), usdc_balance.wei2())
         .unwrap();

      let router_addr = utils::address::universal_router_v2(chain_id).unwrap();
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

      let swap_step3 = SwapStep::new(
         AnyUniswapPool::from_pool(eth_uni),
         uni_amount_out.clone(),
         eth_amount_out.clone(),
         uni.clone(),
         eth.clone(),
      );

      // Build the calldata
      let exec_params = encode_swap(
         client.clone(),
         chain_id,
         vec![swap_step, swap_step2, swap_step3],
         SwapType::ExactInput,
         usdc_amount_in.wei2(),
         eth_amount_out.wei2(),
         usdc.clone(),
         eth.clone(),
         signer,
         alice.address,
         None,
      )
      .await
      .unwrap();

      let block = client.get_block(BlockId::latest()).full().await.unwrap();

      // Prepare the fork enviroment
      let fork_db = factory.new_sandbox_fork();
      let mut evm = new_evm(chain_id.into(), block.as_ref(), fork_db);

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

      let data = evm.balance(alice.address).unwrap();
      let eth_balance = NumericValue::format_wei(data.data, eth.decimals());
      println!("Alice's ETH balance: {}", eth_balance.formatted());
   }

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn can_swap_on_multiple_pools_with_v4_the_middle_pool() {
      let url = "https://reth-ethereum.ithaca.xyz/rpc".parse().unwrap();
      let client = ProviderBuilder::new().connect_http(url);
      let chain_id = 1;

      // Swap WETH for USDC on the WETH/USDC pool
      // Then Swap USDC for WBTC on the USDC/WBTC pool
      // Finally Swap WBTC for WETH on the WBTC/WETH pool
      let mut weth_usdc = UniswapV3Pool::weth_usdc();
      let mut usdc_wbtc = UniswapV4Pool::usdc_wbtc();
      let mut weth_wbtc = UniswapV2Pool::weth_wbtc();

      weth_usdc.update_state(client.clone(), None).await.unwrap();
      usdc_wbtc.update_state(client.clone(), None).await.unwrap();
      weth_wbtc.update_state(client.clone(), None).await.unwrap();

      let usdc = Currency::from(ERC20Token::usdc());
      let weth = Currency::from(ERC20Token::weth());
      let wbtc = address!("0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599");
      let wbtc = ERC20Token {
         chain_id: 1,
         address: wbtc,
         decimals: 8,
         symbol: "WBTC".to_string(),
         name: "Wrapped BTC".to_string(),
         total_supply: U256::ZERO,
      };
      let wbtc = Currency::from(wbtc);

      let weth_amount_in = NumericValue::parse_to_wei("1", weth.decimals());
      let usdc_amount_out = weth_usdc
         .simulate_swap(&weth, weth_amount_in.wei2())
         .unwrap();
      let usdc_amount_out = NumericValue::format_wei(usdc_amount_out, usdc.decimals());

      println!(
         "Quote {} {} For {} {}",
         weth_amount_in.formatted(),
         weth.symbol(),
         usdc_amount_out.formatted(),
         usdc.symbol()
      );

      let wbtc_amount_out = usdc_wbtc
         .simulate_swap(&usdc, usdc_amount_out.wei2())
         .unwrap();
      let wbtc_amount_out = NumericValue::format_wei(wbtc_amount_out, wbtc.decimals());

      println!(
         "Quote {} {} For {} {}",
         usdc_amount_out.formatted(),
         usdc.symbol(),
         wbtc_amount_out.formatted(),
         wbtc.symbol()
      );

      let weth_amount_out = weth_wbtc
         .simulate_swap(&wbtc, wbtc_amount_out.wei2())
         .unwrap();
      let weth_amount_out = NumericValue::format_wei(weth_amount_out, weth.decimals());

      println!(
         "Quote {} {} For {} {}",
         wbtc_amount_out.formatted(),
         wbtc.symbol(),
         weth_amount_out.formatted(),
         weth.symbol()
      );

      let alice = DummyAccount::new(AccountType::EOA, U256::ZERO);
      let signer = SecureSigner::from(alice.key.clone());
      let weth_balance = NumericValue::parse_to_wei("1", weth.decimals());

      let mut factory = ForkFactory::new_sandbox_factory(client.clone(), chain_id, None, None);
      factory.insert_dummy_account(alice.clone());
      factory
         .give_token(alice.address, weth.address(), weth_balance.wei2())
         .unwrap();

      let router_addr = utils::address::universal_router_v2(chain_id).unwrap();
      let swap_step = SwapStep::new(
         AnyUniswapPool::from_pool(weth_usdc),
         weth_amount_in.clone(),
         usdc_amount_out.clone(),
         weth.clone(),
         usdc.clone(),
      );

      let swap_step2 = SwapStep::new(
         AnyUniswapPool::from_pool(usdc_wbtc),
         usdc_amount_out,
         wbtc_amount_out.clone(),
         usdc.clone(),
         wbtc.clone(),
      );

      let swap_step3 = SwapStep::new(
         AnyUniswapPool::from_pool(weth_wbtc),
         wbtc_amount_out.clone(),
         weth_amount_out.clone(),
         wbtc.clone(),
         weth.clone(),
      );

      // Build the calldata
      let exec_params = encode_swap(
         client.clone(),
         chain_id,
         vec![swap_step, swap_step2, swap_step3],
         SwapType::ExactInput,
         weth_amount_in.wei2(),
         weth_amount_out.wei2(),
         weth.clone(),
         weth.clone(),
         signer,
         alice.address,
         None,
      )
      .await
      .unwrap();

      let block = client.get_block(BlockId::latest()).full().await.unwrap();

      // Prepare the fork enviroment
      let fork_db = factory.new_sandbox_fork();
      let mut evm = new_evm(chain_id.into(), block.as_ref(), fork_db);

      let permit2 = permit2_contract(chain_id).unwrap();

      if exec_params.token_needs_approval {
         // Approve the Permit2 contract to spend the tokens
         // it commit changes
         simulate::approve_token(
            &mut evm,
            weth.address(),
            alice.address,
            permit2,
            weth_amount_in.wei2(),
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

      let weth_balance = simulate::erc20_balance(&mut evm, weth.address(), alice.address).unwrap();
      let weth_balance = NumericValue::format_wei(weth_balance, weth.decimals());
      println!("Alice WETH Balance: {}", weth_balance.formatted());
   }
}
