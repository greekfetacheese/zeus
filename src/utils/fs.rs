//! Owner-only file writes (Unix `0600` / Windows owner DACL).
//!
//! Use this for anything under `data/` that should not be world-readable:
//! vault ciphertext, wallet state, RPC endpoints, pairing tokens, etc.

use std::path::{Path, PathBuf};

/// Write `contents` to `path`, readable/writable only by the current owner.
///
/// Creates parent directories. On Unix the file is created with mode `0o600`
/// (existing files are chmod'd back to `0o600`). On Windows the DACL is
/// replaced with a protected owner-only ACE. Other platforms fall back to
/// [`std::fs::write`].
pub fn write_private(path: &Path, contents: &[u8]) -> Result<(), anyhow::Error> {
   if let Some(parent) = path.parent() {
      if !parent.as_os_str().is_empty() {
         std::fs::create_dir_all(parent)?;
      }
   }

   #[cfg(unix)]
   {
      use std::io::Write;
      use std::os::unix::fs::OpenOptionsExt;

      let mut file = std::fs::OpenOptions::new()
         .write(true)
         .create(true)
         .truncate(true)
         .mode(0o600)
         .open(path)?;
      file.write_all(contents)?;
      file.sync_all()?;
      drop(file);

      // `OpenOptions::mode` only applies to newly created files. Tighten an
      // existing world-readable vault / wallet_state on overwrite.
      restrict_file_to_owner(path)?;
   }

   #[cfg(windows)]
   {
      std::fs::write(path, contents)?;
      restrict_file_to_owner(path)?;
   }

   #[cfg(not(any(unix, windows)))]
   {
      std::fs::write(path, contents)?;
   }

   Ok(())
}

/// Same as [`write_private`], but replace `path` via a sibling `*.tmp` + rename.
///
/// A crash mid-write leaves the previous file intact (and at worst a leftover
/// tmp). Used for `vault.data` / `wallet_state.data`.
pub fn write_private_atomic(path: &Path, contents: &[u8]) -> Result<(), anyhow::Error> {
   let tmp = tmp_path(path);
   match write_private(&tmp, contents) {
      Ok(()) => {}
      Err(e) => {
         let _ = std::fs::remove_file(&tmp);
         return Err(e);
      }
   }

   match replace_file(&tmp, path) {
      Ok(()) => Ok(()),
      Err(e) => {
         let _ = std::fs::remove_file(&tmp);
         Err(e)
      }
   }
}

fn tmp_path(path: &Path) -> PathBuf {
   let mut tmp = path.as_os_str().to_os_string();
   tmp.push(".tmp");
   PathBuf::from(tmp)
}

fn replace_file(tmp: &Path, dest: &Path) -> Result<(), anyhow::Error> {
   #[cfg(windows)]
   {
      // `rename` fails if `dest` already exists.
      if dest.exists() {
         std::fs::remove_file(dest)?;
      }
      std::fs::rename(tmp, dest)?;
   }

   #[cfg(not(windows))]
   {
      std::fs::rename(tmp, dest)?;
   }

   Ok(())
}

/// Restrict a directory to the current owner (Unix `0700`).
///
/// Other users can otherwise list `data/` even when the files inside are `0600`.
pub fn restrict_dir_to_owner(path: &Path) -> Result<(), anyhow::Error> {
   #[cfg(unix)]
   {
      use std::os::unix::fs::PermissionsExt;
      let mut perms = std::fs::metadata(path)?.permissions();
      perms.set_mode(0o700);
      std::fs::set_permissions(path, perms)?;
      Ok(())
   }

   #[cfg(not(unix))]
   {
      let _ = path;
      Ok(())
   }
}

/// Restrict `path` to the current owner (Unix `0600` / Windows protected owner DACL).
pub fn restrict_file_to_owner(path: &Path) -> Result<(), anyhow::Error> {
   #[cfg(unix)]
   {
      use std::os::unix::fs::PermissionsExt;
      let mut perms = std::fs::metadata(path)?.permissions();
      perms.set_mode(0o600);
      std::fs::set_permissions(path, perms)?;
      Ok(())
   }

   #[cfg(windows)]
   {
      restrict_file_to_owner_windows(path)
   }

   #[cfg(not(any(unix, windows)))]
   {
      let _ = path;
      Ok(())
   }
}

