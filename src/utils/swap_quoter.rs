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
#[cfg(feature = "dev")]
use tracing::debug;

/// Minimum estimated gas for a swap
const BASE_GAS: u64 = 140_000;
/// An estimate of the gas cost for a hop (intermediate swaps always cost lower gas)
const HOP_GAS: u64 = 80_000;

/// Cap BFS fallback so a high max_hops setting cannot explode.
const MAX_FALLBACK_PATHS: usize = 64;

/// Greedy split granularity across disjoint direct pools.
const SPLIT_CHUNKS: u32 = 10;

/// Skip 2-hop search and splits when the best 1-hop impact is below this (%).
const TINY_IMPACT_PERCENT: f64 = 0.05;

/// Only attempt a fee-tier split when 1-hop impact is at least this (%).
const SPLIT_IMPACT_PERCENT: f64 = 0.15;

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
   path_currencies: Vec<Currency>,
}

#[derive(Clone, Debug)]
struct EvaluatedRoute {
   pools: Vec<Arc<AnyUniswapPool>>,
   path_currencies: Vec<Currency>,
   amount_in: NumericValue,
   amount_out: NumericValue,
   step_amounts: Vec<(U256, U256)>,
   gas_cost_usd: NumericValue,
}

impl EvaluatedRoute {
   fn net_value(&self, currency_out_price: &NumericValue) -> f64 {
      let out_value_usd = self.amount_out.f64() * currency_out_price.f64();
      out_value_usd - self.gas_cost_usd.f64()
   }

