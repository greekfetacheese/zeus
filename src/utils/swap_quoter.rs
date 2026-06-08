use crate::core::ZeusCtx;
use rayon::prelude::*;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, VecDeque};
use std::sync::Arc;

use zeus_eth::{
   alloy_primitives::U256,
   amm::uniswap::{AnyUniswapPool, UniswapPool},
   currency::Currency,
   utils::NumericValue,
};

#[cfg(feature = "dev")]
use std::time::Instant;

/// Minimum estimated gas for a swap
const BASE_GAS: u64 = 140_000;
/// An estimate of the gas cost for a hop (intermidiate swaps always cost lower gas)
const HOP_GAS: u64 = 80_000;

/// Max-heap entry: marginal output gain of allocating one more chunk to a route.
struct MarginalGain {
   gain: U256,
   route_index: usize,
}

impl PartialEq for MarginalGain {
   fn eq(&self, other: &Self) -> bool {
      self.gain == other.gain
   }
}
impl Eq for MarginalGain {}
impl Ord for MarginalGain {
   fn cmp(&self, other: &Self) -> Ordering {
      self.gain.cmp(&other.gain)
   }
}
impl PartialOrd for MarginalGain {
   fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
      Some(self.cmp(other))
   }
}

/// Represents a single atomic swap step within a potentially larger route.
#[derive(Debug, Clone, PartialEq)]
pub struct SwapStep<P: UniswapPool> {
   /// The specific pool used for this swap step.
   pub pool: P,
   /// The exact amount of `currency_in` being swapped in this step.
   pub amount_in: NumericValue,
   /// The simulated amount of `currency_out` received from this step.
   pub amount_out: NumericValue,
   /// The currency being provided to the pool.
   pub currency_in: Currency,
   /// The currency being received from the pool.
   pub currency_out: Currency,
}

impl<P: UniswapPool> SwapStep<P> {
   pub fn new(
      pool: P,
      amount_in: NumericValue,
      amount_out: NumericValue,
      currency_in: Currency,
      currency_out: Currency,
   ) -> Self {
      Self {
         pool,
         amount_in,
         amount_out,
         currency_in,
         currency_out,
      }
   }
}

#[derive(Clone, Debug)]
pub struct RouteStep {
   pub pool: AnyUniswapPool,
   pub currency_in: Currency,
   pub currency_out: Currency,
   pub amount_in: NumericValue,
   pub amount_out: NumericValue,
}

#[derive(Clone, Debug, Default)]
pub struct SplitRoute {
   /// The full path of this individual route.
   pub steps: Vec<RouteStep>,
   /// The portion of the total input amount allocated to this route.
   pub amount_in: NumericValue,
   /// The final output amount from this route.
   pub amount_out: NumericValue,
}

#[derive(Clone, Debug, Default)]
pub struct Quote {
   pub currency_in: Currency,
   pub currency_out: Currency,
   pub amount_in: NumericValue,
   pub amount_out: NumericValue,
   pub swap_steps: Vec<SwapStep<AnyUniswapPool>>,
}

#[derive(Clone, Debug)]
struct Path {
   pools: Vec<Arc<AnyUniswapPool>>,
   // The sequence of currencies, e.g., [currency_in, hop1_currency, currency_out]
   path_currencies: Vec<Currency>,
}

// Internal struct for ranking valid, simulated routes
#[derive(Clone, Debug)]
struct EvaluatedRoute {
   pools: Vec<Arc<AnyUniswapPool>>,
   path_currencies: Vec<Currency>,
   amount_in: NumericValue,
   amount_out: NumericValue,
   gas_cost_usd: NumericValue,
}

impl EvaluatedRoute {
   // Calculates the net value of the route in USD for ranking purposes
   fn net_value(&self, currency_out_price: &NumericValue) -> f64 {
      let out_value_usd = self.amount_out.f64() * currency_out_price.f64();
      out_value_usd - self.gas_cost_usd.f64()
   }
}

const SPLIT_ROUTING_ITERATIONS: u32 = 10;

