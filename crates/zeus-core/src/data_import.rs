//! Import a Zeus data zip produced by [`super::data_export`].
//!
//! Safety:
//! 1. Every zip path is checked against the same persisted-name whitelist used
//!    on export (no zip-slip, no unknown files), and entries that normalise to
//!    the same `data/`-relative path are treated as one — the first wins.
//! 2. `data/vault.data` must be long enough to be a sealed store and unlock with
//!    the credentials the user entered.
//! 3. `data/wallet_state.data` must decrypt with the vault-held key.
//! 4. Only then is the archive staged off to the side and, once every entry has
//!    landed, swapped into the destination `data/` directory — nothing live is
//!    deleted or overwritten before that.

use super::data_export::{archive_rel_parts, is_allowed_archive_parts};
use super::persisted::{PersistedFile, PersistedTree, railgun_db_files};
use super::wallet_state_key::WalletStateKey;
use super::{Vault, WalletState};
use crate::platform::fs::{restrict_dir_to_owner, write_private_from_reader};
use anyhow::anyhow;
use ncrypt_me::Credentials;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use zip::ZipArchive;

/// Prefix of the staging directory an import writes into before it installs anything.
const STAGING_DIR_PREFIX: &str = ".import-staging-";

/// The shortest a sealed Zeus store can be on disk.
///
/// `ncrypt-me`'s container is an 8-byte magic header, then the encrypted-info length as a 4-byte
/// little-endian `u32`, then the encrypted info and the ciphertext. Its `decrypt_data` indexes the
/// header (`&data[0..8]`) before any length check, so a shorter blob panics inside the dependency —
/// and [`Vault::decrypt_bytes`] hands the archive's bytes straight to it. Refusing anything below
/// header + length field (8 + 4) keeps a crafted short blob out of the decryptor.
const MIN_SEALED_BLOB_BYTES: usize = 8 + 4;

/// Files written plus the already-unlocked imported vault.
pub struct ImportedData {
   pub files_written: usize,
   pub vault: Vault,
   /// Sealed stores removed from the destination because the archive did not carry them.
   ///
   /// Each name is the store's `data/`-relative path (a root file, or `railgun/{chain}.db`). Empty
   /// unless the archive brought a different vault key — see [`remove_stores_the_archive_omits`].
   /// Reported to the user: those stores' contents are gone with the key that sealed them.
   pub removed: Vec<String>,
}

