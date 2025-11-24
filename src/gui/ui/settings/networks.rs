use crate::assets::icons::Icons;
use crate::core::{ZeusCtx, client::Rpc};
use crate::gui::{SHARED_GUI, ui::ChainSelect};
use crate::utils::RT;
use eframe::egui::{
   Align, Align2, Button, Color32, CursorIcon, FontId, Grid, Layout, Margin, Order, RichText,
   ScrollArea, Slider, Spinner, TextEdit, Ui, Window, vec2,
};
use egui::Frame;
use std::sync::Arc;
use zeus_eth::alloy_provider::Provider;
use zeus_theme::Theme;

pub struct NetworkSettings {
   open: bool,
   refreshing: bool,
   add_rpc: bool,
   #[allow(dead_code)]
   change_server_port: bool,
   #[allow(dead_code)]
   valid_url: bool,
   url_to_add: String,
   chain_select: ChainSelect,
   size: (f32, f32),
}

impl NetworkSettings {
   pub fn new() -> Self {
      Self {
         open: false,
         refreshing: false,
         add_rpc: false,
         change_server_port: false,
         valid_url: false,
         url_to_add: String::new(),
         chain_select: ChainSelect::new("network_settings_chain_select", 1),
         size: (550.0, 400.0),
      }
   }

   pub fn open(&mut self) {
      self.open = true;
   }

   pub fn close(&mut self) {
      self.open = false;
   }

   fn valid_url(&self) -> bool {
      self.url_to_add.starts_with("http://")
         || self.url_to_add.starts_with("https://")
         || self.url_to_add.starts_with("ws://")
         || self.url_to_add.starts_with("wss://")
   }

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      let tint = theme.image_tint_recommended;

      self.add_rpc(ctx.clone(), theme, ui);

