use std::path::Path;
use std::process::Command;

use anyhow::{Context, bail};
use tracing::info;
use zeus_eth::types::ChainId;

use crate::chains::{has_assets, trustwallet_slug};

pub const DEFAULT_REPO: &str = "https://github.com/trustwallet/assets.git";
pub const DEFAULT_REF: &str = "master";

/// Sparse-clone Trust Wallet `blockchains/{chain}/assets` into `work_dir`.
pub fn download_assets(
   work_dir: &Path,
   repo: &str,
   git_ref: &str,
   chains: &[ChainId],
   force: bool,
) -> anyhow::Result<()> {
   if !force && has_assets(work_dir, chains) {
      info!(
         "Using existing Trust Wallet assets in {}",
         work_dir.display()
      );
      return Ok(());
   }

   if force && work_dir.exists() {
      info!(
         "Removing {} (--force-download)",
         work_dir.display()
      );
      std::fs::remove_dir_all(work_dir)
         .with_context(|| format!("remove {}", work_dir.display()))?;
   }

   if work_dir.exists() {
      let empty = std::fs::read_dir(work_dir)
         .with_context(|| format!("read {}", work_dir.display()))?
         .next()
         .is_none();
      if !empty {
         bail!(
            "{} already exists and is not a complete assets tree; pass --force-download or --skip-download",
            work_dir.display()
         );
      }
      std::fs::remove_dir(work_dir)
         .with_context(|| format!("remove empty {}", work_dir.display()))?;
   }

   if let Some(parent) = work_dir.parent() {
      if !parent.as_os_str().is_empty() {
         std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
      }
   }

   let paths: Vec<String> = chains
      .iter()
      .filter_map(|chain| trustwallet_slug(*chain).map(|slug| format!("blockchains/{slug}/assets")))
      .collect();
   if paths.is_empty() {
      bail!("no Trust Wallet chain directories to download");
   }

   info!(
      "Sparse-cloning {} @ {} into {} ({})",
      repo,
      git_ref,
      work_dir.display(),
      paths.join(", ")
   );

   run_git(&[
      "clone",
      "--depth",
      "1",
      "--filter=blob:none",
      "--sparse",
      "--branch",
      git_ref,
      repo,
      &work_dir.display().to_string(),
   ])?;

   let mut sparse_args = vec![
      "-C".to_string(),
      work_dir.display().to_string(),
      "sparse-checkout".to_string(),
      "set".to_string(),
      "--cone".to_string(),
   ];
   sparse_args.extend(paths);
   let sparse_refs: Vec<&str> = sparse_args.iter().map(|s| s.as_str()).collect();
   run_git(&sparse_refs)?;

   if !has_assets(work_dir, chains) {
      bail!(
         "clone finished but expected assets dirs are missing under {}",
         work_dir.display()
      );
   }

   info!(
      "Trust Wallet assets ready in {}",
      work_dir.display()
   );
   Ok(())
}

fn run_git(args: &[&str]) -> anyhow::Result<()> {
   info!("git {}", args.join(" "));
   let output = Command::new("git")
      .args(args)
      .env("GIT_TERMINAL_PROMPT", "0")
      .output()
      .context("run git (is git installed and on PATH?)")?;
   if !output.status.success() {
      let stderr = String::from_utf8_lossy(&output.stderr);
      let stdout = String::from_utf8_lossy(&output.stdout);
      bail!(
         "git {} failed (status {:?})\nstdout:\n{stdout}\nstderr:\n{stderr}",
         args.join(" "),
         output.status.code()
      );
   }
   Ok(())
}
