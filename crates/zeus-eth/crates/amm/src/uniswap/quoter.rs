use alloy_primitives::U256;
use std::collections::{HashMap, HashSet, VecDeque};

use crate::uniswap::{AnyUniswapPool, UniswapPool, router::SwapStep};
use currency::Currency;
use rayon::prelude::*;
use utils::NumericValue;

/// An estimate of the gas cost for a swap
const BASE_GAS: u64 = 120_000;
/// An estimate of the gas cost for a hop (intermidiate swaps always cost lower gas)
const HOP_GAS: u64 = 60_000;

#[derive(Clone)]
pub struct Route {
   pub amount_in: NumericValue,
   pub amount_out: NumericValue,
   pub pools: Vec<AnyUniswapPool>,
   pub path: Vec<Currency>,
   pub gas_cost_usd: NumericValue,
   pub total_gas_used: u64,
}

#[derive(Clone)]
pub struct QuoteRoutes {
   pub currency_in: Currency,
   pub currency_out: Currency,
   pub routes: Vec<Route>,
}

impl Default for QuoteRoutes {
   fn default() -> Self {
      Self {
         currency_in: Currency::default(),
         currency_out: Currency::default(),
         routes: Vec::new(),
      }
   }
}

impl QuoteRoutes {
   /// Flatten routes into individual swap steps, including simulated output for each step.
   /// Useful for constructing calldata for routers like Uniswap Universal Router.
   ///
   /// Returns a Vec of SwapStep, where each step represents a single pool interaction.
   pub fn get_swap_steps(&self) -> Vec<SwapStep<AnyUniswapPool>> {
      let mut individual_swaps: Vec<SwapStep<AnyUniswapPool>> = Vec::new();

      for route in &self.routes {
         // Initial input amount for the *first step* of this specific route.
         let mut current_step_input_amount = route.amount_in.wei2();

         if current_step_input_amount == U256::ZERO {
            continue;
         }

         for i in 0..route.pools.len() {
            let pool = &route.pools[i];
            let currency_in = &route.path[i];
            let currency_out = &route.path[i + 1];

            let step_amount_in = current_step_input_amount;

            // We must simulate *this specific step* to find its output amount.
            let step_amount_out = match pool.simulate_swap(currency_in, step_amount_in) {
               Ok(out) => out,
               Err(e) => {
                  tracing::warn!(
                      target: "zeus_eth::amm::uniswap::quoter2",
                      "Simulation failed for step {} -> {} in pool {} with input {}: {:?}. Using 0 output.",
                      currency_in.symbol(), currency_out.symbol(), pool.address(), step_amount_in, e
                  );
                  U256::ZERO
               }
            };

            let amount_in = NumericValue::format_wei(step_amount_in, currency_in.decimals());
            let amount_out = NumericValue::format_wei(step_amount_out, currency_out.decimals());

            let swap_step = SwapStep {
               pool: pool.clone(),
               amount_in,
               amount_out,
               currency_in: currency_in.clone(),
               currency_out: currency_out.clone(),
            };

            individual_swaps.push(swap_step);

            // Prepare for the next iteration: the output of the current step
            // becomes the input for the next step within the *same route*.
            current_step_input_amount = step_amount_out;

            // Optional: Add a check if output is zero mid-route, as subsequent steps are likely futile.
            if current_step_input_amount == U256::ZERO && i < route.pools.len() - 1 {
               tracing::warn!(
                   target: "zeus_eth::amm::uniswap::quoter2",
                   "Step {} -> {} resulted in zero output. Subsequent steps in route (Pools: {:?}) might be invalid.",
                   currency_in.symbol(), currency_out.symbol(),
                   route.pools.iter().map(|p| p.address()).collect::<Vec<_>>()
               );
            }
         }
      }

      individual_swaps
   }

   pub fn total_amount_out(&self) -> NumericValue {
      let amount = self.routes.iter().map(|r| r.amount_out.wei2()).sum();
      NumericValue::format_wei(amount, self.currency_out.decimals())
   }

   pub fn total_amount_in(&self) -> NumericValue {
      let amount = self.routes.iter().map(|r| r.amount_in.wei2()).sum();
      NumericValue::format_wei(amount, self.currency_in.decimals())
   }

   pub fn swaps_len(&self) -> usize {
      self.routes.iter().map(|r| r.pools.len()).sum()
   }

