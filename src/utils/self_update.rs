use crate::gui::SHARED_GUI;
use crate::utils::RT;
use anyhow::anyhow;
use minisign_verify::{PublicKey, Signature};
use self_update::backends::github::ReleaseList;
use self_update::{cargo_crate_version, self_replace::self_replace, update::Release};
use std::{env, io::Write};
use zip::ZipArchive;

const PUBLIC_KEY: &str = "RWRXdtAo1pQA54VsAh9XfZQDkO1aateQkMSVk3UAlxOzIF2kJZ9a6vha";

const REPO_OWNER: &str = "greekfetacheese";
const REPO_NAME: &str = "zeus";

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
      .repo_owner(REPO_OWNER)
      .repo_name(REPO_NAME)
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
      let file_name = file.name().to_string();
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
pub fn restart_app() {
   use std::os::windows::process::CommandExt;
   use std::thread;
   use std::time::Duration;

   let current_dir = std::env::current_dir().unwrap();
   let exe = current_dir.join("zeus-gui");
   tracing::info!("Current executable: {}", exe.display());

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
