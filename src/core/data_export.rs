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

use crate::core::persisted::{
   DATA_DIR_NAME, ExportPolicy, PersistedFile, PersistedTree, normal_components,
};
use crate::utils::restrict_file_to_owner;
use anyhow::anyhow;
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

pub use crate::core::persisted::{
   ExportOptions, is_allowed_clear_signing_file, is_allowed_railgun_file,
   is_allowed_rel_parts as is_allowed_archive_parts, is_allowed_token_icon_rel,
};

/// One file that passed the name check, ready to pack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportEntry {
   pub abs_path: PathBuf,
   /// Always `data/...` with `/` separators.
   pub zip_path: String,
}

/// Known core persisted names at the `data/` root.
pub fn persisted_file_names() -> impl Iterator<Item = &'static str> {
   PersistedFile::ALL
      .iter()
      .copied()
      .filter(|f| f.export_policy() == ExportPolicy::Core)
      .map(PersistedFile::name)
}

/// Collect files under `data_dir` that are allowed for `options`.
///
/// Missing files are skipped. Unknown names are never returned.
pub fn collect_export_entries(
   data_dir: &Path,
   options: ExportOptions,
) -> Result<Vec<ExportEntry>, anyhow::Error> {
   let mut entries = Vec::new();

   for file in PersistedFile::ALL {
      if file.export_policy() != ExportPolicy::Core {
         continue;
      }
      let abs = data_dir.join(file.name());
      if !is_regular_file(&abs) {
         continue;
      }
      entries.push(entry_for(abs, Path::new(file.name()))?);
   }

   for tree in PersistedTree::ALL {
      match tree.export_policy() {
         ExportPolicy::Never => continue,
         ExportPolicy::Optional if !options.include_tree(*tree) => continue,
         ExportPolicy::Optional | ExportPolicy::Core => {
            collect_tree(data_dir, *tree, &mut entries)?;
         }
      }
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

/// Relative path inside the zip after a leading `data/` component.
pub fn archive_rel_parts(enclosed: &Path) -> Option<Vec<String>> {
   let parts = normal_components(enclosed)?;
   if parts.first().map(|s| s.as_str()) != Some(DATA_DIR_NAME) || parts.len() < 2 {
      return None;
   }
   Some(parts[1..].to_vec())
}

fn collect_tree(
   data_dir: &Path,
   tree: PersistedTree,
   out: &mut Vec<ExportEntry>,
) -> Result<(), anyhow::Error> {
   let root = data_dir.join(tree.dir_name());
   if !root.is_dir() {
      return Ok(());
   }

   let mut files = Vec::new();
   walk_regular_files(&root, &mut files);

   for abs in files {
      let Some(rel) = abs.strip_prefix(data_dir).ok().map(|p| p.to_path_buf()) else {
         continue;
      };
      let Ok(under) = rel.strip_prefix(tree.dir_name()) else {
         continue;
      };
      if !tree.allows_rel(under) {
         tracing::warn!(
            "Skipping unknown file in data/{}: {}",
            tree.dir_name(),
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
   Some(format!("{}/{}", DATA_DIR_NAME, parts.join("/")))
}

fn is_regular_file(path: &Path) -> bool {
   path.symlink_metadata().map(|m| m.file_type().is_file()).unwrap_or(false)
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
      assert!(is_allowed_archive_parts(&[
         "vault.data".to_string()
      ]));
      assert!(!is_allowed_archive_parts(&[
         "connector.json".to_string()
      ]));
      assert!(!is_allowed_archive_parts(&[
         "railgun".to_string(),
         "notes.txt".to_string()
      ]));
      assert_eq!(
         archive_rel_parts(Path::new("data/vault.data")).as_deref(),
         Some(["vault.data".to_string()].as_slice())
      );
      assert!(archive_rel_parts(Path::new("vault.data")).is_none());
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