/// Verify the archive, stage it, then swap it over matching files under `dest_data_dir`.
///
/// Returns the unlocked vault. Nothing in `dest_data_dir` is deleted or overwritten until every
/// archive entry has been written successfully into a staging directory beside it, so a failure
/// mid-copy leaves the profile exactly as it was.
///
/// `previous_key` is the key held by the vault this import replaces, when it could be read. It is
/// only used to decide whether the destination's sealed stores survive — see
/// [`remove_stores_the_archive_omits`], which runs *after* the swap.
pub fn import_data_from_zip(
   zip_path: &Path,
   dest_data_dir: &Path,
   credentials: Credentials,
   previous_key: Option<&WalletStateKey>,
) -> Result<ImportedData, anyhow::Error> {
   let file = File::open(zip_path).map_err(|e| {
      anyhow!(
         "Failed to open archive {}: {e}",
         zip_path.display()
      )
   })?;
   let mut archive = ZipArchive::new(file).map_err(|e| anyhow!("Invalid zip archive: {e}"))?;

   let entries = list_allowed_entries(&mut archive)?;
   let (vault, imported_key) = verify_imported_vault(&mut archive, &entries, credentials)?;

   std::fs::create_dir_all(dest_data_dir)?;
   if let Err(e) = restrict_dir_to_owner(dest_data_dir) {
      tracing::warn!("Failed to restrict imported data dir: {e}");
   }

   let mut ordered = entries;
   ordered.sort_by_key(|(_, parts)| write_priority(parts));

   // Stage every entry first. The `TempDir` is removed when it drops, so a read/write failure in
   // this loop — a corrupt entry, a full disk — cleans up after itself and never touches the live
   // profile.
   let staging = tempfile::Builder::new()
      .prefix(STAGING_DIR_PREFIX)
      .tempdir_in(dest_data_dir)
      .map_err(|e| anyhow!("Failed to create import staging dir: {e}"))?;
   if let Err(e) = restrict_dir_to_owner(staging.path()) {
      tracing::warn!("Failed to restrict import staging dir: {e}");
   }

   for (index, parts) in &ordered {
      let mut zip_file = archive.by_index(*index)?;
      let staged = join_parts(staging.path(), parts);
      write_private_from_reader(&staged, &mut zip_file)?;
      restrict_parents(staging.path(), parts);
   }

   // Confirm every entry actually landed before the first install. The staging loop is the only
   // writer, so a missing staged file means one entry could not be materialised — installing the
   // others would leave a half-swapped profile: the archive's vault beside the previous state, or
   // the reverse.
   for (_, parts) in &ordered {
      let staged = join_parts(staging.path(), parts);
      if !staged.is_file() {
         return Err(anyhow!(
            "Import staging is incomplete: data/{} was not written",
            parts.join("/")
         ));
      }
   }

   // Every entry landed. Swap them into place. The order is `write_priority` ascending, so the
   // vault is moved last: until that final rename the vault on disk is still the old one.
   for (_, parts) in &ordered {
      let staged = join_parts(staging.path(), parts);
      let dest = join_parts(dest_data_dir, parts);
      if let Some(parent) = dest.parent() {
         if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
         }
      }
      move_into_place(&staged, &dest)?;
      restrict_parents(dest_data_dir, parts);
   }

   // Only now, with the archive fully in place, remove the sealed stores it does not carry: a store
   // removed before the swap would be unrecoverable if the import then failed.
   let removed = remove_stores_the_archive_omits(
      dest_data_dir,
      &ordered,
      &imported_key,
      previous_key,
   );

   Ok(ImportedData {
      files_written: ordered.len(),
      vault,
      removed,
   })
}

/// Move a staged file over its destination.
///
/// Unix `rename` replaces an existing file atomically, so the swap is a no-op copy. Windows refuses
/// to rename over an existing path; there we fall back to the owner-only `tmp` + rename write the
/// rest of `data/` uses. Either way the destination is never removed before a replacement exists.
fn move_into_place(staged: &Path, dest: &Path) -> Result<(), anyhow::Error> {
   match std::fs::rename(staged, dest) {
      Ok(()) => Ok(()),
      Err(_) => {
         let file = File::open(staged)?;
         write_private_from_reader(dest, file)
      }
   }
}

