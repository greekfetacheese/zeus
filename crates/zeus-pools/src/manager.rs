use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use zeus_eth::{
   alloy_primitives::Address,
   amm::uniswap::{AnyUniswapPool, DexKind, UniswapPool, sync::Checkpoint},
   currency::Currency,
   types::{ARBITRUM, BASE, BSC, OPTIMISM},
};

use crate::serde_hashmap;

/// Key: (chain_id, tokenA, tokenB) -> UNIX timestamp in secs
type LastDiscovery = HashMap<(u64, Address, Address), u64>;

/// Key: (chain_id, dex_kind) -> UNIX timestamp in secs
type V4PoolLastDiscovery = HashMap<(u64, DexKind), u64>;

/// Key: (chain_id, dex_kind, fee, tokenA, tokenB) -> Pool
type Pools = HashMap<(u64, DexKind, u32, Currency, Currency), AnyUniswapPool>;

/// Key: (chain_id, dex) -> Checkpoint
type CheckpointMap = HashMap<(u64, DexKind), Checkpoint>;

type IgnoreChains = HashSet<u64>;

fn default_batch_size_for_updating_pool_state() -> usize {
   20
}

fn default_batch_size_for_discovering_pools() -> usize {
   30
}

fn default_concurrency() -> usize {
   1
}

fn default_discover_v4_pools() -> bool {
   false
}

fn default_ignore_chains() -> IgnoreChains {
   let mut chains = HashSet::new();
   chains.insert(BASE);
   chains.insert(OPTIMISM);
   chains.insert(BSC);
   chains.insert(ARBITRUM);
   chains
}

/// JSON layout must stay in lockstep with `zeus::core::context::pool_manager::PoolManager`
/// so the wallet can `include_str!` the blob.
#[derive(Clone, Serialize, Deserialize)]
pub struct PoolManager {
   #[serde(with = "serde_hashmap")]
   pub pools: Pools,

   #[serde(default, with = "serde_hashmap")]
   pub last_discover: LastDiscovery,

   #[serde(default, with = "serde_hashmap")]
   pub v4_pool_last_discover: V4PoolLastDiscovery,

   #[serde(with = "serde_hashmap")]
   pub checkpoints: CheckpointMap,

   #[serde(default = "default_concurrency")]
   pub concurrency: usize,

   #[serde(default = "default_batch_size_for_updating_pool_state")]
   pub batch_size_for_updating_pool_state: usize,

   #[serde(default = "default_batch_size_for_discovering_pools")]
   pub batch_size_for_discovering_pools: usize,

   #[serde(default = "default_discover_v4_pools")]
   pub discover_v4_pools: bool,

   #[serde(default = "default_ignore_chains")]
   pub ignore_chains: IgnoreChains,
}

impl Default for PoolManager {
   fn default() -> Self {
      Self {
         pools: HashMap::new(),
         last_discover: HashMap::new(),
         v4_pool_last_discover: HashMap::new(),
         checkpoints: HashMap::new(),
         concurrency: default_concurrency(),
         batch_size_for_updating_pool_state: default_batch_size_for_updating_pool_state(),
         batch_size_for_discovering_pools: default_batch_size_for_discovering_pools(),
         discover_v4_pools: default_discover_v4_pools(),
         ignore_chains: default_ignore_chains(),
      }
   }
}

impl PoolManager {
   pub fn add_pool(&mut self, pool: impl UniswapPool) {
      let any_pool = AnyUniswapPool::from_pool(pool);
      let key = (
         any_pool.chain_id(),
         any_pool.dex_kind(),
         any_pool.fee().fee(),
         any_pool.currency0().clone(),
         any_pool.currency1().clone(),
      );
      self.pools.insert(key, any_pool);
   }

   pub fn add_checkpoint(&mut self, chain: u64, dex: DexKind, checkpoint: Checkpoint) {
      self.checkpoints.insert((chain, dex), checkpoint);
   }

   pub fn pool_key(pool: &AnyUniswapPool) -> (u64, DexKind, u32, Currency, Currency) {
      (
         pool.chain_id(),
         pool.dex_kind(),
         pool.fee().fee(),
         pool.currency0().clone(),
         pool.currency1().clone(),
      )
   }
}

#[cfg(test)]
mod tests {
   use super::*;
   use zeus_eth::amm::uniswap::{UniswapPool, UniswapV2Pool};

   #[test]
   fn roundtrip_matches_wallet_hashmap_keys() {
      let mut manager = PoolManager::default();
      let pool = UniswapV2Pool::weth_uni();
      manager.add_pool(pool.clone());
      manager.add_checkpoint(
         pool.chain_id(),
         pool.dex_kind(),
         Checkpoint::new(pool.chain_id(), 12_345, pool.dex_kind()),
      );

      let json = serde_json::to_string(&manager).unwrap();
      let loaded: PoolManager = serde_json::from_str(&json).unwrap();
      assert_eq!(loaded.pools.len(), 1);
      assert_eq!(loaded.checkpoints.len(), 1);
      let got = loaded.pools.values().next().unwrap();
      assert_eq!(got.address(), pool.address());
   }
}
