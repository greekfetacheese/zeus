#[cfg(test)]
mod tests {
   use crate::core::{BaseFee, ZeusCtx};
   use crate::gui::ui::dapps::uniswap::swap::get_relevant_pools;

   use zeus_eth::{
      alloy_primitives::{TxKind, U256},
      alloy_provider::{Provider, ProviderBuilder},
      alloy_rpc_types::{BlockId, BlockNumberOrTag},
      amm::{
         AnyUniswapPool, UniswapPool, UniswapV2Pool, UniswapV3Pool, UniswapV4Pool,
         uniswap::{
            quoter::{get_quote, get_quote_with_split_routing},
            universal_router_v2::{SwapStep, SwapType, encode_swap},
         },
      },
      currency::{Currency, ERC20Token, NativeCurrency},
      revm_utils::*,
      utils::{NumericValue, SecureSigner, address_book, batch},
   };

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn can_get_v3_batch_state() {
      let url = "https://reth-ethereum.ithaca.xyz/rpc".parse().unwrap();
      let client = ProviderBuilder::new().connect_http(url);
      let chain_id = 1;

      let ctx = ZeusCtx::new();

      let pool_manager = ctx.pool_manager();
      let v3_pools = pool_manager.get_v3_pools_for_chain(chain_id);

      if v3_pools.len() < 10 {
         panic!("Cannot continue test, less than 10 v3 pools found");
      }

      let mut pools_to_update = Vec::new();
      for pool in &v3_pools {
         if pools_to_update.len() >= 10 {
            break;
         }
         pools_to_update.push(batch::V3Pool {
            pool: pool.address(),
            token0: pool.currency0().address(),
            token1: pool.currency1().address(),
            tickSpacing: pool.fee().tick_spacing(),
         });
      }

      let res = batch::get_v3_state(client, None, pools_to_update).await.unwrap();

      if res.len() < 10 {
         panic!(
            "Requested the state for 10 pools but got back only for {}",
            res.len()
         );
      }
   }

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn v2_swap_amount_consistency() {
      let url = "https://reth-ethereum.ithaca.xyz/rpc".parse().unwrap();
      let client = ProviderBuilder::new().connect_http(url);
      let chain_id = 1;
      let block = client.get_block(BlockId::latest()).await.unwrap().unwrap();
      let block_id = BlockId::Number(BlockNumberOrTag::Number(block.header.number));

      let mut pool = UniswapV2Pool::weth_uni();
      pool.update_state(client.clone(), Some(block_id)).await.unwrap();

      let weth = pool.base_currency();
      let uni = pool.quote_currency();

      let amount_in = NumericValue::parse_to_wei("50", weth.decimals());
      let amount_out = pool.simulate_swap(&weth, amount_in.wei()).unwrap();
      let amount_out = NumericValue::format_wei(amount_out, uni.decimals());
      let slippage = 0.1;

      let mut min_amount_out = amount_out.clone();
      min_amount_out.calc_slippage(slippage, uni.decimals());

      eprintln!(
         "Quote {} {} For {} {}",
         amount_in.formatted(),
         weth.symbol(),
         amount_out.formatted(),
         uni.symbol()
      );

      let alice = DummyAccount::new(AccountType::EOA, U256::ZERO);
      let signer = SecureSigner::from(alice.key.clone());

      let mut factory =
         ForkFactory::new_sandbox_factory(client.clone(), chain_id, None, Some(block_id));
      factory.insert_dummy_account(alice.clone());
      factory.give_token(alice.address, weth.address(), amount_in.wei()).unwrap();

      let fork_db = factory.new_sandbox_fork();

      let swap_step = SwapStep {
         amount_in: amount_in.clone(),
         amount_out: amount_out.clone(),
         pool: pool.clone(),
         currency_in: weth.clone(),
         currency_out: uni.clone(),
      };

      let swap_steps = vec![swap_step];

      let params = encode_swap(
         client,
         chain_id,
         swap_steps,
         SwapType::ExactInput,
         amount_in.wei(),
         min_amount_out.wei(),
         slippage,
         weth.clone(),
         uni.clone(),
         signer,
         alice.address,
         None,
      )
      .await
      .unwrap();

      let router = address_book::universal_router_v2(chain_id).unwrap();
      let permit2 = address_book::permit2_contract(chain_id).unwrap();

      let mut evm = new_evm(chain_id.into(), Some(&block), fork_db);

      simulate::approve_token(
         &mut evm,
         weth.address(),
         alice.address,
         permit2,
         U256::MAX,
      )
      .unwrap();

      evm.tx.caller = alice.address;
      evm.tx.data = params.call_data.clone();
      evm.tx.value = params.value.clone();
      evm.tx.kind = TxKind::Call(router);

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

      let balance = simulate::erc20_balance(&mut evm, uni.address(), alice.address).unwrap();
      assert!(balance >= min_amount_out.wei());
      let balance = NumericValue::format_wei(balance, uni.decimals());

      eprintln!(
         "{} Quote Amount: {}",
         uni.symbol(),
         amount_out.format_abbreviated()
      );

      eprintln!(
         "{} Got from Swap: {}",
         uni.symbol(),
         balance.format_abbreviated()
      );

      eprintln!(
         "{} Quote Amount: {}",
         uni.symbol(),
         amount_out.wei()
      );

      eprintln!(
         "{} Got from Swap: {}",
         uni.symbol(),
         balance.wei()
      );
   }

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn v3_swap_amount_consistency() {
      let url = "https://reth-ethereum.ithaca.xyz/rpc".parse().unwrap();
      let client = ProviderBuilder::new().connect_http(url);
      let chain_id = 1;
      let block = client.get_block(BlockId::latest()).await.unwrap().unwrap();
      let block_id = BlockId::Number(BlockNumberOrTag::Number(block.header.number));

      let mut pool = UniswapV3Pool::usdt_uni();
      pool.update_state(client.clone(), Some(block_id)).await.unwrap();

      let usdt = pool.base_currency();
      let uni = pool.quote_currency();

      let amount_in = NumericValue::parse_to_wei("100000", usdt.decimals());
      let amount_out = pool.simulate_swap(&usdt, amount_in.wei()).unwrap();
      let amount_out = NumericValue::format_wei(amount_out, uni.decimals());
      let slippage = 0.1;

      let mut min_amount_out = amount_out.clone();
      min_amount_out.calc_slippage(slippage, uni.decimals());

      eprintln!(
         "Quote {} {} For {} {}",
         amount_in.formatted(),
         usdt.symbol(),
         amount_out.formatted(),
         uni.symbol()
      );

      let alice = DummyAccount::new(AccountType::EOA, U256::ZERO);
      let signer = SecureSigner::from(alice.key.clone());

      let mut factory =
         ForkFactory::new_sandbox_factory(client.clone(), chain_id, None, Some(block_id));
      factory.insert_dummy_account(alice.clone());
      factory.give_token(alice.address, usdt.address(), amount_in.wei()).unwrap();

      let fork_db = factory.new_sandbox_fork();

      let swap_step = SwapStep {
         amount_in: amount_in.clone(),
         amount_out: amount_out.clone(),
         pool: pool.clone(),
         currency_in: usdt.clone(),
         currency_out: uni.clone(),
      };

      let swap_steps = vec![swap_step];

      let params = encode_swap(
         client,
         chain_id,
         swap_steps,
         SwapType::ExactInput,
         amount_in.wei(),
         min_amount_out.wei(),
         slippage,
         usdt.clone(),
         uni.clone(),
         signer,
         alice.address,
         None,
      )
      .await
      .unwrap();

      let router = address_book::universal_router_v2(chain_id).unwrap();
      let permit2 = address_book::permit2_contract(chain_id).unwrap();

      let mut evm = new_evm(chain_id.into(), Some(&block), fork_db);

      simulate::approve_token(
         &mut evm,
         usdt.address(),
         alice.address,
         permit2,
         U256::MAX,
      )
      .unwrap();

      evm.tx.caller = alice.address;
      evm.tx.data = params.call_data.clone();
      evm.tx.value = params.value.clone();
      evm.tx.kind = TxKind::Call(router);

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

      let balance = simulate::erc20_balance(&mut evm, uni.address(), alice.address).unwrap();
      // assert!(balance >= min_amount_out.wei());
      let balance = NumericValue::format_wei(balance, uni.decimals());

      // assert_eq!(balance.wei(), amount_out.wei());

      eprintln!(
         "{} Quote Amount: {}",
         uni.symbol(),
         amount_out.format_abbreviated()
      );

      eprintln!(
         "{} Got from Swap: {}",
         uni.symbol(),
         balance.format_abbreviated()
      );

      eprintln!(
         "{} Quote Amount: {}",
         uni.symbol(),
         amount_out.wei()
      );

      eprintln!(
         "{} Got from Swap: {}",
         uni.symbol(),
         balance.wei()
      );
   }

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn v4_swap_amount_consistency() {
      let url = "https://reth-ethereum.ithaca.xyz/rpc".parse().unwrap();
      let client = ProviderBuilder::new().connect_http(url);
      let chain_id = 1;
      let block = client.get_block(BlockId::latest()).await.unwrap().unwrap();
      let block_id = BlockId::Number(BlockNumberOrTag::Number(block.header.number));

      let mut pool = UniswapV4Pool::eth_uni();
      pool.update_state(client.clone(), Some(block_id)).await.unwrap();

      let eth = pool.base_currency();
      let uni = pool.quote_currency();

      let amount_in = NumericValue::parse_to_wei("10", eth.decimals());
      let amount_out = pool.simulate_swap(&eth, amount_in.wei()).unwrap();
      let amount_out = NumericValue::format_wei(amount_out, uni.decimals());
      let slippage = 0.1;

      let mut min_amount_out = amount_out.clone();
      min_amount_out.calc_slippage(slippage, uni.decimals());

      eprintln!(
         "Quote {} {} For {} {}",
         amount_in.formatted(),
         eth.symbol(),
         amount_out.formatted(),
         uni.symbol()
      );

      let alice = DummyAccount::new(AccountType::EOA, amount_in.wei());
      let signer = SecureSigner::from(alice.key.clone());

      let mut factory =
         ForkFactory::new_sandbox_factory(client.clone(), chain_id, None, Some(block_id));
      factory.insert_dummy_account(alice.clone());

      let fork_db = factory.new_sandbox_fork();

      let swap_step = SwapStep {
         amount_in: amount_in.clone(),
         amount_out: amount_out.clone(),
         pool: pool.clone(),
         currency_in: eth.clone(),
         currency_out: uni.clone(),
      };

      let swap_steps = vec![swap_step];

      let params = encode_swap(
         client,
         chain_id,
         swap_steps,
         SwapType::ExactInput,
         amount_in.wei(),
         min_amount_out.wei(),
         slippage,
         eth.clone(),
         uni.clone(),
         signer,
         alice.address,
         None,
      )
      .await
      .unwrap();

      let router = address_book::universal_router_v2(chain_id).unwrap();

      let mut evm = new_evm(chain_id.into(), Some(&block), fork_db);

      evm.tx.caller = alice.address;
      evm.tx.data = params.call_data.clone();
      evm.tx.value = params.value.clone();
      evm.tx.kind = TxKind::Call(router);

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

      let balance = simulate::erc20_balance(&mut evm, uni.address(), alice.address).unwrap();
      assert!(balance >= min_amount_out.wei());
      let balance = NumericValue::format_wei(balance, uni.decimals());

      // assert_eq!(balance.wei(), amount_out.wei());

      eprintln!(
         "{} Quote Amount: {}",
         uni.symbol(),
         amount_out.format_abbreviated()
      );

      eprintln!(
         "{} Got from Swap: {}",
         uni.symbol(),
         balance.format_abbreviated()
      );

      eprintln!(
         "{} Quote Amount: {}",
         uni.symbol(),
         amount_out.wei()
      );

      eprintln!(
         "{} Got from Swap: {}",
         uni.symbol(),
         balance.wei()
      );
   }

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn single_v4_swap_eth_to_erc20_mainnet() {
      let chain_id = 1;

      let pool: AnyUniswapPool = UniswapV4Pool::eth_uni().into();
      let currency_in = Currency::from(NativeCurrency::from(chain_id));
      let currency_out = pool.quote_currency().clone();
      let amount_in = NumericValue::parse_to_wei("10", currency_in.decimals());

      let swap_on_v2 = true;
      let swap_on_v3 = true;
      let swap_on_v4 = true;
      let max_hops = 2;
      let max_routes = 1;
      let with_split_routing = true;

      test_swap(
         chain_id,
         amount_in,
         currency_in,
         currency_out,
         swap_on_v2,
         swap_on_v3,
         swap_on_v4,
         max_hops,
         max_routes,
         with_split_routing,
         vec![pool],
      )
      .await
      .unwrap();
   }

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn single_v4_swap_erc20_to_eth_mainnet() {
      let chain_id = 1;

      let pool: AnyUniswapPool = UniswapV4Pool::eth_uni().into();
      let currency_in = pool.quote_currency().clone();
      let currency_out = Currency::from(NativeCurrency::from(chain_id));
      let amount_in = NumericValue::parse_to_wei("1000", currency_in.decimals());

      let swap_on_v2 = true;
      let swap_on_v3 = true;
      let swap_on_v4 = true;
      let max_hops = 2;
      let max_routes = 1;
      let with_split_routing = true;

      test_swap(
         chain_id,
         amount_in,
         currency_in,
         currency_out,
         swap_on_v2,
         swap_on_v3,
         swap_on_v4,
         max_hops,
         max_routes,
         with_split_routing,
         vec![pool],
      )
      .await
      .unwrap();
   }