pub fn get_quote(
   ctx: ZeusCtx,
   amount_to_swap: NumericValue,
   currency_in: Currency,
   currency_out: Currency,
   all_pools: Vec<AnyUniswapPool>,
   eth_price: NumericValue,
   currency_out_price: NumericValue,
   base_fee: u64,
   priority_fee: U256,
   max_hops: usize,
) -> Quote {
   #[cfg(feature = "dev")]
   let now = Instant::now();

   let all_pools: Vec<Arc<AnyUniswapPool>> = all_pools.into_iter().map(Arc::new).collect();

   #[cfg(feature = "debug")]
   tracing::info!(target: "zeus_eth::amm::uniswap::quoter", "All Pools Length: {}", all_pools.len());

   #[cfg(feature = "debug")]
   for pool in &all_pools {
      tracing::info!(target: "zeus_eth::amm::uniswap::quoter", "Pool {} / {} {} Fee: {}", pool.currency0().symbol(), pool.currency1().symbol(), pool.dex_kind().version_str(), pool.fee().fee_percent());
   }

   let all_paths = find_all_paths(
      ctx,
      &all_pools,
      currency_in.clone(),
      currency_out.clone(),
      max_hops,
   );

   #[cfg(feature = "debug")]
   tracing::info!(target: "zeus_eth::amm::uniswap::quoter", "All Paths Length: {}", all_paths.len());

   if all_paths.is_empty() {
      #[cfg(feature = "debug")]
      tracing::warn!(target: "zeus_eth::amm::uniswap::quoter", "No routes found for {} -> {}", currency_in.symbol(), currency_out.symbol());
      return Quote::default();
   }

   // Evaluate and rank each path
   let mut evaluated_routes = evaluate_and_rank_routes(
      all_paths,
      amount_to_swap.clone(),
      &eth_price,
      &currency_out_price,
      base_fee,
      priority_fee,
   );

   #[cfg(feature = "debug")]
   tracing::info!(target: "zeus_eth::amm::uniswap::quoter", "Evaluated Routes Length: {}", evaluated_routes.len());

   // Select the best route
   evaluated_routes.sort_by(|a, b| {
      b.net_value(&currency_out_price)
         .partial_cmp(&a.net_value(&currency_out_price))
         .unwrap_or(std::cmp::Ordering::Equal)
   });

   let quote = if let Some(best_route) = evaluated_routes.into_iter().next() {
      build_quote_from_route(best_route, currency_in, currency_out)
   } else {
      #[cfg(feature = "debug")]
      tracing::warn!(target: "zeus_eth::amm::uniswap::quoter", "No profitable routes found after evaluation.");
      Quote::default()
   };

   #[cfg(feature = "dev")]
   tracing::info!(
      "Quote took {} μs for {} pools",
      now.elapsed().as_micros(),
      all_pools.len()
   );

   quote
}

