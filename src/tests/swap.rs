#[cfg(test)]
mod tests {
   use crate::core::{BaseFee, ZeusCtx};
   use crate::gui::ui::dapps::uniswap::swap::get_relevant_pools;

   use crate::utils::{swap_quoter::*, universal_router_v2::*};

   use zeus_eth::{
      alloy_primitives::{TxKind, U256},
      alloy_provider::Provider,
      alloy_rpc_types::BlockId,
      amm::uniswap::{AnyUniswapPool, UniswapPool, UniswapV4Pool},
      currency::{Currency, ERC20Token, NativeCurrency},
      revm_utils::*,
      utils::{NumericValue, SecureSigner, address_book},
   };

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
      let amount_in = NumericValue::parse_to_wei("500000", currency_in.decimals());

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
   async fn swap_from_eth_to_erc20_base_chain() {
      let chain_id = 8453;

      let currency_in = Currency::from(NativeCurrency::from(chain_id));
      let currency_out = Currency::from(ERC20Token::usdc_base());
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

   /*
   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn swap_from_eth_to_erc20_base_chain_aerodrome() {
      let chain_id = 8453;

      let currency_in = Currency::from(NativeCurrency::from(chain_id));
      let currency_out = Currency::from(ERC20Token::usdc_base());
      let amount_in = NumericValue::parse_to_wei("10", currency_in.decimals());

      let pool: AnyUniswapPool = UniswapV3Pool {
         chain_id: chain_id,
         address: address!("0xb2cc224c1c9feE385f8ad6a55b4d94E92359DC59"),
         fee: FeeAmount::CUSTOM(425),
         currency0: ERC20Token::wrapped_native_token(chain_id).into(),
         currency1: currency_out.clone(),
         dex: DexKind::UniswapV3,
         state: State::default(),
         liquidity_amount0: U256::ZERO,
         liquidity_amount1: U256::ZERO,
      }.into();


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
         vec![pool],
      )
      .await
      .unwrap();
   }
   */

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn swap_from_erc20_to_eth_base_chain() {
      let chain_id = 8453;

      let currency_in = Currency::from(ERC20Token::usdc_base());
      let currency_out = Currency::from(NativeCurrency::from(chain_id));
      let amount_in = NumericValue::parse_to_wei("1000", currency_in.decimals());

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
   async fn swap_from_erc20_to_eth_optimism_chain() {
      let chain_id = 10;

      let currency_in = Currency::from(ERC20Token::usdc_optimism());
      let currency_out = Currency::from(NativeCurrency::from(chain_id));
      let amount_in = NumericValue::parse_to_wei("1000", currency_in.decimals());

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
   async fn swap_from_erc20_to_eth_arbitrum_chain() {
      let chain_id = 42161;

      let currency_in = Currency::from(ERC20Token::usdc_arbitrum());
      let currency_out = Currency::from(NativeCurrency::from(chain_id));
      let amount_in = NumericValue::parse_to_wei("1000", currency_in.decimals());

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
   async fn swap_from_eth_to_erc20_optimism_chain() {
      let chain_id = 10;

      let currency_in = Currency::from(NativeCurrency::from(chain_id));
      let currency_out = Currency::from(ERC20Token::usdt_optimism());
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
   async fn swap_from_eth_to_erc20_arbitrum() {
      let chain_id = 42161;

      let currency_in = Currency::from(NativeCurrency::from(chain_id));
      let currency_out = Currency::from(ERC20Token::usdt_arbitrum());
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
      let updated_pools = pool_manager.update_state_for_pools(ctx.clone(), chain, pools).await?;

      let mut liquid_pools = Vec::new();
      for pool in updated_pools.iter() {
         let has_liquidity = ctx.pool_has_sufficient_liquidity(pool).unwrap_or(false);

         if has_liquidity {
            liquid_pools.push(pool.clone());
         }
      }

      let eth = Currency::from(NativeCurrency::from(chain));
      let eth_price = ctx.get_currency_price(&eth);
      let currency_out_price = ctx.get_currency_price(&currency_out);
      let base_fee = BaseFee::default();
      let priority_fee = NumericValue::parse_to_gwei("1");

      let quote = if with_split_routing {
         get_quote_with_split_routing(
            ctx.clone(),
            amount_in.clone(),
            currency_in.clone(),
            currency_out.clone(),
            liquid_pools,
            eth_price.clone(),
            currency_out_price.clone(),
            base_fee.next,
            priority_fee.wei(),
            max_hops,
            max_routes,
         )
      } else {
         get_quote(
            ctx.clone(),
            amount_in.clone(),
            currency_in.clone(),
            currency_out.clone(),
            liquid_pools,
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
      let min_amount_out = amount_out.calc_slippage(slippage, currency_out.decimals());

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
         ctx.clone(),
         None,
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

      if swap_params.permit2_needs_approval() {
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
