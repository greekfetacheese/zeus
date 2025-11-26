use crate::core::{BaseFee, ZeusCtx, context::Portfolio};
use crate::gui::SHARED_GUI;
use crate::utils::RT;
use anyhow::anyhow;

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;
use zeus_eth::{
   alloy_primitives::U256,
   alloy_provider::Provider,
   alloy_rpc_types::BlockId,
   currency::{Currency, ERC20Token},
   types::{ChainId, SUPPORTED_CHAINS},
   utils::{NumericValue, block::calculate_next_block_base_fee},
};

use self_update::backends::github::ReleaseList;
use self_update::{cargo_crate_version, self_replace::self_replace, update::Release};

use std::{env, io::Write};
use zip::ZipArchive;

const MEASURE_RPCS_INTERVAL: u64 = 200;
const WALLET_STATE_INTERVAL: u64 = 600;
const FEE_INTERVAL: u64 = 60;

pub async fn test_and_measure_rpcs(ctx: ZeusCtx) {
   let client = ctx.get_zeus_client();

   let mut tasks = Vec::new();
   let semaphore = Arc::new(Semaphore::new(5));

   let time = std::time::Instant::now();
   for chain in SUPPORTED_CHAINS {
      let rpcs = client.read(|rpcs| rpcs.get(&chain).unwrap_or(&vec![]).clone());

      for rpc in &rpcs {
         let client = client.clone();
         let semaphore = semaphore.clone();

         if rpc.should_run_check() {
            let rpc = rpc.clone();
            let ctx = ctx.clone();

            let task = RT.spawn(async move {
               let _permit = semaphore.acquire().await.unwrap();
               client.run_check_for(ctx, rpc).await;
            });
            tasks.push(task);
         } else {
            let rpc = rpc.clone();
            let task = RT.spawn(async move {
               let _permit = semaphore.acquire().await.unwrap();
               client.run_latency_check_for(rpc).await;
            });
            tasks.push(task);
         }
      }
   }

   for task in tasks {
      let _ = task.await;
   }

   client.sort_by_fastest();

   RT.spawn_blocking(move || {
      ctx.save_zeus_client();
   });

   tracing::info!(
      "RPC checks took {} secs",
      time.elapsed().as_secs_f32()
   );
}

pub async fn on_startup(ctx: ZeusCtx) {
   ctx.write(|ctx| {
      ctx.on_startup_syncing = true;
   });

   for chain in SUPPORTED_CHAINS {
      let ctx2 = ctx.clone();
      RT.spawn(async move {
         match update_priority_fee(ctx2, chain).await {
            Ok(_) => {
               tracing::trace!("Updated priority fee for chain: {}", chain)
            }
            Err(e) => tracing::error!("Error updating priority fee: {:?}", e),
         }
      });
   }

   let balance_manager = ctx.balance_manager();

   let eth_balance_fut = balance_manager.update_eth_balance_across_wallets_and_chains(ctx.clone());
   let token_balance_fut =
      balance_manager.update_tokens_balance_across_wallets_and_chains(ctx.clone());

   if ctx.pools_need_resync() {
      resync_pools(ctx.clone()).await;
   } else {
      update_token_prices(ctx.clone()).await;
   }

   eth_balance_fut.await;
   token_balance_fut.await;

   // Calculate the portfolio value for all chains
   let ctx_clone = ctx.clone();
   RT.spawn_blocking(move || {
      for chain in SUPPORTED_CHAINS {
         let ctx_clone = ctx_clone.clone();
         let portfolios = ctx_clone.read(|ctx| ctx.portfolio_db.get_all(chain));
         for portfolio in &portfolios {
            ctx_clone.calculate_portfolio_value(chain, portfolio.owner);
         }
      }
      ctx_clone.write(|ctx| {
         ctx.on_startup_syncing = false;
      });
   });

   // Update the base fee for all chains
   for chain in SUPPORTED_CHAINS {
      let ctx = ctx.clone();
      RT.spawn(async move {
         match get_base_fee(ctx.clone(), chain).await {
            Ok(_) => {
               tracing::trace!("Updated base fee for chain: {}", chain)
            }
            Err(e) => tracing::error!("Error updating base fee: {:?}", e),
         }
      });
   }

   let ctx_clone = ctx.clone();
   RT.spawn(async move {
      check_smart_account_status(ctx_clone).await;
   });

   let ctx_clone = ctx.clone();
   RT.spawn_blocking(move || {
      insert_missing_portfolios(ctx_clone);
   });

   let ctx_clone = ctx.clone();
   RT.spawn(async move {
      state_update_interval(ctx_clone).await;
   });

   RT.spawn_blocking(move || {
      ctx.save_all();
   });
}

