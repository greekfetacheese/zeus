//! Pairing token + native-messaging helpers for the local wallet connector.
//!
//! The HTTP JSON-RPC server is localhost-only, but that is not enough: any local
//! process (or a page that can reach loopback) could spoof `origin` and talk to
//! Zeus. A per-session pairing token in a user-only file, delivered to the
//! extension via native messaging, is the capability secret.

use crate::core::context::ctx::data_dir;
use anyhow::anyhow;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use zeus_eth::alloy_primitives::hex;

pub const TOKEN_HEADER: &str = "x-zeus-token";
pub const ORIGIN_HEADER: &str = "x-zeus-origin";
pub const NATIVE_HOST_NAME: &str = "io.github.zeus_wallet";
pub const SESSION_FILE: &str = "connector.json";
/// Pinned unpacked-extension id (`key` in `wallet-connector/manifest.json`).
pub const EXTENSION_ID: &str = "iolkcnlbibolmedkdoffgaeabhimikai";

const TOKEN_BYTES: usize = 32;
const NATIVE_MSG_MAX: usize = 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConnectorSession {
   pub token: String,
   pub port: u16,
}

pub fn generate_pairing_token() -> String {
   let mut bytes = [0u8; TOKEN_BYTES];
   rand::thread_rng().fill_bytes(&mut bytes);
   hex::encode(bytes)
}

pub fn token_matches(expected: &str, provided: &str) -> bool {
   let a = expected.as_bytes();
   let b = provided.as_bytes();
   if a.len() != b.len() {
      return false;
   }
   let mut diff = 0u8;
   for i in 0..a.len() {
      diff |= a[i] ^ b[i];
   }
   diff == 0
}

pub fn parse_dapp_origin(raw: &str) -> Result<String, anyhow::Error> {
   let raw = raw.trim();
   if raw.is_empty() || raw.eq_ignore_ascii_case("null") || raw.eq_ignore_ascii_case("undefined") {
      return Err(anyhow!("missing origin"));
   }

   let (scheme, rest) = if let Some(rest) = raw.strip_prefix("https://") {
      ("https", rest)
   } else if let Some(rest) = raw.strip_prefix("http://") {
      ("http", rest)
   } else {
      return Err(anyhow!("origin must be http(s)"));
   };

   let hostport = rest.strip_suffix('/').unwrap_or(rest);
   if hostport.is_empty() {
      return Err(anyhow!("origin missing host"));
   }
   if hostport.contains('/')
      || hostport.contains('\\')
      || hostport.contains(' ')
      || hostport.contains('?')
      || hostport.contains('#')
   {
      return Err(anyhow!("origin must not include a path"));
   }

   Ok(format!("{scheme}://{hostport}"))
}

pub fn connector_session_path() -> Result<PathBuf, anyhow::Error> {
   Ok(data_dir()?.join(SESSION_FILE))
}

pub fn write_connector_session(
   path: &Path,
   session: &ConnectorSession,
) -> Result<(), anyhow::Error> {
   let body = serde_json::to_vec(session)?;
   crate::utils::write_private(path, &body)
}

pub fn encode_native_frame(payload: &[u8]) -> Vec<u8> {
   let mut out = Vec::with_capacity(4 + payload.len());
   out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
   out.extend_from_slice(payload);
   out
}

pub fn decode_native_frame(buf: &[u8]) -> Result<&[u8], anyhow::Error> {
   if buf.len() < 4 {
      return Err(anyhow!("native frame too short"));
   }
   let len = u32::from_le_bytes(buf[0..4].try_into().unwrap()) as usize;
   let rest = &buf[4..];
   if rest.len() != len {
      return Err(anyhow!("native frame length mismatch"));
   }
   Ok(rest)
}

pub fn is_native_messaging_invocation() -> bool {
   std::env::args()
      .skip(1)
      .any(|arg| arg == "--native-messaging-host" || arg.starts_with("chrome-extension://"))
}