pub fn get_quote_with_split_routing(
   ctx: ZeusCtx,
   amount_to_swap: NumericValue,
   currency_in: Currency,
   currency_out: Currency,
   all_pools: Vec<AnyUniswapPool>,
   eth_price: NumericValue,
   currency_out_price: NumericValue,
   base_fee: u64,
   priority_fee: U256,
   max_hops: usize,
   max_split_routes: usize,
) -> Quote {
   #[cfg(feature = "dev")]
   let now = Instant::now();

   let all_pools: Vec<Arc<AnyUniswapPool>> = all_pools.into_iter().map(Arc::new).collect();

   #[cfg(feature = "debug")]
   tracing::info!(target: "zeus_eth::amm::uniswap::quoter", "All Pools Length: {}", all_pools.len());

   #[cfg(feature = "debug")]
   for pool in &all_pools {
      tracing::info!(target: "zeus_eth::amm::uniswap::quoter", "Pool {} / {} {} Fee: {}", pool.currency0().symbol(), pool.currency1().symbol(), pool.dex_kind().version_str(), pool.fee().fee_percent());
   }

   let all_paths = find_all_paths(
      ctx,
      &all_pools,
      currency_in.clone(),
      currency_out.clone(),
      max_hops,
   );

   if all_paths.is_empty() {
      #[cfg(feature = "debug")]
      tracing::warn!(target: "zeus_eth::amm::uniswap::quoter_split", "No routes found for {} -> {}", currency_in.symbol(), currency_out.symbol());
      return Quote::default();
   }

   let mut candidate_routes = evaluate_and_rank_routes(
      all_paths,
      amount_to_swap.clone(),
      &eth_price,
      &currency_out_price,
      base_fee,
      priority_fee,
   );

   let top_routes: Vec<EvaluatedRoute> =
      candidate_routes.drain(..).take(max_split_routes).collect();

   if top_routes.is_empty() {
      #[cfg(feature = "debug")]
      tracing::warn!(target: "zeus_eth::amm::uniswap::quoter_split", "No viable candidate routes found after ranking.");
      return Quote::default();
   }

   #[cfg(feature = "debug")]
   tracing::info!(target: "zeus_eth::amm::uniswap::quoter_split", "Found {} candidate routes for split routing.", top_routes.len());

   // distribute the input across the best routes
   let total_amount_in_wei = amount_to_swap.wei();
   let chunk_size = total_amount_in_wei / U256::from(SPLIT_ROUTING_ITERATIONS);

   let mut allocations = vec![U256::ZERO; top_routes.len()];

   if chunk_size.is_zero() {
      // Total smaller than the iteration count: just use the best-ranked route.
      allocations[0] = total_amount_in_wei;
   } else {
      // Invariant: current_output[i] == simulate_path(allocations[i]) at all times.
      let mut current_output = vec![U256::ZERO; top_routes.len()];

      // Seed each route's first-chunk marginal gain. This is the only parallel part:
      // N independent simulations.
      let initial: Vec<MarginalGain> = top_routes
         .par_iter()
         .enumerate()
         .map(|(i, route)| MarginalGain {
            gain: simulate_path(&route.pools, &route.path_currencies, chunk_size)
               .unwrap_or_default(),
            route_index: i,
         })
         .collect();
      let mut heap = BinaryHeap::from(initial); // O(n) heapify

      // Commit one chunk per iteration to the route with the highest marginal gain,
      // then refresh only that route's gain (one simulation).
      for _ in 0..SPLIT_ROUTING_ITERATIONS {
         let Some(MarginalGain { gain, route_index }) = heap.pop() else {
            break;
         };

         // `gain` was the exact delta from the previous output (concave, monotone
         // increasing curve), so we can update output without re-simulating it.
         allocations[route_index] += chunk_size;
         current_output[route_index] += gain;

         let route = &top_routes[route_index];
         let next_output = simulate_path(
            &route.pools,
            &route.path_currencies,
            allocations[route_index] + chunk_size,
         )
         .unwrap_or_default();

         heap.push(MarginalGain {
            gain: next_output.saturating_sub(current_output[route_index]),
            route_index,
         });
      }
   }

   // Build the final quote from the distributed amounts (no mutex needed now).
   let final_split_routes: Vec<SplitRoute> = top_routes
      .into_par_iter()
      .enumerate()
      .filter_map(|(i, route_info)| {
         let allocated_amount_wei = allocations[i];
         if allocated_amount_wei.is_zero() {
            return None;
         }

         let mut steps = Vec::new();
         let mut current_amount_in_step = allocated_amount_wei;

         for j in 0..route_info.pools.len() {
            let pool = &route_info.pools[j];
            let currency_in_step = &route_info.path_currencies[j];
            let currency_out_step = &route_info.path_currencies[j + 1];

            let amount_out_wei =
               pool.simulate_swap(currency_in_step, current_amount_in_step).unwrap_or_default();

            steps.push(RouteStep {
               pool: (**pool).clone(),
               currency_in: currency_in_step.clone(),
               currency_out: currency_out_step.clone(),
               amount_in: NumericValue::format_wei(
                  current_amount_in_step,
                  currency_in_step.decimals(),
               ),
               amount_out: NumericValue::format_wei(amount_out_wei, currency_out_step.decimals()),
            });

            current_amount_in_step = amount_out_wei;
         }

         Some(SplitRoute {
            steps,
            amount_in: NumericValue::format_wei(allocated_amount_wei, currency_in.decimals()),
            amount_out: NumericValue::format_wei(current_amount_in_step, currency_out.decimals()),
         })
      })
      .collect();

   // Aggregate results into the final Quote object.
   let total_amount_out_wei: U256 = final_split_routes.iter().map(|r| r.amount_out.wei()).sum();

   let mut swap_steps = Vec::new();

   for split_route in final_split_routes {
      for step in split_route.steps {
         swap_steps.push(SwapStep {
            pool: step.pool,
            currency_in: step.currency_in,
            currency_out: step.currency_out,
            amount_in: step.amount_in,
            amount_out: step.amount_out,
         });
      }
   }

   #[cfg(feature = "dev")]
   tracing::info!(
      "Quote took {} μs for {} pools",
      now.elapsed().as_micros(),
      all_pools.len()
   );

   let amount_out = NumericValue::format_wei(total_amount_out_wei, currency_out.decimals());

   Quote {
      currency_in,
      currency_out,
      amount_in: amount_to_swap,
      amount_out,
      swap_steps,
   }
}