   /// Calculate the total estimated gas cost in USD across all split routes.
   pub fn total_gas_cost_usd(&self) -> f64 {
      self.routes.iter().map(|r| r.gas_cost_usd.f64()).sum()
   }

   pub fn total_gas_used(&self) -> u64 {
      self.routes.iter().map(|r| r.total_gas_used).sum()
   }

   /// Get the effective price after considering gas costs.
   pub fn effective_price_after_gas(
      &self,
      _eth_price: &NumericValue,
      currency_in_price: &NumericValue,
      currency_out_price: &NumericValue,
   ) -> Option<f64> {
      let total_out = self.total_amount_out();
      let total_in = self.total_amount_in();
      let total_gas_cost = self.total_gas_cost_usd();

      if total_in.f64() == 0.0 || currency_out_price.f64() == 0.0 {
         return None;
      }

      let total_out_value_usd = total_out.f64() * currency_out_price.f64();
      let net_output_value_usd = total_out_value_usd - total_gas_cost;

      let total_in_value_usd = total_in.f64() * currency_in_price.f64();

      if total_in_value_usd == 0.0 {
         return None;
      }

      Some(net_output_value_usd / total_in_value_usd) // Ratio of net output value to input value
   }

   pub fn pool_address_path_str(&self) -> String {
      let mut path = String::new();
      for route in &self.routes {
         for pool in &route.pools {
            path.push_str(&pool.address().to_string());
            path.push_str(" -> ");
         }
      }
      path
   }

   pub fn amounts_in(&self) -> String {
      let mut amounts = String::new();
      for route in &self.routes {
         amounts.push_str(&format!("{} ", route.amount_in.formatted().clone()));
         amounts.push_str("\n");
      }
      amounts.trim_end().to_string()
   }

   pub fn currency_path_str(&self) -> String {
      let mut path_str = String::new();
      for (i, route) in self.routes.iter().enumerate() {
         path_str.push_str(&format!("Route {}: ", i + 1));
         let currency_path: Vec<String> = route
            .path
            .iter()
            .map(|currency| currency.symbol().clone())
            .collect();
         path_str.push_str(&currency_path.join(" -> "));
         path_str.push_str(&format!(
            " (Amount In: {}, Amount Out: {}, Gas Cost: ${:.4}, Gas Used: {})\n",
            route.amount_in.formatted(),
            route.amount_out.formatted(),
            route.gas_cost_usd.f64(),
            route.total_gas_used
         ));
         path_str.push_str("   Pools: ");
         let pool_addresses: Vec<String> = route
            .pools
            .iter()
            .map(|pool| format!("{} ({})", pool.address(), pool.fee().fee())) // Show fee too
            .collect();
         path_str.push_str(&pool_addresses.join(", "));
         path_str.push_str("\n");
      }
      path_str.trim_end().to_string()
   }
}

/// Represents a potential path found during graph traversal
#[derive(Clone, Debug)]
struct PotentialPath {
   path: Vec<Currency>,
   pools: Vec<AnyUniswapPool>
}

/// Represents a ranked path after initial simulation and gas estimation
#[derive(Clone, Debug)]
struct RankedPath {
   path: PotentialPath,
   simulated_amount_out: U256,
   gas_cost_usd: NumericValue,
}

