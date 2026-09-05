mod filter;
mod manager;
mod serde_hashmap;
mod snapshot;
mod sync;

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use anyhow::Context;
use clap::Parser;
use tracing::info;
use zeus_eth::{
   amm::uniswap::{DexKind, UniswapPool},
   types::ChainId,
   utils::client::{get_client, retry_layer, throttle_layer},
};

use filter::{filter_liquid_pools, native_price, prefilter_pools};
use manager::PoolManager;
use snapshot::{ChainSnapshot, write_json_atomic};
use sync::{
   DEFAULT_BATCH_SIZE, DEFAULT_BLOCK_RANGE, DEFAULT_CONCURRENCY, parse_dex,
   resolve_dexes_for_chain, sync_chain,
};

/// Generate Zeus `pool_data.json` from historical Uniswap logs.
///
/// Compiles independently of the Zeus GUI (`cargo build -p zeus-pools`).
/// Pipeline: sync logs → save unfiltered per-chain snapshot → drop low-liquidity → encode.
#[derive(Parser, Debug)]
#[command(name = "zeus-pools", version, about)]
struct Args {
   /// EIP-155 chain id or alias. Repeatable. Default: ethereum, optimism, binance, base, arbitrum
   #[arg(long, value_parser = parse_chain)]
   chain: Vec<u64>,

   /// Uniswap dex to sync / filter / write into `pool_data.json`. Repeatable.
   /// Snapshot on disk is left unfiltered (other dexes stay). Default: v4
   #[arg(long, value_parser = parse_dex)]
   dex: Vec<DexKind>,

   /// Directory for unfiltered per-chain snapshots (`pools:{chain}.json`). Never deleted.
   #[arg(long, default_value = "pool_snapshots")]
   snapshot_dir: PathBuf,

   /// Output JSON (`include_str!` target in the Zeus app)
   #[arg(long, default_value = "embedded/pool_data.json")]
   out: PathBuf,

   /// Skip log sync and only filter + encode existing snapshots
   #[arg(long)]
   skip_sync: bool,

   /// Only keep pools that include a base token (WETH/USDC/USDT/DAI + WBTC + LINK).
   /// Pass `--base-tokens-only false` to disable. [default: true]
   #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
   base_tokens_only: bool,

   /// HTTP JSON-RPC URL for Ethereum
   #[arg(long, env = "ETHEREUM_RPC")]
   rpc_ethereum: Option<String>,

   /// HTTP JSON-RPC URL for Optimism
   #[arg(long, env = "OPTIMISM_RPC")]
   rpc_optimism: Option<String>,

   /// HTTP JSON-RPC URL for BNB Smart Chain
   #[arg(long, env = "BINANCE_RPC")]
   rpc_binance: Option<String>,

   /// HTTP JSON-RPC URL for Base
   #[arg(long, env = "BASE_RPC")]
   rpc_base: Option<String>,

   /// HTTP JSON-RPC URL for Arbitrum
   #[arg(long, env = "ARBITRUM_RPC")]
   rpc_arbitrum: Option<String>,

   /// Concurrent `eth_getLogs` / state-update chunks
   #[arg(long, default_value_t = DEFAULT_CONCURRENCY)]
   concurrency: usize,

   /// Pools constructed per log batch
   #[arg(long, default_value_t = DEFAULT_BATCH_SIZE)]
   batch_size: usize,

   /// `eth_getLogs` block chunk size
   #[arg(long, default_value_t = DEFAULT_BLOCK_RANGE)]
   block_range: u64,

   /// Batch size for `batch_update_state` during liquidity filter
   #[arg(long, default_value_t = 20)]
   state_batch_size: usize,
}

fn parse_chain(s: &str) -> Result<u64, String> {
   match s.to_ascii_lowercase().as_str() {
      "eth" | "ethereum" | "mainnet" | "1" => Ok(1),
      "op" | "optimism" | "10" => Ok(10),
      "bsc" | "binance" | "bnb" | "56" => Ok(56),
      "base" | "8453" => Ok(8453),
      "arb" | "arbitrum" | "42161" => Ok(42161),
      other => other.parse::<u64>().map_err(|e| format!("invalid chain `{other}`: {e}")),
   }
}

fn default_chains() -> Vec<ChainId> {
   [1, 10, 56, 8453, 42161]
      .into_iter()
      .map(|id| ChainId::new(id).expect("supported"))
      .collect()
}

fn resolve_dexes(args: &Args) -> Vec<DexKind> {
   if args.dex.is_empty() {
      vec![DexKind::UniswapV4]
   } else {
      let mut dexes = args.dex.clone();
      dexes.sort();
      dexes.dedup();
      dexes
   }
}

fn resolve_chains(args: &Args) -> anyhow::Result<Vec<ChainId>> {
   if args.chain.is_empty() {
      return Ok(default_chains());
   }
   args
      .chain
      .iter()
      .copied()
      .map(|id| {
         let chain = ChainId::new(id)?;
         if chain.is_eth_sepolia() {
            anyhow::bail!("sepolia is not part of the default Zeus pool list");
         }
         Ok(chain)
      })
      .collect()
}

