//! System-tray icon: show/hide the wallet window, lock it, or quit.
//!
//! The tray is built once, on the GUI thread, from [`ZeusApp::new`](crate::gui::app::ZeusApp::new).
//! Its callbacks fire on tray-icon's own thread (its D-Bus worker on Linux), so they never
//! touch the eframe app: they push a [`TrayAction`] and call
//! [`Context::request_repaint`]. [`ZeusApp::logic`](crate::gui::app::ZeusApp::logic) drains
//! them — that hook is the only one eframe keeps calling while the root window is hidden,
//! which is exactly the state the tray puts the app in.

use crate::assets::{ZEUS_TRAY, decode_mark};
use crate::utils::RT;
use egui::Context;
use std::sync::mpsc::{Receiver, channel};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

/// A tray interaction, delivered to [`ZeusApp::logic`](crate::gui::app::ZeusApp::logic).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
   /// Left click on the icon, or the menu item: show the window if hidden, hide it if shown.
   Toggle,
   /// Lock the wallet without quitting.
   Lock,
   /// Save and exit — the same path as the title-bar close button.
   Quit,
}

pub struct Tray {
   /// Dropping this removes the icon from the tray, so it lives for the app's lifetime.
   _icon: TrayIcon,
   actions: Receiver<TrayAction>,
}

impl Tray {
   /// Create the tray icon and its menu.
   ///
   /// Runs before the vault is unlocked, so it must not touch wallet state.
   pub fn build(egui_ctx: Context) -> Result<Self, Box<dyn std::error::Error>> {
      let (tx, actions) = channel();

      let (rgba, width, height) = decode_mark(ZEUS_TRAY)?;
      let icon = Icon::from_rgba(rgba, width, height)?;

      let menu = Menu::new();
      let toggle = MenuItem::new("Show / Hide Zeus", true, None);
      let lock = MenuItem::new("Lock Wallet", true, None);
      let quit = MenuItem::new("Quit Zeus", true, None);
      menu.append_items(&[&toggle, &lock, &PredefinedMenuItem::separator(), &quit])?;

      let builder = TrayIconBuilder::new()
         .with_menu(Box::new(menu))
         // Left click toggles the window, right click opens the menu. Windows and
         // macOS also open the menu on a *left* click by default, which would drop it
         // on top of the window we just restored; Linux ignores this (its StatusNotifier
         // host owns the right-click menu).
         .with_menu_on_left_click(false)
         .with_icon(icon);

      // Which attribute carries the hover text is backend-specific, and setting both
      // makes the Linux host draw the name twice: its `ksni` tooltip is
      // `(title, description)` = `(title, tooltip)`. Windows and macOS read the tooltip
      // only; Linux/BSD read the item *title*, and with the tooltip alone that title is
      // empty, so the host renders nothing. Both measured over D-Bus:
      //
      //   tooltip only -> Title='',      ToolTip=('',      'Zeus')
      //   title only   -> Title='Zeus',  ToolTip=('Zeus',  '')
      //   both         -> Title='Zeus',  ToolTip=('Zeus',  'Zeus')  <- drawn twice
      #[cfg(any(target_os = "linux", target_os = "freebsd"))]
      let builder = builder.with_title("Zeus");
      #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
      let builder = builder.with_tooltip("Zeus");

      let icon_handle = builder.build()?;

      // Ask the session bus whether a host is there to draw it (Linux/BSD). Building the
      // icon says nothing about that, and the answer gates hiding the window.
      refresh_host_availability();

      // The handlers capture plain `Send` values — a `MenuId` clone and the egui
      // context — never the `TrayIcon`, which is not `Send`.
      let toggle_id = toggle.id().clone();
      let lock_id = lock.id().clone();
      let quit_id = quit.id().clone();

      // Left click, on **release only**. Windows and macOS emit a `Click` for the
      // button-down *and* the button-up of a single press, so reacting to every
      // `Click` toggles twice: the window flashes back and vanishes again (with a
      // second notification). The Linux `ksni` backend emits a single `Up`, so this
      // filter drops nothing there.
      {
         let ctx = egui_ctx.clone();
         let tx = tx.clone();
         TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
            if let TrayIconEvent::Click {
               button: MouseButton::Left,
               button_state: MouseButtonState::Up,
               ..
            } = event
            {
               let _ = tx.send(TrayAction::Toggle);
               ctx.request_repaint();
            }
         }));
      }

      {
         let ctx = egui_ctx.clone();
         MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            let action = if *event.id() == quit_id {
               TrayAction::Quit
            } else if *event.id() == lock_id {
               TrayAction::Lock
            } else if *event.id() == toggle_id {
               TrayAction::Toggle
            } else {
               return;
            };
            let _ = tx.send(action);
            ctx.request_repaint();
         }));
      }

      Ok(Self {
         _icon: icon_handle,
         actions,
      })
   }

   /// Every interaction since the last call, without blocking.
   pub fn drain(&self) -> Vec<TrayAction> {
      self.actions.try_iter().collect()
   }
}

