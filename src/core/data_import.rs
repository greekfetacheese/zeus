//! Import a Zeus data zip produced by [`super::data_export`].
//!
//! Safety:
//! 1. Every zip path is checked against the same persisted-name whitelist used
//!    on export (no zip-slip, no unknown files).
//! 2. `data/vault.data` must unlock with the credentials the user entered.
//! 3. `data/wallet_state.data` must decrypt with the vault-held key.
//! 4. Only then are files written over the destination `data/` directory.

use super::data_export::{archive_rel_parts, is_allowed_archive_parts};
use super::persisted::PersistedFile;
use super::{Vault, WalletState};
use crate::utils::{restrict_dir_to_owner, write_private_from_reader};
use anyhow::anyhow;
use ncrypt_me::Credentials;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use zip::ZipArchive;

/// Files written plus the already-unlocked imported vault.
pub struct ImportedData {
   pub files_written: usize,
   pub vault: Vault,
}

/// Verify the archive, then replace matching files under `dest_data_dir`.
///
/// Returns the unlocked vault. Does not write anything if vault unlock
/// or wallet-state decrypt fails.
pub fn import_data_from_zip(
   zip_path: &Path,
   dest_data_dir: &Path,
   credentials: Credentials,
) -> Result<ImportedData, anyhow::Error> {
   let file = File::open(zip_path).map_err(|e| {
      anyhow!(
         "Failed to open archive {}: {e}",
         zip_path.display()
      )
   })?;
   let mut archive = ZipArchive::new(file).map_err(|e| anyhow!("Invalid zip archive: {e}"))?;

   let entries = list_allowed_entries(&mut archive)?;
   let vault = verify_imported_vault(&mut archive, &entries, credentials)?;

   std::fs::create_dir_all(dest_data_dir)?;
   if let Err(e) = restrict_dir_to_owner(dest_data_dir) {
      tracing::warn!("Failed to restrict imported data dir: {e}");
   }

   let mut ordered = entries;
   ordered.sort_by_key(|(_, parts)| write_priority(parts));

   for (index, parts) in &ordered {
      let mut zip_file = archive.by_index(*index)?;
      let dest = join_parts(dest_data_dir, parts);
      write_private_from_reader(&dest, &mut zip_file)?;
      restrict_parents(dest_data_dir, parts);
   }

   Ok(ImportedData {
      files_written: ordered.len(),
      vault,
   })
}

fn list_allowed_entries(
   archive: &mut ZipArchive<File>,
) -> Result<Vec<(usize, Vec<String>)>, anyhow::Error> {
   let mut entries = Vec::new();
   let mut has_vault = false;
   let mut has_wallet_state = false;

   for i in 0..archive.len() {
      let file = archive.by_index(i)?;
      if file.is_dir() {
         continue;
      }
      if file.is_symlink() {
         return Err(anyhow!(
            "Refusing symlink in archive: {}",
            file.name()
         ));
      }

      let Some(enclosed) = file.enclosed_name() else {
         return Err(anyhow!("Unsafe zip path: {}", file.name()));
      };
      let Some(parts) = archive_rel_parts(&enclosed) else {
         return Err(anyhow!(
            "Archive path must be under data/: {}",
            file.name()
         ));
      };
      if !is_allowed_archive_parts(&parts) {
         return Err(anyhow!(
            "Refusing unknown file in archive: {}",
            file.name()
         ));
      }

      if parts.as_slice() == [PersistedFile::Vault.name()] {
         has_vault = true;
      }
      if parts.as_slice() == [PersistedFile::WalletState.name()] {
         has_wallet_state = true;
      }

      entries.push((i, parts));
   }

   if !has_vault {
      return Err(anyhow!("Archive is missing data/vault.data"));
   }
   if !has_wallet_state {
      return Err(anyhow!(
         "Archive is missing data/wallet_state.data"
      ));
   }

   Ok(entries)
}

