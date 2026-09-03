//! UI that allows the user to change the general settings.

use crate::core::ZeusContext;
use egui::{RichText, Ui, vec2};
use egui_elements::{Button, Theme};
use std::collections::HashSet;

pub struct GeneralSettings {
   discover_v4_pools_on_startup: bool,
   concurrency_for_syncing_balances: usize,
   concurrency_for_discovering_pools: usize,
   batch_size_for_syncing_balances: usize,
   batch_size_for_updating_pools_state: usize,
   batch_size_for_discovering_pools: usize,
   ignore_chains: HashSet<u64>,
}

impl GeneralSettings {
   pub fn new(ctx: &mut ZeusContext) -> Self {
      let mut this = Self {
         discover_v4_pools_on_startup: false,
         concurrency_for_syncing_balances: 1,
         concurrency_for_discovering_pools: 1,
         batch_size_for_syncing_balances: 1,
         batch_size_for_updating_pools_state: 1,
         batch_size_for_discovering_pools: 1,
         ignore_chains: HashSet::new(),
      };
      this.sync_from_ctx(ctx);
      this
   }

   pub fn sync_from_ctx(&mut self, ctx: &mut ZeusContext) {
      let pool_manager = ctx.pool_manager.clone();
      let balance_manager = ctx.read_wallet_state(|ws| ws.balance_manager.clone());
      self.discover_v4_pools_on_startup = pool_manager.do_we_discover_v4_pools();
      self.concurrency_for_syncing_balances = balance_manager.concurrency();
      self.concurrency_for_discovering_pools = pool_manager.concurrency();
      self.batch_size_for_syncing_balances = balance_manager.batch_size();
      self.batch_size_for_updating_pools_state = pool_manager.batch_size_for_updating_pools_state();
      self.batch_size_for_discovering_pools = pool_manager.batch_size_for_discovering_pools();
      self.ignore_chains = pool_manager.ignore_chains();
   }

   fn reset_settings(&mut self, ctx: &mut ZeusContext) {
      let pool_manager = ctx.pool_manager.clone();
      let balance_manager = ctx.read_wallet_state(|ws| ws.balance_manager.clone());
      pool_manager.reset_default_settings();
      balance_manager.reset_default_settings();
      self.sync_from_ctx(ctx);
   }

   pub fn show(&mut self, ctx: &mut ZeusContext, theme: &Theme, ui: &mut Ui) {
      ui.spacing_mut().item_spacing = vec2(5.0, 16.0);
      ui.spacing_mut().button_padding = vec2(10.0, 4.0);

      let button_visuals = theme.button_visuals();
      let slider_size = vec2((ui.available_width() * 0.5).min(360.0), 20.0);
      ui.add_space(10.0);

      let header = RichText::new("Pool Manager").size(theme.typography.very_large);
      ui.label(header);

      let text = RichText::new("Reset Settings").size(theme.typography.normal);
      let button = Button::new(text).visuals(button_visuals);

      if ui.add(button).clicked() {
         self.reset_settings(ctx);
      }

      // let text = RichText::new("Discover V4 Pools on startup").size(theme.typography.normal);
      // ui.checkbox(&mut self.discover_v4_pools_on_startup, text);

      /*
      let text =
         RichText::new("Chains to ignore at V4 historic sync").size(theme.typography.normal);
      ui.label(text);
      for chain in ChainId::supported_chains() {
         let text = RichText::new(chain.name()).size(theme.typography.normal);
         let mut ignore = self.ignore_chains.contains(&chain.id());
         ui.checkbox(&mut ignore, text);
         if ignore {
            self.ignore_chains.insert(chain.id());
         } else {
            self.ignore_chains.remove(&chain.id());
         }
      }
      */

      ui.label(
         RichText::new("Concurrency for Discovering & Updating Pools")
            .size(theme.typography.normal),
      );
      ui.allocate_ui(slider_size, |ui| {
         ui.add(egui::Slider::new(
            &mut self.concurrency_for_discovering_pools,
            1..=10,
         ));
      });

      ui.label(RichText::new("Batch Size for Discovering Pools").size(theme.typography.normal));
      ui.allocate_ui(slider_size, |ui| {
         ui.add(egui::Slider::new(
            &mut self.batch_size_for_discovering_pools,
            1..=60,
         ));
      });

      ui.label(RichText::new("Batch Size when updating pools state").size(theme.typography.normal));
      ui.allocate_ui(slider_size, |ui| {
         ui.add(egui::Slider::new(
            &mut self.batch_size_for_updating_pools_state,
            1..=50,
         ));
      });

      ui.separator();
      ui.add_space(10.0);

      let header = RichText::new("Balance Manager").size(theme.typography.very_large);
      ui.label(header);

      ui.label(RichText::new("Concurrency for syncing balances").size(theme.typography.normal));
      ui.allocate_ui(slider_size, |ui| {
         ui.add(egui::Slider::new(
            &mut self.concurrency_for_syncing_balances,
            1..=10,
         ));
      });

      ui.label(RichText::new("Batch Size for syncing balances").size(theme.typography.normal));
      ui.allocate_ui(slider_size, |ui| {
         ui.add(egui::Slider::new(
            &mut self.batch_size_for_syncing_balances,
            1..=50,
         ));
      });
   }

   pub fn save_settings(&self, ctx: &mut ZeusContext) {
      // Balance settings live in the vault and are written on vault save / shutdown.
      let balance_manager = ctx.read_wallet_state(|ws| ws.balance_manager.clone());
      if self.concurrency_for_syncing_balances != balance_manager.concurrency() {
         balance_manager.set_concurrency(self.concurrency_for_syncing_balances);
      }
      if self.batch_size_for_syncing_balances != balance_manager.batch_size() {
         balance_manager.set_batch_size(self.batch_size_for_syncing_balances);
      }

      let _save_pool_manager =
         if self.concurrency_for_discovering_pools != ctx.pool_manager.concurrency() {
            ctx.pool_manager.set_concurrency(self.concurrency_for_discovering_pools);
            true
         } else if self.batch_size_for_updating_pools_state
            != ctx.pool_manager.batch_size_for_updating_pools_state()
         {
            ctx.pool_manager
               .set_batch_size_for_updating_pools_state(self.batch_size_for_updating_pools_state);
            true
         } else if self.batch_size_for_discovering_pools
            != ctx.pool_manager.batch_size_for_discovering_pools()
         {
            ctx.pool_manager
               .set_batch_size_for_discovering_pools(self.batch_size_for_discovering_pools);
            true
         } else if self.discover_v4_pools_on_startup != ctx.pool_manager.do_we_discover_v4_pools() {
            ctx.pool_manager.set_discover_v4_pools(self.discover_v4_pools_on_startup);
            true
         } else if self.ignore_chains != ctx.pool_manager.ignore_chains() {
            ctx.pool_manager.set_ignore_chains(self.ignore_chains.clone());
            true
         } else {
            false
         };
   }
}
