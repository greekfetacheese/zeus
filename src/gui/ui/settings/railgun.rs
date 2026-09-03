//! UI that allows the user to change the railgun settings.

use crate::gui::SHARED_GUI;
use crate::utils::RT;
use crate::{
   assets::Icons,
   core::{ZeusContext, types::RailgunConfig},
   gui::ui::chain_select::ChainSelect,
};
use egui::{Align, Layout, RichText, Ui, vec2};
use egui_elements::{Button, Theme};
use elegance::{Badge, BadgeTone, Slider};
use zeus_eth::types::SUPPORTED_CHAINS;
use zeus_railgun::indexer::syncer::rpc::DEFAULT_BLOCK_RANGE;

use std::sync::Arc;

const BLOCK_RANGE_TIP: &str =
   "If the sync fails often due to invalid root you may need to decrease the block range";

pub struct RailgunSettings {
   chain_select: ChainSelect,
   config: RailgunConfig,
}

impl RailgunSettings {
   pub fn new(ctx: &mut ZeusContext) -> Self {
      Self {
         chain_select: ChainSelect::new("railgun_config_chain", 1).size(vec2(220.0, 25.0)),
         config: ctx.railgun_config.clone(),
      }
   }

   pub fn sync_from_ctx(&mut self, ctx: &mut ZeusContext) {
      self.config = ctx.railgun_config.clone();
   }

   fn block_range(&self) -> u64 {
      let chain = self.chain_select.chain.id();
      *self.config.rpc_syncer_block_range.get(&chain).unwrap_or(&DEFAULT_BLOCK_RANGE)
   }

   fn set_block_range(&mut self, range: u64) {
      let chain = self.chain_select.chain.id();
      self.config.rpc_syncer_block_range.insert(chain, range);
   }

   fn concurrency(&self) -> usize {
      self.config.rpc_syncer_concurrency
   }

   fn set_concurrency(&mut self, concurrency: usize) {
      self.config.rpc_syncer_concurrency = concurrency;
   }

   pub fn show(&mut self, ctx: &mut ZeusContext, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      ui.spacing_mut().item_spacing = vec2(5.0, 16.0);
      ui.spacing_mut().button_padding = vec2(10.0, 4.0);
      ui.add_space(10.0);

      let slider_size = vec2((ui.available_width() * 0.6).min(420.0), 24.0);
      let button_size = vec2(100.0, 35.0);
      let button_visuals = theme.button_visuals();

      let ignore = [10, 56, 8453, 42161];
      self.chain_select.show(ctx, &ignore, theme, icons, ui);
      ui.add_space(10.0);

      let chain = self.chain_select.chain.id();
      let mut enabled = self.config.is_enabled(chain);
      let enable_text = RichText::new("Enable Railgun").size(theme.typography.normal);
      if ui.checkbox(&mut enabled, enable_text).changed() {
         self.config.set_enabled(chain, enabled);
      }

      let q_mark = RichText::new("?").size(theme.typography.normal);
      let info_tip = Badge::new(q_mark, BadgeTone::Info);

      ui.allocate_ui(slider_size, |ui| {
         ui.horizontal(|ui| {
            ui.label(RichText::new("RPC Syncer Block Range").size(theme.typography.normal));
            ui.add(info_tip).on_hover_text(BLOCK_RANGE_TIP);
         });
      });

      let mut block_range = self.block_range();
      let changed = ui
         .allocate_ui(slider_size, |ui| {
            ui.add(Slider::new(&mut block_range, 100..=30_000).desired_width(slider_size.x))
         })
         .inner
         .changed();
      if changed {
         self.set_block_range(block_range);
      }

      ui.label(RichText::new("RPC Syncer Concurrency").size(theme.typography.normal));

      let mut concurrency = self.concurrency();
      let changed = ui
         .allocate_ui(slider_size, |ui| {
            ui.add(Slider::new(&mut concurrency, 1..=10).desired_width(slider_size.x))
         })
         .inner
         .changed();
      if changed {
         self.set_concurrency(concurrency);
      }

      ui.add_space(10.0);
      let ui_size = vec2(ui.available_width() * 0.5, 35.0);

      ui.allocate_ui(ui_size, |ui| {
         ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            let text = RichText::new("Save").size(theme.typography.normal);
            let button = Button::new(text).visuals(button_visuals).min_size(button_size);
            if ui.add(button).clicked() {
               let new_config = self.config.clone();
               post_click(ctx, new_config);
            }

            ui.add_space(10.0);

            let text = RichText::new("Reset").size(theme.typography.normal);
            let button = Button::new(text).visuals(button_visuals).min_size(button_size);
            if ui.add(button).clicked() {
               let new_config = RailgunConfig::default();
               self.config = new_config.clone();

               post_click(ctx, new_config);
            }
         });
      });
   }
}

fn post_click(ctx: &mut ZeusContext, new_config: RailgunConfig) {
   ctx.railgun_config = new_config.clone();

   let new_config_clone = new_config.clone();
   RT.spawn(async move {
      let res = new_config_clone.save();

      match res {
         Ok(_) => {
            SHARED_GUI.write(|gui| {
               gui.msg_window.open("Railgun Config Saved");
               gui.request_repaint();
            });
         }
         Err(e) => {
            SHARED_GUI.write(|gui| {
               gui.msg_window.open(e.to_string());
               gui.request_repaint();
            });
         }
      }

      let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());

      for chain in SUPPORTED_CHAINS {
         if !ctx.railgun_is_supported(chain.into()) {
            continue;
         }

         if !new_config.is_enabled(chain) {
            continue;
         }

         if let Err(e) = ctx.register_railgun_signers(chain, false).await {
            tracing::error!("Error registering Railgun signers: {:?}", e);
         }

         let provider = match ctx.get_railgun_provider(chain, false).await {
            Ok(provider) => provider,
            Err(e) => {
               tracing::error!("Error getting Railgun provider: {:?}", e);
               continue;
            }
         };

         let concurrency = new_config.rpc_syncer_concurrency;
         let block_range = new_config
            .rpc_syncer_block_range
            .get(&chain)
            .cloned()
            .unwrap_or(DEFAULT_BLOCK_RANGE);

         let syncer = provider.utxo_indexer.read().await.rpc_syncer.clone();
         syncer.set_block_range(block_range).await;
         syncer.set_concurrency(concurrency).await;

         if let Err(e) = ctx.sync_railgun(chain, false).await {
            tracing::error!("Error syncing Railgun: {:?}", e);
         }
      }
   });
}
