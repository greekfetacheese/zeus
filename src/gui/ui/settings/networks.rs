//! UI that allows the user to change the network settings.

use crate::assets::icons::Icons;
use crate::core::{ZeusContext, ZeusCtx, client::Rpc};
use crate::gui::{SHARED_GUI, ui::ChainSelect};
use crate::utils::{RT, state};
use eframe::egui::{
   Align, CornerRadius, CursorIcon, FontId, InnerResponse, Layout, Margin, RichText, ScrollArea,
   Spinner, Ui, vec2,
};
use egui_elements::{Button, SecureTextEdit, Theme, visuals::ButtonVisuals};
use egui_lucide::Lucide;
use elegance::{Indicator, IndicatorState};
use std::sync::Arc;
use zeus_eth::alloy_provider::Provider;

enum NetworkView {
   List,
   AddRpc,
   EditRpc,
}

pub struct NetworkSettings {
   view: NetworkView,
   refreshing: bool,
   rpc_to_edit: Option<Rpc>,
   url_to_add: String,
   chain_select: ChainSelect,
}

impl NetworkSettings {
   pub fn new() -> Self {
      let mut chain_select =
         ChainSelect::new("network_settings_chain_select", 1).size(vec2(250.0, 15.0));
      chain_select.show_disabled_chains = true;

      Self {
         view: NetworkView::List,
         refreshing: false,
         rpc_to_edit: None,
         url_to_add: String::new(),
         chain_select,
      }
   }

   pub fn reset_view(&mut self) {
      self.view = NetworkView::List;
      self.rpc_to_edit = None;
      self.url_to_add.clear();
   }

   pub fn open_add_rpc(&mut self) {
      self.view = NetworkView::AddRpc;
   }

   pub fn close_add_rpc(&mut self) {
      self.view = NetworkView::List;
      self.url_to_add.clear();
   }

   pub fn open_rpc_settings(&mut self) {
      self.view = NetworkView::EditRpc;
   }

   pub fn close_rpc_settings(&mut self) {
      self.view = NetworkView::List;
      self.rpc_to_edit = None;
   }

   fn valid_url(&self) -> bool {
      self.url_to_add.starts_with("http://")
         || self.url_to_add.starts_with("https://")
         || self.url_to_add.starts_with("ws://")
         || self.url_to_add.starts_with("wss://")
   }

   fn back_button(&mut self, theme: &Theme, ui: &mut Ui) {
      let text = RichText::new("Back").size(theme.typography.normal);
      let button = Button::new(text).visuals(theme.button_visuals());
      if ui.add(button).clicked() {
         self.reset_view();
      }
   }

   pub fn show(&mut self, ctx: &mut ZeusContext, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      ui.add_space(8.0);

      match self.view {
         NetworkView::List => self.list_ui(ctx, theme, icons, ui),
         NetworkView::AddRpc => self.add_rpc(theme, ui),
         NetworkView::EditRpc => self.rpc_settings(ctx, theme, ui),
      }
   }

