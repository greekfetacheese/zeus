use crate::assets::icons::Icons;
use crate::core::{
   ZeusCtx,
   utils::{RT, update::measure_rpcs},
};
use crate::gui::{SHARED_GUI, ui::ChainSelect};
use eframe::egui::{
   Align, Align2, Button, Color32, FontId, Grid, Layout, Margin, Order, RichText, ScrollArea,
   Spinner, TextEdit, Ui, Window, vec2,
};
use egui::Frame;
use egui_theme::Theme;
use std::sync::Arc;

pub struct NetworkSettings {
   pub open: bool,
   pub refreshing: bool,
   pub add_rpc: bool,
   pub valid_url: bool,
   pub url_to_add: String,
   pub chain_select: ChainSelect,
   pub size: (f32, f32),
}

impl NetworkSettings {
   pub fn new() -> Self {
      Self {
         open: false,
         refreshing: false,
         add_rpc: false,
         valid_url: false,
         url_to_add: String::new(),
         chain_select: ChainSelect::new("network_settings_chain_select", 1),
         size: (500.0, 400.0),
      }
   }

   fn valid_url(&self) -> bool {
      self.url_to_add.starts_with("http://")
         || self.url_to_add.starts_with("https://")
         || self.url_to_add.starts_with("ws://")
         || self.url_to_add.starts_with("wss://")
   }

   pub fn show(
      &mut self,
      ctx: ZeusCtx,
      theme: &Theme,
      icons: Arc<Icons>,
      ui: &mut Ui,
   ) {
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
            let ui_width = ui.available_width();

            ui.add_space(25.0);
            let chain = self.chain_select.chain.id();
            let providers = ctx.rpc_providers();
            let mut rpcs = providers.get_all_fastest(chain);

            ui.horizontal(|ui| {
               ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                  self.chain_select.show(0, theme, icons.clone(), ui);
               });

               ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                  if self.refreshing {
                     ui.add(Spinner::new().size(17.0).color(Color32::WHITE));
                  }

                  let refresh = Button::new(RichText::new("Refresh").size(theme.text_sizes.normal));
                  if ui.add(refresh).clicked() {
                     self.refreshing = true;
                     let ctx = ctx.clone();
                     RT.spawn(async move {
                        measure_rpcs(ctx.clone()).await;
                        SHARED_GUI.write(|gui| {
                           gui.settings.network.refreshing = false;
                        });
                     });
                  }

                  let add_network =
                     Button::new(RichText::new("Add Network").size(theme.text_sizes.normal));
                  if ui.add(add_network).clicked() {
                     self.add_rpc = true;
                  }
               });
            });

            ui.add_space(20.0);
            ui.spacing_mut().item_spacing.y = 15.0;

            ScrollArea::vertical().show(ui, |ui| {
               let column_widths = [
                  ui_width * 0.4, // Url
                  ui_width * 0.1, // Enabled (checkbox)
                  ui_width * 0.1, // Status
                  ui_width * 0.1, // Archive Node
                  ui_width * 0.1, // Latency
                  ui_width * 0.1, // Remove button
               ];

               // Center the grid within the scroll area
               ui.horizontal(|ui| {
                  ui.add_space((ui.available_width() - column_widths.iter().sum::<f32>()) / 2.0);

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
                           ctx.write(|ctx| {
                              let rpc = ctx.providers.rpc_mut(chain, rpc.url.clone());
                              if let Some(rpc) = rpc {
                                 rpc.enabled = !rpc.enabled;
                              }
                           });
                           let ctx_clone = ctx.clone();
                           RT.spawn_blocking(move || {
                              ctx_clone.save_providers();
                           });
                        }

                        // Status column
                        ui.horizontal(|ui| {
                           ui.set_width(column_widths[2]);
                           let icon = if rpc.working {
                              icons.green_circle()
                           } else {
                              icons.red_circle()
                           };
                           ui.add(icon);
                        });

                        // Archive Node column
                        ui.horizontal(|ui| {
                           ui.set_width(column_widths[3]);
                           let icon = if rpc.archive_node {
                              icons.green_circle()
                           } else {
                              icons.red_circle()
                           };
                           ui.add(icon);
                        });

                        // Latency column
                        ui.horizontal(|ui| {
                           ui.set_width(column_widths[4]);
                           ui.label(RichText::new(rpc.latency_str()).size(theme.text_sizes.normal));
                        });

                        // Remove button column
                        let button =
                           Button::new(RichText::new("Remove").size(theme.text_sizes.small));
                        ui.horizontal(|ui| {
                           ui.set_width(column_widths[5]);
                           if ui.add(button).clicked() {
                              ctx.write(|ctx| {
                                 ctx.providers.remove_rpc(chain, rpc.url.clone());
                              });
                              let ctx_clone = ctx.clone();
                              RT.spawn_blocking(move || {
                                 ctx_clone.save_providers();
                              });
                           }
                        });

                        ui.end_row();
                     }
                  });
               });
            });
         });
      self.open = open;
   }

   pub fn add_rpc(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      let mut open = self.add_rpc;

      Window::new(RichText::new("Add Network").size(theme.text_sizes.normal))
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

               ui.add(
                  TextEdit::singleline(&mut self.url_to_add)
                     .hint_text("Enter a url")
                     .font(FontId::proportional(theme.text_sizes.small))
                     .min_size(vec2(ui_width * 0.5, 20.0))
                     .margin(Margin::same(10)),
               );
               ui.add_space(2.0);

               if !self.valid_url() && !self.url_to_add.is_empty() {
                  ui.label(
                     RichText::new("Invalid URL")
                        .size(theme.text_sizes.small)
                        .color(Color32::RED),
                  );
               }

               let button = Button::new(RichText::new("Add").size(theme.text_sizes.normal));
               if self.valid_url() {
                  if ui.add(button).clicked() {
                     let chain = self.chain_select.chain.id();
                     ctx.write(|ctx| {
                        ctx.providers.add_user_rpc(chain, self.url_to_add.clone());
                     });
                     let ctx_clone = ctx.clone();
                     RT.spawn_blocking(move || {
                        ctx_clone.save_providers();
                     });
                  }
               }
            });
         });
      self.add_rpc = open;
   }
}
