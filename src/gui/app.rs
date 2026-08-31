use egui::*;
use zeus_eth::types::SUPPORTED_CHAINS;

use crate::assets::{INTER_BOLD_18, icons::Icons};
use crate::core::ZeusCtx;
use crate::gui::SHARED_GUI;
use crate::server::run_server;
use crate::utils::{
   RT, TimeStamp,
   self_update::check_for_updates,
   state::{on_startup, test_and_measure_rpcs},
};
use egui_elements::overlay::OverlayManager;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub struct ZeusApp {
   pub style_has_been_set: bool,
   pub overlay: OverlayManager,
   pub ctx: ZeusCtx,
   /// Once true, the next close request is allowed to proceed (after delayed cleanup).
   allow_close: Arc<AtomicBool>,
   /// Prevents spawning multiple shutdown tasks while cleanup is in flight.
   shutdown_started: bool,
   /// Set when the window close button is clicked (miniquad quit_requested).
   close_requested: bool,
}

impl ZeusApp {
   pub fn new(egui_ctx: &egui::Context) -> Self {
      let time = std::time::Instant::now();
      let egui_ctx = egui_ctx.clone();

      setup_fonts(&egui_ctx);

      let icons = Icons::new(&egui_ctx).unwrap();
      let icons = Arc::new(icons);

      SHARED_GUI.write(|shared_gui| {
         shared_gui.icons = icons;
         shared_gui.egui_ctx = egui_ctx.clone();
      });

      let mut theme = SHARED_GUI.read(|shared_gui| shared_gui.theme.clone());
      let ctx = SHARED_GUI.read(|shared_gui| shared_gui.ctx.clone());

      theme.install(&egui_ctx);
      SHARED_GUI.write(|shared_gui| shared_gui.theme = theme.clone());

      tracing::info!(
         "ZeusApp loaded in {}ms",
         time.elapsed().as_millis()
      );

      RT.spawn(async move {
         let info = match check_for_updates().await {
            Ok(info) => info,
            Err(e) => {
               tracing::error!("Failed to check for updates: {:?}", e);
               Default::default()
            }
         };

         if info.available {
            SHARED_GUI.write(|gui| {
               gui.update_window.open(info);
            });
         }
      });

      let ctx_clone = ctx.clone();
      RT.spawn(async move {
         test_and_measure_rpcs(ctx_clone).await;
      });

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

      Self {
         style_has_been_set: false,
         overlay: theme.overlay_manager,
         ctx,
         allow_close: Arc::new(AtomicBool::new(false)),
         shutdown_started: false,
         close_requested: false,
      }
   }

   pub fn request_close(&mut self) {
      self.close_requested = true;
   }

   pub fn should_quit(&self) -> bool {
      self.allow_close.load(Ordering::SeqCst)
   }

   fn on_shutdown(&mut self, ctx: &egui::Context) {
      if !self.close_requested {
         return;
      }

      // Final close after cleanup finished — Stage::draw will order_quit.
      if self.allow_close.load(Ordering::SeqCst) {
         return;
      }

      // eframe rollback:
      // if !ctx.input(|i| i.viewport().close_requested()) { return; }
      // ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);

      if self.shutdown_started {
         return;
      }
      self.shutdown_started = true;

      let allow_close = self.allow_close.clone();
      let egui_ctx = ctx.clone();
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

                  if let Err(e) = ctx.save_wallet_state() {
                     tracing::error!("Failed to save wallet state: {:?}", e);
                  }

                  ctx.save_zeus_client();
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
                  }
               }
            }
         }

         SHARED_GUI.write(|gui| {
            gui.ctx.write_vault(|vault| vault.erase());
            gui.ctx.write(|ctx| {
               ctx.current_wallet.erase();
            });

            gui.header.erase();
            gui.wallet_ui.erase(&egui_ctx);
            gui.unlock_vault_ui.erase();
            gui.recover_wallet_ui.erase();
            gui.settings.erase();

            gui.loading_window.reset();
            gui.request_repaint();
         });

         // Allow the next close_requested through, then quit from Stage::draw.
         allow_close.store(true, Ordering::SeqCst);
         // eframe rollback:
         // egui_ctx.send_viewport_cmd(egui::ViewportCommand::Close);
         egui_ctx.request_repaint();
         tracing::info!("Shutdown command sent");
      });
   }

   /*
   impl eframe::App for ZeusApp {
      fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
         egui::Rgba::TRANSPARENT.to_array()
      }

      fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
         self.ui(ui);
      }
   }
   */

   pub fn ui(&mut self, ui: &mut Ui) {
      #[cfg(feature = "dev")]
      let time = std::time::Instant::now();

      SHARED_GUI.write(|gui| {
         let zeus_ctx = gui.ctx.clone();

         zeus_ctx.write(|ctx| {
            self.on_shutdown(ui.ctx());

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
            self.overlay.paint_overlay(ui.ctx(), true);

            // Left panel first so it owns the full window height. Header + nav
            // then sit at the top-left; the top panel is only the message bar.
            egui::Panel::left("left_panel")
               .min_size(260.0)
               .max_size(260.0)
               .resizable(false)
               .frame(left_frame)
               .show_separator_line(false)
               .show(ui, |ui| {
                  if ctx.vault_unlocked {
                     gui.show_left_panel(ctx, ui);
                  }
               });

            egui::Panel::top("top_panel")
               .min_size(200.0)
               .resizable(false)
               .show_separator_line(false)
               .frame(main_frame)
               .show(ui, |ui| {
                  if ctx.vault_unlocked {
                     gui.show_top_panel(ctx, ui);
                  }
               });

            // Paint the Ui that belongs to the central panel
            egui::CentralPanel::default().frame(main_frame).show(ui, |ui| {
               gui.show_central_panel(ctx, ui);
            });

            #[cfg(feature = "dev")]
            gui.fps_metrics.update(time.elapsed().as_secs_f64() * 1000.0);
         });
      });
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
