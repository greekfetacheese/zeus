#[cfg(test)]
mod tests {
   use crate::core::client::Rpc;
   use crate::core::{ZeusCtx, types::BaseFee};
   use crate::gui::ui::dapps::uniswap::swap::get_relevant_pools;

   use crate::utils::{swap_quoter::*, universal_router_v2::*};
   use zeus_wallet::SecureKey;

   use zeus_eth::{
      alloy_primitives::{Address, TxKind, U256},
      alloy_provider::Provider,
      alloy_rpc_types::BlockId,
      amm::uniswap::{
         AnyUniswapPool, DexKind, FeeAmount, UniswapPool, UniswapV2Pool, UniswapV3Pool,
         UniswapV4Pool,
      },
      currency::{Currency, ERC20Token, NativeCurrency},
      revm_utils::*,
      utils::{NumericValue, address_book, client::RpcClient, price_feed::get_eth_price},
   };

   use crate::tests::unlock_ctx;

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn swap_from_weth_to_usdc_mainnet() {
      let chain_id = 1;

      let currency_in = Currency::wrapped_native(chain_id);
      let currency_out = Currency::from(ERC20Token::usdc());
      let amount_in = NumericValue::parse_to_wei("1", currency_in.decimals());

      let swap_on_v2 = true;
      let swap_on_v3 = true;
      let swap_on_v4 = true;
      let max_hops = 2;
      let max_routes = 1;
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
   async fn single_v4_swap_usdc_to_weth_mainnet() {
      let chain_id = 1;

      let pool: AnyUniswapPool = UniswapV4Pool::eth_usdc().into();
      let currency_in = Currency::from(ERC20Token::usdc());
      let currency_out = Currency::wrapped_native(chain_id);
      let amount_in = NumericValue::parse_to_wei("2500", currency_in.decimals());

      let swap_on_v2 = true;
      let swap_on_v3 = true;
      let swap_on_v4 = true;
      let max_hops = 2;
      let max_routes = 1;
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
   async fn swap_from_eth_to_usdc_mainnet_with_split_routing_and_v4_enabled() {
      let chain_id = 1;

      let currency_in = Currency::from(NativeCurrency::from(chain_id));
      let currency_out = Currency::from(ERC20Token::usdc());
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
      let ctx = unlock_ctx();
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

   // ---- Robinhood Chain (4663) swap regressions -------------------------------------------
   //
   // The router deployed there is the 2025+ Universal Router generation, so both swap shapes
   // differ from the older chains: V3/V2 inputs carry a trailing `minHopPriceX36` array, the V4
   // swap struct has a `minHopPriceX36` word before `hookData`, and an ETH trade routed through
   // a V4 *WETH* pool has to wrap the input first (its SETTLE pulls WETH off the router).

   const ROBINHOOD: u64 = 4663;
   const ROBINHOOD_RPC: &str = "https://rpc.mainnet.chain.robinhood.com/";

   /// `unlock_ctx` has no measured endpoint for a chain the app never ran against.
   fn robinhood_ctx() -> ZeusCtx {
      let ctx = unlock_ctx();
      let mut rpc = Rpc::builder(ROBINHOOD_RPC, ROBINHOOD).builtin().enabled().build();
      rpc.check.working = true;
      ctx.get_zeus_client().add_rpc(ROBINHOOD, rpc);
      ctx
   }

   /// Quote, encode and execute `amount` ETH -> USDG on a sandbox fork, returning the USDG the
   /// dummy account actually received.
   async fn robinhood_eth_to_usdg(
      ctx: ZeusCtx,
      amount: NumericValue,
      pools: Vec<AnyUniswapPool>,
   ) -> U256 {
      let chain = ROBINHOOD;
      let client = ctx.get_client(chain).await.unwrap();

      let currency_in = Currency::from(NativeCurrency::from(chain));
      let currency_out = Currency::from(ERC20Token::usdg_robinhood());

      let pools = ctx
         .pool_manager()
         .update_state_for_pools(ctx.clone(), chain, pools)
         .await
         .unwrap();

      let eth_price = get_eth_price(client.clone(), chain, None).await.unwrap_or(3000.0);
      let eth_price = if eth_price > 0.0 { eth_price } else { 3000.0 };

      let base_fee = BaseFee::default();
      let priority_fee = NumericValue::parse_to_gwei("1");
      let quote = get_quote_with_split_routing(
         amount.clone(),
         currency_in.clone(),
         currency_out.clone(),
         pools,
         NumericValue::from_f64(eth_price),
         NumericValue::from_f64(1.0),
         base_fee.next,
         priority_fee.wei(),
         2,
         3,
      );

      assert!(
         !quote.swap_steps.is_empty(),
         "no ETH -> USDG route from the given pools"
      );

      let slippage = 0.5;
      let min_amount_out = quote.amount_out.calc_slippage(slippage, currency_out.decimals());
      let alice = DummyAccount::new(AccountType::EOA, amount.wei());

      let swap_params = encode_swap(
         ctx.clone(),
         SwapRequest {
            chain_id: chain,
            currency_in: currency_in.clone(),
            currency_out: currency_out.clone(),
            amount_in: amount.wei(),
            amount_out_min: min_amount_out.wei(),
            slippage,
            swap_type: SwapType::ExactInput,
            steps: quote.swap_steps,
            signer: SecureKey::from(alice.key.clone()),
            recipient: alice.address,
            deadline_minutes: 5,
            permit2_info: None,
         },
      )
      .await
      .unwrap();

      let block = client.get_block(BlockId::latest()).await.unwrap();
      let mut factory = ForkFactory::new_sandbox_factory(client.clone(), chain, None, None);
      factory.insert_dummy_account(alice.clone());
      let fork_db = factory.new_sandbox_fork();

      let router = address_book::universal_router_v2(chain).unwrap();
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

      evm.tx.chain_id = Some(chain);
      evm.tx.caller = alice.address;
      evm.tx.data = swap_params.call_data.clone();
      evm.tx.value = swap_params.value;
      evm.tx.kind = TxKind::Call(router);

      let res = evm.transact_commit(evm.tx.clone()).unwrap();
      let output = res.output().unwrap_or_default();
      assert!(
         res.is_success(),
         "simulation failed: {} ({} gas)",
         revert_msg(&output),
         res.tx_gas_used()
      );

      eprintln!(
         "{} -> {} USDG in {} gas",
         amount.abbreviated(),
         NumericValue::format_wei(
            simulate::erc20_balance(&mut evm, currency_out.address(), alice.address).unwrap(),
            currency_out.decimals()
         )
         .abbreviated(),
         res.tx_gas_used()
      );

      simulate::erc20_balance(&mut evm, currency_out.address(), alice.address).unwrap()
   }

   /// Every V3 fee tier that has liquidity for WETH/USDG, plus the V2 pair.
   async fn robinhood_v2_v3_pools(client: RpcClient) -> Vec<AnyUniswapPool> {
      let mut pools: Vec<AnyUniswapPool> = Vec::new();

      for addr in [
         "0x52e65B17fB6E5BA00Ed806f37Afcd2DaA50271Ca", // V3 fee 100
         "0x69BfaF19C9f377BB306a89aEd9F6B07e2c1a8d9a", // V3 fee 500
         "0xa9188730Fe85Be88ad499D7d52B099e800fB0334", // V3 fee 3000
         "0x5f009E071F07e92B6C624e83F52F17bBDa34680D", // V3 fee 10000
      ] {
         let pool = UniswapV3Pool::from_address(client.clone(), ROBINHOOD, addr.parse().unwrap())
            .await
            .unwrap();
         pools.push(pool.into());
      }

      let v2 = UniswapV2Pool::from_address(
         client,
         ROBINHOOD,
         "0x8803c117ccae7B5146297876c2A25DF135141C4d".parse().unwrap(),
      )
      .await
      .unwrap();
      pools.push(v2.into());

      pools
   }

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn robinhood_eth_to_usdg_v3_route() {
      let ctx = robinhood_ctx();
      let client = ctx.get_client(ROBINHOOD).await.unwrap();
      let pools = robinhood_v2_v3_pools(client).await;

      let received = robinhood_eth_to_usdg(ctx, NumericValue::parse_to_wei("1", 18), pools).await;

      assert!(received > U256::ZERO);
   }

   /// The route the swap UI picked in the field: ETH wrapped into the V4 WETH/USDG pool
   /// (`fee 500 / tickSpacing 10` = `0xfcfae8fa…6593`), which used to revert with empty data at
   /// ~39.9k gas.
   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn robinhood_eth_to_usdg_v4_weth_pool_route() {
      let ctx = robinhood_ctx();

      let weth_usdg = UniswapV4Pool::from_components(
         ROBINHOOD,
         Currency::wrapped_native(ROBINHOOD),
         Currency::from(ERC20Token::usdg_robinhood()),
         FeeAmount::new(500),
         DexKind::UniswapV4,
         Address::ZERO,
      );

      let received = robinhood_eth_to_usdg(
         ctx,
         NumericValue::parse_to_wei("0.01", 18),
         vec![weth_usdg.into()],
      )
      .await;

      assert!(received > U256::ZERO);
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
      let ctx = unlock_ctx();

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
         amount_in.abbreviated(),
         currency_in.symbol(),
         currency_out.symbol(),
         amount_out.abbreviated()
      );
      eprintln!("Swap Steps Length: {}", swap_steps.len());

      for swap in &swap_steps {
         eprintln!(
            "Swap Step: {} (Wei: {}) {} -> {} (Wei: {}) {} {} ({})",
            swap.amount_in.abbreviated(),
            swap.amount_in.wei(),
            swap.currency_in.symbol(),
            swap.amount_out.abbreviated(),
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
      let signer = SecureKey::from(alice.key.clone());

      let swap_params = encode_swap(
         ctx.clone(),
         SwapRequest {
            chain_id: chain,
            currency_in: currency_in.clone(),
            currency_out: currency_out.clone(),
            amount_in: amount_in.wei(),
            amount_out_min: min_amount_out.wei(),
            slippage,
            swap_type: SwapType::ExactInput,
            steps: swap_steps,
            signer: signer.clone(),
            recipient: alice.address,
            deadline_minutes: 5,
            permit2_info: None,
         },
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
         eprintln!("Gas Used: {}", res.tx_gas_used());
         panic!("Call Failed");
      }

      eprintln!("Router Call Successful");
      eprintln!("Gas Used: {}", res.tx_gas_used());

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
         amount_out.abbreviated()
      );

      eprintln!(
         "{} Got from Swap: {}",
         currency_out.symbol(),
         balance.abbreviated()
      );

      Ok(())
   }
}
