#[cfg(test)]
mod tests {
   use std::time::Instant;

   use crate::core::types::BaseFee;
   use crate::gui::ui::dapps::uniswap::swap::get_relevant_pools;
   use crate::tests::test_ctx;

   use crate::utils::{simulate::*, swap_quoter::*, universal_router_v2::*};
   use zeus_wallet::SecureKey;

   use zeus_eth::revm::context::ContextTr;
   use zeus_eth::{
      alloy_primitives::{Address, U256},
      alloy_rpc_types::BlockId,
      amm::uniswap::{
         AnyUniswapPool, DexKind, FeeAmount, UniswapPool, UniswapV2Pool, UniswapV3Pool,
         UniswapV4Pool,
      },
      currency::{Currency, ERC20Token, NativeCurrency},
      revm_utils::*,
      types::ChainId,
      utils::{NumericValue, address_book, client::RpcClient, price_feed::get_eth_price},
   };

   const MAINNET: u64 = 1;
   const OPTIMISM: u64 = 10;
   const BASE: u64 = 8453;
   const ARBITRUM: u64 = 42161;
   const ROBINHOOD: u64 = 4663;

   /// The slippage every case trades at. Wide enough that the quoter's gas accounting — it converts
   /// gas into output-token terms with a live ETH price — cannot move the floor past the real output.
   const SLIPPAGE: f64 = 0.5;

   // ---------------------------------------------------------------------- harness

