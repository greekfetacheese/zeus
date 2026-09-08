//! UI that allows the user to change the general settings.

use crate::core::ZeusContext;
use crate::utils::RT;
use egui::{Align, Layout, RichText, Ui, vec2};
use egui_elements::{Button, Theme};
use elegance::{Badge, BadgeTone};
use std::collections::HashSet;

const ICONS_TIP: &str = "Allow Zeus to download token icons from tokens.smold.app";
const SOURCIFY_TIP: &str = "Allow Zeus to look up verified contract names on sourcify.dev";

pub struct GeneralSettings {
   fetch_token_icons: bool,
   fetch_contract_names: bool,
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
         fetch_token_icons: false,
         fetch_contract_names: false,
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

   fn persist_misc(ctx: &ZeusContext) {
      let config = ctx.misc_config.clone();
      RT.spawn_blocking(move || {
         if let Err(e) = config.save() {
            tracing::error!("Failed to save misc config: {e}");
         }
      });
   }

   pub fn sync_from_ctx(&mut self, ctx: &mut ZeusContext) {
      let pool_manager = ctx.pool_manager.clone();
      let balance_manager = ctx.read_wallet_state(|ws| ws.balance_manager.clone());
      self.fetch_token_icons = ctx.misc_config.fetch_token_icons();
      self.fetch_contract_names = ctx.misc_config.fetch_contract_names();
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
      ui.spacing_mut().item_spacing = vec2(theme.spacing.xs, theme.spacing.lg);
      ui.spacing_mut().button_padding = vec2(theme.spacing.sm, theme.spacing.xs);

      let button_visuals = theme.button_visuals();
      let slider_size = vec2((ui.available_width() * 0.5).min(360.0), 20.0);
      ui.add_space(10.0);

      let header = RichText::new("External Data").size(theme.typography.very_large);
      ui.label(header);

      let q_mark_text = RichText::new("?").size(theme.typography.normal);
      let qmark = Badge::new(q_mark_text.clone(), BadgeTone::Info);

      let ui_size = vec2(ui.available_width() * 0.3, 30.0);
      let icons_text = RichText::new("Download Token Icons").size(theme.typography.normal);

      ui.allocate_ui_with_layout(
         ui_size,
         Layout::left_to_right(Align::Center),
         |ui| {
            if ui.checkbox(&mut self.fetch_token_icons, icons_text).changed() {
               ctx.misc_config.set_fetch_token_icons(self.fetch_token_icons);
               Self::persist_misc(ctx);
            }

            ui.add(qmark).on_hover_text(ICONS_TIP);
         },
      );

      let names_text = RichText::new("Fetch Contract Names").size(theme.typography.normal);
      let qmark = Badge::new(q_mark_text, BadgeTone::Info);

      ui.allocate_ui_with_layout(
         ui_size,
         Layout::left_to_right(Align::Center),
         |ui| {
            if ui.checkbox(&mut self.fetch_contract_names, names_text).changed() {
               ctx.misc_config.set_fetch_contract_names(self.fetch_contract_names);
               Self::persist_misc(ctx);
            }
            ui.add(qmark).on_hover_text(SOURCIFY_TIP);
         },
      );

      ui.separator();
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
      let mut save_misc = false;
      if self.fetch_token_icons != ctx.misc_config.fetch_token_icons() {
         ctx.misc_config.set_fetch_token_icons(self.fetch_token_icons);
         save_misc = true;
      }
      if self.fetch_contract_names != ctx.misc_config.fetch_contract_names() {
         ctx.misc_config.set_fetch_contract_names(self.fetch_contract_names);
         save_misc = true;
      }
      if save_misc {
         Self::persist_misc(ctx);
      }

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