/// Finds all possible sequences of pools to connect input and output currencies.
fn find_all_paths(
   ctx: ZeusCtx,
   all_pools: &[Arc<AnyUniswapPool>],
   start_currency: Currency,
   end_currency: Currency,
   max_hops: usize,
) -> Vec<Path> {
   // Adjacency list: Currency -> Vec<(NeighborCurrency, Pool)>
   let mut adj: HashMap<Currency, Vec<(Currency, Arc<AnyUniswapPool>)>> = HashMap::new();
   for pool in all_pools {
      // ! Avoid ERC20/ERC20 pairs for V4 pools as these swaps dont work with UniversalRouter
      if pool.dex_kind().is_v4() && pool.currency0().is_erc20() && pool.currency1().is_erc20() {
         continue;
      }

      let has_liquidity = ctx.pool_has_sufficient_liquidity(pool).unwrap_or(false);

      if !has_liquidity {
         continue;
      }

      let c0 = pool.currency0().clone();
      let c1 = pool.currency1().clone();
      adj.entry(c0.clone()).or_default().push((c1.clone(), pool.clone()));
      adj.entry(c1).or_default().push((c0, pool.clone()));
   }

   let mut valid_paths = Vec::new();
   // A queue for BFS: stores the path of pools taken so far
   let mut queue: VecDeque<Path> = VecDeque::new();

   // Handle ETH -> WETH equivalence by treating them as the same starting node
   let weth = Currency::wrapped_native(start_currency.chain_id());
   let start_nodes = if start_currency.is_native() {
      vec![start_currency.clone(), weth.clone()]
   } else {
      vec![start_currency]
   };

   for start_node in start_nodes {
      if let Some(neighbors) = adj.get(&start_node) {
         for (neighbor_currency, pool) in neighbors {
            queue.push_back(Path {
               pools: vec![pool.clone()],
               path_currencies: vec![start_node.clone(), neighbor_currency.clone()],
            });
         }
      }
   }

   while let Some(current_path) = queue.pop_front() {
      if current_path.pools.len() >= max_hops {
         continue;
      }

      let last_currency_in_path = current_path.path_currencies.last().unwrap();

      // Handle ETH/WETH equivalence for the destination
      let is_end_node = if end_currency.is_native() {
         *last_currency_in_path == end_currency || *last_currency_in_path == weth.clone()
      } else {
         *last_currency_in_path == end_currency
      };

      if is_end_node {
         valid_paths.push(current_path.clone());
         // Continue searching, longer paths might yield better results
      }

      if let Some(neighbors) = adj.get(last_currency_in_path) {
         for (next_currency, pool) in neighbors {
            // Avoid cycles by checking if the currency is already in the path
            if !current_path.path_currencies.contains(next_currency) {
               let mut new_pools = current_path.pools.clone();
               new_pools.push(pool.clone());

               let mut new_currencies = current_path.path_currencies.clone();
               new_currencies.push(next_currency.clone());

               queue.push_back(Path {
                  pools: new_pools,
                  path_currencies: new_currencies,
               });
            }
         }
      }
   }
   valid_paths
}

