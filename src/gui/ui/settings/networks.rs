//! UI that allows the user to change the network settings.

use crate::assets::icons::Icons;
use crate::core::{ZeusCtx, ZeusContext, client::Rpc};
use crate::gui::{SHARED_GUI, ui::ChainSelect};
use crate::utils::RT;
use eframe::egui::{
   Align, Align2, CornerRadius, CursorIcon, FontId, Layout, Margin, Order, RichText, ScrollArea,
   Slider, Spinner, Ui, Window, vec2,
};
use egui::Frame;
use std::sync::Arc;
use zeus_eth::alloy_provider::Provider;
use zeus_theme::{ButtonVisuals, OverlayManager, Theme};
use zeus_widgets::{Button, SecureTextEdit};

pub struct NetworkSettings {
   open: bool,
   overlay: OverlayManager,
   refreshing: bool,
   add_rpc: bool,
   rpc_settings_open: bool,
   rpc_to_edit: Option<Rpc>,
   url_to_add: String,
   chain_select: ChainSelect,
   size: (f32, f32),

   #[allow(dead_code)]
   change_server_port: bool,
   #[allow(dead_code)]
   valid_url: bool,
}

impl NetworkSettings {
   pub fn new(overlay: OverlayManager) -> Self {
      let chain_select =
         ChainSelect::new("network_settings_chain_select", 1).size(vec2(200.0, 15.0));
      Self {
         open: false,
         overlay,
         refreshing: false,
         add_rpc: false,
         rpc_settings_open: false,
         rpc_to_edit: None,
         change_server_port: false,
         valid_url: false,
         url_to_add: String::new(),
         chain_select,
         size: (550.0, 400.0),
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self) {
      if !self.open {
         self.overlay.window_opened();
         self.open = true;
      }
   }

   pub fn close(&mut self) {
      self.overlay.window_closed();
      self.open = false;
   }

   pub fn open_add_rpc(&mut self) {
      if !self.add_rpc {
         self.overlay.window_opened();
         self.add_rpc = true;
      }
   }

   pub fn close_add_rpc(&mut self) {
      self.overlay.window_closed();
      self.add_rpc = false;
   }

   pub fn open_rpc_settings(&mut self) {
      if !self.rpc_settings_open {
         self.overlay.window_opened();
         self.rpc_settings_open = true;
      }
   }

   pub fn close_rpc_settings(&mut self) {
      self.overlay.window_closed();
      self.rpc_settings_open = false;
   }

   fn valid_url(&self) -> bool {
      self.url_to_add.starts_with("http://")
         || self.url_to_add.starts_with("https://")
         || self.url_to_add.starts_with("ws://")
         || self.url_to_add.starts_with("wss://")
   }

   pub fn show(&mut self, ctx: &mut ZeusContext, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      if !self.open {
         return;
      }

      let tint = theme.image_tint_recommended;

      self.add_rpc(theme, ui);
      self.rpc_settings(ctx, theme, ui);

      let mut open = self.open;
      let window_frame = theme.frame1;

      Window::new(RichText::new("Network Settings").size(theme.text_sizes.heading))
         .open(&mut open)
         .resizable(false)
         .order(Order::Foreground)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(window_frame)
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);
            ui.spacing_mut().button_padding = vec2(10.0, 8.0);

            let button_visuals = theme.button_visuals();
            let text_edit_visuals = theme.text_edit_visuals();

            ui.add_space(25.0);
            let chain = self.chain_select.chain.id();
            let z_client = ctx.client.clone();
            let mut rpcs = z_client.get_rpcs(chain);

            ui.horizontal(|ui| {
               // Chain Select
               ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                  ui.spacing_mut().button_padding = vec2(8.0, 4.0);
                  self.chain_select.show(0, theme, icons.clone(), ui);
               });

               ui.add_space(30.0);

               ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                  ui.spacing_mut().button_padding = vec2(8.0, 4.0);

                  // Add Network button
                  let text = RichText::new("Add Network").size(theme.text_sizes.normal);
                  let button = Button::new(text).visuals(button_visuals);

                  if ui.add(button).clicked() {
                     self.open_add_rpc();
                  }

                  // Refresh button
                  let icon = match theme.dark_mode {
                     true => icons.refresh_white_x22(tint),
                     false => icons.refresh_dark_x22(tint),
                  };

                  if !self.refreshing {
                     let mut visuals = ButtonVisuals::default();
                     visuals.bg_hover = button_visuals.bg_hover;
                     visuals.corner_radius = CornerRadius::same(25);
                     let button = Button::image(icon).small().visuals(visuals);
                     let res = ui.add(button).on_hover_cursor(CursorIcon::PointingHand);

                     if res.clicked() {
                        self.refreshing = true;

                        RT.spawn(async move {
                           let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
                           let z_client = ctx.get_zeus_client();
                           z_client.run_rpc_checks(ctx.clone()).await;
                           z_client.sort_by_fastest();
                           SHARED_GUI.write(|gui| {
                              gui.settings.network.refreshing = false;
                           });
                        });
                     }
                  } else {
                     ui.add(Spinner::new().size(17.0).color(theme.colors.text));
                  }
               });
            });

            ui.add_space(20.0);
            ui.spacing_mut().item_spacing.y = 15.0;

            let url_width = ui.available_width() * 0.35;
            let mev_protect_width = ui.available_width() * 0.20;
            let others_width = ui.available_width() * 0.12;
            let buttons_width = ui.available_width() * 0.08;

            // Header
            ui.horizontal(|ui| {
               ui.scope(|ui| {
                  ui.set_width(url_width);
                  ui.label(RichText::new("Url").size(theme.text_sizes.normal));
               });

               ui.scope(|ui| {
                  ui.set_width(others_width);
                  ui.label(RichText::new("Enabled").size(theme.text_sizes.normal));
               });

               ui.scope(|ui| {
                  ui.set_width(others_width);
                  ui.label(RichText::new("Status").size(theme.text_sizes.normal));
               });

               ui.scope(|ui| {
                  ui.set_width(others_width);
                  ui.label(RichText::new("Archive").size(theme.text_sizes.normal));
               });

               ui.scope(|ui| {
                  ui.set_width(mev_protect_width);
                  ui.label(RichText::new("MEV Protect").size(theme.text_sizes.normal));
               });

               ui.scope(|ui| {
                  ui.set_width(others_width);
                  ui.label(RichText::new("Latency").size(theme.text_sizes.normal));
               });
            });

            ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
               ui.set_width(ui.available_width());

               for (_url, rpc) in rpcs.iter_mut() {
                  ui.horizontal(|ui| {
                     // Url text edit
                     let mut url = rpc.url.to_string();
                     ui.scope(|ui| {
                        ui.set_width(url_width);
                        ui.add(
                           SecureTextEdit::singleline(&mut url)
                              .visuals(text_edit_visuals)
                              .font(FontId::proportional(theme.text_sizes.small))
                              .min_size(vec2(url_width, 15.0))
                              .margin(Margin::same(5)),
                        );
                     });

                     // Enabled column
                     let was_enabled = rpc.enabled;
                     let res = ui.scope(|ui| {
                        ui.set_width(others_width);
                        ui.add_space(15.0);
                        ui.checkbox(&mut rpc.enabled, "")
                     });

                     if res.inner.clicked() {
                        let z_client = ctx.client.clone();
                        z_client.write(|rpcs_map| {
                           let rpcs_opt = rpcs_map.get_mut(&chain);
                           if let Some(rpcs) = rpcs_opt {
                              if let Some(old_rpc) = rpcs.get_mut(&rpc.url) {
                                 old_rpc.enabled = rpc.enabled;
                              }
                           }
                        });

                        // If we just enabled the RPC, we need to run a check
                        if !was_enabled && rpc.enabled {
                           let rpc = rpc.clone();
                           RT.spawn(async move {
                              let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
                              let z_client = ctx.get_zeus_client();
                              z_client.run_check_for(ctx, rpc).await;
                           });
                        }

                        RT.spawn_blocking(move || {
                           let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
                           ctx.save_zeus_client();
                        });
                     }

                     // Status column
                     ui.scope(|ui| {
                        ui.set_width(others_width);
                        ui.add_space(12.0);
                        let icon = if rpc.is_working() {
                           match rpc.is_fully_functional() {
                              true => icons.green_circle(tint),
                              false => icons.orange_circle(tint),
                           }
                        } else {
                           icons.red_circle(tint)
                        };
                        ui.add(icon);
                     });

                     // Archive Node column
                     ui.scope(|ui| {
                        ui.set_width(others_width);
                        ui.add_space(15.0);
                        let icon = if rpc.is_archive() {
                           icons.green_circle(tint)
                        } else {
                           icons.red_circle(tint)
                        };
                        ui.add(icon);
                     });

                     // MEV Protect column
                     ui.scope(|ui| {
                        ui.set_width(mev_protect_width);
                        ui.add_space(30.0);
                        let icon = if rpc.is_mev_protect() {
                           icons.green_circle(tint)
                        } else {
                           icons.red_circle(tint)
                        };
                        ui.add(icon);
                     });

                     // Latency column
                     ui.scope(|ui| {
                        ui.set_width(others_width);
                        ui.label(RichText::new(rpc.latency_str()).size(theme.text_sizes.normal));
                     });

                     // Settings button
                     let icon = match theme.dark_mode {
                        true => icons.gear_white_x24(tint),
                        false => icons.gear_dark_x24(tint),
                     };

                     let mut visuals = ButtonVisuals::default();
                     visuals.bg_hover = button_visuals.bg_hover;
                     visuals.corner_radius = CornerRadius::same(15);
                     let button = Button::image(icon).small().visuals(visuals);

                     ui.scope(|ui| {
                        ui.set_width(buttons_width);
                        let res = ui.add(button).on_hover_cursor(CursorIcon::PointingHand);

                        if res.clicked() {
                           self.open_rpc_settings();
                           self.rpc_to_edit = Some(rpc.clone());
                        }
                     });

                     // Test button column
                     ui.scope(|ui| {
                        ui.set_width(buttons_width);
                        let text = RichText::new("Test").size(theme.text_sizes.small);
                        let button = Button::new(text).visuals(button_visuals);

                        let test_in_progress = rpc.test_in_progress;

                        if !test_in_progress {
                           if ui.add(button).clicked() {
                              let rpc_clone = rpc.clone();

                              RT.spawn(async move {
                                 let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
                                 let z_client = ctx.get_zeus_client();
                                 z_client.run_check_for(ctx, rpc_clone).await;
                                 z_client.sort_by_fastest();
                              });
                           }
                        } else {
                           ui.add(Spinner::new().size(17.0).color(theme.colors.text));
                        }
                     });

                     // Remove button column
                     let button = Button::new(RichText::new("X").size(theme.text_sizes.small))
                        .visuals(button_visuals);

                     ui.scope(|ui| {
                        ui.set_width(buttons_width);

                        if ui.add(button).clicked() {
                           let z_client = ctx.client.clone();
                           z_client.remove_rpc(chain, rpc.url.clone());

                           RT.spawn_blocking(move || {
                              let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
                              ctx.save_zeus_client();
                           });
                        }
                     });
                  });
               }
            });
         });

      if !open {
         self.close();
      }
   }

   fn _change_server_port(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      let mut open = self.change_server_port;
      let was_open = open;

      Window::new(RichText::new("Server Port").size(theme.text_sizes.normal))
         .open(&mut open)
         .order(Order::Foreground)
         .resizable(false)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_width(150.0);
            ui.set_height(100.0);
            ui.spacing_mut().button_padding = vec2(10.0, 8.0);

            ui.vertical_centered(|ui| {
               ctx.write(|ctx| {
                  let slider = Slider::new(&mut ctx.server_port, 1000..=65535);
                  ui.add(slider);
               });
            });
         });
      if was_open && !open {
         RT.spawn_blocking(move || match ctx.save_server_port() {
            Ok(_) => {
               tracing::info!("Saved server port");
            }
            Err(e) => {
               tracing::error!("Error saving server port: {:?}", e);
            }
         });
      }
      self.change_server_port = open;
   }

   pub fn rpc_settings(&mut self, ctx: &mut ZeusContext, theme: &Theme, ui: &mut Ui) {
      if !self.rpc_settings_open {
         return;
      }

      let mut open = self.rpc_settings_open;
      let window_frame = theme.frame1;

      Window::new(RichText::new("Endpoint Settings").size(theme.text_sizes.large))
         .open(&mut open)
         .resizable(false)
         .order(Order::Tooltip)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(window_frame)
         .show(ui.ctx(), |ui| {
            ui.set_width(300.0);
            ui.set_height(100.0);
            ui.spacing_mut().button_padding = vec2(10.0, 8.0);

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = 15.0;

               if self.rpc_to_edit.is_none() {
                  let text = RichText::new("No RPC selected").size(theme.text_sizes.normal);
                  ui.label(text);
                  return;
               }

               let rpc = self.rpc_to_edit.as_mut().unwrap();

               let text = RichText::new("MEV Protect").size(theme.text_sizes.normal);

               ui.label(text);
               let clicked = ui.checkbox(&mut rpc.mev_protect, "").clicked();

               if clicked {
                  let z_client = ctx.client.clone();
                  z_client.write(|rpcs_map| {
                     if let Some(rpcs) = rpcs_map.get_mut(&rpc.chain_id) {
                        if let Some(old_rpc) = rpcs.get_mut(&rpc.url) {
                           old_rpc.mev_protect = rpc.mev_protect;
                        }
                     }
                  });
                  RT.spawn_blocking(move || {
                     let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
                     ctx.save_zeus_client();
                  });
               }
            });
         });

      if !open {
         self.close_rpc_settings();
      }
   }

   pub fn add_rpc(&mut self, theme: &Theme, ui: &mut Ui) {
      if !self.add_rpc {
         return;
      }

      let mut open = self.add_rpc;
      let window_frame = theme.frame1;

      Window::new(RichText::new("Add Network").size(theme.text_sizes.large))
         .open(&mut open)
         .order(Order::Tooltip)
         .resizable(false)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(window_frame)
         .show(ui.ctx(), |ui| {
            ui.set_width(300.0);
            ui.set_height(100.0);
            ui.spacing_mut().item_spacing = vec2(0.0, 15.0);
            ui.spacing_mut().button_padding = vec2(10.0, 8.0);

            let button_visuals = theme.button_visuals();
            let text_edit_visuals = theme.text_edit_visuals();

            ui.vertical_centered(|ui| {
               let ui_width = ui.available_width();

               let hint_text = RichText::new("Enter a url").size(theme.text_sizes.normal);
               ui.add(
                  SecureTextEdit::singleline(&mut self.url_to_add)
                     .visuals(text_edit_visuals)
                     .hint_text(hint_text)
                     .font(FontId::proportional(theme.text_sizes.normal))
                     .min_size(vec2(ui_width * 0.5, 20.0))
                     .margin(Margin::same(10)),
               );
               ui.add_space(2.0);

               if !self.valid_url() && !self.url_to_add.is_empty() {
                  ui.label(
                     RichText::new("Invalid URL")
                        .size(theme.text_sizes.small)
                        .color(theme.colors.error),
                  );
               }

               if self.refreshing {
                  ui.add(Spinner::new().size(15.0).color(theme.colors.text));
               }

               let text = RichText::new("Add").size(theme.text_sizes.normal);
               let button = Button::new(text).visuals(button_visuals);
               if self.valid_url() {
                  if ui.add_enabled(!self.refreshing, button).clicked() {
                     self.refreshing = true;
                     let chain = self.chain_select.chain.id();
                     validate_rpc(chain, self.url_to_add.clone());
                  }
               }
            });
         });

      if !open {
         self.close_add_rpc();
      }
   }
}

