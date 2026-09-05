use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use tracing::info;
use zeus_eth::amm::uniswap::{AnyUniswapPool, DexKind, sync::Checkpoint};

use crate::manager::PoolManager;

/// Unfiltered historical Uniswap pools for one chain. Never deleted by this CLI.
#[derive(Clone, Serialize, Deserialize)]
pub struct ChainSnapshot {
   pub chain_id: u64,
   pub pools: Vec<AnyUniswapPool>,
   pub checkpoints: Vec<Checkpoint>,
}

impl ChainSnapshot {
   pub fn new(chain_id: u64) -> Self {
      Self {
         chain_id,
         pools: Vec::new(),
         checkpoints: Vec::new(),
      }
   }

   pub fn path(dir: &Path, chain_id: u64) -> PathBuf {
      dir.join(format!("pools:{chain_id}.json"))
   }

   pub fn load_or_new(dir: &Path, chain_id: u64) -> anyhow::Result<Self> {
      let path = Self::path(dir, chain_id);
      if !path.exists() {
         return Ok(Self::new(chain_id));
      }
      let data = std::fs::read_to_string(&path)
         .with_context(|| format!("read snapshot {}", path.display()))?;
      let snap: Self = serde_json::from_str(&data)
         .with_context(|| format!("parse snapshot {}", path.display()))?;
      if snap.chain_id != chain_id {
         anyhow::bail!(
            "snapshot {} has chain_id {}, expected {chain_id}",
            path.display(),
            snap.chain_id
         );
      }
      info!(
         "Loaded snapshot {}: {} pools, {} checkpoints",
         path.display(),
         snap.pools.len(),
         snap.checkpoints.len()
      );
      Ok(snap)
   }

   /// Persist without deleting the previous file (atomic replace).
   pub fn save(&self, dir: &Path) -> anyhow::Result<()> {
      std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
      let path = Self::path(dir, self.chain_id);
      write_json_atomic(&path, self)
         .with_context(|| format!("write snapshot {}", path.display()))?;
      info!(
         "Wrote snapshot {} ({} pools, {} checkpoints)",
         path.display(),
         self.pools.len(),
         self.checkpoints.len()
      );
      Ok(())
   }

   pub fn checkpoint(&self, dex: DexKind) -> Option<&Checkpoint> {
      self.checkpoints.iter().find(|c| c.dex == dex && c.chain_id == self.chain_id)
   }

   pub fn merge_pools(&mut self, pools: Vec<AnyUniswapPool>) {
      let mut map: HashMap<_, AnyUniswapPool> =
         self.pools.drain(..).map(|p| (PoolManager::pool_key(&p), p)).collect();
      for pool in pools {
         map.insert(PoolManager::pool_key(&pool), pool);
      }
      self.pools = map.into_values().collect();
   }

   pub fn set_checkpoint(&mut self, checkpoint: Checkpoint) {
      if let Some(existing) = self
         .checkpoints
         .iter_mut()
         .find(|c| c.dex == checkpoint.dex && c.chain_id == checkpoint.chain_id)
      {
         *existing = checkpoint;
      } else {
         self.checkpoints.push(checkpoint);
      }
   }
}

pub fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> anyhow::Result<()> {
   if let Some(parent) = path.parent() {
      if !parent.as_os_str().is_empty() {
         std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
      }
   }
   let tmp = path.with_extension("json.tmp");
   let bytes = serde_json::to_vec(value)?;
   std::fs::write(&tmp, &bytes).with_context(|| format!("write {}", tmp.display()))?;
   std::fs::rename(&tmp, path)
      .with_context(|| format!("rename {} -> {}", tmp.display(), path.display()))?;
   Ok(())
}

#[cfg(test)]
mod tests {
   use super::*;
   use zeus_eth::amm::uniswap::{DexKind, UniswapPool, UniswapV2Pool};

   #[test]
   fn save_load_and_merge_does_not_drop_old_pools() {
      let tmp = tempfile::tempdir().unwrap();
      let pool = UniswapV2Pool::weth_uni();
      let mut snap = ChainSnapshot::new(pool.chain_id());
      snap.merge_pools(vec![pool.clone().into()]);
      snap.set_checkpoint(Checkpoint::new(
         pool.chain_id(),
         100,
         DexKind::UniswapV2,
      ));
      snap.save(tmp.path()).unwrap();

      let mut loaded = ChainSnapshot::load_or_new(tmp.path(), pool.chain_id()).unwrap();
      assert_eq!(loaded.pools.len(), 1);
      loaded.set_checkpoint(Checkpoint::new(
         pool.chain_id(),
         200,
         DexKind::UniswapV2,
      ));
      loaded.save(tmp.path()).unwrap();

      let again = ChainSnapshot::load_or_new(tmp.path(), pool.chain_id()).unwrap();
      assert_eq!(again.pools.len(), 1);
      assert_eq!(
         again.checkpoint(DexKind::UniswapV2).unwrap().block,
         200
      );
      assert!(tmp.path().join(format!("pools:{}.json", pool.chain_id())).exists());
   }
}
