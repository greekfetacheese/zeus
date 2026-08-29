//! Export Zeus `data/` into a zip that mirrors the on-disk layout.
//!
//! Only known persisted names are packed — never a blind directory walk of
//! `data/`. Optional trees (`clear_signing`, `railgun`, `token_icons`) are
//! included only when requested, and each file name is checked before it
//! enters the archive. The zip is not password-protected; at-rest data is
//! already encrypted.
//!
//! Archive layout (so a later import can drop the zip on `data/`):
//!
//! ```text
//! data/
//!   vault.data
//!   wallet_state.data
//!   ...
//!   railgun/            (optional)
//!   clear_signing/      (optional)
//!   token_icons/        (optional)
//! ```

use crate::utils::restrict_file_to_owner;
use anyhow::anyhow;
use std::fs::File;
use std::io::BufWriter;
use std::path::{Component, Path, PathBuf};
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

/// Files Zeus persists at the root of `data/`.
///
/// `connector.json` and the native-messaging host manifest are omitted:
/// they are machine/session specific and regenerated on start.
pub const CORE_FILE_NAMES: &[&str] = &[
   "vault.data",
   "wallet_state.data",
   "tokens.data",
   "pool_data.data",
   "providers.data",
   "bundler_url.data",
   "address_book.data",
   "price_data.json",
   "theme.json",
   "server_port.json",
   "disabled_chains.json",
   "railgun_config.json",
   "across_settings.json",
];

const CLEAR_SIGNING_DIR: &str = "clear_signing";
const RAILGUN_DIR: &str = "railgun";
const TOKEN_ICONS_DIR: &str = "token_icons";

/// Optional trees the user can add on top of the core files.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExportOptions {
   pub clear_signing: bool,
   pub railgun: bool,
   pub token_icons: bool,
}

/// One file that passed the name check, ready to pack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportEntry {
   pub abs_path: PathBuf,
   /// Always `data/...` with `/` separators.
   pub zip_path: String,
}

/// Known persisted names at the `data/` root.
pub fn persisted_file_names() -> impl Iterator<Item = &'static str> {
   CORE_FILE_NAMES.iter().copied()
}

/// Collect files under `data_dir` that are allowed for `options`.
///
/// Missing files are skipped. Unknown names are never returned.
pub fn collect_export_entries(
   data_dir: &Path,
   options: ExportOptions,
) -> Result<Vec<ExportEntry>, anyhow::Error> {
   let mut entries = Vec::new();

   for name in persisted_file_names() {
      let abs = data_dir.join(name);
      if !is_regular_file(&abs) {
         continue;
      }
      entries.push(entry_for(abs, Path::new(name))?);
   }

   if options.clear_signing {
      collect_flat_dir(
         data_dir,
         CLEAR_SIGNING_DIR,
         is_allowed_clear_signing_file,
         &mut entries,
      )?;
   }
   if options.railgun {
      collect_flat_dir(
         data_dir,
         RAILGUN_DIR,
         is_allowed_railgun_file,
         &mut entries,
      )?;
   }
   if options.token_icons {
      collect_token_icons(data_dir, &mut entries)?;
   }

   Ok(entries)
}

/// Pack `data_dir` into `dest` zip. Returns the number of files written.
pub fn export_data_to_zip(
   data_dir: &Path,
   dest: &Path,
   options: ExportOptions,
) -> Result<usize, anyhow::Error> {
   let entries = collect_export_entries(data_dir, options)?;
   if entries.is_empty() {
      return Err(anyhow!("No persisted files found to export"));
   }

   if let Some(parent) = dest.parent() {
      if !parent.as_os_str().is_empty() {
         std::fs::create_dir_all(parent)?;
      }
   }

   let file = File::create(dest)?;
   let mut zip = ZipWriter::new(BufWriter::new(file));
   let zip_options = SimpleFileOptions::default()
      .compression_method(zip::CompressionMethod::Deflated)
      .unix_permissions(0o600)
      .large_file(true);

   for entry in &entries {
      let mut src = File::open(&entry.abs_path)?;
      zip.start_file(&entry.zip_path, zip_options)?;
      std::io::copy(&mut src, &mut zip)?;
   }

   let writer = zip.finish()?;
   drop(writer);

   if let Err(e) = restrict_file_to_owner(dest) {
      tracing::warn!("Failed to restrict export zip permissions: {e}");
   }

   Ok(entries.len())
}