pub fn run_native_messaging_host() -> Result<(), anyhow::Error> {
   use std::io::{Read, Write};

   let path = connector_session_path()?;
   let session = std::fs::read(&path)
      .map_err(|_| anyhow!("connector session unavailable (is Zeus running?)"))?;

   let mut stdin = std::io::stdin().lock();
   let mut stdout = std::io::stdout().lock();

   loop {
      let mut len_buf = [0u8; 4];
      match stdin.read_exact(&mut len_buf) {
         Ok(()) => {}
         Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
         Err(e) => return Err(e.into()),
      }
      let len = u32::from_le_bytes(len_buf) as usize;
      if len > NATIVE_MSG_MAX {
         return Err(anyhow!("native message too large"));
      }
      let mut req = vec![0u8; len];
      stdin.read_exact(&mut req)?;

      stdout.write_all(&(session.len() as u32).to_le_bytes())?;
      stdout.write_all(&session)?;
      stdout.flush()?;
   }

   Ok(())
}

pub fn register_native_host(exe_path: &Path, workdir: &Path) -> Result<(), anyhow::Error> {
   let exe = exe_path.canonicalize().unwrap_or_else(|_| exe_path.to_path_buf());
   let workdir = workdir.canonicalize().unwrap_or_else(|_| workdir.to_path_buf());

   let host_path = write_native_host_wrapper(&exe, &workdir)?;
   let manifest = serde_json::json!({
      "name": NATIVE_HOST_NAME,
      "description": "Zeus Wallet connector host",
      "path": host_path,
      "type": "stdio",
      "allowed_origins": [format!("chrome-extension://{EXTENSION_ID}/")],
   });
   let body = serde_json::to_vec_pretty(&manifest)?;
   let file_name = format!("{NATIVE_HOST_NAME}.json");
   // Stable copy next to the wrapper. Chrome on Windows discovers hosts via
   // HKCU, so this path is what the registry value points at.
   let manifest_path = data_dir()?.join(&file_name);
   std::fs::write(&manifest_path, &body)?;

   for dir in native_host_dirs() {
      let Some(browser_dir) = dir.parent() else {
         continue;
      };
      if !browser_dir.exists() {
         continue;
      }
      // Best-effort: Program Files is usually not writable without elevation.
      if let Err(e) =
         std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(dir.join(&file_name), &body))
      {
         tracing::debug!("skip native host dir {}: {e}", dir.display());
      }
   }

   #[cfg(windows)]
   register_native_host_registry(&manifest_path);

   Ok(())
}

#[cfg(unix)]
fn shell_single_quote(s: &str) -> String {
   format!("'{}'", s.replace('\'', "'\\''"))
}

fn write_native_host_wrapper(exe: &Path, workdir: &Path) -> Result<PathBuf, anyhow::Error> {
   let dir = data_dir()?;

   #[cfg(unix)]
   {
      use std::io::Write;
      use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

      let path = dir.join("zeus_connector_host.sh");
      let script = format!(
         "#!/bin/sh\ncd {cwd} || exit 1\nexec {exe} --native-messaging-host\n",
         cwd = shell_single_quote(&workdir.to_string_lossy()),
         exe = shell_single_quote(&exe.to_string_lossy()),
      );
      let mut file = std::fs::OpenOptions::new()
         .write(true)
         .create(true)
         .truncate(true)
         .mode(0o700)
         .open(&path)?;
      file.write_all(script.as_bytes())?;
      file.sync_all()?;
      drop(file);
      let mut perms = std::fs::metadata(&path)?.permissions();
      perms.set_mode(0o700);
      std::fs::set_permissions(&path, perms)?;
      Ok(path)
   }

   #[cfg(windows)]
   {
      let path = dir.join("zeus_connector_host.cmd");
      let script = format!(
         "@echo off\r\ncd /d \"{}\"\r\n\"{}\" --native-messaging-host\r\n",
         workdir.to_string_lossy().replace('"', ""),
         exe.to_string_lossy().replace('"', ""),
      );
      std::fs::write(&path, script)?;
      Ok(path)
   }

   #[cfg(not(any(unix, windows)))]
   {
      let _ = (exe, workdir);
      Err(anyhow!(
         "native messaging host is not supported on this OS"
      ))
   }
}

