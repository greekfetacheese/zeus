use std::path::PathBuf;
use std::time::Instant;

use alloy_provider::{Provider, ProviderBuilder};
use anyhow::{Context, anyhow};
use clap::{Parser, ValueEnum};
use tracing::info;
use url::Url;
use zeus_railgun::{
   ChainConfig, RpcSyncer, SnapshotLoader, SubsquidSyncer, UtxoSyncer,
   indexer::syncer::snapshot::EventsSnapshot,
};

/// Generate `events-snapshot:{chain}.data` / `.meta` for Zeus Railgun sync.
///
/// Compiles independently of the Zeus GUI (`cargo build -p zeus-railgun-snapshot`).
/// Resume-safe: an existing blob is extended from its covered tip.
#[derive(Parser, Debug)]
#[command(name = "railgun-snapshot", version, about)]
struct Args {
   /// EIP-155 chain id or alias (`1` / `mainnet`, `11155111` / `sepolia`)
   #[arg(long, default_value = "1", value_parser = parse_chain)]
   chain: u64,

   /// HTTP JSON-RPC URL (required for `--source rpc`; optional for Subsquid if `--to` is set)
   #[arg(long, env = "ETH_RPC_URL")]
   rpc: Option<String>,

   /// Directory for `events-snapshot:{chain}.data` and `.meta`
   #[arg(long, default_value = "data/railgun")]
   out: PathBuf,

   /// Where to pull historical events from
   #[arg(long, value_enum, default_value_t = Source::Rpc)]
   source: Source,

   /// Inclusive start block (default: Railgun smart-wallet deployment)
   #[arg(long)]
   from: Option<u64>,

   /// Inclusive end block (default: chain tip)
   #[arg(long)]
   to: Option<u64>,

   /// `eth_getLogs` chunk size (RPC source only)
   #[arg(long)]
   block_range: Option<u64>,

   /// Concurrent `eth_getLogs` chunks (RPC source only)
   #[arg(long)]
   concurrency: Option<usize>,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Source {
   Rpc,
   Subsquid,
}

fn parse_chain(s: &str) -> Result<u64, String> {
   match s.to_ascii_lowercase().as_str() {
      "mainnet" | "eth" | "ethereum" | "1" => Ok(1),
      "sepolia" | "11155111" => Ok(11155111),
      other => other.parse::<u64>().map_err(|e| format!("invalid chain `{other}`: {e}")),
   }
}

fn init_tracing() {
   let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
      tracing_subscriber::EnvFilter::new("info,zeus_railgun=debug,zeus_railgun_snapshot=info")
   });
   tracing_subscriber::fmt()
      .with_env_filter(filter)
      .with_target(true)
      .compact()
      .init();
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
   init_tracing();
   let args = Args::parse();

   let chain = ChainConfig::from_chain_id(args.chain).ok_or_else(|| {
      anyhow!(
         "chain {} is not a supported Railgun chain",
         args.chain
      )
   })?;

   std::fs::create_dir_all(&args.out)
      .with_context(|| format!("create output dir {}", args.out.display()))?;

   let loader = SnapshotLoader::new(args.out.clone());
   let existing = loader.load(chain.id).await?;
   if existing.block_number > 0 {
      info!(
         "Existing snapshot: {} events, coverage {}..{} in {}",
         existing.events.len(),
         existing.coverage_start,
         existing.block_number,
         args.out.display()
      );
   } else {
      info!(
         "No usable snapshot yet; seeding {} from deployment block {}",
         args.out.display(),
         chain.deployment_block
      );
   }

   let from_block = args.from.unwrap_or(chain.deployment_block);
   let to_block = resolve_to_block(&args, &chain).await?;
   if from_block > to_block {
      return Err(anyhow!(
         "from_block {from_block} is past to_block {to_block}"
      ));
   }

   info!(
      "Generating snapshot chain={} source={:?} {}..{} wallet={}",
      chain.id, args.source, from_block, to_block, chain.railgun_smart_wallet
   );

   let started = Instant::now();
   match args.source {
      Source::Rpc => {
         let rpc = args
            .rpc
            .as_deref()
            .ok_or_else(|| anyhow!("--rpc (or ETH_RPC_URL) is required for --source rpc"))?;
         let url = Url::parse(rpc).with_context(|| format!("parse RPC url `{rpc}`"))?;
         let provider = ProviderBuilder::new().connect_http(url);
         let syncer = RpcSyncer::new(provider, chain.id, chain.railgun_smart_wallet)
            .with_snapshot_loader(loader.clone());
         if let Some(range) = args.block_range {
            UtxoSyncer::set_block_range(&syncer, range).await;
         }
         if let Some(n) = args.concurrency {
            UtxoSyncer::set_concurrency(&syncer, n).await;
         }
         let events = UtxoSyncer::sync(&syncer, from_block, to_block).await?;
         info!(
            "RPC sync returned {} events in range",
            events.len()
         );
      }
      Source::Subsquid => {
         let syncer = SubsquidSyncer::new(&chain.subsquid_endpoint, chain.id)
            .with_snapshot_loader(loader.clone());
         let events = UtxoSyncer::sync(&syncer, from_block, to_block).await?;
         info!(
            "Subsquid sync returned {} events in range",
            events.len()
         );
      }
   }

   let snapshot = loader.load(chain.id).await?;
   summarize(&args.out, chain.id, &snapshot, started)?;
   Ok(())
}

async fn resolve_to_block(args: &Args, chain: &ChainConfig) -> anyhow::Result<u64> {
   if let Some(to) = args.to {
      return Ok(to);
   }

   match args.source {
      Source::Rpc => {
         let rpc = args
            .rpc
            .as_deref()
            .ok_or_else(|| anyhow!("--rpc (or ETH_RPC_URL) is required to resolve chain tip"))?;
         let url = Url::parse(rpc).with_context(|| format!("parse RPC url `{rpc}`"))?;
         let provider = ProviderBuilder::new().connect_http(url);
         let tip = provider.get_block_number().await.context("eth_blockNumber")?;
         Ok(tip)
      }
      Source::Subsquid => {
         let syncer = SubsquidSyncer::new(&chain.subsquid_endpoint, chain.id);
         UtxoSyncer::latest_block(&syncer).await.context("Subsquid latest block")
      }
   }
}

fn summarize(
   out: &PathBuf,
   chain_id: u64,
   snapshot: &EventsSnapshot,
   started: Instant,
) -> anyhow::Result<()> {
   if snapshot.events.is_empty() || snapshot.block_number == 0 {
      return Err(anyhow!(
         "sync finished but snapshot is empty — check RPC/Subsquid coverage for this range"
      ));
   }

   let data_path = out.join(format!("events-snapshot:{chain_id}.data"));
   let meta_path = out.join(format!("events-snapshot:{chain_id}.meta"));
   let data_len = std::fs::metadata(&data_path).map(|m| m.len()).unwrap_or(0);
   let meta_len = std::fs::metadata(&meta_path).map(|m| m.len()).unwrap_or(0);

   info!(
      "Wrote {} events, coverage {}..{}, {:.1}s",
      snapshot.events.len(),
      snapshot.coverage_start,
      snapshot.block_number,
      started.elapsed().as_secs_f64()
   );
   info!(
      "{} ({} bytes), {} ({} bytes)",
      data_path.display(),
      data_len,
      meta_path.display(),
      meta_len
   );
   Ok(())
}