fn insert_missing_portfolios(ctx: ZeusCtx) {
   while !ctx.vault_unlocked() {
      std::thread::sleep(Duration::from_millis(100));
   }

   let wallets = ctx.get_all_wallets_info();
   for chain in SUPPORTED_CHAINS {
      for wallet in &wallets {
         let has_portfolio = ctx.has_portfolio(chain, wallet.address);
         let balance = ctx.get_eth_balance(chain, wallet.address);
         if !balance.is_zero() && !has_portfolio {
            let portfolio = Portfolio::new(wallet.address, chain);
            ctx.write(|ctx| {
               ctx.portfolio_db.insert_portfolio(chain, wallet.address, portfolio);
            });
         }
      }
   }

   for chain in SUPPORTED_CHAINS {
      let portfolios = ctx.read(|ctx| ctx.portfolio_db.get_all(chain));
      for portfolio in &portfolios {
         ctx.calculate_portfolio_value(chain, portfolio.owner);
      }
   }
}

/// Check the smart account status for all wallets across all chains
async fn check_smart_account_status(ctx: ZeusCtx) {
   let accounts = ctx.get_all_wallets_info();
   let mut tasks = Vec::new();

   for chain in SUPPORTED_CHAINS {
      let ctx = ctx.clone();
      let accounts = accounts.clone();

      let task = RT.spawn(async move {
         for account in &accounts {
            if ctx.should_check_delegated_wallet_status(chain, account.address) {
               match ctx.check_delegated_wallet_status(chain, account.address).await {
                  Ok(_) => {}
                  Err(e) => tracing::error!("Error checking smart account status: {:?}", e),
               }
            }
         }
      });
      tasks.push(task);
   }

   for task in tasks {
      let _ = task.await;
   }
}

/// Update the token prices across all chains
async fn update_token_prices(ctx: ZeusCtx) {
   let mut tasks = Vec::new();
   for chain in SUPPORTED_CHAINS {
      let ctx = ctx.clone();
      let task = RT.spawn(async move {
         let price_manager = ctx.price_manager();
         let pool_manager = ctx.pool_manager();

         let portfolio_tokens = ctx.get_all_tokens_from_portfolios(chain);
         let mut inserted = HashSet::new();
         let mut tokens = Vec::new();

         for token in portfolio_tokens {
            if token.is_base() || inserted.contains(&token.address) {
               continue;
            }

            inserted.insert(token.address);
            tokens.push(token);
         }

         match price_manager.update_base_token_prices(ctx.clone(), chain).await {
            Ok(_) => tracing::trace!("Updated base token prices for chain: {}", chain),
            Err(e) => tracing::error!(
               "Error updating base token prices for chain {}: {:?}",
               chain,
               e
            ),
         }

         match price_manager.calculate_prices(ctx, chain, pool_manager, tokens).await {
            Ok(_) => tracing::trace!("Updated token prices for chain: {}", chain),
            Err(e) => tracing::error!(
               "Error updating token prices for chain {}: {:?}",
               chain,
               e
            ),
         }
      });
      tasks.push(task);
   }

   for task in tasks {
      let _ = task.await;
   }
}