/// Protected DACL, current owner full access. No Users/Everyone/inherited ACEs.
#[cfg(windows)]
fn restrict_file_to_owner_windows(path: &Path) -> Result<(), anyhow::Error> {
   use anyhow::anyhow;
   use std::os::windows::ffi::OsStrExt;
   use std::ptr;

   #[link(name = "advapi32")]
   unsafe extern "system" {
      fn ConvertStringSecurityDescriptorToSecurityDescriptorW(
         string_sd: *const u16,
         revision: u32,
         sd: *mut *mut std::ffi::c_void,
         sd_size: *mut u32,
      ) -> i32;
      fn SetFileSecurityW(
         file_name: *const u16,
         security_information: u32,
         security_descriptor: *mut std::ffi::c_void,
      ) -> i32;
   }

   #[link(name = "kernel32")]
   unsafe extern "system" {
      fn LocalFree(h: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
   }

   const SDDL_REVISION_1: u32 = 1;
   const DACL_SECURITY_INFORMATION: u32 = 0x0004;
   const PROTECTED_DACL_SECURITY_INFORMATION: u32 = 0x8000_0000;

   let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
   // D:P = protected (no inheritance). OW = current owner. FA = FILE_ALL_ACCESS.
   let sddl: Vec<u16> = "D:P(A;;FA;;;OW)".encode_utf16().chain(std::iter::once(0)).collect();

   let mut sd: *mut std::ffi::c_void = ptr::null_mut();
   let converted = unsafe {
      ConvertStringSecurityDescriptorToSecurityDescriptorW(
         sddl.as_ptr(),
         SDDL_REVISION_1,
         &mut sd,
         ptr::null_mut(),
      )
   };

   if converted == 0 || sd.is_null() {
      return Err(anyhow!(
         "owner-only ACL: {}",
         std::io::Error::last_os_error()
      ));
   }

   let set = unsafe {
      SetFileSecurityW(
         wide.as_ptr(),
         DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
         sd,
      )
   };

   unsafe {
      LocalFree(sd);
   }

   if set == 0 {
      return Err(anyhow!(
         "owner-only ACL: {}",
         std::io::Error::last_os_error()
      ));
   }

   Ok(())
}

#[cfg(test)]
mod tests {
   use super::*;
   use std::fs;

   #[test]
   fn write_private_creates_owner_only_file() {
      let dir = tempfile::tempdir().unwrap();
      let path = dir.path().join("vault.data");
      write_private(&path, b"ciphertext").unwrap();
      assert_eq!(fs::read(&path).unwrap(), b"ciphertext");

      #[cfg(unix)]
      {
         use std::os::unix::fs::PermissionsExt;
         let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
         assert_eq!(mode, 0o600);
      }
   }

   #[test]
   fn write_private_tightens_existing_world_readable_file() {
      let dir = tempfile::tempdir().unwrap();
      let path = dir.path().join("wallet_state.data");
      fs::write(&path, b"old").unwrap();

      #[cfg(unix)]
      {
         use std::os::unix::fs::PermissionsExt;
         let mut perms = fs::metadata(&path).unwrap().permissions();
         perms.set_mode(0o644);
         fs::set_permissions(&path, perms).unwrap();
         let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
         assert_eq!(mode, 0o644);
      }

      write_private(&path, b"new").unwrap();
      assert_eq!(fs::read(&path).unwrap(), b"new");

      #[cfg(unix)]
      {
         use std::os::unix::fs::PermissionsExt;
         let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
         assert_eq!(mode, 0o600);
      }
   }

   #[test]
   fn write_private_creates_parent_dirs() {
      let dir = tempfile::tempdir().unwrap();
      let path = dir.path().join("nested").join("providers.json");
      write_private(&path, b"{}").unwrap();
      assert_eq!(fs::read(&path).unwrap(), b"{}");
   }

   #[test]
   fn restrict_dir_to_owner_is_owner_only() {
      let dir = tempfile::tempdir().unwrap();
      let path = dir.path().join("data");
      fs::create_dir(&path).unwrap();

      #[cfg(unix)]
      {
         use std::os::unix::fs::PermissionsExt;
         let mut perms = fs::metadata(&path).unwrap().permissions();
         perms.set_mode(0o755);
         fs::set_permissions(&path, perms).unwrap();
      }

      restrict_dir_to_owner(&path).unwrap();

      #[cfg(unix)]
      {
         use std::os::unix::fs::PermissionsExt;
         let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
         assert_eq!(mode, 0o700);
      }
   }

   #[test]
   fn write_private_atomic_replaces_without_leaving_tmp() {
      let dir = tempfile::tempdir().unwrap();
      let path = dir.path().join("vault.data");
      fs::write(&path, b"old").unwrap();

      write_private_atomic(&path, b"new").unwrap();
      assert_eq!(fs::read(&path).unwrap(), b"new");
      assert!(!dir.path().join("vault.data.tmp").exists());

      #[cfg(unix)]
      {
         use std::os::unix::fs::PermissionsExt;
         let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
         assert_eq!(mode, 0o600);
      }
   }

   #[test]
   fn write_private_atomic_creates_missing_file() {
      let dir = tempfile::tempdir().unwrap();
      let path = dir.path().join("wallet_state.data");
      write_private_atomic(&path, b"sealed").unwrap();
      assert_eq!(fs::read(&path).unwrap(), b"sealed");
      assert!(!dir.path().join("wallet_state.data.tmp").exists());
   }
}