/// Whether an icon we register would actually be drawn (see [`watcher`]).
///
/// A `false` answer means hiding the window must not happen yet.
pub fn host_available() -> bool {
   watcher::present()
}

/// Ask the session bus again, in the background. Cheap enough for the frame path.
pub fn refresh_host_availability() {
   watcher::refresh();
}

/// The StatusNotifier host question, asked of the session bus.
///
/// Building the icon proves nothing on Linux: the `ksni` backend runs with
/// `assume_sni_available(true)`, so `Tray::build` succeeds even where no host exists — ksni's
/// own docs say the icon "may never appear if SNI support is truly absent". A window hidden
/// behind an icon nobody draws is unreachable (only a dapp prompt or a restart brings it
/// back), so this answer decides whether hiding is allowed at all.
///
/// The probe is a D-Bus round trip and never runs on the frame path: it starts when the tray
/// is built and is re-asked by a minimize that finds the answer unusable. Until a probe
/// answers, the answer is `false`, which keeps the window on screen — the safe direction.
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
mod watcher {
   use std::sync::atomic::{AtomicBool, Ordering};

   /// The name a StatusNotifier host owns (KDE's, and the name the GNOME AppIndicator
   /// extension takes).
   const WATCHER: &str = "org.kde.StatusNotifierWatcher";

   static PRESENT: AtomicBool = AtomicBool::new(false);
   static PROBING: AtomicBool = AtomicBool::new(false);

   /// Ask the session bus, off the frame path.
   pub fn refresh() {
      // One probe at a time: a minimize is noticed on every frame until it takes effect.
      if PROBING.swap(true, Ordering::SeqCst) {
         return;
      }

      // A plain thread rather than `RT.spawn_blocking`: this is zbus's blocking API, and
      // zbus `block_on`s a runtime of its own, which tokio refuses to nest inside a runtime
      // thread. Under `async-io` that is only a hazard on paper; running off every runtime
      // makes it a non-question.
      std::thread::spawn(move || {
         let present = ask().unwrap_or(false);
         PRESENT.store(present, Ordering::Relaxed);
         PROBING.store(false, Ordering::SeqCst);
      });
   }

   /// The last answered result.
   pub fn present() -> bool {
      PRESENT.load(Ordering::Relaxed)
   }

   fn ask() -> zbus::Result<bool> {
      let connection = zbus::blocking::Connection::session()?;
      let dbus = zbus::blocking::fdo::DBusProxy::new(&connection)?;
      let watcher = zbus::names::BusName::from_static_str(WATCHER).map_err(zbus::Error::Names)?;

      // The proxy reports its own error type; the caller only cares that the bus could not
      // be asked, and folds that into "no host".
      dbus.name_has_owner(watcher).map_err(|e| zbus::Error::FDO(Box::new(e)))
   }
}

/// Windows and macOS report a refused icon registration themselves (`Shell_NotifyIcon`,
/// `NSStatusBar`), so an icon that built is an icon the OS is showing.
#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
mod watcher {
   pub fn refresh() {}

   pub fn present() -> bool {
      true
   }
}