/// Remove the destination's sealed stores that this archive does not carry.
///
/// A sealed store opens only with the key in `vault.data`, and an import replaces that file. When
/// the archive brings a *different* key, every sealed store it does not carry is sealed under a key
/// that no longer exists anywhere: nothing can read it again. Leaving it in place is what makes the
/// store silently revert to its defaults — and, for tx history, what leaves `tx_history.db` failing
/// to open on every later start while new transactions stop being written at all, since the open
/// failure falls back to a handle that does not persist.
///
/// The *same* key means the stores on disk still open, so they stay: an archive re-imported on the
/// install it was taken from — a selective export, or a restore — keeps them. That is also why an
/// unknown previous key removes nothing: it is not evidence that a store is unreadable.
///
/// The per-chain Railgun databases (`data/railgun/railgun:{chain}.db`) are sealed with a vault-held
/// key too, but they are not [`PersistedFile`]s, so the catalog walk above never sees them. They are
/// enumerated from disk and put through the same decision.
///
/// Returns the `data/`-relative paths removed, so the caller can tell the user what went with the
/// old vault.
fn remove_stores_the_archive_omits(
   dest_data_dir: &Path,
   entries: &[(usize, Vec<String>)],
   imported_key: &WalletStateKey,
   previous_key: Option<&WalletStateKey>,
) -> Vec<String> {
   let Some(previous_key) = previous_key else {
      return Vec::new();
   };
   if previous_key.matches(imported_key) {
      return Vec::new();
   }

   let mut removed = Vec::new();

   for file in PersistedFile::ALL.iter().copied().filter(|f| f.is_sealed()) {
      let carried = entries.iter().any(|(_, parts)| parts.as_slice() == [file.name()]);

      if carried {
         continue;
      }

      let path = dest_data_dir.join(file.name());
      if !path.is_file() {
         continue;
      }

      match std::fs::remove_file(&path) {
         Ok(()) => {
            tracing::warn!(
               "Removed {}: sealed with the replaced vault's key and not in the archive",
               file.name()
            );
            removed.push(file.name().to_string());
         }
         Err(e) => tracing::error!("Failed to remove {}: {e}", path.display()),
      }
   }

   // `railgun:{chain}.db` is sealed with a vault-held key, but is not a `PersistedFile`: enumerate
   // what is actually on disk and match it against the archive's `["railgun", name]` entries.
   let railgun_dir = PersistedTree::Railgun.dir_name();
   for name in railgun_db_files(dest_data_dir) {
      let carried = entries
         .iter()
         .any(|(_, parts)| parts.as_slice() == [railgun_dir, name.as_str()]);

      if carried {
         continue;
      }

      let path = dest_data_dir.join(railgun_dir).join(&name);
      match std::fs::remove_file(&path) {
         Ok(()) => {
            tracing::warn!(
               "Removed {railgun_dir}/{name}: sealed with the replaced vault's key and not in the archive"
            );
            removed.push(format!("{railgun_dir}/{name}"));
         }
         Err(e) => tracing::error!("Failed to remove {}: {e}", path.display()),
      }
   }

   removed
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
            file.name()?
         ));
      }

      let Some(enclosed) = file.enclosed_name() else {
         return Err(anyhow!("Unsafe zip path: {}", file.name()?));
      };
      let Some(parts) = archive_rel_parts(&enclosed) else {
         return Err(anyhow!(
            "Archive path must be under data/: {}",
            file.name()?
         ));
      };
      // The clear-signing descriptor cache is gone: descriptors now come only from the embedded,
      // pin-verified pack. An archive written by an older Zeus still carries it, and refusing that
      // entry would make every pre-refactor backup unimportable on first run — skip it, don't stage.
      if parts.first() == Some(&"clear_signing".to_string()) {
         tracing::warn!(
            "Skipping legacy clear-signing entry in archive: {}",
            file.name()?
         );
         continue;
      }
      if !is_allowed_archive_parts(&parts) {
         return Err(anyhow!(
            "Refusing unknown file in archive: {}",
            file.name()?
         ));
      }

      // Two spellings of one path — `data/x` and `data/./x` are distinct zip entries, because the
      // central directory keys on the raw name — normalise to the same `parts`. Keep the first:
      // both the staged file and the destination are derived from `parts`, so a later duplicate
      // would stage over the first, and the swap would then rename that one staged file twice. The
      // second `move_into_place` fails on the consumed source and aborts the swap, leaving the
      // archive's `wallet_state.data` beside the previous install's `vault.data`. `index_of` reads
      // the first occurrence during verification, so the first is the one that gets installed.
      if entries.iter().any(|(_, existing)| existing == &parts) {
         tracing::warn!(
            "Ignoring duplicate archive entry {} (already present as data/{})",
            file.name()?,
            parts.join("/")
         );
         continue;
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
) -> Result<(Vault, WalletStateKey), anyhow::Error> {
   let vault_idx = index_of(entries, PersistedFile::Vault)
      .ok_or_else(|| anyhow!("Archive is missing data/vault.data"))?;
   let state_idx = index_of(entries, PersistedFile::WalletState)
      .ok_or_else(|| anyhow!("Archive is missing data/wallet_state.data"))?;

   let vault_bytes = read_entry(archive, vault_idx)?;
   let state_bytes = read_entry(archive, state_idx)?;

   // Refuse an implausibly short blob before any decryptor sees it: `decrypt_data` indexes the
   // ncrypt-me header without a length check, so a blob under 12 bytes panics inside the
   // dependency. `data/wallet_state.data` fails cleanly on its own, but the guard names it too so
   // both archive reads are refused the same way.
   ensure_sealed_blob_long_enough(PersistedFile::Vault.name(), &vault_bytes)?;
   ensure_sealed_blob_long_enough(PersistedFile::WalletState.name(), &state_bytes)?;

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

   Ok((vault, key))
}

