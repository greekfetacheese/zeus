use crate::core::ZeusCtx;
use crate::core::urls::ZeusUrl;
use crate::gui::SHARED_GUI;
use crate::utils::RT;
use anyhow::anyhow;
use minisign_verify::{PublicKey, Signature};
use self_replace::self_replace;
use self_update::{backends::github::ReleaseList, cargo_crate_version};
use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;
use zip::ZipArchive;

const PUBLIC_KEY: &str = "RWRXdtAo1pQA54VsAh9XfZQDkO1aateQkMSVk3UAlxOzIF2kJZ9a6vha";

const REPO_OWNER: &str = "greekfetacheese";
const REPO_NAME: &str = "zeus";

/// Where this process was started from, captured before an update can replace it.
///
/// The restart cannot ask the OS for its own path: once `self_replace` has swapped the running
/// binary, `/proc/self/exe` points at the old, unlinked inode and the kernel reports it as
/// `"<path> (deleted)"` (measured), which spawns nothing. Nor can the name be assumed — a user
/// may rename the binary, and `self_replace` keeps the path it replaced rather than the name the
/// release archive uses. So it is resolved once, at startup, while it is still the truth:
/// see [`remember_startup_exe`].
static STARTUP_EXE: OnceLock<Option<PathBuf>> = OnceLock::new();

#[derive(Debug, Clone, Default)]
pub struct UpdateInfo {
   pub available: bool,
   pub version: Option<String>,
   pub download_url: Option<String>,
   pub asset_name: Option<String>,
}

/// Query GitHub for a newer Zeus release and open the update window if one exists.
pub fn do_check_for_updates(ctx: ZeusCtx) {
   if !ctx.read(|ctx| ctx.misc_config.check_for_updates()) {
      return;
   }

   RT.spawn(async move {
      let info = match check_for_updates().await {
         Ok(info) => info,
         Err(e) => {
            tracing::error!("Failed to check for updates: {:?}", e);
            Default::default()
         }
      };

      if info.available {
         SHARED_GUI.write(|gui| {
            gui.update_window.open(info);
            gui.request_repaint();
         });
      }
   });
}

pub async fn check_for_updates() -> Result<UpdateInfo, anyhow::Error> {
   // The endpoint catalog is the single source of truth for where Zeus looks
   // for its own releases; keep the self_update owner/repo in step with it.
   debug_assert_eq!(
      format!("https://github.com/{REPO_OWNER}/{REPO_NAME}"),
      ZeusUrl::ZeusReleases.base()
   );

   let current_version = cargo_crate_version!().to_string();

   let releases = ReleaseList::configure()
      .repo_owner(REPO_OWNER)
      .repo_name(REPO_NAME)
      .build()?
      .fetch()?;

   let target = self_update::get_target();

   // The newest release by *tag*, not the newest one carrying an asset for this platform: an asset
   // whose name stops matching (a renamed file, a different archive format) used to move the update
   // back to an older release, which then compared equal or lower and reported "up to date" — so a
   // release reached nobody. Naming is a packaging detail and must not decide the newest version.
   //
   // Parsed rather than trusted to arrive in order, and tags that are not versions are skipped: the
   // repository carries per-crate tags too, and one of those must not stop the check.
   let newest = releases
      .into_iter()
      .filter_map(|release| {
         let version = release.version().trim_start_matches('v').to_owned();
         semver::Version::parse(&version).ok().map(|semver| (semver, version, release))
      })
      .max_by(|a, b| a.0.cmp(&b.0));

   let Some((new_semver, new_version, release)) = newest else {
      return Ok(UpdateInfo::default());
   };

   let current_semver = semver::Version::parse(&current_version)?;

   // Compare versions
   if new_semver <= current_semver {
      tracing::info!("Current version is up to date");
      return Ok(UpdateInfo {
         available: false,
         ..Default::default()
      });
   }

   // The release is newer than this build, so there is an update to report whatever this platform
   // can install. Whether a build of it exists for this platform is a separate question, and the
   // answer belongs to the window rather than to the version check.
   let asset = release.asset_for(target, None).or_else(|| {
      release
         .assets()
         .iter()
         .find(|a| is_build_for_platform(a.name(), target))
         .cloned()
   });

   let Some(asset) = asset else {
      tracing::warn!(
         "Zeus {new_version} is newer than {current_version}, but its release has no build for \
          {target}. Assets offered: {}",
         release.assets().iter().map(|asset| asset.name()).collect::<Vec<_>>().join(", ")
      );

      return Ok(UpdateInfo {
         available: true,
         version: Some(new_version),
         download_url: None,
         asset_name: None,
      });
   };

   Ok(UpdateInfo {
      available: true,
      version: Some(new_version),
      download_url: Some(asset.download_url().to_owned()),
      asset_name: Some(asset.name().to_owned()),
   })
}

