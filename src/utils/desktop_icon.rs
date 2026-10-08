//! Give the Zeus binary its own icon in the Linux file manager.
//!
//! A file's icon on Linux comes from GIO's `standard::icon`, and for an ELF binary that is
//! MIME-derived: it is `application-x-executable` whatever the program is and whichever
//! `.desktop` entry names it (measured on a real install — the VS Code binary resolves to
//! `application-x-executable` even though `code.desktop` has `Exec=` pointing straight at it).
//! There is no icon slot in an ELF, and none is consulted.
//!
//! The mechanism that does work is the file manager's own per-file custom icon, which is the
//! GVFS metadata key `metadata::custom-icon`. Nemo implements it — its "Custom Icon" property
//! picker writes exactly this key. We point it at the embedded mark, written out beside the
//! binary as `zeus.png`.
//!
//! That metadata store is `~/.local/share/gvfs-metadata/`, i.e. *outside* Zeus's folder: this is
//! the one thing Zeus writes beyond its own directory (the readme says so). It is per-user, it
//! is harmless to lose, and nothing here is allowed to fail a launch — a machine without
//! `gio` (glib's CLI) just keeps the generic icon.
//!
//! Windows needs none of this: its icon is a real resource in the executable, see `build.rs`.

/// `zeus-gui --install-file-icon`
///
/// Runs the install and exits without starting the GUI. Also the repair path after moving the
/// app folder, though a normal launch re-checks and fixes that on its own.
pub const INSTALL_FLAG: &str = "--install-file-icon";

/// Name of the mark written beside the executable, for the metadata to point at.
#[cfg(target_os = "linux")]
const ICON_FILE_NAME: &str = "zeus.png";

/// The GVFS metadata key the file managers read for a per-file icon.
#[cfg(target_os = "linux")]
const CUSTOM_ICON_KEY: &str = "metadata::custom-icon";

/// Whether this process was started to install the file-manager icon.
pub fn is_install_invocation() -> bool {
   std::env::args().skip(1).any(|arg| arg == INSTALL_FLAG)
}

/// Point the file manager's icon for the running executable at the Zeus mark.
///
/// Idempotent and self-healing: the metadata holds an absolute `file://` URI, so moving the app
/// folder would leave a stale entry behind — the check below runs on every launch and rewrites
/// it for the new location. On an up-to-date install that is a single `gio info` read.
pub fn install_file_icon() {
   #[cfg(target_os = "linux")]
   if let Err(e) = install() {
      tracing::warn!("File-manager icon: {e}");
   }

   #[cfg(not(target_os = "linux"))]
   tracing::info!("The file-manager icon is a Linux feature, nothing to do");
}

#[cfg(target_os = "linux")]
fn install() -> anyhow::Result<()> {
   use anyhow::{Context, anyhow};
   use std::ffi::OsStr;

   let exe = std::env::current_exe().context("cannot resolve the running executable")?;
   let dir = exe
      .parent()
      .ok_or_else(|| anyhow!("{} has no parent directory", exe.display()))?;
   let png = dir.join(ICON_FILE_NAME);

   // Rewrite the mark only when it is missing or stale, so a steady-state launch leaves the
   // folder untouched.
   if !matches!(
      std::fs::read(&png).map(|have| have == crate::assets::ZEUS_ICON),
      Ok(true)
   ) {
      std::fs::write(&png, crate::assets::ZEUS_ICON)
         .with_context(|| format!("cannot write {}", png.display()))?;
   }

   let uri = file_uri(&png);
   if current_icon(&exe)?.as_deref() == Some(uri.as_str()) {
      return Ok(());
   }

   let args = [
      OsStr::new("set"),
      exe.as_os_str(),
      OsStr::new(CUSTOM_ICON_KEY),
      OsStr::new(&uri),
   ];
   let out = gio(args)?;
   if !out.status.success() {
      return Err(anyhow!(
         "`gio set` failed: {}",
         String::from_utf8_lossy(&out.stderr).trim()
      ));
   }

   tracing::info!(
      "Set the file-manager icon for {} to {uri}",
      exe.display()
   );
   Ok(())
}

/// The icon metadata currently recorded for `path`, if any.
#[cfg(target_os = "linux")]
fn current_icon(path: &std::path::Path) -> anyhow::Result<Option<String>> {
   use std::ffi::OsStr;

   let args = [
      OsStr::new("info"),
      OsStr::new("-a"),
      OsStr::new(CUSTOM_ICON_KEY),
      path.as_os_str(),
   ];
   let out = gio(args)?;
   if !out.status.success() {
      // A path GIO cannot describe simply has no icon recorded; not worth reporting.
      return Ok(None);
   }

   let key = format!("{CUSTOM_ICON_KEY}:");
   let stdout = String::from_utf8_lossy(&out.stdout);
   Ok(stdout
      .lines()
      .find_map(|line| line.trim().strip_prefix(&key))
      .map(|value| value.trim().to_owned()))
}

/// A `file://` URI for `path`, percent-encoding everything outside the unreserved set so the
/// value still resolves for a directory with a space (`feta recipe`) or a non-UTF-8 name.
///
/// The metadata is keyed by URI, and the file manager decodes it with `g_filename_from_uri`.
#[cfg(target_os = "linux")]
fn file_uri(path: &std::path::Path) -> String {
   use std::os::unix::ffi::OsStrExt;

   const HEX: &[u8; 16] = b"0123456789ABCDEF";

   let mut uri = String::from("file://");
   for &byte in path.as_os_str().as_bytes() {
      if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/') {
         uri.push(byte as char);
      } else {
         uri.push('%');
         uri.push(HEX[usize::from(byte >> 4)] as char);
         uri.push(HEX[usize::from(byte & 0x0F)] as char);
      }
   }

   uri
}

#[cfg(target_os = "linux")]
fn gio<I, S>(args: I) -> anyhow::Result<std::process::Output>
where
   I: IntoIterator<Item = S>,
   S: AsRef<std::ffi::OsStr>,
{
   use anyhow::Context;

   std::process::Command::new("gio")
      .args(args)
      .output()
      .context("cannot run `gio` (is glib installed?)")
}