fn validate_rpc(chain: u64, url: String) {
   let default = false;
   let enabled = true;
   let mev_protect = false;
   let rpc = Rpc::new(url.clone(), chain, default, enabled, mev_protect);

   RT.spawn(async move {
      let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());

      let client = match ctx.connect_to_rpc(&rpc).await {
         Ok(client) => client,
         Err(e) => {
            tracing::error!("Error getting client using {} {}", rpc.url, e);
            SHARED_GUI.write(|gui| {
               gui.open_msg_window("Failed to connect to RPC", e.to_string());
               gui.settings.network.refreshing = false;
            });
            return;
         }
      };

      let rpc_chain = match client.get_chain_id().await {
         Ok(chain) => chain,
         Err(e) => {
            tracing::error!("Error getting chain using {} {}", rpc.url, e);
            SHARED_GUI.write(|gui| {
               gui.open_msg_window("Failed to get chain ID", e.to_string());
               gui.settings.network.refreshing = false;
            });
            return;
         }
      };

      if rpc_chain != chain {
         tracing::error!(
            "Chain mismatch, RPC {} is for chain {}",
            rpc.url,
            rpc_chain
         );
         SHARED_GUI.write(|gui| {
            gui.open_msg_window(
               "Chain Mismatch",
               format!("RPC {} is for chain {}", rpc.url, rpc_chain),
            );
            gui.settings.network.refreshing = false;
         });
         return;
      }

      let z_client = ctx.get_zeus_client();
      z_client.add_rpc(chain, rpc.clone());
      z_client.run_check_for(ctx.clone(), rpc).await;

      let ctx_clone = ctx.clone();
      RT.spawn_blocking(move || {
         ctx_clone.save_zeus_client();
      });

      SHARED_GUI.write(|gui| {
         gui.open_msg_window("Success!", "RPC added successfully");
         gui.settings.network.url_to_add.clear();
         gui.settings.network.close_add_rpc();
         gui.settings.network.refreshing = false;
      });
   });
}