   /// A pool to quote against, named by address.
   ///
   /// V4 pools have no address of their own — the router settles them through the singleton
   /// PoolManager, keyed by `keccak(poolKey)` — so those are passed in already built.
   #[derive(Clone)]
   enum PoolRef {
      V2(&'static str),
      V3(&'static str),
      V4(Box<UniswapV4Pool>),
   }

   impl PoolRef {
      fn v4(pool: UniswapV4Pool) -> Self {
         PoolRef::V4(Box::new(pool))
      }

      async fn build(
         &self,
         client: &RpcClient,
         chain: u64,
      ) -> Result<AnyUniswapPool, anyhow::Error> {
         Ok(match self {
            PoolRef::V2(addr) => {
               UniswapV2Pool::from_address(client.clone(), chain, addr.parse()?).await?.into()
            }
            PoolRef::V3(addr) => {
               UniswapV3Pool::from_address(client.clone(), chain, addr.parse()?).await?.into()
            }
            PoolRef::V4(pool) => pool.as_ref().clone().into(),
         })
      }
   }

   /// One swap regression: how much of what, on which chain, through which pools.
   ///
   /// Pools and prices are inputs — never discovery, never the pool/price managers' caches (both of
   /// which read this machine's `data/`). A case that wants a route the pools cannot serve in one hop
   /// simply omits the one-hop pool, which is how the chained cases below are kept deterministic.
   struct SwapCase {
      chain: u64,
      amount_in: NumericValue,
      currency_in: Currency,
      currency_out: Currency,
      pools: Vec<PoolRef>,
      /// USD per unit of currency out, for the quoter's gas accounting. Stablecoins are $1; native out
      /// is filled from the live ETH price; anything else sets it explicitly.
      currency_out_price: f64,
      max_hops: usize,
      max_routes: usize,
      split: bool,
   }

   impl SwapCase {
      fn new(
         chain: u64,
         amount_in: &str,
         currency_in: Currency,
         currency_out: Currency,
         pools: Vec<PoolRef>,
      ) -> Self {
         Self {
            chain,
            amount_in: NumericValue::parse_to_wei(amount_in, currency_in.decimals()),
            currency_in,
            currency_out,
            pools,
            currency_out_price: 1.0,
            max_hops: 2,
            max_routes: 1,
            split: false,
         }
      }

      /// Stablecoins (and USDG) are $1; native output is filled from the live ETH price. A case that
      /// trades into a non-stable token sets `currency_out_price` directly.
      fn split(mut self, max_hops: usize, max_routes: usize) -> Self {
         self.split = true;
         self.max_hops = max_hops;
         self.max_routes = max_routes;
         self
      }
   }

   struct SwapOutcome {
      quoted: NumericValue,
      min_out: U256,
      received: U256,
   }

   impl SwapOutcome {
      /// The router delivered at least the floor the quote promised.
      fn assert_delivered(&self) {
         assert!(
            self.min_out > U256::ZERO,
            "the quote's floor is zero, so this case proves nothing"
         );
         assert!(
            self.received >= self.min_out,
            "received {} wei < floor {} wei (quoted {} wei)",
            self.received,
            self.min_out,
            self.quoted.wei()
         );
      }
   }

   /// Quote, encode and execute a swap the way the app does.
   ///
   /// The fork is prepared through `prepare_fork` with `swap_prefetch_accounts`, the Permit2 allowance
   /// is committed into that fork, and the router call runs through `simulate_on_fork_db` — the same
   /// helpers `swap_via_ur` uses, so a test that passes says the *app's* path works, not just the
   /// encoder. Only the approval broadcast and the confirm window are left out; a revert therefore
   /// reports the on-chain reason instead of a bare panic.
   async fn run_swap(case: SwapCase) -> Result<SwapOutcome, anyhow::Error> {
      let SwapCase {
         chain,
         amount_in,
         currency_in,
         currency_out,
         pools,
         currency_out_price,
         max_hops,
         max_routes,
         split,
      } = case;

      let chain_id = ChainId::from(chain);
      let ctx = test_ctx(chain);
      let client = ctx.get_client(chain).await?;

      let pools = {
         let mut built = Vec::with_capacity(pools.len());
         for pool in &pools {
            built.push(pool.build(&client, chain).await?);
         }
         built
      };
      let pools = ctx.pool_manager().update_state_for_pools(ctx.clone(), chain, pools).await?;
      let pool_count = pools.len();
      anyhow::ensure!(
         pool_count > 0,
         "none of the pools given for chain {chain} have on-chain state"
      );

      let eth_price = match get_eth_price(client.clone(), chain, None).await {
         Ok(price) if price > 0.0 => price,
         _ => 3000.0,
      };
      let currency_out_price = if currency_out.is_native() {
         eth_price
      } else {
         currency_out_price
      };

      let base_fee = BaseFee::default();
      let priority_fee = NumericValue::parse_to_gwei("1");
      let quote = if split {
         get_quote_with_split_routing(
            amount_in.clone(),
            currency_in.clone(),
            currency_out.clone(),
            pools,
            NumericValue::from_f64(eth_price),
            NumericValue::from_f64(currency_out_price),
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
            NumericValue::from_f64(eth_price),
            NumericValue::from_f64(currency_out_price),
            base_fee.next,
            priority_fee.wei(),
            max_hops,
         )
      };

      anyhow::ensure!(
         !quote.swap_steps.is_empty(),
         "no {} -> {} route through the {pool_count} pools given",
         currency_in.symbol(),
         currency_out.symbol()
      );

      eprintln!(
         "Quote {} {} -> {} {} in {} step(s)",
         amount_in.abbreviated(),
         currency_in.symbol(),
         quote.amount_out.abbreviated(),
         currency_out.symbol(),
         quote.swap_steps.len()
      );
      for (i, step) in quote.swap_steps.iter().enumerate() {
         eprintln!(
            "  hop {i}: {} {} -> {} {} | {} {}% ({})",
            step.amount_in.abbreviated(),
            step.currency_in.symbol(),
            step.amount_out.abbreviated(),
            step.currency_out.symbol(),
            step.pool.dex_kind().as_str(),
            step.pool.fee().fee_percent(),
            step.pool.address()
         );
      }

      let min_out = quote.amount_out.calc_slippage(SLIPPAGE, currency_out.decimals());
      anyhow::ensure!(min_out.wei() > U256::ZERO, "quote rounds to zero");

      let alice = DummyAccount::new(
         AccountType::EOA,
         if currency_in.is_native() {
            amount_in.wei()
         } else {
            U256::ZERO
         },
      );

      let params = encode_swap(
         ctx.clone(),
         SwapRequest {
            chain_id: chain,
            currency_in: currency_in.clone(),
            currency_out: currency_out.clone(),
            amount_in: amount_in.wei(),
            amount_out_min: min_out.wei(),
            slippage: SLIPPAGE,
            swap_type: SwapType::ExactInput,
            // No Permit2 signature: the fork gets a plain allowance instead (see below), which is the
            // `PERMIT2_TRANSFER_FROM` pull the router performs either way.
            permit2_info: None,
            steps: quote.swap_steps.clone(),
            signer: SecureKey::from(alice.key.clone()),
            recipient: alice.address,
            deadline_minutes: 5,
         },
      )
      .await?;

      let (block, _) = pinned_head(ctx.clone(), chain_id, BlockId::latest()).await?;
      let router = address_book::universal_router_v2(chain)?;
      let permit2 = address_book::permit2_contract(chain)?;

      let accounts = swap_prefetch_accounts(
         chain_id,
         alice.address,
         router,
         permit2,
         block.header.beneficiary,
         &currency_in,
         &currency_out,
         &quote.swap_steps,
      );
      let hop_pools = quote.swap_steps.iter().map(|s| s.pool.clone()).collect::<Vec<_>>();

      let mut factory = prepare_fork(
         ctx.clone(),
         chain_id,
         &block,
         accounts,
         StoragePrefetch::Pools(hop_pools),
      )
      .await?;

      if currency_in.is_erc20() {
         factory.give_token(
            alice.address,
            currency_in.address(),
            amount_in.wei(),
         )?;
      }

      // An ERC-20 input is paid through Permit2, so the allowance has to exist on the fork the swap
      // runs against. `permit2_needs_approval()` is false without a `Permit2Info` (the app only builds
      // one when it has to sign), so gate on the currency, not on that flag.
      let fork_db = factory.new_sandbox_fork();
      let fork_db = if currency_in.is_erc20() {
         let mut evm = new_evm(chain_id, Some(&block), fork_db);
         simulate::approve_token(
            &mut evm,
            currency_in.address(),
            alice.address,
            permit2,
            U256::MAX,
         )?;
         evm.db().clone()
      } else {
         fork_db
      };

      let time = Instant::now();
      let (sim, received) = simulate_on_fork_db(
         chain_id,
         &block,
         fork_db,
         ForkSimRequest {
            from: alice.address,
            interact_to: router,
            call_data: params.call_data.clone(),
            value: params.value,
            gas_limit: None,
            authorization_list: vec![],
         },
         |evm, _| {
            if currency_out.is_native() {
               // A native output means the input was an ERC-20, so the dummy account started with no
               // ETH: its whole balance after the swap is the output.
               Ok(evm.balance(alice.address).map(|state| state.data).unwrap_or_default())
            } else {
               simulate::erc20_balance(evm, currency_out.address(), alice.address)
            }
         },
      )?;
      let received = received?;

      eprintln!(
         "  executed in {} ms, {} gas: received {} {}",
         time.elapsed().as_millis(),
         sim.sim_res.tx_gas_used(),
         NumericValue::format_wei(received, currency_out.decimals()).abbreviated(),
         currency_out.symbol()
      );

      Ok(SwapOutcome {
         quoted: quote.amount_out,
         min_out: min_out.wei(),
         received,
      })
   }

   // ---------------------------------------------------------------------- pools

   /// The WETH/USDC pools to quote against, by chain: the V2 pair and the 0.05% V3 pool, read off each
   /// chain's V2/V3 factories.
   fn weth_usdc_pools(chain: u64) -> Vec<PoolRef> {
      let addrs: &[(&'static str, bool)] = match chain {
         MAINNET => &[
            ("0xB4e16d0168e52d35CaCD2c6185b44281Ec28C9Dc", true),
            (
               "0x88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640",
               false,
            ),
         ],
         BASE => &[
            ("0x88A43bbDF9D098eEC7bCEda4e2494615dfD9bB9C", true),
            (
               "0xd0b53D9277642d899DF5C87A3966A349A798F224",
               false,
            ),
         ],
         OPTIMISM => &[(
            "0x85149247691df622eaF1a8Bd0CaFd40BC45154a9",
            false,
         )],
         ARBITRUM => &[(
            "0xC6962004f452bE9203591991D15f6b388e09E8D0",
            false,
         )],
         _ => &[],
      };

      addrs
         .iter()
         .map(|(addr, is_v2)| {
            if *is_v2 {
               PoolRef::V2(addr)
            } else {
               PoolRef::V3(addr)
            }
         })
         .collect()
   }

   /// Mainnet WETH/USDC plus the V4 pools Zeus ships constants for, and the USDC/USDT V3 pool — enough
   /// for the quoter to choose between V2, V3 and V4 for the same pairs.
   fn mainnet_pools() -> Vec<PoolRef> {
      let mut pools = weth_usdc_pools(MAINNET);
      pools.extend([
         PoolRef::V3("0x3416cF6C708Da44DB2624D63ea0AAef7113527C6"), // USDC/USDT 0.01%
         PoolRef::v4(UniswapV4Pool::eth_usdc()),
         PoolRef::v4(UniswapV4Pool::usdc_usdt()),
      ]);
      pools
   }

   /// Every V3 fee tier with liquidity for WETH/USDG, plus the V2 pair.
   fn robinhood_pools() -> Vec<PoolRef> {
      vec![
         PoolRef::V3("0x52e65B17fB6E5BA00Ed806f37Afcd2DaA50271Ca"), // fee 100
         PoolRef::V3("0x69BfaF19C9f377BB306a89aEd9F6B07e2c1a8d9a"), // fee 500
         PoolRef::V3("0xa9188730Fe85Be88ad499D7d52B099e800fB0334"), // fee 3000
         PoolRef::V3("0x5f009E071F07e92B6C624e83F52F17bBDa34680D"), // fee 10000
         PoolRef::V2("0x8803c117ccae7B5146297876c2A25DF135141C4d"),
      ]
   }

   // ---------------------------------------------------------------------- mainnet

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn mainnet_eth_to_usdc() {
      let outcome = run_swap(SwapCase::new(
         MAINNET,
         "10",
         Currency::from(NativeCurrency::from(MAINNET)),
         Currency::from(ERC20Token::usdc()),
         mainnet_pools(),
      ))
      .await
      .unwrap();

      outcome.assert_delivered();
   }

   /// Large enough that one pool would move too much price, so the quoter spreads it — the case that
   /// exercises several slices and the wrap/re-wrap rules that come with them.
   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn mainnet_eth_to_usdc_split() {
      let outcome = run_swap(
         SwapCase::new(
            MAINNET,
            "300",
            Currency::from(NativeCurrency::from(MAINNET)),
            Currency::from(ERC20Token::usdc()),
            mainnet_pools(),
         )
         .split(2, 5),
      )
      .await
      .unwrap();

      outcome.assert_delivered();
   }

   /// No one-hop WETH/USDT pool is given, so the route has to chain V3 then V4 — the shape whose
   /// intermediate hop reverted `TRANSFER_FAILED` (a quoted amount even 1 wei above the previous hop's
   /// real output is more than the router holds, so intermediate hops must spend its balance).
   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn mainnet_eth_to_usdt_two_hop_v3_then_v4() {
      let outcome = run_swap(SwapCase::new(
         MAINNET,
         "10",
         Currency::from(NativeCurrency::from(MAINNET)),
         Currency::from(ERC20Token::usdt()),
         vec![
            PoolRef::V3("0x88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640"), // WETH/USDC 0.05%
            PoolRef::v4(UniswapV4Pool::usdc_usdt()),
         ],
      ))
      .await
      .unwrap();

      outcome.assert_delivered();
   }

   /// The shape the old suite failed on: a size one 0.01% pool will not serve alone, chained into the
   /// V4 USDC/USDT pool. The first hop's real output lands below its quote, so the second hop has to
   /// spend the router's balance instead of the quoted amount — otherwise `safeTransfer` reverts
   /// `TRANSFER_FAILED` (this is the case that used to fail with `Call Failed`, 3.8M gas).
   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn mainnet_eth_to_usdt_large_two_hop_v3_fee_100_then_v4() {
      let outcome = run_swap(SwapCase::new(
         MAINNET,
         "300",
         Currency::from(NativeCurrency::from(MAINNET)),
         Currency::from(ERC20Token::usdt()),
         vec![
            PoolRef::V3("0xE0554a476A092703abdB3Ef35c80e0D76d32939F"), // WETH/USDC 0.01%
            PoolRef::v4(UniswapV4Pool::usdc_usdt()),
         ],
      ))
      .await
      .unwrap();

      outcome.assert_delivered();
   }

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn mainnet_weth_to_usdc() {
      let outcome = run_swap(SwapCase::new(
         MAINNET,
         "10",
         Currency::wrapped_native(MAINNET),
         Currency::from(ERC20Token::usdc()),
         mainnet_pools(),
      ))
      .await
      .unwrap();

      outcome.assert_delivered();
   }

   /// ERC-20 in, native out, through the V4 pool: the unwrap path plus the Permit2 allowance the
   /// router pulls. The old harness never granted that allowance (`permit2_needs_approval()` is false
   /// when `permit2_info` is `None`), so every ERC-20-input case failed on the allowance, not the code.
   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn mainnet_usdc_to_eth_v4() {
      let outcome = run_swap(SwapCase::new(
         MAINNET,
         "2500",
         Currency::from(ERC20Token::usdc()),
         Currency::from(NativeCurrency::from(MAINNET)),
         vec![PoolRef::v4(UniswapV4Pool::eth_usdc())],
      ))
      .await
      .unwrap();

      outcome.assert_delivered();
   }

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn mainnet_usdc_to_usdt_v4() {
      let outcome = run_swap(SwapCase::new(
         MAINNET,
         "10000",
         Currency::from(ERC20Token::usdc()),
         Currency::from(ERC20Token::usdt()),
         vec![PoolRef::v4(UniswapV4Pool::usdc_usdt())],
      ))
      .await
      .unwrap();

      outcome.assert_delivered();
   }

   // ---------------------------------------------------------------------- other chains

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn optimism_eth_to_usdc() {
      let outcome = run_swap(SwapCase::new(
         OPTIMISM,
         "1",
         Currency::from(NativeCurrency::from(OPTIMISM)),
         Currency::from(ERC20Token::usdc_optimism()),
         weth_usdc_pools(OPTIMISM),
      ))
      .await
      .unwrap();

      outcome.assert_delivered();
   }

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn base_eth_to_usdc() {
      let outcome = run_swap(SwapCase::new(
         BASE,
         "1",
         Currency::from(NativeCurrency::from(BASE)),
         Currency::from(ERC20Token::usdc_base()),
         weth_usdc_pools(BASE),
      ))
      .await
      .unwrap();

      outcome.assert_delivered();
   }

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn arbitrum_eth_to_usdc() {
      let outcome = run_swap(SwapCase::new(
         ARBITRUM,
         "1",
         Currency::from(NativeCurrency::from(ARBITRUM)),
         Currency::from(ERC20Token::usdc_arbitrum()),
         weth_usdc_pools(ARBITRUM),
      ))
      .await
      .unwrap();

      outcome.assert_delivered();
   }

   // ---------------------------------------------------------------------- Robinhood
   //
   // The router deployed there is the 2025+ Universal Router generation, so both swap shapes differ
   // from the older chains: V3/V2 inputs carry a trailing `minHopPriceX36` array, the V4 swap struct
   // has a `minHopPriceX36` word before `hookData`, and an ETH trade routed through a V4 *WETH* pool
   // has to wrap the input first — that pool's SETTLE pulls WETH off the router.

   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn robinhood_eth_to_usdg_v2_v3() {
      let outcome = run_swap(SwapCase::new(
         ROBINHOOD,
         "1",
         Currency::from(NativeCurrency::from(ROBINHOOD)),
         Currency::from(ERC20Token::usdg_robinhood()),
         robinhood_pools(),
      ))
      .await
      .unwrap();

      outcome.assert_delivered();
   }

   /// The route the swap UI picked in the field: ETH wrapped into the V4 WETH/USDG pool
   /// (`fee 500 / tickSpacing 10` = `0xfcfae8fa…6593`), which reverted with empty data at ~39.9k gas.
   #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
   async fn robinhood_eth_to_usdg_v4_weth_pool() {
      let weth_usdg = UniswapV4Pool::from_components(
         ROBINHOOD,
         Currency::wrapped_native(ROBINHOOD),
         Currency::from(ERC20Token::usdg_robinhood()),
         FeeAmount::new(500),
         DexKind::UniswapV4,
         Address::ZERO,
      );

      let outcome = run_swap(SwapCase::new(
         ROBINHOOD,
         "0.5",
         Currency::from(NativeCurrency::from(ROBINHOOD)),
         Currency::from(ERC20Token::usdg_robinhood()),
         vec![PoolRef::v4(weth_usdg)],
      ))
      .await
      .unwrap();

      outcome.assert_delivered();
   }

   // ---------------------------------------------------------------------- discovery
   //
   // These cover which pools the UI offers (`get_relevant_pools`), not the router, so they only need
   // pool metadata and never touch the network.

   fn assert_discovers(currency_in: Currency, currency_out: Currency) {
      let pools = get_relevant_pools(
         test_ctx(MAINNET),
         true,
         true,
         true,
         &currency_in,
         &currency_out,
      );

      assert!(
         !pools.is_empty(),
         "no relevant pools for {} -> {}",
         currency_in.symbol(),
         currency_out.symbol()
      );

      let weth = Currency::wrapped_native(MAINNET);
      let touches_in = pools.iter().any(|p| p.have(&currency_in) || p.have(&weth));
      let touches_out = pools.iter().any(|p| p.have(&currency_out) || p.have(&weth));

      assert!(
         touches_in && touches_out,
         "{} -> {}: discovered {} pools but they do not span the pair",
         currency_in.symbol(),
         currency_out.symbol(),
         pools.len()
      );
   }

   #[test]
   fn discovery_usdc_to_eth_mainnet() {
      assert_discovers(
         Currency::from(ERC20Token::usdc()),
         Currency::from(NativeCurrency::from(MAINNET)),
      );
   }

   #[test]
   fn discovery_eth_to_link_mainnet() {
      assert_discovers(
         Currency::from(NativeCurrency::from(MAINNET)),
         Currency::from(ERC20Token::link()),
      );
   }

   #[test]
   fn discovery_link_to_eth_mainnet() {
      assert_discovers(
         Currency::from(ERC20Token::link()),
         Currency::from(NativeCurrency::from(MAINNET)),
      );
   }
}
