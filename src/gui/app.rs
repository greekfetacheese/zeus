use egui::*;
use zeus_eth::types::SUPPORTED_CHAINS;

use crate::assets::{INTER_BOLD_18, icons::Icons};
use crate::core::ZeusCtx;
use crate::gui::SHARED_GUI;
use crate::server::run_server;
use crate::utils::{
   RT,
   self_update::check_for_updates,
   state::{on_startup, test_and_measure_rpcs},
};
use eframe::{
   CreationContext,
   egui::{self, Frame},
};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use zeus_theme::{OverlayManager, window::*};

pub struct ZeusApp {
   pub style_has_been_set: bool,
   pub overlay: OverlayManager,
   pub ctx: ZeusCtx,
   /// Once true, the next close request is allowed to proceed (after delayed cleanup).
   allow_close: Arc<AtomicBool>,
   /// Prevents spawning multiple shutdown tasks while cleanup is in flight.
   shutdown_started: bool,
}

impl ZeusApp {
   pub fn new(cc: &CreationContext) -> Self {
      let time = std::time::Instant::now();
      let egui_ctx = cc.egui_ctx.clone();

      setup_fonts(&egui_ctx);

      let icons = Icons::new(&cc.egui_ctx).unwrap();
      let icons = Arc::new(icons);

      SHARED_GUI.write(|shared_gui| {
         shared_gui.icons = icons;
         shared_gui.egui_ctx = egui_ctx.clone();
      });

      let theme = SHARED_GUI.read(|shared_gui| shared_gui.theme.clone());
      let ctx = SHARED_GUI.read(|shared_gui| shared_gui.ctx.clone());
      egui_ctx.set_global_style(theme.style.clone());

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

      let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis();
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
      }
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
            let save_res = RT
               .spawn_blocking({
                  let zeus_ctx = zeus_ctx.clone();
                  move || zeus_ctx.encrypt_and_save_vault(None, None)
               })
               .await;
            match save_res {
               Ok(Ok(())) => tracing::info!("Vault saved on shutdown"),
               Ok(Err(e)) => tracing::error!("Failed to save vault on shutdown: {:?}", e),
               Err(e) => tracing::error!("Vault save task failed: {:?}", e),
            }

            zeus_ctx.save_pool_manager();
            zeus_ctx.save_currency_db();
            zeus_ctx.save_price_manager();

            for chain in SUPPORTED_CHAINS {
               let provider_res = zeus_ctx.get_railgun_provider(chain, false).await;
               if let Ok(provider) = provider_res {
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

         SHARED_GUI.write(|gui| {
            gui.ctx.write_vault(|vault| vault.erase());
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
}

impl eframe::App for ZeusApp {
   fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
      egui::Rgba::TRANSPARENT.to_array()
   }

   fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
      #[cfg(feature = "dev")]
      let time = std::time::Instant::now();

      SHARED_GUI.write(|gui| {
         let zeus_ctx = gui.ctx.clone();

         zeus_ctx.write(|ctx| {
            self.on_shutdown(ui.ctx());

            // This is needed for Windows
            if !self.style_has_been_set {
               let style = gui.theme.style.clone();
               ui.set_global_style(style);
               self.style_has_been_set = true;
            }

            let window = WindowCtx::new("Zeus", 35.0, &gui.theme);
            let color = gui.theme.colors.bg;
            let panel_frame = Frame::new().fill(color);
            self.overlay.paint_overlay(ui.ctx(), true);
            gui.inject_elegance_theme(ui.ctx());

            window_frame(ui, window, |ui| {
               #[cfg(feature = "dev")]
               zeus_theme::utils::apply_theme_changes(&mut gui.theme, ui);

               // Paint the Ui that belongs to the top panel
               egui::Panel::top("top_panel")
                  .min_size(150.0)
                  .resizable(false)
                  .show_separator_line(false)
                  .frame(panel_frame)
                  .show(ui, |ui| {
                     if ctx.vault_unlocked {
                        gui.show_top_panel(ctx, ui);
                     }
                  });

               // Paint the Ui that belongs to the left panel
               egui::Panel::left("left_panel")
                  .min_size(150.0)
                  .max_size(150.0)
                  .resizable(false)
                  .frame(panel_frame)
                  .show_separator_line(false)
                  .show(ui, |ui| {
                     if ctx.vault_unlocked {
                        ui.add_space(10.0);
                        gui.show_left_panel(ctx, ui);
                     }
                  });

               if gui.should_show_right_panel() {
                  // Paint the Ui that belongs to the left panel
                  egui::Panel::right("right_panel")
                     .min_size(150.0)
                     .resizable(false)
                     .show_separator_line(false)
                     .frame(panel_frame)
                     .show(ui, |ui| {
                        if ctx.vault_unlocked {
                           gui.show_right_panel(ui);
                        }
                     });
               }

               // Paint the Ui that belongs to the central panel
               egui::CentralPanel::default().frame(panel_frame).show(ui, |ui| {
                  gui.show_central_panel(ctx, ui);
               });
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