      let mut open = self.open;
      Window::new(RichText::new("Network Settings").size(theme.text_sizes.heading))
         .open(&mut open)
         .resizable(false)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);
            ui.spacing_mut().button_padding = vec2(10.0, 8.0);

            ui.add_space(25.0);
            let chain = self.chain_select.chain.id();
            let z_client = ctx.get_zeus_client();
            let mut rpcs = z_client.get_rpcs(chain);

            ui.horizontal(|ui| {
               ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                  self.chain_select.show(0, theme, icons.clone(), ui);
               });

               ui.add_space(30.0);

               ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                  let add_network =
                     Button::new(RichText::new("Add Network").size(theme.text_sizes.normal));
                  if ui.add(add_network).clicked() {
                     self.add_rpc = true;
                  }

                  let icon = match theme.dark_mode {
                     true => icons.refresh_white_x28(tint),
                     false => icons.refresh_dark_x28(tint),
                  };

                  if !self.refreshing {
                     let res = ui.add(icon).on_hover_cursor(CursorIcon::PointingHand);

                     if res.clicked() {
                        let ctx = ctx.clone();
                        self.refreshing = true;
                        RT.spawn(async move {
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

            ScrollArea::vertical().show(ui, |ui| {
               let column_widths = [
                  ui.available_width() * 0.4, // Url
                  ui.available_width() * 0.1, // Enabled (checkbox)
                  ui.available_width() * 0.1, // Status
                  ui.available_width() * 0.1, // Archive Node
                  ui.available_width() * 0.1, // Latency
                  ui.available_width() * 0.1, // Test button
                  ui.available_width() * 0.1, // Remove button
               ];

               Grid::new("rpc_grid").spacing([10.0, 10.0]).show(ui, |ui| {
                  ui.set_width(column_widths.iter().sum::<f32>());

                  // Header
                  ui.label(RichText::new("Url").size(theme.text_sizes.large));
                  ui.label(RichText::new("Enabled").size(theme.text_sizes.large));
                  ui.label(RichText::new("Status").size(theme.text_sizes.large));
                  ui.label(RichText::new("Archive").size(theme.text_sizes.large));
                  ui.label(RichText::new("Latency").size(theme.text_sizes.large));

                  ui.end_row();

                  for rpc in rpcs.iter_mut() {
                     // Url column
                     ui.horizontal(|ui| {
                        ui.set_width(column_widths[0]);
                        ui.add(
                           TextEdit::singleline(&mut rpc.url)
                              .font(FontId::proportional(theme.text_sizes.normal))
                              .min_size(vec2(column_widths[0] * 0.8, 25.0))
                              .margin(Margin::same(10)),
                        );
                     });

                     // Enabled column
                     let res = ui.horizontal(|ui| {
                        ui.set_width(column_widths[1]);
                        ui.checkbox(&mut rpc.enabled, "")
                     });

                     if res.inner.clicked() {
                        let z_client = ctx.get_zeus_client();
                        z_client.write(|z_client| {
                           let rpcs_opt = z_client.get_mut(&chain);
                           if let Some(rpcs) = rpcs_opt {
                              for rpc_mut in rpcs.iter_mut() {
                                 if rpc_mut.url == rpc.url {
                                    rpc_mut.enabled = rpc.enabled;
                                    break;
                                 }
                              }
                           }
                        });

                        let ctx_clone = ctx.clone();
                        RT.spawn_blocking(move || {
                           ctx_clone.save_zeus_client();
                        });
                     }

                     // Status column
                     ui.horizontal(|ui| {
                        ui.set_width(column_widths[2]);
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
                     ui.horizontal(|ui| {
                        ui.set_width(column_widths[3]);
                        let icon = if rpc.is_archive() {
                           icons.green_circle(tint)
                        } else {
                           icons.red_circle(tint)
                        };
                        ui.add(icon);
                     });

                     // Latency column
                     ui.horizontal(|ui| {
                        ui.set_width(column_widths[4]);
                        ui.label(RichText::new(rpc.latency_str()).size(theme.text_sizes.normal));
                     });

                     // Test button column
                     ui.horizontal(|ui| {
                        ui.set_width(column_widths[5]);
                        let button =
                           Button::new(RichText::new("Test").size(theme.text_sizes.small));
                        if ui.add(button).clicked() {
                           let ctx_clone = ctx.clone();
                           let rpc_clone = rpc.clone();
                           self.refreshing = true;
                           RT.spawn(async move {
                              let z_client = ctx_clone.get_zeus_client();
                              z_client.run_check_for(ctx_clone, rpc_clone).await;
                              z_client.sort_by_fastest();

                              SHARED_GUI.write(|gui| {
                                 gui.settings.network.refreshing = false;
                              });
                           });
                        }
                     });

                     // Remove button column
                     let button = Button::new(RichText::new("Remove").size(theme.text_sizes.small));
                     ui.horizontal(|ui| {
                        ui.set_width(column_widths[5]);
                        if ui.add(button).clicked() {
                           let z_client = ctx.get_zeus_client();
                           z_client.remove_rpc(chain, rpc.url.clone());
                           let ctx_clone = ctx.clone();
                           RT.spawn_blocking(move || {
                              ctx_clone.save_zeus_client();
                           });
                        }
                     });

                     ui.end_row();
                  }
               });
            });
         });
      self.open = open;
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

   pub fn add_rpc(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      let mut open = self.add_rpc;

      Window::new(RichText::new("Add Network").size(theme.text_sizes.large))
         .open(&mut open)
         .order(Order::Foreground)
         .resizable(false)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_width(300.0);
            ui.set_height(100.0);
            ui.spacing_mut().button_padding = vec2(10.0, 8.0);

            ui.vertical_centered(|ui| {
               let ui_width = ui.available_width();

               let hint_text = RichText::new("Enter a url").size(theme.text_sizes.normal);
               ui.add(
                  TextEdit::singleline(&mut self.url_to_add)
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
                  ui.add(Spinner::new().size(15.0).color(Color32::WHITE));
               }

               let button = Button::new(RichText::new("Add").size(theme.text_sizes.normal));
               if self.valid_url() {
                  if ui.add_enabled(!self.refreshing, button).clicked() {
                     self.refreshing = true;
                     let chain = self.chain_select.chain.id();
                     validate_rpc(ctx.clone(), chain, self.url_to_add.clone());
                  }
               }
            });
         });
      self.add_rpc = open;
   }
}

fn validate_rpc(ctx: ZeusCtx, chain: u64, url: String) {
   let default = false;
   let enabled = true;
   let mev_protect = false;
   let rpc = Rpc::new(url.clone(), chain, default, enabled, mev_protect);

   RT.spawn(async move {
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
         gui.settings.network.add_rpc = false;
         gui.settings.network.refreshing = false;
      });
   });
}