   fn hop_count(&self) -> usize {
      self.pools.len()
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
   quote_internal(
      amount_to_swap,
      currency_in,
      currency_out,
      all_pools,
      eth_price,
      currency_out_price,
      base_fee,
      priority_fee,
      max_hops,
      false,
      1,
   )
}

pub fn get_quote_with_split_routing(
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
   quote_internal(
      amount_to_swap,
      currency_in,
      currency_out,
      all_pools,
      eth_price,
      currency_out_price,
      base_fee,
      priority_fee,
      max_hops,
      true,
      max_split_routes.max(1),
   )
}

fn quote_internal(
   amount_to_swap: NumericValue,
   currency_in: Currency,
   currency_out: Currency,
   all_pools: Vec<AnyUniswapPool>,
   eth_price: NumericValue,
   currency_out_price: NumericValue,
   base_fee: u64,
   priority_fee: U256,
   max_hops: usize,
   split: bool,
   max_split_routes: usize,
) -> Quote {
   #[cfg(feature = "dev")]
   let now = Instant::now();

   if amount_to_swap.is_zero() || max_hops == 0 {
      return Quote::default();
   }

   let all_pools: Vec<Arc<AnyUniswapPool>> = all_pools.into_iter().map(Arc::new).collect();

   #[cfg(feature = "dev")]
   debug!(
      target: "zeus_eth::amm::uniswap::quoter",
      "All Pools Length: {}",
      all_pools.len()
   );

   let direct_paths = find_direct_paths(&all_pools, &currency_in, &currency_out);
   let mut directs = evaluate_paths(
      direct_paths,
      &amount_to_swap,
      &eth_price,
      base_fee,
      priority_fee,
   );

   sort_routes(&mut directs, &currency_out_price);
   let best_direct = directs.first().cloned();
   let impact = best_direct.as_ref().and_then(direct_impact_percent).unwrap_or(f64::INFINITY);

   let mut best = best_direct.clone();

   // Tiny impact on a liquid 1-hop: skip 2-hop search. This is the small-$ fast path.
   let skip_longer = best.is_some() && impact < TINY_IMPACT_PERCENT;

   if !skip_longer && max_hops >= 2 {
      let two_hop_paths = find_two_hop_paths(&all_pools, &currency_in, &currency_out);
      let mut two_hops = evaluate_paths(
         two_hop_paths,
         &amount_to_swap,
         &eth_price,
         base_fee,
         priority_fee,
      );
      sort_routes(&mut two_hops, &currency_out_price);
      best = pick_better(
         best,
         two_hops.into_iter().next(),
         &currency_out_price,
      );
   }

   if best.is_none() && max_hops > 2 {
      let fallback = find_fallback_paths(
         &all_pools,
         currency_in.clone(),
         currency_out.clone(),
         max_hops,
      );
      let mut evaluated = evaluate_paths(
         fallback,
         &amount_to_swap,
         &eth_price,
         base_fee,
         priority_fee,
      );
      sort_routes(&mut evaluated, &currency_out_price);
      best = evaluated.into_iter().next();
   }

   let quote =
      if split && max_split_routes > 1 && !directs.is_empty() && impact >= SPLIT_IMPACT_PERCENT {
         split_direct_pools(
            &directs,
            &amount_to_swap,
            &currency_in,
            &currency_out,
            &eth_price,
            &currency_out_price,
            base_fee,
            priority_fee,
            max_split_routes,
            best,
         )
      } else if let Some(route) = best {
         build_quote_from_route(route, currency_in, currency_out)
      } else {
         #[cfg(feature = "dev")]
         debug!(
            target: "zeus_eth::amm::uniswap::quoter",
            "No routes found for {} -> {}",
            currency_in.symbol(),
            currency_out.symbol()
         );
         Quote::default()
      };

   #[cfg(feature = "dev")]
   debug!(
      "Quote took {} μs for {} pools",
      now.elapsed().as_micros(),
      all_pools.len()
   );

   quote
}

fn find_direct_paths(
   pools: &[Arc<AnyUniswapPool>],
   currency_in: &Currency,
   currency_out: &Currency,
) -> Vec<Path> {
   let mut paths = Vec::new();
   for pool in pools {
      let Some(cin) = pool_currency_for(pool, currency_in) else {
         continue;
      };
      let Some(cout) = pool_currency_for(pool, currency_out) else {
         continue;
      };
      if same_asset(&cin, &cout) {
         continue;
      }
      paths.push(Path {
         pools: vec![pool.clone()],
         path_currencies: vec![cin, cout],
      });
   }
   paths
}

fn find_two_hop_paths(
   pools: &[Arc<AnyUniswapPool>],
   currency_in: &Currency,
   currency_out: &Currency,
) -> Vec<Path> {
   let mut paths = Vec::new();
   for p1 in pools {
      let Some(cin) = pool_currency_for(p1, currency_in) else {
         continue;
      };
      let Some(mid) = other_currency(p1, currency_in) else {
         continue;
      };
      if !mid.is_base() || same_asset(&mid, currency_out) || same_asset(&mid, currency_in) {
         continue;
      }

      for p2 in pools {
         if pool_key(p1) == pool_key(p2) {
            continue;
         }
         // Require the hop token to be the same representation on both pools
         // (no ETH/WETH translation mid-route).
         if !p2.have(&mid) {
            continue;
         }
         let Some(cout) = pool_currency_for(p2, currency_out) else {
            continue;
         };
         if same_asset(&mid, &cout) {
            continue;
         }
         paths.push(Path {
            pools: vec![p1.clone(), p2.clone()],
            path_currencies: vec![cin.clone(), mid.clone(), cout],
         });
      }
   }
   paths
}

fn find_fallback_paths(
   all_pools: &[Arc<AnyUniswapPool>],
   start_currency: Currency,
   end_currency: Currency,
   max_hops: usize,
) -> Vec<Path> {
   let mut adj: HashMap<Currency, Vec<(Currency, Arc<AnyUniswapPool>)>> = HashMap::new();
   for pool in all_pools {
      let c0 = pool.currency0().clone();
      let c1 = pool.currency1().clone();
      adj.entry(c0.clone()).or_default().push((c1.clone(), pool.clone()));
      adj.entry(c1).or_default().push((c0, pool.clone()));
   }

   let mut valid_paths = Vec::new();
   let mut queue: VecDeque<Path> = VecDeque::new();

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
      if valid_paths.len() >= MAX_FALLBACK_PATHS {
         break;
      }

      let hops = current_path.pools.len();
      if hops > max_hops {
         continue;
      }

      let last_currency_in_path = current_path.path_currencies.last().unwrap();
      let is_end_node = if end_currency.is_native() {
         *last_currency_in_path == end_currency || *last_currency_in_path == weth
      } else {
         *last_currency_in_path == end_currency
      };

      if is_end_node {
         valid_paths.push(current_path.clone());
      }

      if hops == max_hops {
         continue;
      }

      if let Some(neighbors) = adj.get(last_currency_in_path) {
         for (next_currency, pool) in neighbors {
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

fn evaluate_paths(
   paths: Vec<Path>,
   amount_in: &NumericValue,
   eth_price: &NumericValue,
   base_fee: u64,
   priority_fee: U256,
) -> Vec<EvaluatedRoute> {
   paths
      .into_par_iter()
      .filter_map(|path| {
         let (amount_out_wei, step_amounts) = simulate_path_with_steps(
            &path.pools,
            &path.path_currencies,
            amount_in.wei(),
         )?;
         let decimals = path.path_currencies.last()?.decimals();
         let (gas_cost_usd, _) = estimate_gas_cost_for_route(
            eth_price,
            base_fee,
            priority_fee,
            path.pools.len(),
         );
         Some(EvaluatedRoute {
            pools: path.pools,
            path_currencies: path.path_currencies,
            amount_in: amount_in.clone(),
            amount_out: NumericValue::format_wei(amount_out_wei, decimals),
            step_amounts,
            gas_cost_usd,
         })
      })
      .collect()
}

fn simulate_path_with_steps(
   path: &[Arc<AnyUniswapPool>],
   path_currencies: &[Currency],
   amount_in: U256,
) -> Option<(U256, Vec<(U256, U256)>)> {
   if path.is_empty() || path_currencies.len() != path.len() + 1 {
      return None;
   }
   let mut current = amount_in;
   let mut steps = Vec::with_capacity(path.len());
   for i in 0..path.len() {
      if current.is_zero() {
         return None;
      }
      let out = path[i].simulate_swap(&path_currencies[i], current).ok()?;
      if out.is_zero() {
         return None;
      }
      steps.push((current, out));
      current = out;
   }
   Some((current, steps))
}

fn estimate_gas_cost_for_route(
   eth_price: &NumericValue,
   base_fee: u64,
   priority_fee: U256,
   num_hops: usize,
) -> (NumericValue, u64) {
   if num_hops == 0 {
      return (NumericValue::default(), 0);
   }
   let total_gas = BASE_GAS + HOP_GAS * (num_hops as u64 - 1);
   let gas_price_wei = U256::from(base_fee) + priority_fee;
   let cost_in_wei = gas_price_wei * U256::from(total_gas);
   let cost_eth = NumericValue::format_wei(cost_in_wei, 18);
   let cost_in_usd = NumericValue::from_f64(cost_eth.f64() * eth_price.f64());
   (cost_in_usd, total_gas)
}

fn sort_routes(routes: &mut [EvaluatedRoute], currency_out_price: &NumericValue) {
   routes.sort_by(|a, b| {
      b.net_value(currency_out_price)
         .partial_cmp(&a.net_value(currency_out_price))
         .unwrap_or(Ordering::Equal)
         .then_with(|| a.hop_count().cmp(&b.hop_count()))
   });
}

fn pick_better(
   a: Option<EvaluatedRoute>,
   b: Option<EvaluatedRoute>,
   currency_out_price: &NumericValue,
) -> Option<EvaluatedRoute> {
   match (a, b) {
      (None, None) => None,
      (Some(x), None) => Some(x),
      (None, Some(y)) => Some(y),
      (Some(x), Some(y)) => {
         if y.net_value(currency_out_price) > x.net_value(currency_out_price) {
            Some(y)
         } else {
            Some(x)
         }
      }
   }
}

fn direct_impact_percent(route: &EvaluatedRoute) -> Option<f64> {
   if route.pools.len() != 1 {
      return None;
   }
   let pool = &route.pools[0];
   let cin = route.path_currencies.first()?;
   let spot = pool.calculate_price(cin).ok()?;
   let fee_fraction = pool.fee().fee_percent() as f64 / 100.0;
   let ideal = route.amount_in.f64() * (1.0 - fee_fraction) * spot;
   if ideal <= 0.0 {
      return None;
   }
   Some((1.0 - (route.amount_out.f64() / ideal)) * 100.0)
}

fn split_direct_pools(
   directs: &[EvaluatedRoute],
   amount_to_swap: &NumericValue,
   currency_in: &Currency,
   currency_out: &Currency,
   eth_price: &NumericValue,
   currency_out_price: &NumericValue,
   base_fee: u64,
   priority_fee: U256,
   max_split_routes: usize,
   best_single: Option<EvaluatedRoute>,
) -> Quote {
   let total = amount_to_swap.wei();
   let chunk = total / U256::from(SPLIT_CHUNKS);
   if chunk.is_zero() {
      return best_single
         .map(|r| build_quote_from_route(r, currency_in.clone(), currency_out.clone()))
         .unwrap_or_default();
   }

   // Rank at chunk size so a fee-tier that is bad at 100% still gets a slice.
   let mut ranked: Vec<(usize, U256)> = directs
      .iter()
      .enumerate()
      .filter_map(|(i, route)| {
         let out = simulate_path_with_steps(&route.pools, &route.path_currencies, chunk)?.0;
         Some((i, out))
      })
      .collect();
   ranked.sort_by(|a, b| b.1.cmp(&a.1));
   ranked.truncate(max_split_routes);

   if ranked.len() < 2 {
      return best_single
         .map(|r| build_quote_from_route(r, currency_in.clone(), currency_out.clone()))
         .unwrap_or_default();
   }

   let candidates: Vec<&EvaluatedRoute> = ranked.iter().map(|(i, _)| &directs[*i]).collect();
   let n = candidates.len();
   let mut allocations = vec![U256::ZERO; n];
   let mut current_output = vec![U256::ZERO; n];

   let initial: Vec<MarginalGain> = candidates
      .iter()
      .enumerate()
      .map(|(i, route)| MarginalGain {
         gain: simulate_path_with_steps(&route.pools, &route.path_currencies, chunk)
            .map(|(out, _)| out)
            .unwrap_or_default(),
         route_index: i,
      })
      .collect();
   let mut heap = BinaryHeap::from(initial);

   for _ in 0..SPLIT_CHUNKS {
      let Some(MarginalGain { gain, route_index }) = heap.pop() else {
         break;
      };
      if gain.is_zero() {
         continue;
      }
      allocations[route_index] += chunk;
      current_output[route_index] += gain;

      let route = candidates[route_index];
      let next_output = simulate_path_with_steps(
         &route.pools,
         &route.path_currencies,
         allocations[route_index] + chunk,
      )
      .map(|(out, _)| out)
      .unwrap_or_default();

      heap.push(MarginalGain {
         gain: next_output.saturating_sub(current_output[route_index]),
         route_index,
      });
   }

   let leftover = total.saturating_sub(chunk * U256::from(SPLIT_CHUNKS));
   if !leftover.is_zero() {
      if let Some((best_i, _)) = allocations.iter().enumerate().max_by_key(|(_, amt)| *amt) {
         allocations[best_i] += leftover;
      }
   }

   let used: Vec<(usize, U256)> = allocations
      .iter()
      .copied()
      .enumerate()
      .filter(|(_, amt)| !amt.is_zero())
      .collect();

   if used.len() < 2 {
      return best_single
         .map(|r| build_quote_from_route(r, currency_in.clone(), currency_out.clone()))
         .unwrap_or_default();
   }

   let mut swap_steps = Vec::new();
   let mut total_out = U256::ZERO;
   for (i, amt) in &used {
      let route = candidates[*i];
      let Some((out, steps)) = simulate_path_with_steps(&route.pools, &route.path_currencies, *amt)
      else {
         continue;
      };
      total_out += out;
      swap_steps.extend(steps_to_swap_steps(route, &steps));
   }

   if swap_steps.is_empty() || total_out.is_zero() {
      return best_single
         .map(|r| build_quote_from_route(r, currency_in.clone(), currency_out.clone()))
         .unwrap_or_default();
   }

   let split_out = NumericValue::format_wei(total_out, currency_out.decimals());
   let (split_gas_usd, _) =
      estimate_gas_cost_for_route(eth_price, base_fee, priority_fee, used.len());
   let split_net = split_out.f64() * currency_out_price.f64() - split_gas_usd.f64();

   if let Some(single) = best_single {
      if single.net_value(currency_out_price) >= split_net {
         return build_quote_from_route(single, currency_in.clone(), currency_out.clone());
      }
   }

   Quote {
      currency_in: currency_in.clone(),
      currency_out: currency_out.clone(),
      amount_in: amount_to_swap.clone(),
      amount_out: split_out,
      swap_steps,
   }
}

fn steps_to_swap_steps(
   route: &EvaluatedRoute,
   steps: &[(U256, U256)],
) -> Vec<SwapStep<AnyUniswapPool>> {
   steps
      .iter()
      .enumerate()
      .map(|(j, (amt_in, amt_out))| {
         let cin = &route.path_currencies[j];
         let cout = &route.path_currencies[j + 1];
         SwapStep {
            pool: (*route.pools[j]).clone(),
            currency_in: cin.clone(),
            currency_out: cout.clone(),
            amount_in: NumericValue::format_wei(*amt_in, cin.decimals()),
            amount_out: NumericValue::format_wei(*amt_out, cout.decimals()),
         }
      })
      .collect()
}

fn build_quote_from_route(
   route: EvaluatedRoute,
   currency_in: Currency,
   currency_out: Currency,
) -> Quote {
   let swap_steps = steps_to_swap_steps(&route, &route.step_amounts);
   Quote {
      currency_in,
      currency_out,
      amount_in: route.amount_in,
      amount_out: route.amount_out,
      swap_steps,
   }
}

fn same_asset(a: &Currency, b: &Currency) -> bool {
   if a == b {
      return true;
   }
   a.chain_id() == b.chain_id() && a.is_weth_or_eth() && b.is_weth_or_eth()
}

fn pool_currency_for(pool: &AnyUniswapPool, currency: &Currency) -> Option<Currency> {
   let c0 = pool.currency0();
   let c1 = pool.currency1();
   if same_asset(currency, c0) {
      Some(c0.clone())
   } else if same_asset(currency, c1) {
      Some(c1.clone())
   } else {
      None
   }
}

fn other_currency(pool: &AnyUniswapPool, currency: &Currency) -> Option<Currency> {
   let c0 = pool.currency0();
   let c1 = pool.currency1();
   if same_asset(currency, c0) {
      Some(c1.clone())
   } else if same_asset(currency, c1) {
      Some(c0.clone())
   } else {
      None
   }
}

fn pool_key(
   pool: &AnyUniswapPool,
) -> (
   u64,
   zeus_eth::alloy_primitives::Address,
   zeus_eth::alloy_primitives::B256,
) {
   (pool.chain_id(), pool.address(), pool.id())
}

#[cfg(test)]
mod tests {
   use super::*;
   use zeus_eth::{
      alloy_primitives::{Address, U256, address},
      amm::uniswap::{DexKind, State, UniswapV2Pool, state::PoolReserves},
      currency::{Currency, ERC20Token, NativeCurrency},
   };

   fn token(addr: Address, symbol: &str, decimals: u8) -> ERC20Token {
      ERC20Token {
         chain_id: 1,
         address: addr,
         decimals,
         symbol: symbol.into(),
         name: symbol.into(),
         total_supply: U256::ZERO,
      }
   }

   fn v2_pool(
      addr: Address,
      a: ERC20Token,
      b: ERC20Token,
      reserve_a: U256,
      reserve_b: U256,
   ) -> AnyUniswapPool {
      let mut pool = UniswapV2Pool::new(1, addr, a.clone(), b.clone(), DexKind::UniswapV2);
      let (r0, r1) = if pool.currency0().address() == a.address {
         (reserve_a, reserve_b)
      } else {
         (reserve_b, reserve_a)
      };
      pool.set_state(State::v2(PoolReserves::new(r0, r1, 0)));
      pool.into()
   }

   fn eth() -> Currency {
      Currency::from(NativeCurrency::from(1u64))
   }

   fn weth() -> ERC20Token {
      ERC20Token::weth()
   }

   fn usdt() -> ERC20Token {
      ERC20Token::usdt()
   }

   fn usdc() -> ERC20Token {
      ERC20Token::usdc()
   }

   fn uni() -> ERC20Token {
      token(
         address!("1f9840a85d5aF5bf1D1762F925BDADdC4201F984"),
         "UNI",
         18,
      )
   }

   fn wei18(s: &str) -> U256 {
      NumericValue::parse_to_wei(s, 18).wei()
   }

   fn wei6(s: &str) -> U256 {
      NumericValue::parse_to_wei(s, 6).wei()
   }

   fn eth_usdt_pool(addr: Address, eth_reserve: &str, usdt_reserve: &str) -> AnyUniswapPool {
      v2_pool(
         addr,
         weth(),
         usdt(),
         wei18(eth_reserve),
         wei6(usdt_reserve),
      )
   }

   fn quote_single(
      amount: NumericValue,
      cin: Currency,
      cout: Currency,
      pools: Vec<AnyUniswapPool>,
      max_hops: usize,
   ) -> Quote {
      get_quote(
         amount,
         cin,
         cout,
         pools,
         NumericValue::from_f64(2000.0),
         NumericValue::from_f64(1.0),
         1_000_000_000,
         U256::from(1_000_000_000u64),
         max_hops,
      )
   }

   fn quote_split(
      amount: NumericValue,
      cin: Currency,
      cout: Currency,
      pools: Vec<AnyUniswapPool>,
      max_hops: usize,
      max_routes: usize,
   ) -> Quote {
      get_quote_with_split_routing(
         amount,
         cin,
         cout,
         pools,
         NumericValue::from_f64(2000.0),
         NumericValue::from_f64(1.0),
         1_000_000_000,
         U256::from(1_000_000_000u64),
         max_hops,
         max_routes,
      )
   }

   #[test]
   fn native_eth_uses_weth_direct_pool() {
      let pool = eth_usdt_pool(
         address!("1111111111111111111111111111111111111111"),
         "10000",
         "20000000",
      );
      let amount = NumericValue::parse_to_wei("1", 18);
      let quote = quote_single(
         amount,
         eth(),
         Currency::from(usdt()),
         vec![pool],
         2,
      );
      assert_eq!(quote.swap_steps.len(), 1);
      assert!(quote.swap_steps[0].currency_in.is_native_wrapped());
      assert!(!quote.amount_out.is_zero());
   }

   #[test]
   fn prefers_direct_over_two_hop() {
      let direct = eth_usdt_pool(
         address!("1111111111111111111111111111111111111111"),
         "10000",
         "20000000",
      );
      let weth_usdc = v2_pool(
         address!("2222222222222222222222222222222222222222"),
         weth(),
         usdc(),
         wei18("10"),
         wei6("20000"),
      );
      let usdc_usdt = v2_pool(
         address!("3333333333333333333333333333333333333333"),
         usdc(),
         usdt(),
         wei6("20000"),
         wei6("20000"),
      );
      let amount = NumericValue::parse_to_wei("1", 18);
      let quote = quote_single(
         amount,
         eth(),
         Currency::from(usdt()),
         vec![direct.clone(), weth_usdc, usdc_usdt],
         2,
      );
      assert_eq!(quote.swap_steps.len(), 1);
      assert_eq!(
         quote.swap_steps[0].pool.address(),
         direct.address()
      );
   }

   #[test]
   fn two_hop_via_base_when_no_direct() {
      let uni_weth = v2_pool(
         address!("1111111111111111111111111111111111111111"),
         uni(),
         weth(),
         wei18("100000"),
         wei18("1000"),
      );
      let weth_usdt = eth_usdt_pool(
         address!("2222222222222222222222222222222222222222"),
         "10000",
         "20000000",
      );
      let amount = NumericValue::parse_to_wei("100", 18);
      let quote = quote_single(
         amount,
         Currency::from(uni()),
         Currency::from(usdt()),
         vec![uni_weth, weth_usdt],
         2,
      );
      assert_eq!(quote.swap_steps.len(), 2);
      assert!(!quote.amount_out.is_zero());
   }

   #[test]
   fn max_hops_one_does_not_use_two_hop() {
      let uni_weth = v2_pool(
         address!("1111111111111111111111111111111111111111"),
         uni(),
         weth(),
         wei18("100000"),
         wei18("1000"),
      );
      let weth_usdt = eth_usdt_pool(
         address!("2222222222222222222222222222222222222222"),
         "10000",
         "20000000",
      );
      let amount = NumericValue::parse_to_wei("100", 18);
      let quote = quote_single(
         amount,
         Currency::from(uni()),
         Currency::from(usdt()),
         vec![uni_weth, weth_usdt],
         1,
      );
      assert!(quote.swap_steps.is_empty());
      assert!(quote.amount_out.is_zero());
   }

   #[test]
   fn tiny_swap_does_not_split_across_equal_pools() {
      let a = eth_usdt_pool(
         address!("1111111111111111111111111111111111111111"),
         "10000",
         "20000000",
      );
      let b = eth_usdt_pool(
         address!("2222222222222222222222222222222222222222"),
         "10000",
         "20000000",
      );
      let amount = NumericValue::parse_to_wei("0.01", 18);
      let quote = quote_split(
         amount,
         eth(),
         Currency::from(usdt()),
         vec![a, b],
         2,
         5,
      );
      assert_eq!(quote.swap_steps.len(), 1);
      assert!(!quote.amount_out.is_zero());
   }

   #[test]
   fn large_swap_splits_across_direct_pools() {
      let a = eth_usdt_pool(
         address!("1111111111111111111111111111111111111111"),
         "200",
         "400000",
      );
      let b = eth_usdt_pool(
         address!("2222222222222222222222222222222222222222"),
         "200",
         "400000",
      );
      let amount = NumericValue::parse_to_wei("100", 18);
      let single = quote_single(
         amount.clone(),
         eth(),
         Currency::from(usdt()),
         vec![a.clone()],
         2,
      );
      let split = quote_split(
         amount,
         eth(),
         Currency::from(usdt()),
         vec![a, b],
         2,
         5,
      );
      assert_eq!(split.swap_steps.len(), 2);
      assert!(
         split.amount_out.wei() > single.amount_out.wei(),
         "split {} vs single {}",
         split.amount_out.wei(),
         single.amount_out.wei()
      );
   }
}