   /* 
   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn single_v4_swap_erc20_to_erc20_mainnet() {
      let chain_id = 1;

      let pool: AnyUniswapPool = UniswapV4Pool::usdc_usdt().into();
      let currency_in = Currency::from(ERC20Token::usdc());
      let currency_out = Currency::from(ERC20Token::usdt());
      let amount_in = NumericValue::parse_to_wei("10000", currency_in.decimals());

      let swap_on_v2 = true;
      let swap_on_v3 = true;
      let swap_on_v4 = true;
      let max_hops = 2;
      let max_routes = 1;
      let with_split_routing = true;

      test_swap(
         chain_id,
         amount_in,
         currency_in,
         currency_out,
         swap_on_v2,
         swap_on_v3,
         swap_on_v4,
         max_hops,
         max_routes,
         with_split_routing,
         vec![pool],
      )
      .await
      .unwrap();
   }
   */

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn swap_from_eth_to_erc20_mainnet_with_split_routing_and_v4_enabled() {
      let chain_id = 1;

      let currency_in = Currency::from(NativeCurrency::from(chain_id));
      let currency_out = Currency::from(ERC20Token::usdt());
      let amount_in = NumericValue::parse_to_wei("300", currency_in.decimals());

      let swap_on_v2 = true;
      let swap_on_v3 = true;
      let swap_on_v4 = true;
      let max_hops = 6;
      let max_routes = 5;
      let with_split_routing = true;

      test_swap(
         chain_id,
         amount_in,
         currency_in,
         currency_out,
         swap_on_v2,
         swap_on_v3,
         swap_on_v4,
         max_hops,
         max_routes,
         with_split_routing,
         Vec::new(),
      )
      .await
      .unwrap();
   }

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn swap_from_erc20_to_eth_mainnet_with_split_routing_and_v4_enabled() {
      let chain_id = 1;

      let currency_in = Currency::from(ERC20Token::usdt());
      let currency_out = Currency::from(NativeCurrency::from(chain_id));
      let amount_in = NumericValue::parse_to_wei("300000", currency_in.decimals());

      let swap_on_v2 = true;
      let swap_on_v3 = true;
      let swap_on_v4 = true;
      let max_hops = 6;
      let max_routes = 5;
      let with_split_routing = true;

      test_swap(
         chain_id,
         amount_in,
         currency_in,
         currency_out,
         swap_on_v2,
         swap_on_v3,
         swap_on_v4,
         max_hops,
         max_routes,
         with_split_routing,
         Vec::new(),
      )
      .await
      .unwrap();
   }

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn swap_from_eth_to_erc20_mainnet() {
      let chain_id = 1;

      let currency_in = Currency::from(NativeCurrency::from(chain_id));
      let currency_out = Currency::from(ERC20Token::usdt());
      let amount_in = NumericValue::parse_to_wei("10", currency_in.decimals());

      let swap_on_v2 = true;
      let swap_on_v3 = true;
      let swap_on_v4 = false;
      let max_hops = 4;
      let max_routes = 10;
      let with_split_routing = false;

      test_swap(
         chain_id,
         amount_in,
         currency_in,
         currency_out,
         swap_on_v2,
         swap_on_v3,
         swap_on_v4,
         max_hops,
         max_routes,
         with_split_routing,
         Vec::new(),
      )
      .await
      .unwrap();
   }

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn swap_from_erc20_to_erc20_mainnet() {
      let chain_id = 1;

      let currency_in = Currency::from(ERC20Token::link());
      let currency_out = Currency::from(ERC20Token::dai());
      let amount_in = NumericValue::parse_to_wei("200", currency_in.decimals());

      let swap_on_v2 = true;
      let swap_on_v3 = true;
      let swap_on_v4 = false;
      let max_hops = 4;
      let max_routes = 10;
      let with_split_routing = false;

      test_swap(
         chain_id,
         amount_in,
         currency_in,
         currency_out,
         swap_on_v2,
         swap_on_v3,
         swap_on_v4,
         max_hops,
         max_routes,
         with_split_routing,
         Vec::new(),
      )
      .await
      .unwrap();
   }

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn swap_from_erc20_to_eth_mainnet() {
      let chain_id = 1;

      let currency_in = Currency::from(ERC20Token::usdc());
      let currency_out = Currency::from(NativeCurrency::from(chain_id));
      let amount_in = NumericValue::parse_to_wei("100000", currency_in.decimals());

      let swap_on_v2 = true;
      let swap_on_v3 = true;
      let swap_on_v4 = false;
      let max_hops = 4;
      let max_routes = 10;
      let with_split_routing = false;

      test_swap(
         chain_id,
         amount_in,
         currency_in,
         currency_out,
         swap_on_v2,
         swap_on_v3,
         swap_on_v4,
         max_hops,
         max_routes,
         with_split_routing,
         Vec::new(),
      )
      .await
      .unwrap();
   }