pub fn is_allowed_clear_signing_file(name: &str) -> bool {
   name == "index.eip712.json" || name == "index.calldata.json" || is_keccak256_json(name)
}

pub fn is_allowed_railgun_file(name: &str) -> bool {
   if let Some(rest) = name.strip_prefix("railgun:") {
      return rest.strip_suffix(".db").is_some_and(is_chain_id);
   }
   if let Some(rest) = name.strip_prefix("events-snapshot:") {
      return rest.strip_suffix(".data").is_some_and(is_chain_id)
         || rest.strip_suffix(".meta").is_some_and(is_chain_id);
   }
   false
}

pub fn is_allowed_token_icon_rel(rel: &Path) -> bool {
   let Some(parts) = normal_components(rel) else {
      return false;
   };
   if parts.len() != 3 {
      return false;
   }
   is_chain_id(&parts[0]) && is_token_address_dir(&parts[1]) && is_icon_file(&parts[2])
}

fn collect_flat_dir(
   data_dir: &Path,
   dir_name: &str,
   allow: fn(&str) -> bool,
   out: &mut Vec<ExportEntry>,
) -> Result<(), anyhow::Error> {
   let dir = data_dir.join(dir_name);
   if !dir.is_dir() {
      return Ok(());
   }

   let read = match std::fs::read_dir(&dir) {
      Ok(read) => read,
      Err(_) => return Ok(()),
   };

   for entry in read.flatten() {
      let abs = entry.path();
      if !is_regular_file(&abs) {
         continue;
      }
      let name = entry.file_name();
      let Some(name) = name.to_str() else {
         continue;
      };
      if !allow(name) {
         tracing::warn!("Skipping unknown file in data/{dir_name}: {name}");
         continue;
      }
      let rel = Path::new(dir_name).join(name);
      out.push(entry_for(abs, &rel)?);
   }

   Ok(())
}

fn collect_token_icons(data_dir: &Path, out: &mut Vec<ExportEntry>) -> Result<(), anyhow::Error> {
   let root = data_dir.join(TOKEN_ICONS_DIR);
   if !root.is_dir() {
      return Ok(());
   }

   let mut files = Vec::new();
   walk_regular_files(&root, &mut files);

   for abs in files {
      let Some(rel) = abs.strip_prefix(data_dir).ok().map(|p| p.to_path_buf()) else {
         continue;
      };
      let Ok(under_icons) = rel.strip_prefix(TOKEN_ICONS_DIR) else {
         continue;
      };
      if !is_allowed_token_icon_rel(under_icons) {
         tracing::warn!(
            "Skipping unknown token icon path: {}",
            rel.display()
         );
         continue;
      }
      out.push(entry_for(abs, &rel)?);
   }

   Ok(())
}

fn walk_regular_files(dir: &Path, out: &mut Vec<PathBuf>) {
   let Ok(read) = std::fs::read_dir(dir) else {
      return;
   };
   for entry in read.flatten() {
      let path = entry.path();
      let Ok(meta) = path.symlink_metadata() else {
         continue;
      };
      if meta.file_type().is_symlink() {
         continue;
      }
      if meta.is_dir() {
         walk_regular_files(&path, out);
      } else if meta.is_file() {
         out.push(path);
      }
   }
}

fn entry_for(abs_path: PathBuf, rel: &Path) -> Result<ExportEntry, anyhow::Error> {
   let zip_path =
      zip_path_for(rel).ok_or_else(|| anyhow!("Invalid export path: {}", rel.display()))?;
   Ok(ExportEntry { abs_path, zip_path })
}

fn zip_path_for(rel: &Path) -> Option<String> {
   let parts = normal_components(rel)?;
   if parts.is_empty() {
      return None;
   }
   Some(format!("data/{}", parts.join("/")))
}

