use alloy_primitives::U256;
use std::collections::{HashMap, VecDeque};

use crate::uniswap::{AnyUniswapPool, UniswapPool, router::SwapStep};
use currency::Currency;
use utils::NumericValue;

/// Minimum estimated gas for a swap
/// 21k the base gas for a trasaction
/// 100k for a swap
/// 25k for the sweep at the end
const BASE_GAS: u64 = 146_000;
/// An estimate of the gas cost for a hop (intermidiate swaps always cost lower gas)
const HOP_GAS: u64 = 80_000;


#[derive(Clone, Debug)]
pub struct RouteStep {
   pub pool: AnyUniswapPool,
   pub currency_in: Currency,
   pub currency_out: Currency,
   pub amount_in: NumericValue,
   pub amount_out: NumericValue,
}

#[derive(Clone, Debug, Default)]
pub struct Quote {
   pub currency_in: Currency,
   pub currency_out: Currency,
   pub amount_in: NumericValue,
   pub amount_out: NumericValue,
   pub route: Option<Vec<RouteStep>>,
   pub swap_steps: Vec<SwapStep<AnyUniswapPool>>,
}

// Internal struct for pathfinding
#[derive(Clone, Debug)]
struct Path {
    pools: Vec<AnyUniswapPool>,
    // The sequence of currencies, e.g., [currency_in, hop1_currency, currency_out]
    path_currencies: Vec<Currency>, 
}