fn verify_imported_vault(
   archive: &mut ZipArchive<File>,
   entries: &[(usize, Vec<String>)],
   credentials: Credentials,
) -> Result<Vault, anyhow::Error> {
   let vault_idx = index_of(entries, PersistedFile::Vault)
      .ok_or_else(|| anyhow!("Archive is missing data/vault.data"))?;
   let state_idx = index_of(entries, PersistedFile::WalletState)
      .ok_or_else(|| anyhow!("Archive is missing data/wallet_state.data"))?;

   let vault_bytes = read_entry(archive, vault_idx)?;
   let state_bytes = read_entry(archive, state_idx)?;

   let mut vault = Vault::default();
   vault.set_credentials(credentials);

   let decrypted = match vault.decrypt_bytes(vault_bytes) {
      Ok(data) => data,
      Err(e) => {
         vault.erase();
         return Err(e);
      }
   };

   if let Err(e) = vault.load(decrypted) {
      vault.erase();
      return Err(anyhow!("Imported vault is corrupted: {e}"));
   }

   let key = match vault.wallet_state_key() {
      Ok(key) => key,
      Err(e) => {
         vault.erase();
         return Err(e);
      }
   };

   if let Err(e) = WalletState::decrypt_from_bytes(&key, &state_bytes) {
      vault.erase();
      return Err(e);
   }

   Ok(vault)
}

fn read_entry(archive: &mut ZipArchive<File>, index: usize) -> Result<Vec<u8>, anyhow::Error> {
   let mut file = archive.by_index(index)?;
   let mut buf = Vec::new();
   file.read_to_end(&mut buf)?;
   Ok(buf)
}

fn index_of(entries: &[(usize, Vec<String>)], file: PersistedFile) -> Option<usize> {
   entries
      .iter()
      .find(|(_, parts)| parts.as_slice() == [file.name()])
      .map(|(i, _)| *i)
}

fn write_priority(parts: &[String]) -> u8 {
   match parts {
      [name] if name == PersistedFile::Vault.name() => 2,
      [name] if name == PersistedFile::WalletState.name() => 1,
      _ => 0,
   }
}

fn join_parts(root: &Path, parts: &[String]) -> PathBuf {
   parts.iter().fold(root.to_path_buf(), |p, s| p.join(s))
}

fn restrict_parents(root: &Path, parts: &[String]) {
   let mut dir = root.to_path_buf();
   for part in parts.iter().take(parts.len().saturating_sub(1)) {
      dir.push(part);
      if let Err(e) = restrict_dir_to_owner(&dir) {
         tracing::warn!("Failed to restrict {}: {e}", dir.display());
      }
   }
}

#[cfg(test)]
mod tests {
   use super::*;
   use crate::core::data_export::{ExportOptions, export_data_to_zip};
   use crate::core::{Vault, WalletState};
   use ncrypt_me::{Argon2, Credentials};
   use secure_types::SecureString;
   use std::fs;
   use std::io::Write;
   use zeus_wallet::SecureHDWallet;
   use zip::ZipWriter;
   use zip::write::SimpleFileOptions;

   fn creds(user: &str, pass: &str) -> Credentials {
      Credentials::new(
         SecureString::from(user),
         SecureString::from(pass),
         SecureString::from(pass),
      )
   }

   fn sample_encrypted_vault(credentials: Credentials) -> (Vec<u8>, Vec<u8>) {
      let mut vault = Vault::default();
      vault.set_credentials(credentials);
      vault.set_hd_wallet(SecureHDWallet::random());
      vault.ensure_wallet_state_key().unwrap();
      let key = vault.wallet_state_key().unwrap();
      let state = WalletState::default();
      let sealed_state = state.encrypt_to_bytes(&key).unwrap();
      let argon = Argon2::new(8_000, 1, 1);
      let sealed_vault = vault.encrypt(Some(argon)).unwrap();
      vault.erase();
      (sealed_vault, sealed_state)
   }

   fn write_zip(path: &Path, files: &[(&str, &[u8])]) {
      if let Some(parent) = path.parent() {
         fs::create_dir_all(parent).unwrap();
      }
      let file = File::create(path).unwrap();
      let mut zip = ZipWriter::new(file);
      let options = SimpleFileOptions::default();
      for (name, bytes) in files {
         zip.start_file(*name, options).unwrap();
         zip.write_all(bytes).unwrap();
      }
      zip.finish().unwrap();
   }

   #[test]
   fn import_rejects_unknown_files() {
      let dir = tempfile::tempdir().unwrap();
      let zip_path = dir.path().join("bad.zip");
      write_zip(
         &zip_path,
         &[
            ("data/vault.data", b"x"),
            ("data/wallet_state.data", b"y"),
            ("data/secret.txt", b"nope"),
         ],
      );
      let dest = dir.path().join("out");
      fs::create_dir(&dest).unwrap();
      let err = match import_data_from_zip(&zip_path, &dest, creds("u", "p")) {
         Err(e) => e,
         Ok(_) => panic!("expected unknown file error"),
      };
      assert!(err.to_string().contains("unknown file"));
      assert!(fs::read_dir(&dest).unwrap().next().is_none());
   }