fn normal_components(path: &Path) -> Option<Vec<String>> {
   let mut parts = Vec::new();
   for component in path.components() {
      match component {
         Component::Normal(s) => {
            let s = s.to_str()?;
            if s.is_empty() || s == "." || s == ".." {
               return None;
            }
            parts.push(s.to_string());
         }
         Component::CurDir => {}
         _ => return None,
      }
   }
   Some(parts)
}

fn is_regular_file(path: &Path) -> bool {
   path.symlink_metadata().map(|m| m.file_type().is_file()).unwrap_or(false)
}

fn is_chain_id(s: &str) -> bool {
   !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
}

fn is_keccak256_json(name: &str) -> bool {
   let Some(stem) = name.strip_suffix(".json") else {
      return false;
   };
   stem.len() == 64 && stem.chars().all(|c| c.is_ascii_hexdigit())
}

fn is_token_address_dir(s: &str) -> bool {
   let Some(hex) = s.strip_prefix("0x") else {
      return false;
   };
   hex.len() == 40 && hex.chars().all(|c| c.is_ascii_hexdigit())
}

fn is_icon_file(name: &str) -> bool {
   name == "x32.png" || name == "x24.png"
}

#[cfg(test)]
mod tests {
   use super::*;
   use std::fs;
   use std::io::Read;
   use zip::ZipArchive;

   fn write_file(path: &Path, bytes: &[u8]) {
      if let Some(parent) = path.parent() {
         fs::create_dir_all(parent).unwrap();
      }
      fs::write(path, bytes).unwrap();
   }

   #[test]
   fn persisted_file_names_covers_core_store() {
      let names: Vec<_> = persisted_file_names().collect();
      assert!(names.contains(&"vault.data"));
      assert!(names.contains(&"wallet_state.data"));
      assert!(!names.contains(&"connector.json"));
   }

   #[test]
   fn collect_skips_unknown_root_files() {
      let dir = tempfile::tempdir().unwrap();
      write_file(&dir.path().join("vault.data"), b"vault");
      write_file(&dir.path().join("secret.txt"), b"nope");
      write_file(&dir.path().join("connector.json"), b"{}");

      let entries = collect_export_entries(dir.path(), ExportOptions::default()).unwrap();
      let names: Vec<_> = entries.iter().map(|e| e.zip_path.as_str()).collect();
      assert_eq!(names, ["data/vault.data"]);
   }

   #[test]
   fn collect_skips_missing_core_files() {
      let dir = tempfile::tempdir().unwrap();
      write_file(&dir.path().join("theme.json"), b"{}");

      let entries = collect_export_entries(dir.path(), ExportOptions::default()).unwrap();
      assert_eq!(entries.len(), 1);
      assert_eq!(entries[0].zip_path, "data/theme.json");
   }

   #[test]
   fn optional_trees_are_off_by_default() {
      let dir = tempfile::tempdir().unwrap();
      write_file(&dir.path().join("vault.data"), b"vault");
      write_file(
         &dir.path().join("railgun").join("railgun:1.db"),
         b"db",
      );
      write_file(
         &dir.path().join("clear_signing").join("index.eip712.json"),
         b"{}",
      );
      write_file(
         &dir
            .path()
            .join("token_icons")
            .join("1")
            .join("0x1111111111111111111111111111111111111111")
            .join("x32.png"),
         b"png",
      );

      let entries = collect_export_entries(dir.path(), ExportOptions::default()).unwrap();
      assert_eq!(entries.len(), 1);
      assert_eq!(entries[0].zip_path, "data/vault.data");
   }