// TODO: Support V4 Pools
/// Get a quote with split routes across multiple paths
pub fn get_quote(
   amount_to_swap: U256,
   currency_in: Currency,
   currency_out: Currency,
   all_pools: Vec<AnyUniswapPool>,
   eth_price: NumericValue,
   currency_out_price: NumericValue,
   base_fee: u64,
   priority_fee: U256,
) -> QuoteRoutes {
   let max_hops = 5; // Max intermediate tokens
   let max_paths_to_consider = 5; // How many top paths to use for splitting

   let relevant_pools = get_relevant_pools(&currency_in, &currency_out, &all_pools);
   tracing::info!(target: "zeus_eth::amm::uniswap::quoter2", "Relevant Pools: {:?}", relevant_pools.len());
   let mut valid_pools = Vec::new();
   for pool in relevant_pools {
      if pool.enough_liquidity() {
         valid_pools.push(pool);
      }
   }
   if valid_pools.is_empty() {
      tracing::warn!(target: "zeus_eth::amm::uniswap::quoter2", "No relevant pools found for {}/{}", currency_in.symbol(), currency_out.symbol());
      return QuoteRoutes::default();
   }

   let mut potential_paths = find_potential_paths(&valid_pools, &currency_in, &currency_out, max_hops);
   
   if currency_in.is_native() {
      let wrapped = currency_in.to_weth_currency();
      let paths = find_potential_paths(&valid_pools, &wrapped, &currency_out, max_hops);
      potential_paths.extend(paths);
   }
   
   if currency_out.is_native() {
      let wrapped = currency_out.to_weth_currency();
      let paths = find_potential_paths(&valid_pools, &currency_in, &wrapped, max_hops);
      potential_paths.extend(paths);
   }

   if potential_paths.is_empty() {
      tracing::warn!(target: "zeus_eth::amm::uniswap::quoter2", "No potential paths found for {}/{}", currency_in.symbol(), currency_out.symbol());
      return QuoteRoutes::default();
   }

   let ranked_paths = rank_paths(
      potential_paths,
      amount_to_swap,
      &currency_in,
      &currency_out,
      &eth_price,
      &currency_out_price,
      base_fee,
      priority_fee,
   );

   if ranked_paths.is_empty() {
      return QuoteRoutes::default();
   }

   let top_paths = select_unique_top_paths(ranked_paths, max_paths_to_consider);
   let allocations = optimize_allocation_iterative(amount_to_swap, &top_paths);

   let final_routes = build_final_routes(
      &top_paths,
      &allocations,
      &currency_in,
      &currency_out,
      &eth_price,
      base_fee,
      priority_fee,
   );

   QuoteRoutes {
      currency_in: currency_in.clone(),
      currency_out: currency_out.clone(),
      routes: final_routes,
   }
}

/// Filters pools relevant to the swap pair.
fn get_relevant_pools(
   currency_in: &Currency,
   currency_out: &Currency,
   all_pools: &[AnyUniswapPool], // Borrow slice
) -> Vec<AnyUniswapPool> {
   // Keep pools that contain either the input or output currency.
   // Also keep pools connecting common intermediate tokens (like stables or WETH).
   let mut relevant_pools = Vec::new();
   let mut added_pools = HashSet::new();

   for pool in all_pools {
      let pool_addr = pool.address();

      let involves_in = pool.have(currency_in);
      let involves_out = pool.have(currency_out);
      let involves_common_base = pool.currency0().is_base() && pool.currency1().is_base();

      // if currency in is ETH also treat it as WETH
      if currency_in.is_native() {
         let involves_in = pool.have(&currency_in.to_weth_currency());
         if involves_in && !added_pools.contains(&pool_addr) {
            relevant_pools.push(pool.clone());
            added_pools.insert(pool_addr);
         }
      }

      // if currency out is ETH also treat it as WETH
      if currency_out.is_native() {
         let involves_out = pool.have(&currency_out.to_weth_currency());
         if involves_out && !added_pools.contains(&pool_addr) {
            relevant_pools.push(pool.clone());
            added_pools.insert(pool_addr);
         }
      }

      // Include pools involving in/out tokens directly
      // Include pools connecting two common base tokens (potential intermediate hops)
      if (involves_in || involves_out || involves_common_base) && !added_pools.contains(&pool_addr) {
         relevant_pools.push(pool.clone());
         added_pools.insert(pool_addr);
      }
   }
   relevant_pools
}

/// Build an adjacency list: Currency -> Vec<(NeighborCurrency, Pool)>
fn build_adjacency_list(pools: &[AnyUniswapPool]) -> HashMap<Currency, Vec<(Currency, AnyUniswapPool)>> {
   let mut adj = HashMap::new();
   for pool in pools {
      let token0 = pool.currency0();
      let token1 = pool.currency1();

      adj.entry(token0.clone())
         .or_insert_with(Vec::new)
         .push((token1.clone(), pool.clone()));
      adj.entry(token1.clone())
         .or_insert_with(Vec::new)
         .push((token0.clone(), pool.clone()));
   }
   adj
}