// Internal struct for ranking valid, simulated routes
#[derive(Clone, Debug)]
struct EvaluatedRoute {
    steps: Vec<RouteStep>,
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



pub fn get_quote(
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

    // Find all possible paths from currency_in to currency_out
    let all_paths = find_all_paths(
        &all_pools,
        currency_in.clone(),
        currency_out.clone(),
        max_hops,
    );

    if all_paths.is_empty() {
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
    
    // Select the best route
    evaluated_routes.sort_by(|a, b| 
        b.net_value(&currency_out_price)
         .partial_cmp(&a.net_value(&currency_out_price))
         .unwrap_or(std::cmp::Ordering::Equal)
    );
    
    if let Some(best_route) = evaluated_routes.into_iter().next() {
        // Build the final quote from the best route
        build_quote_from_route(best_route, currency_in, currency_out)
    } else {
        tracing::warn!(target: "zeus_eth::amm::uniswap::quoter", "No profitable routes found after evaluation.");
        Quote::default()
    }
}



/// Finds all possible sequences of pools to connect input and output currencies.
fn find_all_paths(
    all_pools: &[AnyUniswapPool],
    start_currency: Currency,
    end_currency: Currency,
    max_hops: usize,
) -> Vec<Path> {
    // Adjacency list: Currency -> Vec<(NeighborCurrency, Pool)>
    let mut adj: HashMap<Currency, Vec<(Currency, AnyUniswapPool)>> = HashMap::new();
    for pool in all_pools {
        if !pool.enough_liquidity() || pool.dex_kind().is_uniswap_v4() { // Skip V4 for now as in old code
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
    let start_nodes = if start_currency.is_native() {
        vec![start_currency.clone(), start_currency.to_weth_currency()]
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
            *last_currency_in_path == end_currency || *last_currency_in_path == end_currency.to_weth_currency()
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
        .into_iter()
        .filter_map(|path| {
            let mut steps = Vec::new();
            let mut current_amount_in = amount_in.wei2();
            
            // Simulate the swap through the path, step-by-step
            for i in 0..path.pools.len() {
                let pool = &path.pools[i];
                let currency_in_step = &path.path_currencies[i];
                let currency_out_step = &path.path_currencies[i+1];
                
                if current_amount_in.is_zero() {
                    return None; // Path dried up
                }

                match pool.simulate_swap(currency_in_step, current_amount_in) {
                    Ok(amount_out_wei) => {
                        steps.push(RouteStep {
                           pool: pool.clone(),
                           currency_in: currency_in_step.clone(),
                           currency_out: currency_out_step.clone(),
                           amount_in: NumericValue::format_wei(current_amount_in, currency_in_step.decimals()),
                           amount_out: NumericValue::format_wei(amount_out_wei, currency_out_step.decimals()),
                        });
                        current_amount_in = amount_out_wei;
                    }
                    Err(_) => return None, // Simulation failed for this path
                }
            }
            
            if steps.is_empty() || current_amount_in.is_zero() {
                return None;
            }

            let final_amount_out = steps.last().unwrap().amount_out.clone();

            let (gas_cost_usd, _) = estimate_gas_cost_for_route(
                eth_price, 
                base_fee, 
                priority_fee, 
                &path.pools
            );

            Some(EvaluatedRoute {
                steps,
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
   pools: &[AnyUniswapPool],
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


fn build_quote_from_route(route: EvaluatedRoute, currency_in: Currency, currency_out: Currency) -> Quote {
    let swap_steps = route.steps.iter().map(|step| SwapStep {
        pool: step.pool.clone(),
        currency_in: step.currency_in.clone(),
        currency_out: step.currency_out.clone(),
        amount_in: step.amount_in.clone(),
        amount_out: step.amount_out.clone(),
    }).collect();

    Quote {
        currency_in,
        currency_out,
        amount_in: route.amount_in,
        amount_out: route.amount_out,
        route: Some(route.steps),
        swap_steps,
    }
}







/*
// credits: https://github.com/mouseless0x/rusty-sando
/// Juiced implementation of https://research.ijcaonline.org/volume65/number14/pxc3886165.pdf
/// splits range in more intervals, search intervals concurrently, compare, repeat till termination
pub fn quadratic_search<F>(optimize_fn: F, lower_bound: U256, upper_bound: U256) -> (U256, U256)
where
   F: Fn(U256) -> U256 + Sync + Send,
{
   //
   //            [EXAMPLE WITH 10 BOUND INTERVALS]
   //
   //     (first)              (mid)               (last)
   //        ▼                   ▼                   ▼
   //        +---+---+---+---+---+---+---+---+---+---+
   //        |   |   |   |   |   |   |   |   |   |   |
   //        +---+---+---+---+---+---+---+---+---+---+
   //        ▲   ▲   ▲   ▲   ▲   ▲   ▲   ▲   ▲   ▲   ▲
   //        0   1   2   3   4   5   6   7   8   9   X
   //
   //  * [0, X] = search range
   //  * Find revenue at each interval
   //  * Find index of interval with highest revenue
   //  * Search again with bounds set to adjacent index of highest

   let base = U256::from(1000000u64); // 1e6
   // Define tolerance relative to the range, avoid division by zero
   let range = upper_bound.saturating_sub(lower_bound);
   let tolerance = if range > base {
      (range / base).max(U256::from(1)) // Relative tolerance (e.g., 0.0001%), minimum 1 wei
   } else {
      U256::from(1) // Minimum tolerance of 1 wei if range is small
   };

   let number_of_intervals: u64 = 15; // Number of points to check in each iteration
   let mut lower = lower_bound;
   let mut upper = upper_bound;
   let mut best_input = lower_bound; // Start with lower bound as initial best guess
   let mut max_output = optimize_fn(best_input); // Evaluate initial guess

   loop {
      // Termination condition: If bounds are crossed or range is within tolerance
      if lower > upper || upper.saturating_sub(lower) < tolerance {
         // tracing::debug!(target:"sor::quadratic", "Terminate search. Lower: {}, Upper: {}, BestIn: {}, MaxOut: {}", lower, upper, best_input, max_output);
         break;
      }

      // Ensure interval calculation doesn't underflow/overflow with large numbers
      let step = upper.saturating_sub(lower) / U256::from(number_of_intervals);

      // Create intervals, ensure bounds are included
      let mut intervals = Vec::with_capacity(number_of_intervals as usize + 1);
      for i in 0..=number_of_intervals {
         let point = lower + step * U256::from(i);
         // Clamp point to be within [lower, upper] bounds precisely
         intervals.push(point.min(upper).max(lower));
      }
      // Ensure upper bound is always the last interval point if step calculation had remainder
      if *intervals.last().unwrap() < upper {
         intervals.push(upper);
      }
      intervals.dedup(); // Remove potential duplicates from clamping/step remainder

      // Evaluate outputs in parallel
      let evaluation_results: Vec<(U256, U256)> = intervals
         .par_iter()
         .map(|&input_amount| (input_amount, optimize_fn(input_amount)))
         .collect();

      // Find the input amount that gave the maximum output in this iteration
      let Some((current_best_input_in_iteration, current_max_output_in_iteration)) = evaluation_results
         .iter()
         .max_by_key(|&(_, output)| output)
         .cloned()
      // Clone the tuple (U256, U256)
      else {
         // Should not happen if intervals is not empty
         //  tracing::warn!(target:"sor::quadratic", "Quadratic search iteration yielded no results. Terminating.");
         break;
      };

      let Some(highest_index) = evaluation_results
         .iter()
         .position(|(input, _)| *input == current_best_input_in_iteration)
      else {
         // tracing::error!(target:"sor::quadratic", "Could not find index of best input. Terminating.");
         break; // Should not happen
      };

      // Update overall best result found so far
      if current_max_output_in_iteration > max_output {
         max_output = current_max_output_in_iteration;
         best_input = current_best_input_in_iteration;
      }
      // tracing::debug!(target:"sor::quadratic", "Iter best in: {}, best out: {}. New bounds search.", current_best_input_in_iteration, current_max_output_in_iteration);

      // Narrow the search bounds based on the index of the maximum
      // Ensure bounds don't get stuck or cross invalidly
      let lower_idx = if highest_index > 0 {
         highest_index - 1
      } else {
         0
      };
      let upper_idx = if highest_index < intervals.len() - 1 {
         highest_index + 1
      } else {
         intervals.len() - 1
      };

      let next_lower = intervals[lower_idx];
      let next_upper = intervals[upper_idx];

      // Prevent getting stuck if bounds don't change
      if next_lower == lower && next_upper == upper {
         // Bounds didn't narrow sufficiently, maybe tolerance is hit or function is flat
         // We can break or try a slightly different interval strategy
         // tracing::debug!(target:"sor::quadratic", "Bounds stuck. Terminating search. Lower: {}, Upper: {}", lower, upper);
         break;
      }

      lower = next_lower;
      upper = next_upper;

      // Add a safeguard against excessively small ranges causing infinite loops if tolerance is tricky
      if upper == lower && tolerance > U256::ZERO {
         // tracing::debug!(target:"sor::quadratic", "Bounds converged exactly. Terminating.");
         break;
      }
   }

   // Return the best input amount found and its corresponding output
   (best_input, max_output)
}
*/