   #[test]
   fn test_relevant_pools_usdc_to_eth() {
      let chain = 1;
      let ctx = ZeusCtx::new();
      let currency_in = Currency::from(ERC20Token::usdc());
      let currency_out = Currency::from(NativeCurrency::from(chain));

      let pools = get_relevant_pools(ctx, true, true, true, &currency_in, &currency_out);

      eprintln!("========== Relevant Pools ==========");
      for pool in &pools {
         eprintln!(
            "Pool {} / {} - {} ({}%)",
            pool.currency0().symbol(),
            pool.currency1().symbol(),
            pool.dex_kind().as_str(),
            pool.fee().fee_percent()
         );
      }
   }

   #[test]
   fn test_relevant_pools_link_to_eth() {
      let chain = 1;
      let ctx = ZeusCtx::new();
      let currency_in = Currency::from(ERC20Token::link());
      let currency_out = Currency::from(NativeCurrency::from(chain));

      let pools = get_relevant_pools(ctx, true, true, true, &currency_in, &currency_out);

      eprintln!("========== Relevant Pools ==========");
      for pool in &pools {
         eprintln!(
            "Pool {} / {} - {} ({}%)",
            pool.currency0().symbol(),
            pool.currency1().symbol(),
            pool.dex_kind().as_str(),
            pool.fee().fee_percent()
         );
      }
   }