/// Find potential paths using BFS, returning full pool sequences. Avoids cycles.
fn find_potential_paths(
   pools: &[AnyUniswapPool],
   start: &Currency,
   end: &Currency,
   max_hops: usize, // Max number of *intermediate* tokens (path length = max_hops + 1)
) -> Vec<PotentialPath> {
   let adj = build_adjacency_list(pools);
   let mut paths = Vec::new();
   let mut queue: VecDeque<(Vec<Currency>, Vec<AnyUniswapPool>)> = VecDeque::new();

   queue.push_back((vec![start.clone()], Vec::new()));

   while let Some((current_path_nodes, current_path_pools)) = queue.pop_front() {
      let current_node = current_path_nodes.last().unwrap();
      let current_hops = current_path_pools.len(); // Number of pools = number of hops

      // Check if we reached the destination
      if current_node == end {
         if !current_path_pools.is_empty() {
            // Ensure it's a valid path with at least one hop
            paths.push(PotentialPath {
               path: current_path_nodes.clone(),
               pools: current_path_pools.clone(),
            });
         }
         // Don't continue searching further from the end node unless multi-route paths are desired differently
         continue;
      }

      if current_hops >= max_hops {
         continue;
      }

      // Explore neighbors
      if let Some(neighbors) = adj.get(current_node) {
         for (next_node, pool) in neighbors {
            // Avoid cycles: Check if the next node is already in the current path
            if !current_path_nodes.contains(next_node) {
               let mut next_path_nodes = current_path_nodes.clone();
               next_path_nodes.push(next_node.clone());

               let mut next_path_pools = current_path_pools.clone();
               next_path_pools.push(pool.clone());

               queue.push_back((next_path_nodes, next_path_pools));
            }
         }
      }
   }
   paths
}

/// Simulate a swap along a specific path with a given sequence of pools.
fn simulate_path_with_pools(
   amount_in: U256,
   path_nodes: &[Currency],       // e.g., [USDT, WETH] or [USDT, USDC, WETH]
   path_pools: &[AnyUniswapPool], // e.g., [USDT_WETH_Pool] or [USDT_USDC_Pool, USDC_WETH_Pool]
) -> Option<U256> {
   if path_nodes.len() != path_pools.len() + 1 || path_pools.is_empty() {
      // tracing::error!(target:"sor", "Mismatched nodes ({}) and pools ({}) for simulation", path_nodes.len(), path_pools.len());
      return None;
   }

   let mut current_amount = amount_in;

   for i in 0..path_pools.len() {
      let pool = &path_pools[i];
      let expected_in = &path_nodes[i];
      let expected_out = &path_nodes[i + 1];

      if !pool.have(expected_in) || !pool.have(expected_out) {
         // tracing::warn!(target: "sor", "Pool {} does not connect {} -> {}", pool.address(), expected_in.symbol(), expected_out.symbol());
         return None;
      }

      match pool.simulate_swap(expected_in, current_amount) {
         Ok(amount_out) => {
            if amount_out == U256::ZERO && current_amount > U256::ZERO {
               // tracing::warn!(target: "sor", "Simulation resulted in zero output for pool {} ({}), hop {}->{}. Input: {}", pool.address(), pool.fee().fee(), expected_in.symbol(), expected_out.symbol(), current_amount);
            }
            current_amount = amount_out;
         }
         Err(_e) => {
            //  tracing::error!(target: "sor", "Simulation error in pool {}: {:?}", pool.address(), e);
            return None;
         }
      }
   }
   Some(current_amount)
}

/// Estimate gas cost for a route based on its pools.
fn estimate_gas_cost_for_route(
   eth_price: &NumericValue,
   base_fee: u64,      // Includes base + priority now conceptually
   priority_fee: U256, // Maybe rename base_fee -> gas_price
   pools: &[AnyUniswapPool],
) -> (NumericValue, u64) {
   if pools.is_empty() {
      return (NumericValue::default(), 0);
   }
   let num_hops = pools.len();
   let total_gas = BASE_GAS + HOP_GAS * (num_hops as u64 - 1); // Base for first, hop for subsequent

   let total_gas_used_u256 = U256::from(total_gas);
   let gas_price_wei = U256::from(base_fee) + priority_fee; // Total price per gas unit

   let cost_in_wei = gas_price_wei * total_gas_used_u256;

   let cost_eth = NumericValue::format_wei(cost_in_wei, 18); // ETH has 18 decimals
   let cost_in_usd = NumericValue::value(cost_eth.f64(), eth_price.f64());
   (cost_in_usd, total_gas)
}

