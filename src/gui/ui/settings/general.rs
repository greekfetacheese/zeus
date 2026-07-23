//! UI that allows the user to change the general settings.

use crate::utils::RT;
use crate::{core::ZeusContext, gui::SHARED_GUI};
use egui::{Align2, Order, RichText, ScrollArea, Slider, Ui, Window, vec2};
use std::collections::HashSet;
use zeus_eth::types::ChainId;
use zeus_theme::{OverlayManager, Theme};
use zeus_widgets::Button;

pub struct GeneralSettings {
   open: bool,
   overlay: OverlayManager,
   sync_v4_pools_on_startup: bool,
   concurrency_for_syncing_balances: usize,
   concurrency_for_syncing_pools: usize,
   batch_size_for_syncing_balances: usize,
   batch_size_for_updating_pools_state: usize,
   batch_size_for_syncing_pools: usize,
   ignore_chains: HashSet<u64>,
   size: (f32, f32),
}

impl GeneralSettings {
   pub fn new(ctx: &mut ZeusContext, overlay: OverlayManager) -> Self {
      let pool_manager = ctx.pool_manager.clone();
      let balance_manager = ctx.balance_manager.clone();
      Self {
         open: false,
         overlay,
         sync_v4_pools_on_startup: pool_manager.do_we_sync_v4_pools(),
         concurrency_for_syncing_balances: balance_manager.concurrency(),
         concurrency_for_syncing_pools: pool_manager.concurrency(),
         batch_size_for_syncing_balances: balance_manager.batch_size(),
         batch_size_for_updating_pools_state: pool_manager.batch_size_for_updating_pools_state(),
         batch_size_for_syncing_pools: pool_manager.batch_size_for_syncing_pools(),
         ignore_chains: pool_manager.ignore_chains(),
         size: (400.0, 550.0),
      }
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

   pub fn is_open(&self) -> bool {
      self.open
   }

   fn reset_settings(&mut self, ctx: &mut ZeusContext) {
      let pool_manager = ctx.pool_manager.clone();
      let balance_manager = ctx.balance_manager.clone();
      pool_manager.reset_default_settings();
      balance_manager.reset_default_settings();

      self.sync_v4_pools_on_startup = pool_manager.do_we_sync_v4_pools();
      self.concurrency_for_syncing_balances = balance_manager.concurrency();
      self.concurrency_for_syncing_pools = pool_manager.concurrency();
      self.batch_size_for_syncing_balances = balance_manager.batch_size();
      self.batch_size_for_updating_pools_state = pool_manager.batch_size_for_updating_pools_state();
      self.batch_size_for_syncing_pools = pool_manager.batch_size_for_syncing_pools();
      self.ignore_chains = pool_manager.ignore_chains();

      RT.spawn_blocking(move || {
         let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
         ctx.save_pool_manager();
         ctx.save_balance_manager();
      });
   }

   pub fn show(&mut self, ctx: &mut ZeusContext, theme: &Theme, ui: &mut Ui) {
      if !self.open {
         return;
      }

      let mut open = self.open;

      let title = RichText::new("General Settings").size(theme.text_sizes.heading);
      let window_frame = theme.frame1;

      Window::new(title)
         .open(&mut open)
         .resizable(false)
         .collapsible(false)
         .order(Order::Foreground)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(window_frame)
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);
            ui.spacing_mut().item_spacing = vec2(5.0, 20.0);
            ui.spacing_mut().button_padding = vec2(10.0, 4.0);

            let button_visuals = theme.button_visuals();

            ui.vertical_centered(|ui| {
               ScrollArea::vertical().show(ui, |ui| {
                  let slider_size = vec2(ui.available_width() * 0.4, 20.0);

                  let header = RichText::new("Pool Manager").size(theme.text_sizes.very_large);
                  ui.label(header);

                  let text = RichText::new("Reset Settings").size(theme.text_sizes.normal);
                  let button = Button::new(text).visuals(button_visuals);

                  if ui.add(button).clicked() {
                     self.reset_settings(ctx);
                  }

                  let text =
                     RichText::new("Sync V4 Pools on startup").size(theme.text_sizes.normal);
                  ui.checkbox(&mut self.sync_v4_pools_on_startup, text);

                  let text = RichText::new("Chains to ignore at V4 historic sync")
                     .size(theme.text_sizes.normal);
                  ui.label(text);
                  for chain in ChainId::supported_chains() {
                     let text = RichText::new(chain.name()).size(theme.text_sizes.normal);
                     let mut ignore = self.ignore_chains.contains(&chain.id());
                     ui.checkbox(&mut ignore, text);
                     if ignore {
                        self.ignore_chains.insert(chain.id());
                     } else {
                        self.ignore_chains.remove(&chain.id());
                     }
                  }

                  ui.label(
                     RichText::new("Concurrency for Syncing & Updating Pools")
                        .size(theme.text_sizes.normal),
                  );
                  ui.allocate_ui(slider_size, |ui| {
                     ui.add(Slider::new(
                        &mut self.concurrency_for_syncing_pools,
                        1..=10,
                     ));
                  });

                  ui.label(
                     RichText::new("Batch Size for Syncing Pools").size(theme.text_sizes.normal),
                  );
                  ui.allocate_ui(slider_size, |ui| {
                     ui.add(Slider::new(
                        &mut self.batch_size_for_syncing_pools,
                        1..=60,
                     ));
                  });

                  ui.label(
                     RichText::new("Batch Size when updating pools state")
                        .size(theme.text_sizes.normal),
                  );
                  ui.allocate_ui(slider_size, |ui| {
                     ui.add(Slider::new(
                        &mut self.batch_size_for_updating_pools_state,
                        1..=50,
                     ));
                  });

                  let header = RichText::new("Balance Manager").size(theme.text_sizes.very_large);
                  ui.label(header);

                  ui.label(
                     RichText::new("Concurrency for syncing balances")
                        .size(theme.text_sizes.normal),
                  );
                  ui.allocate_ui(slider_size, |ui| {
                     ui.add(Slider::new(
                        &mut self.concurrency_for_syncing_balances,
                        1..=10,
                     ));
                  });

                  ui.label(
                     RichText::new("Batch Size for syncing balances").size(theme.text_sizes.normal),
                  );
                  ui.allocate_ui(slider_size, |ui| {
                     ui.add(Slider::new(
                        &mut self.batch_size_for_syncing_balances,
                        1..=50,
                     ));
                  });
               });
            });
         });

      let closed = self.open && !open;
      // self.open = open;

      if closed {
         self.close();
         self.save_settings(ctx);
      }
   }

   fn save_settings(&self, ctx: &mut ZeusContext) {
      let save_balance_manager =
         if self.concurrency_for_syncing_balances != ctx.balance_manager.concurrency() {
            ctx.balance_manager.set_concurrency(self.concurrency_for_syncing_balances);
            true
         } else if self.batch_size_for_syncing_balances != ctx.balance_manager.batch_size() {
            ctx.balance_manager.set_batch_size(self.batch_size_for_syncing_balances);
            true
         } else {
            false
         };

      let save_pool_manager = if self.concurrency_for_syncing_pools
         != ctx.pool_manager.concurrency()
      {
         ctx.pool_manager.set_concurrency(self.concurrency_for_syncing_pools);
         true
      } else if self.batch_size_for_updating_pools_state
         != ctx.pool_manager.batch_size_for_updating_pools_state()
      {
         ctx.pool_manager
            .set_batch_size_for_updating_pools_state(self.batch_size_for_updating_pools_state);
         true
      } else if self.batch_size_for_syncing_pools != ctx.pool_manager.batch_size_for_syncing_pools()
      {
         ctx.pool_manager
            .set_batch_size_for_syncing_pools(self.batch_size_for_syncing_pools);
         true
      } else if self.sync_v4_pools_on_startup != ctx.pool_manager.do_we_sync_v4_pools() {
         ctx.pool_manager.set_sync_v4_pools(self.sync_v4_pools_on_startup);
         true
      } else if self.ignore_chains != ctx.pool_manager.ignore_chains() {
         ctx.pool_manager.set_ignore_chains(self.ignore_chains.clone());
         true
      } else {
         false
      };

      RT.spawn_blocking(move || {
         let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
         if save_balance_manager {
            ctx.save_balance_manager();
         }

         if save_pool_manager {
            let _res = ctx.save_pool_manager();
         }
      });
   }
}