   #[test]
   fn import_rejects_missing_vault() {
      let dir = tempfile::tempdir().unwrap();
      let zip_path = dir.path().join("novault.zip");
      write_zip(&zip_path, &[("data/wallet_state.data", b"y")]);
      let dest = dir.path().join("out");
      fs::create_dir(&dest).unwrap();
      let err = match import_data_from_zip(&zip_path, &dest, creds("u", "p")) {
         Err(e) => e,
         Ok(_) => panic!("expected missing vault error"),
      };
      assert!(err.to_string().contains("vault.data"));
   }

   #[test]
   fn import_rejects_zip_slip() {
      let dir = tempfile::tempdir().unwrap();
      let zip_path = dir.path().join("slip.zip");
      write_zip(
         &zip_path,
         &[
            ("data/vault.data", b"x"),
            ("data/wallet_state.data", b"y"),
            ("data/../vault.data", b"evil"),
         ],
      );
      let dest = dir.path().join("out");
      fs::create_dir(&dest).unwrap();
      let err = match import_data_from_zip(&zip_path, &dest, creds("u", "p")) {
         Err(e) => e,
         Ok(_) => panic!("expected zip-slip error"),
      };
      let msg = err.to_string();
      assert!(
         msg.contains("Unsafe") || msg.contains("unknown") || msg.contains("under data"),
         "{msg}"
      );
   }

   #[test]
   fn import_roundtrip_replaces_files() {
      let credentials = creds("user", "pass");
      let (vault_bytes, state_bytes) = sample_encrypted_vault(credentials.clone());

      let src = tempfile::tempdir().unwrap();
      fs::write(src.path().join("vault.data"), &vault_bytes).unwrap();
      fs::write(src.path().join("wallet_state.data"), &state_bytes).unwrap();
      fs::write(
         src.path().join("theme.json"),
         b"{\"kind\":\"TokyoNight\"}",
      )
      .unwrap();
      fs::create_dir_all(src.path().join("railgun")).unwrap();
      fs::write(
         src.path().join("railgun").join("railgun:1.db"),
         b"db",
      )
      .unwrap();

      let zip_path = src.path().join("zeus-data.zip");
      export_data_to_zip(
         src.path(),
         &zip_path,
         ExportOptions {
            railgun: true,
            ..ExportOptions::default()
         },
      )
      .unwrap();

      let dest = tempfile::tempdir().unwrap();
      let dest_data = dest.path().join("data");
      fs::create_dir(&dest_data).unwrap();
      fs::write(dest_data.join("theme.json"), b"old").unwrap();
      fs::write(dest_data.join("price_data.json"), b"keep-me").unwrap();

      let mut imported = import_data_from_zip(&zip_path, &dest_data, credentials).unwrap();
      assert_eq!(imported.files_written, 4);
      imported.vault.erase();
      assert_eq!(
         fs::read(dest_data.join("vault.data")).unwrap(),
         vault_bytes
      );
      assert_eq!(
         fs::read(dest_data.join("wallet_state.data")).unwrap(),
         state_bytes
      );
      assert_eq!(
         fs::read(dest_data.join("theme.json")).unwrap(),
         b"{\"kind\":\"TokyoNight\"}"
      );
      assert_eq!(
         fs::read(dest_data.join("railgun").join("railgun:1.db")).unwrap(),
         b"db"
      );
      assert_eq!(
         fs::read(dest_data.join("price_data.json")).unwrap(),
         b"keep-me"
      );
   }

   #[test]
   fn wrong_credentials_do_not_write() {
      let (vault_bytes, state_bytes) = sample_encrypted_vault(creds("user", "pass"));
      let dir = tempfile::tempdir().unwrap();
      let zip_path = dir.path().join("zeus-data.zip");
      write_zip(
         &zip_path,
         &[
            ("data/vault.data", &vault_bytes),
            ("data/wallet_state.data", &state_bytes),
         ],
      );
      let dest = dir.path().join("out");
      fs::create_dir(&dest).unwrap();
      fs::write(dest.join("theme.json"), b"old").unwrap();

      let err = match import_data_from_zip(&zip_path, &dest, creds("user", "wrong")) {
         Err(e) => e,
         Ok(_) => panic!("expected unlock error"),
      };
      assert!(err.to_string().contains("unlock vault"));
      assert!(!dest.join("vault.data").exists());
      assert_eq!(fs::read(dest.join("theme.json")).unwrap(), b"old");
   }
}
