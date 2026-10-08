use egui::*;
use zeus_eth::types::SUPPORTED_CHAINS;

use crate::assets::{INTER_BOLD_18, icons::Icons};
use crate::core::{WalletInfo, ZeusCtx};
use crate::gui::SHARED_GUI;
use crate::gui::tray::{self, Tray, TrayAction};
use crate::gui::ui::record_input_activity;
use crate::server::run_server;
use crate::utils::{RT, TimeStamp, state::on_startup};
use eframe::{
   CreationContext,
   egui::{self, Frame},
};
use elegance::{BadgeTone, Toast};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub struct ZeusApp {
   pub style_has_been_set: bool,
   pub ctx: ZeusCtx,
   /// Once true, the next close request is allowed to proceed (after delayed cleanup).
   allow_close: Arc<AtomicBool>,
   /// Prevents spawning multiple shutdown tasks while cleanup is in flight.
   ///
   /// Shared, because the tray's Quit item starts the same shutdown from its own
   /// thread, with no egui frame in between to observe a plain flag.
   shutdown_started: Arc<AtomicBool>,
   /// The tray icon and its action queue.
   ///
   /// `None` when the tray could not be created (no StatusNotifier host, for
   /// instance). Minimize-to-tray then stays unavailable rather than hiding a
   /// window that nothing could bring back.
   tray: Option<Tray>,
   /// True while the root window is hidden in the tray.
   hidden_to_tray: bool,
}