/// Simulates each path and calculates its value.
fn evaluate_and_rank_routes(
   paths: Vec<Path>,
   amount_in: NumericValue,
   eth_price: &NumericValue,
   _currency_out_price: &NumericValue,
   base_fee: u64,
   priority_fee: U256,
) -> Vec<EvaluatedRoute> {
   paths
      .into_par_iter()
      .filter_map(|path| {
         let mut current_amount_in = amount_in.wei();

         for i in 0..path.pools.len() {
            let pool = &path.pools[i];
            let currency_in_step = &path.path_currencies[i];

            if current_amount_in.is_zero() {
               return None;
            }

            match pool.simulate_swap(currency_in_step, current_amount_in) {
               Ok(amount_out_wei) => current_amount_in = amount_out_wei,
               Err(_) => return None,
            }
         }

         if current_amount_in.is_zero() {
            return None;
         }

         let final_amount_out = NumericValue::format_wei(
            current_amount_in,
            path.path_currencies.last().unwrap().decimals(),
         );

         let (gas_cost_usd, _) =
            estimate_gas_cost_for_route(eth_price, base_fee, priority_fee, &path.pools);

         Some(EvaluatedRoute {
            pools: path.pools,
            path_currencies: path.path_currencies,
            amount_in: amount_in.clone(),
            amount_out: final_amount_out,
            gas_cost_usd,
         })
      })
      .collect()
}

fn estimate_gas_cost_for_route(
   eth_price: &NumericValue,
   base_fee: u64,
   priority_fee: U256,
   pools: &[Arc<AnyUniswapPool>],
) -> (NumericValue, u64) {
   if pools.is_empty() {
      return (NumericValue::default(), 0);
   }
   let num_hops = pools.len();
   let total_gas = BASE_GAS + HOP_GAS * (num_hops as u64 - 1);
   let total_gas_used_u256 = U256::from(total_gas);
   let gas_price_wei = U256::from(base_fee) + priority_fee;
   let cost_in_wei = gas_price_wei * total_gas_used_u256;
   let cost_eth = NumericValue::format_wei(cost_in_wei, 18);
   let cost_in_usd = NumericValue::from_f64(cost_eth.f64() * eth_price.f64());
   (cost_in_usd, total_gas)
}

fn build_quote_from_route(
   route: EvaluatedRoute,
   currency_in: Currency,
   currency_out: Currency,
) -> Quote {
   let mut steps = Vec::new();
   let mut current_amount_in = route.amount_in.wei();

   // Re-simulate the best path one last time to build the final step-by-step structs.
   for i in 0..route.pools.len() {
      let pool = &route.pools[i];
      let currency_in_step = &route.path_currencies[i];
      let currency_out_step = &route.path_currencies[i + 1];

      let amount_out_wei =
         pool.simulate_swap(currency_in_step, current_amount_in).unwrap_or_default();

      steps.push(RouteStep {
         pool: (**pool).clone(),
         currency_in: currency_in_step.clone(),
         currency_out: currency_out_step.clone(),
         amount_in: NumericValue::format_wei(current_amount_in, currency_in_step.decimals()),
         amount_out: NumericValue::format_wei(amount_out_wei, currency_out_step.decimals()),
      });

      current_amount_in = amount_out_wei;
   }

   let mut swap_steps = Vec::new();

   for step in steps {
      swap_steps.push(SwapStep {
         pool: step.pool.clone(),
         currency_in: step.currency_in.clone(),
         currency_out: step.currency_out.clone(),
         amount_in: step.amount_in.clone(),
         amount_out: step.amount_out.clone(),
      });
   }

   Quote {
      currency_in,
      currency_out,
      amount_in: route.amount_in,
      amount_out: route.amount_out,
      swap_steps,
   }
}

/// Simulates a swap through a full path of pools.
fn simulate_path(
   path: &[Arc<AnyUniswapPool>],
   path_currencies: &[Currency],
   amount_in: U256,
) -> Result<U256, anyhow::Error> {
   let mut current_amount = amount_in;
   for i in 0..path.len() {
      let pool = &path[i];
      let currency_in_step = &path_currencies[i];
      if current_amount.is_zero() {
         return Ok(U256::ZERO);
      }
      current_amount = pool.simulate_swap(currency_in_step, current_amount)?;
   }
   Ok(current_amount)
}
