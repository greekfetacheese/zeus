use tracing::{info, warn};
use zeus_eth::{
   amm::uniswap::{
      DexKind,
      sync::{SyncConfig, sync_pools},
   },
   types::ChainId,
   utils::client::RpcClient,
};

use std::path::Path;

use crate::snapshot::ChainSnapshot;

pub const DEFAULT_CONCURRENCY: usize = 4;
pub const DEFAULT_BATCH_SIZE: usize = 30;
pub const DEFAULT_BLOCK_RANGE: u64 = 5_000;

/// Uniswap V2/V3/V4 that exist on `chain` (Pancake is excluded).
pub fn uniswap_dexes(chain_id: u64) -> Vec<DexKind> {
   let mut dexes: Vec<DexKind> =
      DexKind::all(chain_id).into_iter().filter(|d| d.is_uniswap()).collect();

   if !dexes.iter().any(|d| d.is_uniswap_v4())
      && DexKind::UniswapV4.creation_block(chain_id).is_ok()
   {
      dexes.push(DexKind::UniswapV4);
   }

   dexes.retain(|d| match d.creation_block(chain_id) {
      Ok(_) => true,
      Err(e) => {
         warn!(
            "Skipping {} on chain {chain_id}: no creation block ({e})",
            d.as_str()
         );
         false
      }
   });
   dexes
}

pub fn parse_dex(s: &str) -> Result<DexKind, String> {
   match s.to_ascii_lowercase().as_str() {
      "v2" | "uniswap-v2" | "uniswapv2" | "uniswap_v2" => Ok(DexKind::UniswapV2),
      "v3" | "uniswap-v3" | "uniswapv3" | "uniswap_v3" => Ok(DexKind::UniswapV3),
      "v4" | "uniswap-v4" | "uniswapv4" | "uniswap_v4" => Ok(DexKind::UniswapV4),
      other => Err(format!(
         "invalid dex `{other}` (expected v2 / v3 / v4, or uniswap-v2 / uniswap-v3 / uniswap-v4)"
      )),
   }
}

/// Keep only requested Uniswap dexes that have a creation block on `chain`.
pub fn resolve_dexes_for_chain(chain_id: u64, requested: &[DexKind]) -> Vec<DexKind> {
   let available = uniswap_dexes(chain_id);
   requested
      .iter()
      .copied()
      .filter(|dex| {
         if available.contains(dex) {
            true
         } else {
            warn!(
               "Skipping {} on chain {chain_id}: not a supported Uniswap dex here",
               dex.as_str()
            );
            false
         }
      })
      .collect()
}

/// Fetch PairCreated / PoolCreated / Initialize logs from each dex's last
/// checkpoint (or factory deployment) to tip, merge into the unfiltered snapshot,
/// and save after every dex so a crash can resume.
pub async fn sync_chain(
   client: RpcClient,
   chain: ChainId,
   snapshot: &mut ChainSnapshot,
   snapshot_dir: &Path,
   dexes: &[DexKind],
   concurrency: usize,
   batch_size: usize,
   block_range: u64,
) -> anyhow::Result<()> {
   if dexes.is_empty() {
      anyhow::bail!("no Uniswap dexes to sync on chain {}", chain.id());
   }

   for &dex in dexes {
      let from_block = snapshot
         .checkpoint(dex)
         .map(|c| c.block)
         .unwrap_or(dex.creation_block(chain.id())?);

      info!(
         "Syncing {} on {} from block {from_block}",
         dex.as_str(),
         chain.name()
      );

      let config = SyncConfig::new(
         chain.id(),
         vec![dex],
         concurrency,
         batch_size,
         Some(from_block),
         None,
      );
      let results = sync_pools(client.clone(), config, block_range).await?;

      for res in results {
         info!(
            "{}: {} new pools, checkpoint block {}",
            res.checkpoint.dex.as_str(),
            res.pools.len(),
            res.checkpoint.block
         );
         snapshot.merge_pools(res.pools);
         snapshot.set_checkpoint(res.checkpoint);
      }
      snapshot.save(snapshot_dir)?;
   }

   Ok(())
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn parse_dex_aliases() {
      assert_eq!(parse_dex("v4").unwrap(), DexKind::UniswapV4);
      assert_eq!(
         parse_dex("uniswap-v2").unwrap(),
         DexKind::UniswapV2
      );
      assert_eq!(parse_dex("V3").unwrap(), DexKind::UniswapV3);
      assert!(parse_dex("pancake").is_err());
   }
}