impl ZeusApp {
   pub fn new(cc: &CreationContext) -> Self {
      let time = std::time::Instant::now();
      let egui_ctx = cc.egui_ctx.clone();

      // setup_fonts(&egui_ctx);

      // Lazy load the icons
      let egui_ctx2 = cc.egui_ctx.clone();
      RT.spawn_blocking(move || {
         SHARED_GUI.write(|shared_gui| {
            shared_gui.egui_ctx = egui_ctx2.clone();
         });

         let icons = match Icons::new(&egui_ctx2) {
            Ok(icons) => icons,
            Err(e) => {
               let title = "Fatal Error";
               let err = format!("Failed to load icons: {e}");
               Toast::new(title)
                  .description(err)
                  .tone(BadgeTone::Danger)
                  .persistent()
                  .show(&egui_ctx2);
               tracing::error!("Failed to load icons: {e}");
               return;
            }
         };

         let icons = Arc::new(icons);

         SHARED_GUI.write(|shared_gui| {
            shared_gui.icons = icons;
         });
      });

      let mut theme = SHARED_GUI.read(|shared_gui| shared_gui.theme.clone());
      let ctx = SHARED_GUI.read(|shared_gui| shared_gui.ctx.clone());

      theme.install(&egui_ctx);
      SHARED_GUI.write(|shared_gui| shared_gui.theme = theme.clone());

      tracing::info!(
         "ZeusApp loaded in {}ms",
         time.elapsed().as_millis()
      );

      let now = TimeStamp::now_as_millis().unwrap_or_default().timestamp();
      ctx.write(|ctx| {
         for chain in SUPPORTED_CHAINS {
            ctx.check_for_available_rpcs(now, chain, 0);
         }
      });

      let ctx_clone = ctx.clone();
      RT.spawn(async move {
         loop {
            if ctx_clone.vault_unlocked() {
               tracing::info!("Vault unlocked, starting syncing");
               on_startup(ctx_clone).await;
               break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
         }
      });

      let ctx_clone = ctx.clone();
      RT.spawn(async move {
         let _r = run_server(ctx_clone).await;
      });

      let ctx_autolock = ctx.clone();
      RT.spawn(async move {
         autolock_watcher(ctx_autolock).await;
      });

      // Linux only, and best effort: the file manager's icon for this binary (`gio` + GVFS
      // metadata), which re-checks itself every launch so a moved folder heals, and the
      // application-menu entry, which is opt-in and therefore only re-applied when the user
      // has asked for it. See `utils::desktop_integration`.
      RT.spawn_blocking(crate::utils::desktop_integration::install_file_icon);

      if ctx.read(|ctx| ctx.misc_config.desktop_integration()) {
         RT.spawn_blocking(crate::utils::desktop_integration::install_desktop_entry);
      }

      // Built here rather than in `main` so the icon can drive the root viewport;
      // eframe calls this from `resumed`, i.e. with the event loop already running.
      let tray = match Tray::build(cc.egui_ctx.clone()) {
         Ok(tray) => Some(tray),
         Err(e) => {
            tracing::warn!("System tray unavailable, minimize-to-tray is disabled: {e}");
            None
         }
      };

      Self {
         style_has_been_set: false,
         ctx,
         allow_close: Arc::new(AtomicBool::new(false)),
         shutdown_started: Arc::new(AtomicBool::new(false)),
         tray,
         hidden_to_tray: false,
      }
   }

   /// The `App::logic` hook: tray actions and the minimize-to-tray transition.
   ///
   /// eframe runs this once before every [`Self::ui`], and — crucially — also while the
   /// root window is hidden, provided something requested a repaint (the tray handlers
   /// always do). It must not paint.
   fn handle_tray(&mut self, egui_ctx: &egui::Context) {
      // Collect first: the borrow of `self.tray` must end before the actions below,
      // which need `&mut self`.
      let Some(actions) = self.tray.as_ref().map(Tray::drain) else {
         return;
      };

      for action in actions {
         match action {
            TrayAction::Toggle => self.toggle_window(egui_ctx),
            TrayAction::Lock => self.lock_wallet(egui_ctx),
            TrayAction::Quit => {
               begin_shutdown(
                  egui_ctx.clone(),
                  self.allow_close.clone(),
                  self.shutdown_started.clone(),
               );
               return;
            }
         }
      }

      // The OS minimize button hides the window to the tray. It cannot be
      // intercepted: winit 0.30 has no `minimizable`, and `EnableButtons` is
      // unimplemented on X11 (`ViewportCommand::EnableButtons` would only help on
      // Windows/macOS). So the window is minimized for the frame or two it takes
      // us to notice, then hidden.
      let minimized = egui_ctx.input(|i| i.viewport().minimized).unwrap_or(false);
      if minimized && !self.hidden_to_tray && self.minimize_to_tray_enabled() {
         self.hidden_to_tray = true;
         tray::hide_window(egui_ctx);
         tray::notify_hidden();
      }
   }

   fn toggle_window(&mut self, egui_ctx: &egui::Context) {
      if self.hidden_to_tray {
         self.hidden_to_tray = false;
         tray::show_window(egui_ctx);
      } else {
         self.hidden_to_tray = true;
         tray::hide_window(egui_ctx);
         tray::notify_hidden();
      }
   }

   fn lock_wallet(&mut self, egui_ctx: &egui::Context) {
      let locked = SHARED_GUI.write(|gui| {
         let zeus_ctx = gui.ctx.clone();
         let locked = zeus_ctx.write(|ctx| {
            let lock = ctx.vault_unlocked && !ctx.locked;
            if lock {
               ctx.locked = true;
               tracing::info!("Wallet locked from the system tray");
            }
            lock
         });
         gui.request_repaint();
         locked
      });

      // Only when this call is what locked it: a wallet that was already locked gets
      // no second notification.
      if locked {
         tray::notify_locked();
      }

      egui_ctx.request_repaint();
   }

   /// The persisted "Minimize to Tray" preference.
   ///
   /// Read through the shared context rather than an app field because the setting
   /// can change on the settings page between two calls to this hook.
   fn minimize_to_tray_enabled(&self) -> bool {
      SHARED_GUI.read(|gui| gui.ctx.read(|ctx| ctx.misc_config.minimize_to_tray()))
   }

   fn on_shutdown(&mut self, ctx: &egui::Context) {
      if !ctx.input(|i| i.viewport().close_requested()) {
         return;
      }

      // Final close after cleanup finished, do not cancel.
      if self.allow_close.load(Ordering::SeqCst) {
         return;
      }

      ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);

      begin_shutdown(
         ctx.clone(),
         self.allow_close.clone(),
         self.shutdown_started.clone(),
      );
   }
}

