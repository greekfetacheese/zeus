//! System-tray icon: show/hide the wallet window, lock it, or quit.
//!
//! The tray is built once, on the GUI thread, from [`ZeusApp::new`](crate::gui::app::ZeusApp::new).
//! Its callbacks fire on tray-icon's own thread (its D-Bus worker on Linux), so they never
//! touch the eframe app: they push a [`TrayAction`] and call
//! [`Context::request_repaint`]. [`ZeusApp::logic`](crate::gui::app::ZeusApp::logic) drains
//! them — that hook is the only one eframe keeps calling while the root window is hidden,
//! which is exactly the state the tray puts the app in.

use crate::assets::TRAY_ICON_PNG;
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

      let image = image::load_from_memory(TRAY_ICON_PNG)?.into_rgba8();
      let (width, height) = image.dimensions();
      let icon = Icon::from_rgba(image.into_raw(), width, height)?;

      let menu = Menu::new();
      let toggle = MenuItem::new("Show / Hide Zeus", true, None);
      let lock = MenuItem::new("Lock Wallet", true, None);
      let quit = MenuItem::new("Quit Zeus", true, None);
      menu.append_items(&[&toggle, &lock, &PredefinedMenuItem::separator(), &quit])?;

      let icon_handle = TrayIconBuilder::new()
         .with_menu(Box::new(menu))
         // Left click toggles the window, right click opens the menu. Windows and
         // macOS also open the menu on a *left* click by default, which would drop it
         // on top of the window we just restored; Linux ignores this (its StatusNotifier
         // host owns the right-click menu).
         .with_menu_on_left_click(false)
         .with_tooltip("Zeus")
         .with_icon(icon)
         .build()?;

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

/// Hide the root window, leaving the tray icon as the only way back.
pub fn hide_window(egui_ctx: &Context) {
   egui_ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
   // Clear the minimized state too: the window is usually hidden straight from
   // the OS minimize button, and a window that is *both* hidden and minimized
   // pops back into the taskbar instead of reappearing in front.
   egui_ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
   egui_ctx.request_repaint();
}

/// Bring the root window back, in front of whatever the user was doing.
pub fn show_window(egui_ctx: &Context) {
   egui_ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
   egui_ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
   egui_ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
   egui_ctx.request_repaint();
}

/// Tell the user where the window went.
///
/// `notify-rust` blocks, so this runs on a worker thread. A missing notification
/// daemon is logged rather than fatal — the tray icon is still there.
pub fn notify_hidden() {
   RT.spawn_blocking(|| {
      let shown = notify_rust::Notification::new()
         .appname("Zeus")
         .summary("Zeus is still running")
         .body(
            "The window was minimized to the system tray. Click the Zeus tray icon to bring it \
             back, or quit from the tray menu.",
         )
         .timeout(notify_rust::Timeout::Milliseconds(8000))
         .show();

      if let Err(e) = shown {
         tracing::warn!("Failed to show the tray notification: {e}");
      }
   });
}