/// Whether a release asset is a build for `target`.
///
/// The rule is the platform word *and* the architecture word in the file name: the release step
/// names its archives `zeus-linux-x64.zip` and `zeus-windows-x64.zip`, and self_update's own
/// `asset_for` cannot match those — it looks for the target triple, which the names do not carry — so
/// this is what decides whether a download is offered. It reads a name, though, and names change,
/// which is exactly why the *version* check no longer depends on it: see [`check_for_updates`].
///
/// The architecture is read too: the platform word alone matched an x64 archive on an aarch64 host,
/// and installing that replaces the running binary with one the machine cannot exec.
fn is_build_for_platform(asset_name: &str, target: &str) -> bool {
   let name = asset_name.to_lowercase();

   let os_matches =
      (target.contains("windows") && name.contains("windows") && name.ends_with(".zip"))
         || (target.contains("linux") && name.contains("linux"))
         || (target.contains("darwin") && (name.contains("macos") || name.contains("darwin")));

   if !os_matches {
      return false;
   }

   // Match the architecture as a whole token, never a substring, so `arm64` cannot satisfy an
   // `x86_64` host and an `x64` buried in a longer word is not the architecture. An architecture
   // Zeus does not publish for is not guessed at.
   let arch = target.split('-').next().unwrap_or(target);
   let accepted: &[&str] = match arch {
      "x86_64" => &["x64", "x86_64", "amd64"],
      "aarch64" => &["arm64", "aarch64"],
      _ => return false,
   };

   name
      .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
      .any(|token| accepted.contains(&token))
}

#[cfg(test)]
mod asset_name_tests {
   use super::*;

   /// The names the releases actually carry, pinned. This rule decides whether a download is
   /// offered, so a rename should fail here rather than quietly in a user's updater.
   #[test]
   fn the_platform_word_in_the_asset_name_selects_the_build() {
      let linux = "x86_64-unknown-linux-gnu";
      assert!(is_build_for_platform("zeus-linux-x64.zip", linux));
      assert!(is_build_for_platform("Zeus-Linux-X64.zip", linux));

      let windows = "x86_64-pc-windows-msvc";
      assert!(is_build_for_platform(
         "zeus-windows-x64.zip",
         windows
      ));

      let macos = "aarch64-apple-darwin";
      assert!(is_build_for_platform(
         "zeus-macos-arm64.zip",
         macos
      ));

      // Not a build for this platform. The connector ships beside Zeus but installs nothing on its
      // own, and a name that has lost its platform word is the case that used to hide a release from
      // every user of that platform.
      assert!(!is_build_for_platform(
         "wallet-connector.zip",
         linux
      ));
      assert!(!is_build_for_platform("zeus-x64.zip", linux));
      assert!(!is_build_for_platform(
         "zeus-linux-x64.zip",
         windows
      ));
      assert!(!is_build_for_platform(
         "zeus-linux-x64.zip",
         macos
      ));
   }