fn native_host_dirs() -> Vec<PathBuf> {
   #[cfg(target_os = "macos")]
   {
      let Some(home) = std::env::var_os("HOME") else {
         return Vec::new();
      };
      let support = PathBuf::from(home).join("Library/Application Support");
      return [
         "Google/Chrome",
         "Google/Chrome Beta",
         "Google/Chrome Canary",
         "Chromium",
         "BraveSoftware/Brave-Browser",
         "Microsoft Edge",
         "Vivaldi",
      ]
      .into_iter()
      .map(|p| support.join(p).join("NativeMessagingHosts"))
      .collect();
   }

   #[cfg(target_os = "linux")]
   {
      let Some(home) = std::env::var_os("HOME") else {
         return Vec::new();
      };
      let config = PathBuf::from(home).join(".config");
      return [
         "google-chrome",
         "google-chrome-beta",
         "google-chrome-unstable",
         "chromium",
         "BraveSoftware/Brave-Browser",
         "microsoft-edge",
         "microsoft-edge-beta",
         "microsoft-edge-dev",
         "vivaldi",
         "opera",
      ]
      .into_iter()
      .map(|p| config.join(p).join("NativeMessagingHosts"))
      .collect();
   }

   #[cfg(target_os = "windows")]
   {
      windows_native_host_dirs()
   }

   #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
   {
      Vec::new()
   }
}

/// Chrome-family install / profile roots we try on Windows.
///
/// Installers live under Program Files. Profiles live under `%LOCALAPPDATA%`.
/// Chrome does not scan these folders (it uses the registry); we still write
/// the manifest where the browser tree exists, matching the other OSes.
#[cfg(target_os = "windows")]
const WINDOWS_BROWSER_REL: &[&str] = &[
   r"Google\Chrome",
   r"Google\Chrome Beta",
   r"Google\Chrome SxS",
   r"Chromium",
   r"BraveSoftware\Brave-Browser",
   r"Microsoft\Edge",
   r"Vivaldi",
   r"Opera Software\Opera Stable",
];

#[cfg(target_os = "windows")]
fn windows_native_host_dirs() -> Vec<PathBuf> {
   let mut dirs = Vec::new();

   if let Some(local) = std::env::var_os("LOCALAPPDATA") {
      let local = PathBuf::from(local);
      dirs.extend(WINDOWS_BROWSER_REL.iter().map(|p| local.join(p).join("NativeMessagingHosts")));
   }

   let mut roots: Vec<PathBuf> = ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"]
      .into_iter()
      .filter_map(|key| std::env::var_os(key).map(PathBuf::from))
      .collect();
   if roots.is_empty() {
      roots.push(PathBuf::from(r"C:\Program Files"));
      roots.push(PathBuf::from(r"C:\Program Files (x86)"));
   }

   for root in roots {
      dirs.extend(WINDOWS_BROWSER_REL.iter().map(|p| root.join(p).join("NativeMessagingHosts")));
   }

   dirs
}

/// Chrome on Windows finds native hosts only through the registry.
/// HKCU so we don't need elevation. Default value = path to the manifest JSON.
#[cfg(windows)]
fn register_native_host_registry(manifest_path: &Path) {
   let path = manifest_path.to_string_lossy().into_owned();
   let keys = [
      format!(r"HKCU\Software\Google\Chrome\NativeMessagingHosts\{NATIVE_HOST_NAME}"),
      format!(r"HKCU\Software\Google\Chrome Beta\NativeMessagingHosts\{NATIVE_HOST_NAME}"),
      format!(r"HKCU\Software\Chromium\NativeMessagingHosts\{NATIVE_HOST_NAME}"),
      format!(r"HKCU\Software\BraveSoftware\Brave-Browser\NativeMessagingHosts\{NATIVE_HOST_NAME}"),
      format!(r"HKCU\Software\Microsoft\Edge\NativeMessagingHosts\{NATIVE_HOST_NAME}"),
      format!(r"HKCU\Software\Vivaldi\NativeMessagingHosts\{NATIVE_HOST_NAME}"),
   ];
   for key in keys {
      if let Err(e) = std::process::Command::new("reg")
         .args(["add", &key, "/ve", "/t", "REG_SZ", "/d", &path, "/f"])
         .status()
      {
         tracing::debug!("skip native host registry {key}: {e}");
      }
   }
}