async fn state_update_interval(ctx: ZeusCtx) {
   let mut wallet_state_passed = Instant::now();
   let mut fee_time_passed = Instant::now();
   let mut rpc_measure_time_passed = Instant::now();

   loop {
      if wallet_state_passed.elapsed().as_secs() > WALLET_STATE_INTERVAL {
         let manager = ctx.balance_manager();
         manager.update_eth_balance_across_wallets_and_chains(ctx.clone()).await;
         manager.update_tokens_balance_across_wallets_and_chains(ctx.clone()).await;
         update_token_prices(ctx.clone()).await;

         for chain in SUPPORTED_CHAINS {
            let portfolios = ctx.read(|ctx| ctx.portfolio_db.get_all(chain));
            for portfolio in &portfolios {
               ctx.calculate_portfolio_value(chain, portfolio.owner);
            }
         }

         check_smart_account_status(ctx.clone()).await;
         ctx.save_delegated_wallets();
         ctx.save_portfolio_db();
         ctx.save_price_manager();
         ctx.save_balance_manager();

         wallet_state_passed = Instant::now();
      }

      if fee_time_passed.elapsed().as_secs() > FEE_INTERVAL {
         for chain in SUPPORTED_CHAINS {
            match update_priority_fee(ctx.clone(), chain).await {
               Ok(_) => {
                  tracing::trace!("Updated priority fee for chain: {}", chain)
               }
               Err(e) => tracing::error!(
                  "Error updating priority fee for chain {}: {:?}",
                  chain,
                  e
               ),
            }

            match get_base_fee(ctx.clone(), chain).await {
               Ok(_) => {
                  tracing::trace!("Updated base fee for chain: {}", chain)
               }
               Err(e) => tracing::error!("Error updating base fee: {:?}", e),
            }
         }
         fee_time_passed = Instant::now();
      }

      if rpc_measure_time_passed.elapsed().as_secs() > MEASURE_RPCS_INTERVAL {
         let z_client = ctx.get_zeus_client();
         z_client.run_latency_checks().await;
         rpc_measure_time_passed = Instant::now();
      }

      tokio::time::sleep(Duration::from_secs(1)).await;
   }
}

pub async fn get_base_fee(ctx: ZeusCtx, chain: u64) -> Result<BaseFee, anyhow::Error> {
   let z_client = ctx.get_zeus_client();
   let chain = ChainId::new(chain)?;

   if chain.is_ethereum() {
      let block = z_client
         .request(chain.id(), |client| async move {
            client.get_block(BlockId::latest()).await.map_err(|e| anyhow!("{:?}", e))
         })
         .await?;

      if let Some(block) = block {
         let base_fee = block.header.base_fee_per_gas.unwrap_or_default();
         let next_base_fee = calculate_next_block_base_fee(block);
         ctx.update_base_fee(chain.id(), base_fee, next_base_fee);

         return Ok(BaseFee::new(base_fee, next_base_fee));
      } else {
         return Err(anyhow!("Latest block not found"));
      }
   }

   let gas_price = z_client
      .request(chain.id(), |client| async move {
         client.get_gas_price().await.map_err(|e| anyhow!("{:?}", e))
      })
      .await?;

   let fee: u64 = gas_price.try_into()?;
   ctx.update_base_fee(chain.id(), fee, fee);
   Ok(BaseFee::new(fee, fee))
}

pub async fn update_priority_fee(ctx: ZeusCtx, chain: u64) -> Result<(), anyhow::Error> {
   let z_client = ctx.get_zeus_client();
   let chain = ChainId::new(chain)?;
   if chain.is_ethereum() || chain.is_optimism() || chain.is_base() {
      let fee = z_client
         .request(chain.id(), |client| async move {
            client.get_max_priority_fee_per_gas().await.map_err(|e| anyhow!("{:?}", e))
         })
         .await?;

      let fee_value = NumericValue::format_to_gwei(U256::from(fee));

      if fee_value.is_zero() {
         return Err(anyhow!(
            "Rpc returned bad data, Fee (Wei) {} For Chain: {}",
            fee,
            chain.id()
         ));
      }

      ctx.update_priority_fee(chain.id(), fee_value);
   }
   Ok(())
}