/// Save everything, erase the in-memory vault material, then let the close request through.
///
/// Shared by the title-bar close button and the tray's Quit item — the tray reaches it from
/// its own thread, possibly with the window hidden and no egui frame running, so it takes
/// the egui context instead of `&mut ZeusApp`.
fn begin_shutdown(
   egui_ctx: egui::Context,
   allow_close: Arc<AtomicBool>,
   shutdown_started: Arc<AtomicBool>,
) {
   if shutdown_started.swap(true, Ordering::SeqCst) {
      return;
   }

   RT.spawn(async move {
      let zeus_ctx = SHARED_GUI.write(|gui| {
         gui.loading_window.open("Saving vault...");
         gui.request_repaint();
         gui.ctx.clone()
      });

      let unlocked = zeus_ctx.read(|z| z.vault_unlocked);
      if unlocked {
         let ctx = zeus_ctx.clone();
         let _ = RT
            .spawn_blocking(move || {
               if let Err(e) = ctx.encrypt_and_save_vault(None, None) {
                  tracing::error!("Failed to save vault: {:?}", e);
               }

               ctx.balance_manager().remove_zero_balances();

               if let Err(e) = ctx.save_wallet_state() {
                  tracing::error!("Failed to save wallet state: {:?}", e);
               }

               ctx.save_client_manager();
               ctx.save_pool_manager();
               ctx.save_currency_db();
               ctx.save_address_book();
               ctx.save_price_manager();
            })
            .await;

         SHARED_GUI.write(|gui| {
            gui.loading_window.open("Compacting Railgun DB...");
            gui.request_repaint();
         });

         let dev_build = cfg!(feature = "dev");

         if !dev_build {
            for chain in SUPPORTED_CHAINS {
               let provider_res = zeus_ctx.get_railgun_provider(chain, false).await;
               if let Ok(provider) = provider_res {
                  if provider.is_syncing().await {
                     continue;
                  }

                  match provider.compact().await {
                     Ok(compacted) => match compacted {
                        true => tracing::info!("Compacted Railgun DB for chain {}", chain),
                        false => tracing::info!(
                           "Railgun DB for chain {} does not need compact",
                           chain
                        ),
                     },
                     Err(e) => tracing::error!(
                        "Error compacting Railgun DB for chain {}: {:?}",
                        chain,
                        e
                     ),
                  }

                  match provider.compact_events_snapshot().await {
                     Ok(true) => tracing::info!(
                        "Compacted Railgun events snapshot for chain {}",
                        chain
                     ),
                     Ok(false) => tracing::info!(
                        "Railgun events snapshot for chain {} does not need compact",
                        chain
                     ),
                     Err(e) => tracing::error!(
                        "Error compacting Railgun events snapshot for chain {}: {:?}",
                        chain,
                        e
                     ),
                  }
               }
            }
         }
      }

      SHARED_GUI.write(|gui| {
         gui.ctx.write_vault(|vault| vault.erase());
         gui.ctx.write(|ctx| {
            ctx.vault_unlocked = false;
            ctx.wallet_info_cache.clear();
            ctx.current_wallet = WalletInfo::default();
         });

         gui.account_panel.erase();
         gui.wallet_ui.erase(&egui_ctx);
         gui.unlock_vault_ui.erase();
         gui.recover_wallet_ui.erase();
         gui.lock_screen.erase();
         gui.settings.erase();

         gui.loading_window.reset();
         gui.request_repaint();
      });

      // Allow the next close_requested through, then re-request close.
      allow_close.store(true, Ordering::SeqCst);
      egui_ctx.send_viewport_cmd(egui::ViewportCommand::Close);
      egui_ctx.request_repaint();
      tracing::info!("Shutdown command sent");
   });
}

