//! Embedded Railgun sidecar sources (prover + waku broadcaster).
//!
//! The build script (`build.rs`) packs the source files at compile time.
//! At runtime we write them out to the data directory so a single binary
//! distribution can still run the Node.js sidecars.
//!
//! This module also handles:
//! - Smart extraction (only when the embedded content actually changes)
//! - Automatic `npm install --production` when node_modules is missing
//! - Clear errors when Node.js / npm is not installed on the user's machine

use anyhow::{anyhow, Result};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Embedded files for the Railgun ZK prover sidecar (snarkjs).
pub mod prover {
    pub const PACKAGE_JSON: &[u8] = include_bytes!(concat!(
        env!("OUT_DIR"),
        "/railgun_sidecars/prover/package.json"
    ));
    pub const INDEX_JS: &[u8] = include_bytes!(concat!(
        env!("OUT_DIR"),
        "/railgun_sidecars/prover/src/index.js"
    ));
}

/// Embedded files for the Waku broadcaster sidecar.
pub mod waku {
    pub const PACKAGE_JSON: &[u8] = include_bytes!(concat!(
        env!("OUT_DIR"),
        "/railgun_sidecars/waku/package.json"
    ));
    pub const INDEX_JS: &[u8] = include_bytes!(concat!(
        env!("OUT_DIR"),
        "/railgun_sidecars/waku/src/index.js"
    ));
}

/// Computes a stable hash of the currently embedded sidecar sources.
/// This is used as a version marker so we only re-extract when the
/// embedded files actually changed.
pub fn current_sidecars_hash() -> String {
    let mut hasher = Sha256::new();

    // Prover
    hasher.update(b"prover:");
    hasher.update(prover::PACKAGE_JSON);
    hasher.update(prover::INDEX_JS);

    // Waku
    hasher.update(b"waku:");
    hasher.update(waku::PACKAGE_JSON);
    hasher.update(waku::INDEX_JS);

    let result = hasher.finalize();
    format!("{:x}", result)
}

/// Writes the version marker file inside a sidecar directory.
fn write_version_marker(dir: &Path, hash: &str) -> Result<()> {
    let marker = dir.join(".railgun-sidecar-version");
    std::fs::write(&marker, hash)?;
    Ok(())
}

/// Reads the version marker if it exists.
fn read_version_marker(dir: &Path) -> Option<String> {
    let marker = dir.join(".railgun-sidecar-version");
    std::fs::read_to_string(marker).ok()
}

/// Extracts the sidecars **only if needed** (first time or embedded content changed).
///
/// Creates the directory structure under `<base>/railgun_sidecars/{prover,waku}`.
/// Writes a `.railgun-sidecar-version` file so future runs can skip extraction.
///
/// Returns (prover_dir, waku_dir).
pub fn ensure_sidecars_extracted(base_dir: impl AsRef<Path>) -> Result<(PathBuf, PathBuf)> {
    let base = base_dir.as_ref();
    let root = base.join("railgun_sidecars");

    let prover_dir = root.join("prover");
    let waku_dir = root.join("waku");

    let expected_hash = current_sidecars_hash();

    // Check if we can skip extraction entirely
    let prover_ok = prover_dir.exists()
        && read_version_marker(&prover_dir).as_deref() == Some(expected_hash.as_str());

    let waku_ok = waku_dir.exists()
        && read_version_marker(&waku_dir).as_deref() == Some(expected_hash.as_str());

    if prover_ok && waku_ok {
        return Ok((prover_dir, waku_dir));
    }

    // Need to (re)extract
    std::fs::create_dir_all(&prover_dir)?;
    std::fs::create_dir_all(&waku_dir)?;
    std::fs::create_dir_all(prover_dir.join("src"))?;
    std::fs::create_dir_all(waku_dir.join("src"))?;

    // Write files
    std::fs::write(prover_dir.join("package.json"), prover::PACKAGE_JSON)?;
    std::fs::write(prover_dir.join("src/index.js"), prover::INDEX_JS)?;

    std::fs::write(waku_dir.join("package.json"), waku::PACKAGE_JSON)?;
    std::fs::write(waku_dir.join("src/index.js"), waku::INDEX_JS)?;

    // Write version markers
    write_version_marker(&prover_dir, &expected_hash)?;
    write_version_marker(&waku_dir, &expected_hash)?;

    Ok((prover_dir, waku_dir))
}

/// Runs `npm install --production` inside the given sidecar directory,
/// but **only** if `node_modules` does not already exist.
///
/// This is the key piece that makes the embedded sidecars actually runnable
/// in a production single-binary distribution.
pub fn ensure_npm_dependencies(sidecar_dir: &Path) -> Result<()> {
    let node_modules = sidecar_dir.join("node_modules");

    if node_modules.exists() {
        // Already installed
        return Ok(());
    }

    // Run npm install
    let status = Command::new("npm")
        .arg("install")
        .arg("--production")
        .arg("--no-audit")
        .arg("--no-fund")
        .current_dir(sidecar_dir)
        .status();

    match status {
        Ok(exit) if exit.success() => {
            // Success — node_modules should now exist
            Ok(())
        }
        Ok(exit) => Err(anyhow!(
            "npm install failed in {} (exit code {:?})",
            sidecar_dir.display(),
            exit.code()
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err(anyhow!(
                "Node.js / npm is required for Railgun privacy features but was not found on this system.\n\n\
                 Please install Node.js from https://nodejs.org and make sure the 'npm' command is available in your PATH.\n\n\
                 After installing Node.js, restart Zeus."
            ))
        }
        Err(e) => Err(anyhow!(
            "Failed to spawn 'npm' for sidecar at {}: {}",
            sidecar_dir.display(),
            e
        )),
    }
}

/// High-level helper: ensures the sidecars are extracted **and** their
/// npm dependencies are installed.
///
/// This is the recommended function to call before starting the clients.
/// It is smart about when to do work (hash + node_modules check).
///
/// Returns the two sidecar directories (prover, waku).
pub fn ensure_sidecars_ready() -> Result<(PathBuf, PathBuf)> {
    let data = std::env::current_dir()?.join("data");
    if !data.exists() {
        std::fs::create_dir_all(&data)?;
    }

    let (prover_dir, waku_dir) = ensure_sidecars_extracted(&data)?;

    // Install dependencies if missing (this is where we detect missing Node.js/npm)
    ensure_npm_dependencies(&prover_dir)?;
    ensure_npm_dependencies(&waku_dir)?;

    Ok((prover_dir, waku_dir))
}

/// Convenience that extracts into Zeus' conventional data directory.
/// (Kept for backward compatibility with earlier code.)
pub fn extract_sidecars_to_zeus_data() -> Result<(PathBuf, PathBuf)> {
    ensure_sidecars_extracted(std::env::current_dir()?.join("data"))
}

/// Checks whether Node.js appears to be available on the system.
/// Useful for showing a friendly message in the UI before trying to start clients.
pub fn is_node_available() -> bool {
    Command::new("node")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}