   fn list_ui(&mut self, ctx: &mut ZeusContext, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      ui.spacing_mut().button_padding = vec2(10.0, 8.0);

      let button_visuals = theme.button_visuals();
      let text_edit_visuals = theme.text_edit_visuals();

      let chain = self.chain_select.chain.id();
      let z_client = ctx.client.clone();
      let mut rpcs = z_client.get_rpcs(chain);

      ui.add_space(10.0);

      ui.horizontal(|ui| {
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.spacing_mut().button_padding = vec2(8.0, 4.0);
            self.chain_select.show(ctx, &[0], theme, icons.clone(), ui);
         });

         ui.add_space(30.0);

         ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().button_padding = vec2(8.0, 10.0);

            let disabled = ctx.is_chain_disabled(chain);
            let text = match disabled {
               true => RichText::new("Enable Network").size(theme.typography.normal),
               false => RichText::new("Disable Network").size(theme.typography.normal),
            };

            let button = Button::new(text).visuals(button_visuals);

            if ui.add(button).clicked() {
               if disabled {
                  ctx.enable_chain(chain);
               } else {
                  ctx.disable_chain(chain);
               }

               RT.spawn_blocking(move || {
                  let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
                  ctx.save_disabled_chains();
               });
            }

            let text = RichText::new("Add RPC Url").size(theme.typography.normal);
            let button = Button::new(text).visuals(button_visuals);

            if ui.add(button).clicked() {
               self.open_add_rpc();
            }

            let icon = Lucide::RefreshCw.size(20.0).color(theme.colors.text).image();

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

      ui.add_space(12.0);

      // Status columns + actions are fixed; leftover width goes to the URL so
      // Test / X stay on-screen. Fractions of available_width overflowed.
      const COL_SPACING: f32 = 8.0;
      const HEADER_H: f32 = 24.0;
      const ROW_H: f32 = 32.0;
      const ENABLED_W: f32 = 64.0;
      const STATUS_W: f32 = 52.0;
      const ARCHIVE_W: f32 = 60.0;
      const MEV_W: f32 = 88.0;
      const LATENCY_W: f32 = 64.0;
      const ACTIONS_W: f32 = 148.0;
      const N_GAPS: f32 = 6.0;

      let fixed = ENABLED_W + STATUS_W + ARCHIVE_W + MEV_W + LATENCY_W + ACTIONS_W;
      let url_w = (ui.available_width() - fixed - COL_SPACING * N_GAPS).max(140.0);

      ui.spacing_mut().item_spacing = vec2(COL_SPACING, 8.0);
      ui.spacing_mut().button_padding = vec2(8.0, 4.0);

      ui.horizontal(|ui| {
         ui.spacing_mut().item_spacing.x = COL_SPACING;
         rpc_col(ui, url_w, HEADER_H, |ui| {
            ui.label(RichText::new("Url").size(theme.typography.small));
         });
         rpc_col(ui, ENABLED_W, HEADER_H, |ui| {
            ui.label(RichText::new("Enabled").size(theme.typography.small));
         });
         rpc_col(ui, STATUS_W, HEADER_H, |ui| {
            ui.label(RichText::new("Status").size(theme.typography.small));
         });
         rpc_col(ui, ARCHIVE_W, HEADER_H, |ui| {
            ui.label(RichText::new("Archive").size(theme.typography.small));
         });
         rpc_col(ui, MEV_W, HEADER_H, |ui| {
            ui.label(RichText::new("MEV Protect").size(theme.typography.small));
         });
         rpc_col(ui, LATENCY_W, HEADER_H, |ui| {
            ui.label(RichText::new("Latency").size(theme.typography.small));
         });
      });

      ScrollArea::vertical().auto_shrink([false; 2]).content_margin(5).show(ui, |ui| {
         ui.set_width(ui.available_width());
         ui.spacing_mut().item_spacing = vec2(COL_SPACING, 8.0);

         for (_url, rpc) in rpcs.iter_mut() {
            ui.horizontal(|ui| {
               ui.spacing_mut().item_spacing.x = COL_SPACING;

               let mut url = rpc.url.to_string();
               rpc_col(ui, url_w, ROW_H, |ui| {
                  ui.add(
                     SecureTextEdit::singleline(&mut url)
                        .visuals(text_edit_visuals)
                        .font(FontId::proportional(theme.typography.small))
                        .min_size(vec2(url_w, 20.0))
                        .margin(Margin::same(5)),
                  );
               });

               let was_enabled = rpc.enabled;
               let res = rpc_col(ui, ENABLED_W, ROW_H, |ui| {
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

                  if !was_enabled && rpc.enabled {
                     let rpc = rpc.clone();
                     RT.spawn(async move {
                        let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
                        let z_client = ctx.get_zeus_client();
                        z_client.run_check_for(ctx.clone(), rpc).await;

                        post_enable_rpc(ctx, chain).await
                     });
                  }

                  RT.spawn_blocking(move || {
                     let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
                     ctx.save_zeus_client();
                  });
               }

               rpc_col(ui, STATUS_W, ROW_H, |ui| {
                  ui.add(rpc_status_indicator(rpc));
               });

               rpc_col(ui, ARCHIVE_W, ROW_H, |ui| {
                  ui.add(rpc_archive_indicator(rpc));
               });

               rpc_col(ui, MEV_W, ROW_H, |ui| {
                  ui.add(rpc_mev_indicator(rpc));
               });

               rpc_col(ui, LATENCY_W, ROW_H, |ui| {
                  ui.label(RichText::new(rpc.latency_str()).size(theme.typography.small));
               });

               rpc_col(ui, ACTIONS_W, ROW_H, |ui| {
                  ui.spacing_mut().item_spacing.x = 4.0;

                  let icon = Lucide::Settings.size(20.0).color(theme.colors.text).image();
                  let mut visuals = ButtonVisuals::default();
                  visuals.bg_hover = button_visuals.bg_hover;
                  visuals.corner_radius = CornerRadius::same(15);
                  let settings_btn = Button::image(icon).small().visuals(visuals);
                  if ui.add(settings_btn).on_hover_cursor(CursorIcon::PointingHand).clicked() {
                     self.open_rpc_settings();
                     self.rpc_to_edit = Some(rpc.clone());
                  }

                  if rpc.test_in_progress {
                     ui.add(Spinner::new().size(14.0).color(theme.colors.text));
                  } else {
                     let text = RichText::new("Test").size(theme.typography.normal);
                     let button = Button::new(text).visuals(button_visuals);
                     if ui.add(button).clicked() {
                        let rpc_clone = rpc.clone();
                        RT.spawn(async move {
                           let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
                           let z_client = ctx.get_zeus_client();
                           z_client.run_check_for(ctx, rpc_clone).await;
                           z_client.sort_by_fastest();
                        });
                     }
                  }

                  ui.add_space(5.0);

                  let button = Button::new(RichText::new("X").size(theme.typography.normal))
                     .visuals(button_visuals);
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
   }

   fn rpc_settings(&mut self, ctx: &mut ZeusContext, theme: &Theme, ui: &mut Ui) {
      ui.spacing_mut().button_padding = vec2(10.0, 8.0);
      ui.spacing_mut().item_spacing.y = 15.0;

      self.back_button(theme, ui);
      ui.label(RichText::new("Endpoint Settings").size(theme.typography.large));

      if self.rpc_to_edit.is_none() {
         let text = RichText::new("No RPC selected").size(theme.typography.normal);
         ui.label(text);
         return;
      }

      let rpc = self.rpc_to_edit.as_mut().unwrap();

      let text = RichText::new("MEV Protect").size(theme.typography.normal);
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
   }

   fn add_rpc(&mut self, theme: &Theme, ui: &mut Ui) {
      ui.spacing_mut().item_spacing = vec2(0.0, 15.0);
      ui.spacing_mut().button_padding = vec2(10.0, 8.0);

      self.back_button(theme, ui);
      ui.label(RichText::new("Add RPC").size(theme.typography.large));

      let button_visuals = theme.button_visuals();
      let text_edit_visuals = theme.text_edit_visuals();
      let ui_width = ui.available_width();

      let hint_text = RichText::new("Enter a url").size(theme.typography.normal);
      ui.add(
         SecureTextEdit::singleline(&mut self.url_to_add)
            .visuals(text_edit_visuals)
            .hint_text(hint_text)
            .font(FontId::proportional(theme.typography.normal))
            .min_size(vec2(ui_width * 0.5, 20.0))
            .margin(Margin::same(10)),
      );

      if !self.valid_url() && !self.url_to_add.is_empty() {
         ui.label(
            RichText::new("Invalid URL")
               .size(theme.typography.small)
               .color(theme.colors.error),
         );
      }

      if self.refreshing {
         ui.add(Spinner::new().size(15.0).color(theme.colors.text));
      }

      let text = RichText::new("Add").size(theme.typography.normal);
      let button = Button::new(text).visuals(button_visuals);
      if self.valid_url() {
         if ui.add_enabled(!self.refreshing, button).clicked() {
            self.refreshing = true;
            let chain = self.chain_select.chain.id();
            validate_rpc(chain, self.url_to_add.clone());
         }
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
            SHARED_GUI.write(|gui| {
               gui.open_msg_window(format!(
                  "Failed to connect to RPC: {}",
                  e.to_string()
               ));
               gui.settings.network.refreshing = false;
            });
            return;
         }
      };

      let rpc_chain = match client.get_chain_id().await {
         Ok(chain) => chain,
         Err(e) => {
            SHARED_GUI.write(|gui| {
               gui.open_msg_window(format!(
                  "Failed to get chain ID: {}",
                  e.to_string()
               ));
               gui.settings.network.refreshing = false;
            });
            return;
         }
      };

      if rpc_chain != chain {
         SHARED_GUI.write(|gui| {
            gui.open_msg_window(format!(
               "Chain Mismatch, RPC {} is for chain {}",
               rpc.url, rpc_chain
            ));
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
         gui.open_msg_window("RPC added successfully");
         gui.settings.network.url_to_add.clear();
         gui.settings.network.close_add_rpc();
         gui.settings.network.refreshing = false;
      });

      post_enable_rpc(ctx, chain).await
   });
}

fn rpc_col<R>(
   ui: &mut Ui,
   width: f32,
   height: f32,
   add_contents: impl FnOnce(&mut Ui) -> R,
) -> InnerResponse<R> {
   ui.allocate_ui_with_layout(
      vec2(width, height),
      Layout::left_to_right(Align::Center),
      |ui| {
         ui.set_min_size(vec2(width, height));
         ui.set_max_size(vec2(width, height));
         add_contents(ui)
      },
   )
}

fn rpc_status_indicator(rpc: &Rpc) -> Indicator {
   if rpc.is_working() {
      match rpc.is_fully_functional() {
         true => Indicator::new(IndicatorState::On).size(12.0),
         false => Indicator::new(IndicatorState::Connecting).size(12.0),
      }
   } else {
      Indicator::new(IndicatorState::Off).size(12.0)
   }
}

fn rpc_archive_indicator(rpc: &Rpc) -> Indicator {
   if rpc.is_archive() {
      Indicator::new(IndicatorState::On).size(12.0)
   } else {
      Indicator::new(IndicatorState::Off).size(12.0)
   }
}

fn rpc_mev_indicator(rpc: &Rpc) -> Indicator {
   if rpc.is_mev_protect() {
      Indicator::new(IndicatorState::On).size(12.0)
   } else {
      Indicator::new(IndicatorState::Off).size(12.0)
   }
}

async fn post_enable_rpc(ctx: ZeusCtx, chain: u64) {
   if ctx.is_chain_disabled(chain) {
      return;
   }

   let z_client = ctx.get_zeus_client();

   let rpcs = z_client.get_rpcs(chain);
   let valid_rpcs = rpcs.iter().filter(|rpc| rpc.1.is_enabled() && rpc.1.is_working()).count();
   let available_rpc = z_client.rpc_available(chain);

   // If we only have 1 active RPC, that means maybe is the first time Zeus started so we need to do
   // full state sync
   if valid_rpcs == 1 && available_rpc {
      state::sync_state(ctx.clone(), chain).await;
   }
}