   /// The platform word is not enough: an x64 archive must not be offered to an ARM host, because
   /// installing it swaps the running binary for one the machine cannot exec.
   #[test]
   fn the_architecture_word_in_the_asset_name_selects_the_build() {
      let linux_arm = "aarch64-unknown-linux-gnu";
      assert!(!is_build_for_platform(
         "zeus-linux-x64.zip",
         linux_arm
      ));
      assert!(is_build_for_platform(
         "zeus-linux-arm64.zip",
         linux_arm
      ));
      assert!(is_build_for_platform(
         "zeus-linux-aarch64.zip",
         linux_arm
      ));

      let windows_arm = "aarch64-pc-windows-msvc";
      assert!(!is_build_for_platform(
         "zeus-windows-x64.zip",
         windows_arm
      ));

      // Whole tokens only: an `x64` inside a longer word is not the architecture, and an arm64
      // archive does not satisfy an x86_64 host.
      let linux = "x86_64-unknown-linux-gnu";
      assert!(is_build_for_platform(
         "zeus-linux-amd64.zip",
         linux
      ));
      assert!(is_build_for_platform(
         "zeus-linux-x86_64.zip",
         linux
      ));
      assert!(!is_build_for_platform(
         "zeus-linux-x64bit.zip",
         linux
      ));
      assert!(!is_build_for_platform(
         "zeus-linux-arm64.zip",
         linux
      ));

      // An architecture Zeus does not build for is not guessed at.
      assert!(!is_build_for_platform(
         "zeus-linux-x64.zip",
         "riscv64gc-unknown-linux-gnu"
      ));
   }
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

   let binary_name = if cfg!(windows) {
      "zeus-gui.exe"
   } else {
      "zeus-gui"
   };

   let sig_name = "signature.minisig";

   let mut new_binary_path = None;
   let mut sig_file_path = None;

   for i in 0..archive.len() {
      let mut file = archive.by_index(i)?;
      let file_name = file.name()?.to_string();
      if file_name == binary_name
         || file_name.ends_with("/zeus-gui")
         || file_name.ends_with("/zeus-gui.exe")
      {
         let out_path = tmp_dir.path().join(binary_name);
         let mut outfile = std::fs::File::create(&out_path)?;
         std::io::copy(&mut file, &mut outfile)?;
         new_binary_path = Some(out_path);
      } else if file_name == sig_name || file_name.ends_with("/signature.minisig") {
         let out_path = tmp_dir.path().join(sig_name);
         let mut outfile = std::fs::File::create(&out_path)?;
         std::io::copy(&mut file, &mut outfile)?;
         sig_file_path = Some(out_path);
      }
   }

   let new_binary_path =
      new_binary_path.ok_or_else(|| anyhow!("Could not find zeus-gui executable in ZIP"))?;

   let sig_file_path =
      sig_file_path.ok_or_else(|| anyhow!("Could not find signature file in ZIP"))?;

   let public_key = PublicKey::from_base64(PUBLIC_KEY)?;
   let content = std::fs::read(new_binary_path.clone())?;
   let signature = Signature::from_file(sig_file_path.clone())?;

   match public_key.verify(&content, &signature, false) {
      Ok(_) => {
         tracing::info!("Signature verification successful");
      }
      Err(e) => {
         tracing::error!("Signature verification failed: {:?}", e);
         return Err(anyhow!("Signature verification failed"));
      }
   }

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

/// Remember where this process was started from. Call once, early in `main`.
pub fn remember_startup_exe() {
   STARTUP_EXE.get_or_init(|| {
      let exe = std::env::current_exe().ok();
      match &exe {
         Some(exe) => tracing::debug!("Started from {}", exe.display()),
         None => tracing::warn!("Cannot resolve the Zeus binary; the updater will not restart it"),
      }
      exe
   });
}

/// The program to start in order to come back up.
fn restart_target() -> PathBuf {
   if let Some(exe) = STARTUP_EXE.get().and_then(Option::as_ref) {
      return exe.clone();
   }

   // Nothing was remembered, so fall back to the name the release archive ships, beside the
   // working directory. Only reachable if the binary could not be resolved at startup.
   let name = if cfg!(windows) {
      "zeus-gui.exe"
   } else {
      "zeus-gui"
   };
   std::env::current_dir().unwrap_or_default().join(name)
}

#[cfg(unix)]
pub fn restart_app() {
   use std::thread;
   use std::time::Duration;

   let exe = restart_target();
   tracing::info!("Restarting {}", exe.display());

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
pub fn restart_app() {
   use std::os::windows::process::CommandExt;
   use std::thread;
   use std::time::Duration;

   let exe = restart_target();
   tracing::info!("Restarting {}", exe.display());

   for _ in 0..3 {
      match std::process::Command::new(&exe).creation_flags(0x00000008).spawn() {
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

   RT.spawn_blocking(move || {
      SHARED_GUI.write(|gui| {
         gui.update_window.auto_restart_failed();
         gui.request_repaint();
      });
   });
}