/// If needed re-sync pools for all tokens across all chains
pub async fn resync_pools(ctx: ZeusCtx) {
   ctx.write(|ctx| {
      ctx.data_syncing = true;
   });

   tracing::info!("Resyncing pools");

   for chain in SUPPORTED_CHAINS {
      let ctx = ctx.clone();
      RT.spawn(async move {
         let mut tokens = ctx.get_all_tokens_from_portfolios(chain);
         let base_tokens = ERC20Token::base_tokens(chain);
         tokens.extend(base_tokens);

         let pool_manager = ctx.pool_manager();

         match pool_manager.sync_pools_for_tokens(ctx.clone(), chain, tokens).await {
            Ok(_) => {}
            Err(e) => tracing::error!(
               "Failed to sync pools for chain_id {} {}",
               chain,
               e
            ),
         }

         let portfolio_tokens = ctx.get_all_tokens_from_portfolios(chain);
         let mut currencies = Vec::new();
         let mut tokens = Vec::new();
         let mut inserted = HashSet::new();

         for token in &portfolio_tokens {
            if token.is_base() || inserted.contains(&token.address) {
               continue;
            }

            inserted.insert(token.address);
            currencies.push(Currency::from(token.clone()));
            tokens.push(token.clone());
         }

         match pool_manager.update_for_currencies(ctx.clone(), chain, currencies).await {
            Ok(_) => {
               tracing::trace!("Updated price manager for chain: {}", chain)
            }
            Err(e) => tracing::error!(
               "Error updating price manager for chain {}: {:?}",
               chain,
               e
            ),
         }
      });
   }

   ctx.write(|ctx| {
      ctx.data_syncing = false;
   });

   RT.spawn_blocking(move || {
      for chain in SUPPORTED_CHAINS {
         let portfolios = ctx.read(|ctx| ctx.portfolio_db.get_all(chain));
         for portfolio in &portfolios {
            ctx.calculate_portfolio_value(chain, portfolio.owner);
         }
      }
      ctx.save_portfolio_db();
      ctx.save_pool_manager();
      ctx.save_price_manager();
   });
}

#[derive(Debug, Clone, Default)]
pub struct UpdateInfo {
   pub available: bool,
   pub version: Option<String>,
   pub download_url: Option<String>,
   pub asset_name: Option<String>,
}

pub async fn check_for_updates() -> Result<UpdateInfo, anyhow::Error> {
   let current_version = cargo_crate_version!().to_string();

   let releases: Vec<Release> = ReleaseList::configure()
      .repo_owner("greekfetacheese")
      .repo_name("zeus")
      .build()?
      .fetch()?;

   let target = self_update::get_target();

   let latest_release = releases.into_iter().find(|r| {
      r.assets.iter().any(|asset| {
         let name = asset.name.to_lowercase();
         if target.contains("windows") {
            name.contains("windows") && name.ends_with(".zip")
         } else if target.contains("linux") {
            name.contains("linux") && name.ends_with(".zip")
         } else if target.contains("darwin") || target.contains("macos") {
            name.contains("macos") || name.contains("darwin")
         } else {
            false
         }
      })
   });

   let Some(release) = latest_release else {
      return Ok(UpdateInfo {
         available: false,
         version: None,
         download_url: None,
         asset_name: None,
      });
   };

   let new_version = release.version.trim_start_matches('v').to_owned();
   let new_semver = semver::Version::parse(&new_version)?;
   let current_semver = semver::Version::parse(&current_version)?;

   // Compare versions
   if new_semver <= current_semver {
      tracing::info!("Current version is up to date");
      return Ok(UpdateInfo {
         available: false,
         ..Default::default()
      });
   }

   // Find the correct asset for current platform
   let asset = release.asset_for(target, None).or_else(|| {
      release
         .assets
         .iter()
         .find(|a| {
            let n = a.name.to_lowercase();
            (target.contains("windows") && n.contains("windows") && n.ends_with(".zip"))
               || (target.contains("linux") && n.contains("linux"))
               || (target.contains("darwin") && (n.contains("macos") || n.contains("darwin")))
         })
         .cloned()
   });

   let Some(asset) = asset else {
      return Ok(UpdateInfo {
         available: false,
         ..Default::default()
      });
   };

   Ok(UpdateInfo {
      available: true,
      version: Some(new_version),
      download_url: Some(asset.download_url.clone()),
      asset_name: Some(asset.name.clone()),
   })
}

