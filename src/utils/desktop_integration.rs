//! Zeus's Linux desktop integration: the file-manager icon, and the application-menu entry.
//!
//! Two separate things live here, with different consent.
//!
//! * [`install_file_icon`] runs on every launch. A file's icon on Linux comes from GIO's
//!   `standard::icon`, and for an ELF binary that is MIME-derived: it is
//!   `application-x-executable` whatever the program is and whichever `.desktop` entry names it
//!   (measured against a real install — the VS Code binary resolves to
//!   `application-x-executable` even though `code.desktop` sets `Exec=` to it). There is no icon
//!   slot in an ELF, and none is consulted. The mechanism that does work is the file manager's
//!   own per-file custom icon, the GVFS metadata key `metadata::custom-icon` — Nemo's "Custom
//!   Icon" picker writes exactly this key. The value is a URI, so the mark is written beside the
//!   binary as `zeus.png` and the metadata points at it.
//!
//! * [`install_desktop_entry`] / [`uninstall_desktop_entry`] are **opt-in**
//!   (`MiscConfig::desktop_integration`): a `zeus.desktop` plus the mark in the user's icon
//!   theme, so Zeus appears in the application menu with its own icon and can be pinned.
//!   Nothing here is needed for the wallet to work.
//!
//! Both write outside Zeus's own folder (`~/.local/share/gvfs-metadata/`,
//! `~/.local/share/applications/`, `~/.local/share/icons/`) — the only places Zeus writes that
//! are not its own directory. Everything is best effort: a missing `gio`, a read-only
//! `~/.local/share` or an unset `HOME` only logs. Nothing here may fail a launch.
//!
//! Windows needs none of this: its icon is a real resource in the executable, see `build.rs`.

/// The application id, and the basename of the `.desktop` entry — they have to agree.
///
/// On Wayland the compositor resolves a window's icon from `<app_id>.desktop`; on X11
/// `StartupWMClass` is matched against the window's WM_CLASS, which `with_app_id` sets.
/// Without it winit falls back to the window *title*, which carries the version — and so can
/// never match a stable entry.
pub const APP_ID: &str = "zeus";

/// `zeus --install-desktop`: opt in, install the menu entry, and exit without the GUI.
pub const INSTALL_FLAG: &str = "--install-desktop";

/// `zeus --uninstall-desktop`: opt out, remove the menu entry, and exit.
pub const UNINSTALL_FLAG: &str = "--uninstall-desktop";

/// Name of the mark written beside the executable, for the GVFS metadata to point at.
#[cfg(target_os = "linux")]
const ICON_FILE_NAME: &str = "zeus-logo.png";

/// The GVFS metadata key the file managers read for a per-file icon.
#[cfg(target_os = "linux")]
const CUSTOM_ICON_KEY: &str = "metadata::custom-icon";

/// The application-menu entry's filename.
#[cfg(target_os = "linux")]
const DESKTOP_FILE_NAME: &str = "zeus.desktop";

/// Whether this process was started to install the application-menu entry.
pub fn is_install_invocation() -> bool {
   std::env::args().skip(1).any(|arg| arg == INSTALL_FLAG)
}

/// Whether this process was started to remove the application-menu entry.
pub fn is_uninstall_invocation() -> bool {
   std::env::args().skip(1).any(|arg| arg == UNINSTALL_FLAG)
}

/// Record the menu-entry choice and make the system match it.
///
/// The CLI flags' entry point: they run without the GUI, so they never touch an in-memory
/// config. Onboarding and Settings already hold the config they are about to save, so they
/// call the install/uninstall pair directly rather than reopening the file.
pub fn set_enabled(enabled: bool) {
   use crate::core::types::MiscConfig;

   let mut config = MiscConfig::load_from_file().unwrap_or_else(|_| MiscConfig::new());
   config.set_desktop_integration(enabled);
   if let Err(e) = config.save() {
      tracing::warn!("Desktop integration: cannot record the choice: {e}");
   }

   if enabled {
      install_desktop_entry();
   } else {
      uninstall_desktop_entry();
   }
}

/// Point the file manager's icon for the running executable at the Zeus mark.
///
/// Idempotent and self-healing: the metadata holds an absolute `file://` URI, so moving the app
/// folder would leave a stale entry behind — the check below runs on every launch and rewrites
/// it for the new location. On an up-to-date install that is a single `gio info` read.
pub fn install_file_icon() {
   #[cfg(target_os = "linux")]
   if let Err(e) = install_file_icon_linux() {
      tracing::warn!("File-manager icon: {e}");
   }

   #[cfg(not(target_os = "linux"))]
   tracing::info!("The file-manager icon is a Linux feature, nothing to do");
}

/// Add Zeus to the application menu, with its own icon.
pub fn install_desktop_entry() {
   #[cfg(target_os = "linux")]
   if let Err(e) = install_desktop_entry_linux() {
      tracing::warn!("Application-menu entry: {e}");
   }

   #[cfg(not(target_os = "linux"))]
   tracing::info!("The application-menu entry is a Linux feature, nothing to do");
}

/// Remove the application-menu entry Zeus installed.
pub fn uninstall_desktop_entry() {
   #[cfg(target_os = "linux")]
   if let Err(e) = uninstall_desktop_entry_linux() {
      tracing::warn!("Application-menu entry: {e}");
   }

   #[cfg(not(target_os = "linux"))]
   tracing::info!("The application-menu entry is a Linux feature, nothing to do");
}