fn rpc_map(args: &Args) -> HashMap<u64, String> {
   let mut map = HashMap::new();
   if let Some(url) = &args.rpc_ethereum {
      map.insert(1, url.clone());
   }
   if let Some(url) = &args.rpc_optimism {
      map.insert(10, url.clone());
   }
   if let Some(url) = &args.rpc_binance {
      map.insert(56, url.clone());
   }
   if let Some(url) = &args.rpc_base {
      map.insert(8453, url.clone());
   }
   if let Some(url) = &args.rpc_arbitrum {
      map.insert(42161, url.clone());
   }
   map
}

fn init_tracing() {
   let filter = tracing_subscriber::EnvFilter::try_from_default_env()
      .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,zeus_eth=info,zeus_pools=info"));
   tracing_subscriber::fmt()
      .with_env_filter(filter)
      .with_target(true)
      .compact()
      .init();
}

async fn rpc_client(url: &str) -> anyhow::Result<zeus_eth::utils::client::RpcClient> {
   let retry = retry_layer(10, 400, 330);
   let throttle = throttle_layer(10);
   get_client(url, retry, throttle, 120).await
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
   init_tracing();
   let args = Args::parse();
   let chains = resolve_chains(&args)?;
   let requested_dexes = resolve_dexes(&args);
   let rpcs = rpc_map(&args);
   let started = Instant::now();

   info!(
      "Generating Uniswap pool list for chains {:?} dexes {:?}",
      chains.iter().map(|c| c.id()).collect::<Vec<_>>(),
      requested_dexes.iter().map(|d| d.as_str()).collect::<Vec<_>>()
   );

   let mut manager = PoolManager::default();

   for chain in &chains {
      let url = rpcs.get(&chain.id()).ok_or_else(|| {
         anyhow::anyhow!(
            "missing RPC URL for chain {} ({})",
            chain.id(),
            chain.name()
         )
      })?;
      let client = rpc_client(url)
         .await
         .with_context(|| format!("connect RPC for {}", chain.name()))?;

      let mut snapshot = ChainSnapshot::load_or_new(&args.snapshot_dir, chain.id())?;
      let dexes = resolve_dexes_for_chain(chain.id(), &requested_dexes);
      if dexes.is_empty() {
         tracing::warn!(
            "No requested Uniswap dexes on {} — skipping",
            chain.name()
         );
         continue;
      }

      if !args.skip_sync {
         sync_chain(
            client.clone(),
            *chain,
            &mut snapshot,
            &args.snapshot_dir,
            &dexes,
            args.concurrency,
            args.batch_size,
            args.block_range,
         )
         .await
         .with_context(|| format!("sync Uniswap pools on {}", chain.name()))?;
         snapshot.save(&args.snapshot_dir)?;
      } else if snapshot.pools.is_empty() {
         anyhow::bail!(
            "no snapshot for chain {} in {} (run without --skip-sync first)",
            chain.id(),
            args.snapshot_dir.display()
         );
      }

      let selected: Vec<_> = snapshot
         .pools
         .iter()
         .filter(|p| dexes.contains(&p.dex_kind()))
         .cloned()
         .collect();
      info!(
         "Chain {}: processing {} / {} snapshot pools for {:?}",
         chain.id(),
         selected.len(),
         snapshot.pools.len(),
         dexes.iter().map(|d| d.as_str()).collect::<Vec<_>>()
      );

      let selected = prefilter_pools(*chain, selected, args.base_tokens_only);
      if selected.is_empty() {
         tracing::warn!(
            "Chain {}: nothing left after fee / base-token prefilter",
            chain.id()
         );
         for checkpoint in snapshot.checkpoints {
            if dexes.contains(&checkpoint.dex) {
               manager.add_checkpoint(checkpoint.chain_id, checkpoint.dex, checkpoint);
            }
         }
         continue;
      }

      let price = native_price(client.clone(), *chain)
         .await
         .with_context(|| format!("native price on {}", chain.name()))?;

      let filtered = filter_liquid_pools(
         client,
         *chain,
         selected,
         price,
         args.concurrency,
         args.state_batch_size,
      )
      .await
      .with_context(|| format!("filter pools on {}", chain.name()))?;

      for pool in filtered {
         manager.add_pool(pool);
      }
      for checkpoint in snapshot.checkpoints {
         if dexes.contains(&checkpoint.dex) {
            manager.add_checkpoint(checkpoint.chain_id, checkpoint.dex, checkpoint);
         }
      }
   }

   if manager.pools.is_empty() {
      anyhow::bail!("filtered pool list is empty — check RPCs, snapshots, and liquidity gate");
   }

   write_json_atomic(&args.out, &manager)
      .with_context(|| format!("write {}", args.out.display()))?;
   info!(
      "Wrote {} pools, {} checkpoints to {} in {:.1}s",
      manager.pools.len(),
      manager.checkpoints.len(),
      args.out.display(),
      started.elapsed().as_secs_f64()
   );
   Ok(())
}