   #[test]
   fn optional_trees_include_only_verified_names() {
      let dir = tempfile::tempdir().unwrap();
      write_file(&dir.path().join("vault.data"), b"vault");
      write_file(
         &dir.path().join("railgun").join("railgun:1.db"),
         b"db",
      );
      write_file(
         &dir.path().join("railgun").join("events-snapshot:1.data"),
         b"snap",
      );
      write_file(
         &dir.path().join("railgun").join("events-snapshot:1.meta"),
         b"meta",
      );
      write_file(
         &dir.path().join("railgun").join("notes.txt"),
         b"nope",
      );
      write_file(
         &dir.path().join("clear_signing").join("index.calldata.json"),
         b"{}",
      );
      write_file(
         &dir.path().join("clear_signing").join(&format!("{}.json", "ab".repeat(32))),
         b"{}",
      );
      write_file(
         &dir.path().join("clear_signing").join("random.json"),
         b"nope",
      );
      write_file(
         &dir
            .path()
            .join("token_icons")
            .join("8453")
            .join("0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
            .join("x24.png"),
         b"png",
      );
      write_file(
         &dir
            .path()
            .join("token_icons")
            .join("8453")
            .join("not-an-address")
            .join("x24.png"),
         b"png",
      );

      let options = ExportOptions {
         clear_signing: true,
         railgun: true,
         token_icons: true,
      };
      let mut names: Vec<_> = collect_export_entries(dir.path(), options)
         .unwrap()
         .into_iter()
         .map(|e| e.zip_path)
         .collect();
      names.sort();

      assert_eq!(
         names,
         vec![
            "data/clear_signing/abababababababababababababababababababababababababababababababab.json"
               .to_string(),
            "data/clear_signing/index.calldata.json".to_string(),
            "data/railgun/events-snapshot:1.data".to_string(),
            "data/railgun/events-snapshot:1.meta".to_string(),
            "data/railgun/railgun:1.db".to_string(),
            "data/token_icons/8453/0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/x24.png".to_string(),
            "data/vault.data".to_string(),
         ]
      );
   }

   #[test]
   fn name_checks_reject_path_traversal_and_junk() {
      assert!(!is_allowed_railgun_file("../vault.data"));
      assert!(!is_allowed_railgun_file("railgun:1.db.bak"));
      assert!(!is_allowed_clear_signing_file(
         "index.eip712.json.bak"
      ));
      assert!(!is_allowed_token_icon_rel(Path::new(
         "../vault.data"
      )));
      assert!(!is_allowed_token_icon_rel(Path::new(
         "1/0xaa/x32.png"
      )));
      assert!(is_allowed_token_icon_rel(Path::new(
         "1/0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/x32.png"
      )));
      assert!(zip_path_for(Path::new("..")).is_none());
      assert!(zip_path_for(Path::new("/vault.data")).is_none());
      assert_eq!(
         zip_path_for(Path::new("vault.data")).as_deref(),
         Some("data/vault.data")
      );
   }

   #[test]
   fn zip_replicates_data_directory_layout() {
      let dir = tempfile::tempdir().unwrap();
      write_file(&dir.path().join("vault.data"), b"vault-bytes");
      write_file(
         &dir.path().join("wallet_state.data"),
         b"state-bytes",
      );
      write_file(
         &dir.path().join("railgun").join("railgun:1.db"),
         b"db-bytes",
      );

      let zip_path = dir.path().join("out").join("zeus-data.zip");
      let options = ExportOptions {
         railgun: true,
         ..ExportOptions::default()
      };
      let n = export_data_to_zip(dir.path(), &zip_path, options).unwrap();
      assert_eq!(n, 3);

      let file = File::open(&zip_path).unwrap();
      let mut archive = ZipArchive::new(file).unwrap();
      let mut names: Vec<_> = (0..archive.len())
         .map(|i| archive.by_index(i).unwrap().name().to_string())
         .collect();
      names.sort();
      assert_eq!(
         names,
         [
            "data/railgun/railgun:1.db",
            "data/vault.data",
            "data/wallet_state.data"
         ]
      );

      let mut vault = String::new();
      archive.by_name("data/vault.data").unwrap().read_to_string(&mut vault).unwrap();
      assert_eq!(vault, "vault-bytes");
   }

   #[test]
   fn export_errors_when_nothing_to_pack() {
      let dir = tempfile::tempdir().unwrap();
      let dest = dir.path().join("empty.zip");
      let err = export_data_to_zip(dir.path(), &dest, ExportOptions::default()).unwrap_err();
      assert!(err.to_string().contains("No persisted files"));
      assert!(!dest.exists());
   }
}