#[cfg(target_os = "linux")]
fn install_file_icon_linux() -> anyhow::Result<()> {
   use anyhow::{Context, anyhow};
   use std::ffi::OsStr;

   let exe = std::env::current_exe().context("cannot resolve the running executable")?;
   let dir = exe
      .parent()
      .ok_or_else(|| anyhow!("{} has no parent directory", exe.display()))?;
   let png = dir.join(ICON_FILE_NAME);

   write_if_changed(&png, crate::assets::ZEUS_ICON)?;

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

#[cfg(target_os = "linux")]
fn install_desktop_entry_linux() -> anyhow::Result<()> {
   use anyhow::Context;

   let exe = std::env::current_exe().context("cannot resolve the running executable")?;
   let exe = exe
      .to_str()
      .ok_or_else(|| anyhow::anyhow!("{} is not valid UTF-8", exe.display()))?;

   let applications = data_home()?.join("applications");
   write_if_changed(
      &applications.join(DESKTOP_FILE_NAME),
      desktop_entry(exe).as_bytes(),
   )?;

   for &(size, png) in crate::assets::ZEUS_ICON_SIZES {
      let icon = data_home()?.join(format!(
         "icons/hicolor/{size}x{size}/apps/{APP_ID}.png"
      ));
      write_if_changed(&icon, png)?;
   }

   tracing::info!("Installed the application-menu entry for {exe}");
   Ok(())
}

#[cfg(target_os = "linux")]
fn uninstall_desktop_entry_linux() -> anyhow::Result<()> {
   remove_if_exists(&data_home()?.join("applications").join(DESKTOP_FILE_NAME))?;

   for &(size, _) in crate::assets::ZEUS_ICON_SIZES {
      let icon = data_home()?.join(format!(
         "icons/hicolor/{size}x{size}/apps/{APP_ID}.png"
      ));
      remove_if_exists(&icon)?;
   }

   tracing::info!("Removed the application-menu entry");
   Ok(())
}

/// The `zeus.desktop` contents.
///
/// `TryExec` is what keeps a moved or deleted app folder from leaving a dead launcher behind:
/// per the Desktop Entry Specification, an entry whose `TryExec` is not an executable file
/// *"may be ignored (not be used in menus, for example)"*, which is how the file managers
/// behave in practice.
#[cfg(target_os = "linux")]
fn desktop_entry(exe: &str) -> String {
   format!(
      "[Desktop Entry]\n\
       Type=Application\n\
       Version=1.0\n\
       Name=Zeus\n\
       GenericName=Ethereum Wallet\n\
       Comment=A seedless, self-custodial Ethereum wallet that just works.\n\
       Exec={}\n\
       TryExec={exe}\n\
       Icon={APP_ID}\n\
       Terminal=false\n\
       Categories=Finance;Network;\n\
       StartupWMClass={APP_ID}\n\
       StartupNotify=true\n",
      exec_arg(exe)
   )
}

/// The `Exec=` form of a path: quoted when it holds anything the key's tokenizer treats
/// specially, with the reserved characters escaped and `%` doubled (it introduces a field code).
///
/// `TryExec` is not passed through this: it is a bare path, not a command line.
#[cfg(target_os = "linux")]
fn exec_arg(path: &str) -> String {
   let quote = path.chars().any(|c| c.is_whitespace() || "\"'\\><~|&;$*?#()`".contains(c));

   let mut out = String::with_capacity(path.len() + 2);
   if quote {
      out.push('"');
   }
   for c in path.chars() {
      match c {
         // Reserved inside a quoted argument, per the Desktop Entry Specification.
         '"' | '`' | '$' | '\\' => {
            out.push('\\');
            out.push(c);
         }
         '%' => out.push_str("%%"),
         _ => out.push(c),
      }
   }
   if quote {
      out.push('"');
   }
   out
}

/// `$XDG_DATA_HOME`, else `$HOME/.local/share`. A relative `XDG_DATA_HOME` is ignored, as the
/// basedir spec requires.
#[cfg(target_os = "linux")]
fn data_home() -> anyhow::Result<std::path::PathBuf> {
   use anyhow::anyhow;

   if let Some(dir) = std::env::var_os("XDG_DATA_HOME") {
      let dir = std::path::PathBuf::from(dir);
      if dir.is_absolute() {
         return Ok(dir);
      }
   }

   let home =
      std::env::var_os("HOME").ok_or_else(|| anyhow!("neither XDG_DATA_HOME nor HOME is set"))?;
   Ok(std::path::PathBuf::from(home).join(".local/share"))
}

/// Write `bytes` only when the target does not already hold them, so launching an
/// already-installed copy touches nothing.
#[cfg(target_os = "linux")]
fn write_if_changed(path: &std::path::Path, bytes: &[u8]) -> anyhow::Result<()> {
   use anyhow::Context;

   if std::fs::read(path).map(|have| have == bytes).unwrap_or(false) {
      return Ok(());
   }

   if let Some(parent) = path.parent() {
      std::fs::create_dir_all(parent)
         .with_context(|| format!("cannot create {}", parent.display()))?;
   }

   std::fs::write(path, bytes).with_context(|| format!("cannot write {}", path.display()))
}

/// Delete `path`, treating "already gone" as success.
#[cfg(target_os = "linux")]
fn remove_if_exists(path: &std::path::Path) -> anyhow::Result<()> {
   use anyhow::Context;

   match std::fs::remove_file(path) {
      Ok(()) => Ok(()),
      Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
      Err(e) => Err(e).with_context(|| format!("cannot remove {}", path.display())),
   }
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