   #[test]
   fn test_relevant_pools_eth_to_link() {
      let chain = 1;
      let ctx = ZeusCtx::new();
      let currency_in = Currency::from(NativeCurrency::from(chain));
      let currency_out = Currency::from(ERC20Token::link());

      let pools = get_relevant_pools(ctx, true, true, true, &currency_in, &currency_out);

      eprintln!("========== Relevant Pools ==========");
      for pool in &pools {
         eprintln!(
            "Pool {} / {} - {} ({}%)",
            pool.currency0().symbol(),
            pool.currency1().symbol(),
            pool.dex_kind().as_str(),
            pool.fee().fee_percent()
         );
      }
   }

   async fn test_swap(
      chain: u64,
      amount_in: NumericValue,
      currency_in: Currency,
      currency_out: Currency,
      swap_on_v2: bool,
      swap_on_v3: bool,
      swap_on_v4: bool,
      max_hops: usize,
      max_routes: usize,
      with_split_routing: bool,
      given_pools: Vec<AnyUniswapPool>,
   ) -> Result<(), anyhow::Error> {
      let ctx = ZeusCtx::new();
      ctx.write(|ctx| ctx.providers.all_working());

      let pools = if given_pools.is_empty() {
         let relevant_pools = get_relevant_pools(
            ctx.clone(),
            swap_on_v2,
            swap_on_v3,
            swap_on_v4,
            &currency_in,
            &currency_out,
         );
         relevant_pools
      } else {
         given_pools
      };

      let pool_manager = ctx.pool_manager();
      pool_manager.update_state_for_pools(ctx.clone(), chain, pools).await?;

      let pools = get_relevant_pools(
         ctx.clone(),
         swap_on_v2,
         swap_on_v3,
         swap_on_v4,
         &currency_in,
         &currency_out,
      );

      let eth = Currency::from(NativeCurrency::from(chain));
      let eth_price = ctx.get_currency_price(&eth);
      let currency_out_price = ctx.get_currency_price(&currency_out);
      let base_fee = BaseFee::default();
      let priority_fee = NumericValue::parse_to_gwei("1");

      let quote = if with_split_routing {
         get_quote_with_split_routing(
            amount_in.clone(),
            currency_in.clone(),
            currency_out.clone(),
            pools,
            eth_price.clone(),
            currency_out_price.clone(),
            base_fee.next,
            priority_fee.wei(),
            max_hops,
            max_routes,
         )
      } else {
         get_quote(
            amount_in.clone(),
            currency_in.clone(),
            currency_out.clone(),
            pools,
            eth_price.clone(),
            currency_out_price.clone(),
            base_fee.next,
            priority_fee.wei(),
            max_hops,
         )
      };

      let slippage = 0.5;
      let swap_steps = quote.swap_steps;
      let amount_out = quote.amount_out;
      let mut min_amount_out = amount_out.clone();
      min_amount_out.calc_slippage(slippage, currency_out.decimals());

      eprintln!(
         "Quote {} {} For {} {}",
         amount_in.format_abbreviated(),
         currency_in.symbol(),
         currency_out.symbol(),
         amount_out.format_abbreviated()
      );
      eprintln!("Swap Steps Length: {}", swap_steps.len());

      for swap in &swap_steps {
         eprintln!(
            "Swap Step: {} (Wei: {}) {} -> {} (Wei: {}) {} {} ({})",
            swap.amount_in.format_abbreviated(),
            swap.amount_in.wei(),
            swap.currency_in.symbol(),
            swap.amount_out.format_abbreviated(),
            swap.amount_out.wei(),
            swap.currency_out.symbol(),
            swap.pool.dex_kind().as_str(),
            swap.pool.fee().fee()
         );
      }

      let client = ctx.get_client(chain).await?;

      let eth_balance = if currency_in.is_native() {
         amount_in.wei()
      } else {
         U256::ZERO
      };

      let alice = DummyAccount::new(AccountType::EOA, eth_balance);
      let signer = SecureSigner::from(alice.key.clone());

      let swap_params = encode_swap(
         client.clone(),
         chain,
         swap_steps,
         SwapType::ExactInput,
         amount_in.wei(),
         min_amount_out.wei(),
         slippage,
         currency_in.clone(),
         currency_out.clone(),
         signer.clone(),
         alice.address,
         None,
      )
      .await?;

      let block = client.get_block(BlockId::latest()).await.unwrap();
      let mut factory = ForkFactory::new_sandbox_factory(client.clone(), chain, None, None);
      factory.insert_dummy_account(alice.clone());

      if currency_in.is_erc20() {
         factory.give_token(
            alice.address,
            currency_in.address(),
            amount_in.wei(),
         )?;
      }

      let fork_db = factory.new_sandbox_fork();
      let router_addr = address_book::universal_router_v2(chain).unwrap();
      let permit2 = address_book::permit2_contract(chain).unwrap();

      let mut evm = new_evm(chain.into(), block.as_ref(), fork_db);

      if swap_params.token_needs_approval {
         simulate::approve_token(
            &mut evm,
            currency_in.address(),
            alice.address,
            permit2,
            U256::MAX,
         )
         .unwrap();
      }

      evm.tx.caller = alice.address;
      evm.tx.data = swap_params.call_data.clone();
      evm.tx.value = swap_params.value.clone();
      evm.tx.kind = TxKind::Call(router_addr);

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

      let currency_out_balance = if currency_out.is_erc20() {
         simulate::erc20_balance(&mut evm, currency_out.address(), alice.address).unwrap()
      } else {
         let state = evm.balance(alice.address).unwrap();
         state.data
      };

      assert!(currency_out_balance >= min_amount_out.wei());
      let balance = NumericValue::format_wei(currency_out_balance, currency_out.decimals());

      eprintln!(
         "{} Quote Amount: {}",
         currency_out.symbol(),
         amount_out.format_abbreviated()
      );

      eprintln!(
         "{} Got from Swap: {}",
         currency_out.symbol(),
         balance.format_abbreviated()
      );

      Ok(())
   }
}