fn read_entry(archive: &mut ZipArchive<File>, index: usize) -> Result<Vec<u8>, anyhow::Error> {
   let mut file = archive.by_index(index)?;
   let mut buf = Vec::new();
   file.read_to_end(&mut buf)?;
   Ok(buf)
}

/// Refuse a sealed blob too short to carry `ncrypt-me`'s header.
///
/// `file` is the `data/`-relative name, named in the error alongside the length so the refusal
/// identifies the entry without the caller having to map it back.
fn ensure_sealed_blob_long_enough(file: &str, bytes: &[u8]) -> Result<(), anyhow::Error> {
   if bytes.len() >= MIN_SEALED_BLOB_BYTES {
      return Ok(());
   }
   Err(anyhow!(
      "data/{file} is {} bytes, shorter than the {MIN_SEALED_BLOB_BYTES}-byte minimum for a sealed store",
      bytes.len()
   ))
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
   use crate::data_export::{ExportOptions, export_data_to_zip};
   use crate::{Vault, WalletState};
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

   /// A sealed vault + wallet state pair, and the key that seals every store beside them.
   fn sample_encrypted_vault(credentials: Credentials) -> (Vec<u8>, Vec<u8>, WalletStateKey) {
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
      (sealed_vault, sealed_state, key)
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

   fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
      haystack.windows(needle.len()).position(|w| w == needle)
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
      let err = match import_data_from_zip(&zip_path, &dest, creds("u", "p"), None) {
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
      let err = match import_data_from_zip(&zip_path, &dest, creds("u", "p"), None) {
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
      let err = match import_data_from_zip(&zip_path, &dest, creds("u", "p"), None) {
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
      let (vault_bytes, state_bytes, _) = sample_encrypted_vault(credentials.clone());

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
      fs::write(dest_data.join("price_data.data"), b"keep-me").unwrap();

      // No previous key: the sealed `price_data.data` below is left alone, because nothing here says
      // it cannot be read — see `remove_stores_the_archive_omits`.
      let mut imported = import_data_from_zip(&zip_path, &dest_data, credentials, None).unwrap();
      assert_eq!(imported.files_written, 4);
      assert!(imported.removed.is_empty());
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
         fs::read(dest_data.join("price_data.data")).unwrap(),
         b"keep-me"
      );
   }

   /// An archive written by an older Zeus carries `data/clear_signing/`. That cache no longer
   /// exists, so its entry must be skipped instead of failing the whole restore on first run.
   #[test]
   fn legacy_descriptor_cache_entries_are_skipped() {
      let credentials = creds("user", "pass");
      let (vault_bytes, state_bytes, _) = sample_encrypted_vault(credentials.clone());

      let dir = tempfile::tempdir().unwrap();
      let zip_path = dir.path().join("legacy.zip");
      write_zip(
         &zip_path,
         &[
            ("data/vault.data", &vault_bytes),
            ("data/wallet_state.data", &state_bytes),
            ("data/clear_signing/index.eip712.json", b"{}"),
         ],
      );

      let dest = dir.path().join("out");
      fs::create_dir(&dest).unwrap();

      let mut imported = import_data_from_zip(&zip_path, &dest, credentials, None)
         .expect("a legacy archive must still import");
      imported.vault.erase();

      assert_eq!(
         imported.files_written, 2,
         "the legacy cache entry is not staged"
      );
      assert!(
         !dest.join("clear_signing").exists(),
         "the obsolete cache tree must not be recreated"
      );
      assert_eq!(
         fs::read(dest.join("vault.data")).unwrap(),
         vault_bytes
      );
      assert_eq!(
         fs::read(dest.join("wallet_state.data")).unwrap(),
         state_bytes
      );
   }

   #[test]
   fn wrong_credentials_do_not_write() {
      let (vault_bytes, state_bytes, _) = sample_encrypted_vault(creds("user", "pass"));
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

      let err = match import_data_from_zip(&zip_path, &dest, creds("user", "wrong"), None) {
         Err(e) => e,
         Ok(_) => panic!("expected unlock error"),
      };
      assert!(err.to_string().contains("unlock vault"));
      assert!(!dest.join("vault.data").exists());
      assert_eq!(fs::read(dest.join("theme.json")).unwrap(), b"old");
   }

   /// A sealed store the archive does not carry is sealed with the key of the vault being replaced,
   /// so once the archive's vault is adopted nothing can read it again. Leaving it is what silently
   /// reverts that store to its defaults — and, for tx history, what leaves the wrong-key file
   /// failing to open on every later start while new transactions stop being written at all.
   #[test]
   fn a_sealed_store_the_archive_omits_is_removed_when_the_key_changes() {
      let credentials = creds("user", "pass");
      let (vault_bytes, state_bytes, _archive_key) = sample_encrypted_vault(credentials.clone());
      // The install being imported over: a different vault, so a different key.
      let (_old_vault, _old_state, old_key) = sample_encrypted_vault(creds("other", "other"));

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
      // Two sealed stores the archive does not carry, and a plaintext file that is not key-bound.
      fs::write(
         dest.join("tx_history.db"),
         b"sealed under the old key",
      )
      .unwrap();
      fs::write(
         dest.join("security.data"),
         b"sealed under the old key",
      )
      .unwrap();
      fs::write(dest.join("theme.json"), b"not key-bound").unwrap();

      let mut imported =
         import_data_from_zip(&zip_path, &dest, credentials, Some(&old_key)).unwrap();

      imported.vault.erase();

      let mut removed = imported.removed.clone();
      removed.sort_unstable();
      assert_eq!(
         removed,
         vec!["security.data".to_string(), "tx_history.db".to_string()]
      );
      assert!(!dest.join("tx_history.db").exists());
      assert!(!dest.join("security.data").exists());
      assert_eq!(
         fs::read(dest.join("theme.json")).unwrap(),
         b"not key-bound",
         "a file that is not sealed with the key is not affected by the import"
      );
   }

   /// The same key means the sealed stores already on disk still open, so an archive re-imported on
   /// the install it was taken from — a selective export, or a restore — keeps them.
   #[test]
   fn a_sealed_store_the_archive_omits_is_kept_when_the_key_is_the_same() {
      let credentials = creds("user", "pass");
      let (vault_bytes, state_bytes, archive_key) = sample_encrypted_vault(credentials.clone());

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
      fs::write(dest.join("pool_data.data"), b"still readable").unwrap();

      let mut imported =
         import_data_from_zip(&zip_path, &dest, credentials, Some(&archive_key)).unwrap();

      imported.vault.erase();

      assert!(imported.removed.is_empty());
      assert_eq!(
         fs::read(dest.join("pool_data.data")).unwrap(),
         b"still readable",
         "the same key still opens it"
      );
   }

   /// A Railgun DB the archive omits is sealed with the replaced vault's key exactly like a sealed
   /// root store, so it must be removed and named. One the archive carries is replaced, not removed,
   /// and the public events snapshot is not key-bound at all.
   #[test]
   fn a_railgun_db_the_archive_omits_is_removed_when_the_key_changes() {
      let credentials = creds("user", "pass");
      let (vault_bytes, state_bytes, _archive_key) = sample_encrypted_vault(credentials.clone());
      let (_old_vault, _old_state, old_key) = sample_encrypted_vault(creds("other", "other"));

      let dir = tempfile::tempdir().unwrap();
      let zip_path = dir.path().join("zeus-data.zip");
      write_zip(
         &zip_path,
         &[
            ("data/vault.data", &vault_bytes),
            ("data/wallet_state.data", &state_bytes),
            ("data/railgun/railgun:1.db", b"archive db"),
         ],
      );

      let dest = dir.path().join("out");
      fs::create_dir(&dest).unwrap();
      fs::create_dir(dest.join("railgun")).unwrap();
      fs::write(
         dest.join("railgun").join("railgun:1.db"),
         b"sealed under the old key",
      )
      .unwrap();
      fs::write(
         dest.join("railgun").join("railgun:137.db"),
         b"sealed under the old key",
      )
      .unwrap();
      fs::write(
         dest.join("railgun").join("events-snapshot:1.db"),
         b"public cache",
      )
      .unwrap();

      let mut imported =
         import_data_from_zip(&zip_path, &dest, credentials, Some(&old_key)).unwrap();

      imported.vault.erase();

      assert_eq!(
         imported.removed,
         vec!["railgun/railgun:137.db".to_string()],
         "only the omitted per-chain DB is reported"
      );
      assert!(
         !dest.join("railgun").join("railgun:137.db").exists(),
         "a Railgun DB the archive omits cannot survive the new key"
      );
      assert_eq!(
         fs::read(dest.join("railgun").join("railgun:1.db")).unwrap(),
         b"archive db",
         "a Railgun DB the archive carries is replaced, not removed"
      );
      assert!(
         dest.join("railgun").join("events-snapshot:1.db").exists(),
         "the events snapshot is a public cache, not sealed with a vault key"
      );
   }

   /// The same key still opens the on-disk Railgun databases, so a selective export or a restore
   /// keeps them.
   #[test]
   fn a_railgun_db_the_archive_omits_is_kept_when_the_key_is_the_same() {
      let credentials = creds("user", "pass");
      let (vault_bytes, state_bytes, archive_key) = sample_encrypted_vault(credentials.clone());

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
      fs::create_dir(dest.join("railgun")).unwrap();
      fs::write(
         dest.join("railgun").join("railgun:1.db"),
         b"still readable",
      )
      .unwrap();

      let mut imported =
         import_data_from_zip(&zip_path, &dest, credentials, Some(&archive_key)).unwrap();

      imported.vault.erase();

      assert!(imported.removed.is_empty());
      assert_eq!(
         fs::read(dest.join("railgun").join("railgun:1.db")).unwrap(),
         b"still readable",
         "the same key still opens it"
      );
   }

   /// A failure part-way through the copy must not touch the live profile. The archive is staged
   /// off to the side, so the corrupt entry fails before any destination file is replaced — under
   /// the old delete-then-write order the first entry would already be in place.
   #[test]
   fn a_failed_write_leaves_the_live_directory_untouched() {
      let credentials = creds("user", "pass");
      let (vault_bytes, state_bytes, _key) = sample_encrypted_vault(credentials.clone());

      let dir = tempfile::tempdir().unwrap();
      let zip_path = dir.path().join("zeus-data.zip");

      let corrupt = b"CORRUPT-ME-CORRUPT-ME-CORRUPT-ME";
      let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
      let file = File::create(&zip_path).unwrap();
      let mut zip = ZipWriter::new(file);
      // Written first so the old order would land it in the live profile before the corrupt entry.
      zip.start_file("data/providers.data", options).unwrap();
      zip.write_all(b"new providers payload").unwrap();
      zip.start_file("data/theme.json", options).unwrap();
      zip.write_all(corrupt).unwrap();
      zip.start_file("data/wallet_state.data", options).unwrap();
      zip.write_all(&state_bytes).unwrap();
      zip.start_file("data/vault.data", options).unwrap();
      zip.write_all(&vault_bytes).unwrap();
      zip.finish().unwrap();

      // Break the stored payload's checksum without touching the entry the vault check reads.
      let mut raw = fs::read(&zip_path).unwrap();
      let pos = find_subslice(&raw, corrupt).expect("the stored payload must be findable");
      raw[pos] ^= 0xff;
      fs::write(&zip_path, &raw).unwrap();

      let dest = dir.path().join("out");
      fs::create_dir(&dest).unwrap();
      fs::write(
         dest.join("providers.data"),
         b"pre-existing providers",
      )
      .unwrap();
      fs::write(dest.join("theme.json"), b"pre-existing theme").unwrap();
      fs::write(
         dest.join("wallet_state.data"),
         b"pre-existing state",
      )
      .unwrap();
      fs::write(dest.join("vault.data"), b"pre-existing vault").unwrap();

      let err = match import_data_from_zip(&zip_path, &dest, credentials, None) {
         Err(e) => e,
         Ok(_) => panic!("expected the corrupt entry to fail the import"),
      };
      assert!(
         err.to_string().contains("checksum"),
         "expected a checksum failure, got: {err}"
      );

      assert_eq!(
         fs::read(dest.join("providers.data")).unwrap(),
         b"pre-existing providers"
      );
      assert_eq!(
         fs::read(dest.join("theme.json")).unwrap(),
         b"pre-existing theme"
      );
      assert_eq!(
         fs::read(dest.join("wallet_state.data")).unwrap(),
         b"pre-existing state"
      );
      assert_eq!(
         fs::read(dest.join("vault.data")).unwrap(),
         b"pre-existing vault"
      );

      let staging: Vec<String> = fs::read_dir(&dest)
         .unwrap()
         .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
         .filter(|name| name.starts_with(STAGING_DIR_PREFIX))
         .collect();
      assert!(
         staging.is_empty(),
         "staging left behind: {staging:?}"
      );
   }

   /// Two archive entries that normalise to one `data/`-relative path — `data/x` and `data/./x` —
   /// are distinct zip entries (the central directory keys on the raw name) but collide once the
   /// `./` is dropped, so both would stage into one file and the swap would rename that file twice.
   /// The fix keeps the first, which is also the entry `index_of` verifies, so the import completes
   /// and the first-spelled payload is the one installed. Both payload *and* vault must land: the
   /// whole point is to avoid the half-swapped profile where the archive's `wallet_state.data` sits
   /// beside the previous install's `vault.data` (whose key can no longer read it).
   #[test]
   fn two_spellings_of_one_path_import_as_one_entry() {
      let credentials = creds("user", "pass");
      let (vault_bytes, state_bytes, _) = sample_encrypted_vault(credentials.clone());

      let later_spelling = b"the data/./wallet_state.data payload, which must lose";

      let dir = tempfile::tempdir().unwrap();
      let zip_path = dir.path().join("collision.zip");
      write_zip(
         &zip_path,
         &[
            ("data/wallet_state.data", &state_bytes),
            (
               "data/./wallet_state.data",
               later_spelling.as_slice(),
            ),
            ("data/vault.data", &vault_bytes),
         ],
      );

      let dest = dir.path().join("out");
      fs::create_dir(&dest).unwrap();

      let mut imported = match import_data_from_zip(&zip_path, &dest, credentials, None) {
         Ok(imported) => imported,
         Err(e) => panic!("the two spellings must fold into one entry, got: {e}"),
      };
      imported.vault.erase();

      assert_eq!(
         imported.files_written, 2,
         "the duplicate spelling is dropped, not counted as a file"
      );
      assert_eq!(
         fs::read(dest.join("wallet_state.data")).unwrap(),
         state_bytes,
         "the first spelling is the one installed — and the one verification read"
      );
      assert_eq!(
         fs::read(dest.join("vault.data")).unwrap(),
         vault_bytes,
         "the vault still lands, so the profile is not left half-swapped"
      );
   }

   /// A sealed store shorter than `ncrypt-me`'s header (8 bytes) plus its encrypted-info length
   /// field (4) makes `decrypt_data` index `&data[0..8]` past the end: a panic inside the
   /// dependency, not a clean error. The import must refuse it by name before any decryptor runs
   /// and leave the live directory byte-identical.
   #[test]
   fn a_short_vault_blob_is_refused_before_it_reaches_a_decryptor() {
      let credentials = creds("user", "pass");
      let (_vault_bytes, state_bytes, _) = sample_encrypted_vault(credentials.clone());

      let dir = tempfile::tempdir().unwrap();
      let zip_path = dir.path().join("short-vault.zip");
      write_zip(
         &zip_path,
         &[
            ("data/vault.data", b"0123"),
            ("data/wallet_state.data", &state_bytes),
         ],
      );

      let dest = dir.path().join("out");
      fs::create_dir(&dest).unwrap();
      fs::write(dest.join("keep-me.txt"), b"untouched").unwrap();

      let err = match import_data_from_zip(&zip_path, &dest, credentials, None) {
         Err(e) => e,
         Ok(_) => panic!("a 4-byte vault must be refused, not decrypted"),
      };
      let msg = err.to_string();
      assert!(msg.contains("data/vault.data"), "{msg}");
      assert!(msg.contains("4 bytes"), "{msg}");

      assert!(!dest.join("vault.data").exists());
      assert!(!dest.join("wallet_state.data").exists());
      assert_eq!(
         fs::read(dest.join("keep-me.txt")).unwrap(),
         b"untouched"
      );
   }
}