impl eframe::App for ZeusApp {
   fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
      egui::Rgba::TRANSPARENT.to_array()
   }

   /// Runs before every `ui`, and also while the root window is hidden — the state
   /// minimize-to-tray leaves it in. No painting is allowed here.
   fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
      self.handle_tray(ctx);
   }

   fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
      #[cfg(feature = "dev")]
      let time = std::time::Instant::now();

      SHARED_GUI.write(|gui| {
         let zeus_ctx = gui.ctx.clone();

         zeus_ctx.write(|ctx| {
            self.on_shutdown(ui.ctx());

            // Reset the auto-lock idle timer on real user input.
            if ctx.vault_unlocked && !ctx.locked {
               record_input_activity(ui.ctx(), ctx);
            }

            #[cfg(feature = "dev")]
            gui.theme.install(ui.ctx());

            // This is needed for Windows
            if !self.style_has_been_set {
               let style = gui.theme.style();
               ui.set_global_style(style);
               self.style_has_been_set = true;
            }

            let bg = gui.theme.colors.bg;
            let main_frame = Frame::new().fill(bg);

            let left_frame_bg = match ctx.vault_unlocked {
               true => gui.theme.frame1.fill,
               false => bg,
            };

            let left_frame = Frame::new().fill(left_frame_bg);

            // Left panel first so it owns the full window height. Account panel + nav
            // then sit at the top-left; the top panel is only the message bar.
            egui::Panel::left("left_panel")
               .min_size(260.0)
               .max_size(260.0)
               .resizable(false)
               .frame(left_frame)
               .show_separator_line(false)
               .show(ui, |ui| {
                  if ctx.vault_unlocked && !ctx.locked {
                     gui.show_left_panel(ctx, ui);
                  }
               });

            egui::Panel::top("top_panel")
               .min_size(200.0)
               .resizable(false)
               .show_separator_line(false)
               .frame(main_frame)
               .show(ui, |ui| {
                  if ctx.vault_unlocked && !ctx.locked {
                     gui.show_top_panel(ctx, ui);
                  }
               });

            // Paint the Ui that belongs to the central panel
            egui::CentralPanel::default().frame(main_frame).show(ui, |ui| {
               gui.show_central_panel(ctx, ui);
            });

            let icons = gui.icons.clone();
            let theme = &gui.theme;
            gui.settings.show(ctx, icons, theme, ui);

            #[cfg(feature = "dev")]
            gui.fps_metrics.update(time.elapsed().as_secs_f64() * 1000.0);
         });
      });
   }
}

/// Auto-lock watcher: a slow tick that locks the UI once the idle time
/// exceeds the configured timeout.
///
/// Lives off the frame path because egui
/// only repaints on demand, so an idle window would otherwise never check.vv
async fn autolock_watcher(ctx: ZeusCtx) {
   loop {
      tokio::time::sleep(Duration::from_secs(1)).await;

      let mut locked_now = false;
      ctx.write(|ctx| {
         if !ctx.vault_unlocked || ctx.locked {
            return;
         }

         let Some(idle_secs) = ctx.security.autolock.idle_secs() else {
            return;
         };

         let now = TimeStamp::now_as_millis().unwrap_or_default().timestamp();
         if now.saturating_sub(ctx.last_activity_ms) >= idle_secs * 1000 {
            ctx.locked = true;
            locked_now = true;
         }
      });

      if locked_now {
         tracing::info!("Auto-lock: idle timeout reached, locking the UI");
         SHARED_GUI.write(|gui| gui.request_repaint());
      }
   }
}

pub fn setup_fonts(ctx: &egui::Context) {
   let mut fonts = FontDefinitions::default();

   let inter_bold = FontData::from_static(INTER_BOLD_18);
   fonts.font_data.insert("inter_bold".to_owned(), Arc::new(inter_bold));

   let mut newfam = std::collections::BTreeMap::new();
   newfam.insert(
      FontFamily::Name("inter_bold".into()),
      vec!["inter_bold".to_owned()],
   );
   fonts.families.append(&mut newfam);

   ctx.set_fonts(fonts);
}