/// Hide the root window, leaving the tray icon as the only way back.
///
/// Targets [`egui::ViewportId::ROOT`] explicitly rather than the context's *current*
/// viewport: `send_viewport_cmd` resolves that to whatever viewport is mid-pass, which
/// is only meaningful on the frame path, and one caller here runs off it.
pub fn hide_window(egui_ctx: &Context) {
   egui_ctx.send_viewport_cmd_to(
      egui::ViewportId::ROOT,
      egui::ViewportCommand::Visible(false),
   );
   // Clear the minimized state too: the window is usually hidden straight from
   // the OS minimize button, and a window that is *both* hidden and minimized
   // pops back into the taskbar instead of reappearing in front.
   egui_ctx.send_viewport_cmd_to(
      egui::ViewportId::ROOT,
      egui::ViewportCommand::Minimized(false),
   );
   egui_ctx.request_repaint();
}

/// Bring the root window back, in front of whatever the user was doing.
///
/// Explicit [`egui::ViewportId::ROOT`] for the same reason as [`hide_window`]: this is
/// called from `GUI::bring_to_front` on a worker thread as well as from the frame path.
pub fn show_window(egui_ctx: &Context) {
   egui_ctx.send_viewport_cmd_to(
      egui::ViewportId::ROOT,
      egui::ViewportCommand::Visible(true),
   );
   egui_ctx.send_viewport_cmd_to(
      egui::ViewportId::ROOT,
      egui::ViewportCommand::Minimized(false),
   );
   egui_ctx.send_viewport_cmd_to(
      egui::ViewportId::ROOT,
      egui::ViewportCommand::Focus,
   );
   egui_ctx.request_repaint();
}

/// Show a desktop notification off the frame path.
///
/// `notify-rust` blocks, so this runs on a worker thread. A missing notification
/// daemon is logged rather than fatal — the tray icon is still there.
fn notify(summary: &'static str, body: &'static str) {
   RT.spawn_blocking(move || {
      let shown = notify_rust::Notification::new()
         .appname("Zeus")
         .summary(summary)
         .body(body)
         .timeout(notify_rust::Timeout::Milliseconds(8000))
         .show();

      if let Err(e) = shown {
         tracing::warn!("Failed to show the '{summary}' notification: {e}");
      }
   });
}

/// Tell the user where the window went.
pub fn notify_hidden() {
   notify(
      "Zeus is still running",
      "The window was minimized to the system tray. Click the Zeus tray icon to bring it back, \
       or quit from the tray menu.",
   );
}

/// Tell the user the wallet is locked, since the tray locked it with the window hidden.
pub fn notify_locked() {
   notify(
      "Zeus is locked",
      "The wallet was locked from the system tray. Open the Zeus window and enter your \
       credentials to unlock it.",
   );
}

#[cfg(test)]
mod tests {
   use super::*;

   /// `refresh_host_availability` is called from the frame path — a minimize is noticed on
   /// every frame until it takes effect — so it has to return immediately and leave the
   /// D-Bus round trip to a worker. Repeats must not queue one probe each either.
   #[test]
   fn refreshing_never_blocks_the_caller() {
      let start = std::time::Instant::now();
      for _ in 0..50 {
         refresh_host_availability();
      }

      assert!(
         start.elapsed() < std::time::Duration::from_millis(50),
         "50 refreshes took {:?}",
         start.elapsed()
      );
   }

   /// What the frame path reads has to be what the bus says. Ignored because it needs a
   /// session bus, and because "no host" is a legitimate answer (it is the case the guard
   /// exists for) — on a host-less desktop the assertion still holds, it just proves less.
   #[cfg(any(target_os = "linux", target_os = "freebsd"))]
   #[test]
   #[ignore = "needs a session bus with a StatusNotifier host"]
   fn the_probe_reports_what_the_bus_says() {
      let connection = zbus::blocking::Connection::session().expect("a session bus");
      let dbus = zbus::blocking::fdo::DBusProxy::new(&connection).expect("the bus proxy");
      let watcher = zbus::names::BusName::from_static_str("org.kde.StatusNotifierWatcher")
         .expect("a valid bus name");
      let direct = dbus.name_has_owner(watcher).expect("name_has_owner");

      refresh_host_availability();
      for _ in 0..50 {
         if host_available() == direct {
            break;
         }
         std::thread::sleep(std::time::Duration::from_millis(100));
      }

      assert_eq!(host_available(), direct);
   }
}