/// Simulate, calculate gas, and rank potential paths.
fn rank_paths(
   potential_paths: Vec<PotentialPath>,
   amount_to_swap: U256,
   _currency_in: &Currency,
   currency_out: &Currency,
   eth_price: &NumericValue,
   currency_out_price: &NumericValue,
   base_fee: u64,
   priority_fee: U256,
) -> Vec<RankedPath> {
   let mut ranked: Vec<_> = potential_paths
      .into_par_iter()
      .filter_map(|p| {
         // Simulate with the full amount to get an initial estimate
         simulate_path_with_pools(amount_to_swap, &p.path, &p.pools).and_then(|simulated_output| {
            if simulated_output == U256::ZERO && amount_to_swap > U256::ZERO {
               //  tracing::debug!(target:"sor", "Path resulted in zero output during ranking, discarding: {:?} -> {:?}", p.path.first().map(|c|c.symbol()), p.path.last().map(|c|c.symbol()));
               None // Discard paths that yield zero output for non-zero input
            } else {
               let (gas_cost_usd, _) =
                  estimate_gas_cost_for_route(eth_price, base_fee, priority_fee, &p.pools);
               Some(RankedPath {
                  path: p,
                  simulated_amount_out: simulated_output,
                  gas_cost_usd,
               })
            }
         })
      })
      .collect();

   // Calculate net benefit (output value - gas cost) for sorting
   // Note: This uses the simulation with the *full* amount_to_swap.
   // The actual benefit will depend on the final allocated amount.
   ranked.sort_by(|a, b| {
      let a_out_val =
         NumericValue::format_wei(a.simulated_amount_out, currency_out.decimals()).f64() * currency_out_price.f64();
      let b_out_val =
         NumericValue::format_wei(b.simulated_amount_out, currency_out.decimals()).f64() * currency_out_price.f64();

      let a_net_benefit = a_out_val - a.gas_cost_usd.f64();
      let b_net_benefit = b_out_val - b.gas_cost_usd.f64();

      // Sort descending by net benefit
      b_net_benefit
         .partial_cmp(&a_net_benefit)
         .unwrap_or(std::cmp::Ordering::Equal)
   });

   // tracing::debug!(target: "sor", "Ranking complete. {} viable paths.", ranked.len());
   // You might want to log the top few paths here for debugging
   // for (i, path) in ranked.iter().take(5).enumerate() {
   //     let path_str: Vec<String> = path.path.path.iter().map(|c| c.symbol().clone()).collect();
   //     tracing::debug!(target:"sor", "Rank {}: Path: {}, Out: {}, GasUSD: {}", i+1, path_str.join(" -> "), NumericValue::format_wei(path.simulated_amount_out, currency_out.decimals()).formatted(), path.gas_cost_usd.formatted());
   // }

   ranked
}

/// Select top N paths ensuring unique pool sequences.
fn select_unique_top_paths(ranked_paths: Vec<RankedPath>, max_paths: usize) -> Vec<RankedPath> {
   let mut selected_paths = Vec::new();
   let mut used_pool_sequences = HashSet::new();

   for ranked_path in ranked_paths {
      // Create a unique key for the sequence of pool addresses
      let pool_key: String = ranked_path
         .path
         .pools
         .iter()
         .map(|p| p.address().to_string())
         .collect::<Vec<_>>()
         .join("-");

      if !used_pool_sequences.contains(&pool_key) {
         used_pool_sequences.insert(pool_key);
         selected_paths.push(ranked_path);
         if selected_paths.len() >= max_paths {
            break;
         }
      }
   }
   selected_paths
}