pub async fn update_zeus(download_url: &str, asset_name: &str) -> Result<(), anyhow::Error> {
   let tmp_dir = tempfile::Builder::new().prefix("zeus-update").tempdir()?;

   let archive_path = tmp_dir.path().join(asset_name);
   tracing::info!("Downloading update from: {}", download_url);

   let client = reqwest::Client::builder().user_agent("zeus-updater/1.0").build()?;

   let mut response = client
      .get(download_url)
      .header("Accept", "application/octet-stream")
      .send()
      .await?;

   if !response.status().is_success() {
      return Err(anyhow!("Download failed: {}", response.status()));
   }

   let total_size = response.content_length();
   let mut file = std::fs::File::create(&archive_path)?;
   let mut downloaded: u64 = 0;

   while let Some(chunk) = response.chunk().await? {
      downloaded += chunk.len() as u64;
      file.write_all(&chunk)?;

      if let Some(total) = total_size {
         let percent = (downloaded as f64 / total as f64) * 100.0;
         SHARED_GUI.write(|gui| {
            gui.loading_window.open(format!("Download progress: {:.1}%", percent));
         });
      }
   }

   file.sync_all()?;
   drop(file);

   tracing::info!("Downloaded to {:?}", archive_path);

   tracing::info!("Extracting from {}", archive_path.display());
   tracing::info!("Extracting to {}", tmp_dir.path().display());

   let archive_file = std::fs::File::open(&archive_path)?;
   let mut archive = ZipArchive::new(archive_file)?;

   let expected_name = if cfg!(windows) {
      "zeus-gui.exe"
   } else {
      "zeus-gui"
   };

   let mut new_binary_path = None;
   for i in 0..archive.len() {
      let mut file = archive.by_index(i)?;
      if file.name() == expected_name
         || file.name().ends_with("/zeus-gui")
         || file.name().ends_with("/zeus-gui.exe")
      {
         let out_path = tmp_dir.path().join(expected_name);
         let mut outfile = std::fs::File::create(&out_path)?;
         std::io::copy(&mut file, &mut outfile)?;
         new_binary_path = Some(out_path);
         break;
      }
   }

   let new_binary_path =
      new_binary_path.ok_or_else(|| anyhow!("Could not find zeus-gui executable in ZIP"))?;

   #[cfg(unix)]
   {
      use std::os::unix::fs::PermissionsExt;
      match std::fs::set_permissions(
         &new_binary_path,
         std::fs::Permissions::from_mode(0o755),
      ) {
         Ok(_) => {}
         Err(e) => {
            tracing::error!("Could not set permissions on new binary: {:?}", e);
         }
      }
   }

   tracing::info!("Extracted new binary: {:?}", new_binary_path);

   self_replace(&new_binary_path)?;

   Ok(())
}

#[cfg(unix)]
pub fn restart_app() {
   use std::thread;
   use std::time::Duration;

   let current_dir = std::env::current_dir().unwrap();
   let exe = current_dir.join("zeus-gui");
   tracing::info!("Current executable: {}", exe.display());

   for _ in 0..3 {
      match std::process::Command::new(&exe).spawn() {
         Ok(_) => {
            tracing::info!("Restart successful!");
            std::process::exit(0);
         }
         Err(e) => {
            tracing::warn!("Restart failed: {}", e);
            thread::sleep(Duration::from_millis(300));
         }
      }
   }

   // ask user to start manually
   RT.spawn_blocking(move || {
      SHARED_GUI.write(|gui| {
         gui.update_window.auto_restart_failed();
         gui.request_repaint();
      });
   });
}

#[cfg(windows)]
pub fn restart_app() -> ! {
   use std::os::windows::process::CommandExt;

   let current_dir = std::env::current_dir().unwrap();
   let exe = current_dir.join("zeus-gui");
   tracing::info!("Current executable: {}", exe.display());

   let _ = std::process::Command::new(&exe).creation_flags(0x00000008).spawn();

   std::process::exit(0);
}
