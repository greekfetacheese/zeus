mod chains;
mod download;
mod make_token_data;
mod remove_garbage;
mod resize_icons;
mod token_data;

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use anyhow::Context;
use clap::Parser;
use tracing::info;
use zeus_eth::types::ChainId;

use chains::{parse_chain, require_chain, token_chains};
use download::{DEFAULT_REF, DEFAULT_REPO, download_assets};
use make_token_data::{DEFAULT_MAX_CONCURRENT_TOKENS, make_token_data, write_token_blob};
use remove_garbage::remove_garbage;
use resize_icons::resize_icons;

/// Generate Zeus `token_data.data` from Trust Wallet assets.
///
/// Compiles independently of the Zeus GUI (`cargo build -p zeus-tokens`).
/// Pipeline (same as the original util): download → remove garbage → resize icons → make token data.
#[derive(Parser, Debug)]
#[command(name = "zeus-tokens", version, about)]
struct Args {
   /// EIP-155 chain id or alias. Repeatable. Default: ethereum, optimism, binance, base, arbitrum
   #[arg(long, value_parser = parse_chain)]
   chain: Vec<u64>,

   /// Directory for the Trust Wallet sparse clone (or a pre-downloaded tree)
   #[arg(long, default_value = "token_data")]
   work_dir: PathBuf,

   /// Output blob path (`include_bytes!` target in the Zeus app)
   #[arg(long, default_value = "embedded/token_data.data")]
   out: PathBuf,

   /// Skip git sparse-clone and use `--work-dir` as-is
   #[arg(long)]
   skip_download: bool,

   /// Delete `--work-dir` and clone Trust Wallet assets again
   #[arg(long)]
   force_download: bool,

   /// Trust Wallet assets git URL
   #[arg(long, default_value = DEFAULT_REPO)]
   repo: String,

   /// Git ref / branch (Trust Wallet uses `master`)
   #[arg(long, default_value = DEFAULT_REF)]
   git_ref: String,

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

   /// Concurrent token liquidity checks
   #[arg(long, default_value_t = DEFAULT_MAX_CONCURRENT_TOKENS)]
   concurrency: usize,
}

fn init_tracing() {
   let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
      tracing_subscriber::EnvFilter::new("info,zeus_eth=info,zeus_tokens=info")
   });
   tracing_subscriber::fmt()
      .with_env_filter(filter)
      .with_target(true)
      .compact()
      .init();
}

fn resolve_chains(args: &Args) -> anyhow::Result<Vec<ChainId>> {
   if args.chain.is_empty() {
      return Ok(token_chains());
   }
   args.chain.iter().copied().map(require_chain).collect()
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
   init_tracing();
   let args = Args::parse();
   let chains = resolve_chains(&args)?;
   let started = Instant::now();

   info!(
      "Generating token list for chains {:?}",
      chains.iter().map(|c| c.id()).collect::<Vec<_>>()
   );

   if args.skip_download && args.force_download {
      anyhow::bail!("--skip-download and --force-download cannot be used together");
   }

   if !args.skip_download {
      download_assets(
         &args.work_dir,
         &args.repo,
         &args.git_ref,
         &chains,
         args.force_download,
      )
      .context("download Trust Wallet assets")?;
   } else {
      info!(
         "Skipping download; using assets in {}",
         args.work_dir.display()
      );
   }

   remove_garbage(&args.work_dir, &chains).context("remove garbage")?;
   resize_icons(&args.work_dir, &chains).context("resize icons")?;

   let tokens = make_token_data(
      &args.work_dir,
      &chains,
      &rpc_map(&args),
      args.concurrency,
   )
   .await
   .context("make token data")?;

   write_token_blob(&args.out, &tokens)?;
   info!("Done in {:.1}s", started.elapsed().as_secs_f64());
   Ok(())
}