/// Optimize allocation iteratively across the selected top paths.
fn optimize_allocation_iterative(
   total_amount_in: U256,
   top_paths: &[RankedPath],
) -> Vec<U256> {
   let num_paths = top_paths.len();
   if num_paths == 0 {
      return Vec::new();
   }

   if num_paths == 1 {
      return vec![total_amount_in];
   }

   let mut allocations = vec![U256::ZERO; num_paths];
   // Use a small percentage or a fixed minimum increment
   // let increment = (total_amount_in / U256::from(100)).max(U256::from(1)); // e.g., 1% or minimum 1 wei
   // Let's use a number of steps instead to avoid issues with very large/small amounts
   let num_steps = 100; // Number of iterations to distribute the amount
   let increment = total_amount_in / U256::from(num_steps);

   if increment == U256::ZERO {
      allocations[0] = total_amount_in;
      return allocations;
   }

   let mut remaining_amount = total_amount_in;

   // Pre-calculate current total output to avoid redundant simulations inside loop
   let mut current_total_output: U256 = U256::ZERO;

   // Iteratively assign increments to the path yielding the best marginal output
   while remaining_amount >= increment {
      let mut best_path_index = None;
      let mut marginal_gains = Vec::with_capacity(num_paths);

      // Calculate marginal gain for adding 'increment' to each path
      for i in 0..num_paths {
         let path_info = &top_paths[i];
         let current_alloc = allocations[i];
         let test_alloc = current_alloc + increment;

         // Simulate the output for the *current* allocation on this path
         let output_before =
            simulate_path_with_pools(current_alloc, &path_info.path.path, &path_info.path.pools).unwrap_or(U256::ZERO);

         // Simulate the output for the *test* allocation on this path
         let output_after =
            simulate_path_with_pools(test_alloc, &path_info.path.path, &path_info.path.pools).unwrap_or(U256::ZERO);

         // Ensure subtraction doesn't underflow if output decreases (unlikely but possible with weird fee structures)
         let marginal_gain = output_after.saturating_sub(output_before);
         marginal_gains.push(marginal_gain);
      }

      // Find the path with the highest marginal gain
      let maybe_best = marginal_gains
         .iter()
         .enumerate()
         .max_by_key(|&(_, gain)| gain);

      if let Some((index, gain)) = maybe_best {
         if *gain > U256::ZERO {
            // Only allocate if there's a positive gain
            best_path_index = Some(index);
         } else {
            break;
         }
      }

      // Allocate the increment to the best path found
      if let Some(index) = best_path_index {
         allocations[index] += increment;
         remaining_amount -= increment;

         let output_before = simulate_path_with_pools(
            allocations[index] - increment,
            &top_paths[index].path.path,
            &top_paths[index].path.pools,
         )
         .unwrap_or(U256::ZERO);
         let output_after = simulate_path_with_pools(
            allocations[index],
            &top_paths[index].path.path,
            &top_paths[index].path.pools,
         )
         .unwrap_or(U256::ZERO);
         current_total_output = current_total_output.saturating_sub(output_before) + output_after;
      } else {
         // No path improved the output, stop.
         break;
      }
   }

   // Distribute any remaining dust amount proportionally or to the best path
   if remaining_amount > U256::ZERO {
      if let Some(last_index) = allocations
         .iter()
         .enumerate()
         .max_by_key(|&(_, &alloc)| alloc)
         .map(|(i, _)| i)
      {
         allocations[last_index] += remaining_amount;
      } else if !allocations.is_empty() {
         allocations[0] += remaining_amount;
      }
   }

   let allocated_sum: U256 = allocations.iter().sum();
   if allocated_sum != total_amount_in {
      tracing::warn!(target:"zeus_eth::amm::uniswap::quoter2", "Allocation mismatch: Expected {}, got {}. Diff: {}", total_amount_in, allocated_sum, total_amount_in.abs_diff(allocated_sum));
   }

   allocations
}

/// Build the final Vec<Route> based on allocated amounts.
fn build_final_routes(
   top_paths: &[RankedPath],
   allocations: &[U256],
   currency_in: &Currency,
   currency_out: &Currency,
   eth_price: &NumericValue,
   base_fee: u64,
   priority_fee: U256,
) -> Vec<Route> {
   let mut final_routes = Vec::new();

   for (i, ranked_path) in top_paths.iter().enumerate() {
      let allocated_amount = allocations[i];

      if allocated_amount > U256::ZERO {
         // Simulate the path with the specific allocated amount
         if let Some(final_amount_out) = simulate_path_with_pools(
            allocated_amount,
            &ranked_path.path.path,
            &ranked_path.path.pools,
         ) {
            // Recalculate gas cost for this route
            let (gas_cost_usd, total_gas_used) =
               estimate_gas_cost_for_route(eth_price, base_fee, priority_fee, &ranked_path.path.pools);

            let route = Route {
               amount_in: NumericValue::format_wei(allocated_amount, currency_in.decimals()),
               amount_out: NumericValue::format_wei(final_amount_out, currency_out.decimals()),
               pools: ranked_path.path.pools.clone(),
               path: ranked_path.path.path.clone(),
               gas_cost_usd,
               total_gas_used,
            };
            final_routes.push(route);
         }
      }
   }

   final_routes.sort_by(|a, b| b.amount_in.wei2().cmp(&a.amount_in.wei2()));

   final_routes
}

/*

// credits: https://github.com/mouseless0x/rusty-sando
pub fn quadratic_search<F>(optimize_fn: F, lower_bound: U256, upper_bound: U256) -> (U256, U256)
where
   F: Fn(U256) -> U256 + Sync + Send, // Ensure Send bound for parallel execution
{
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
