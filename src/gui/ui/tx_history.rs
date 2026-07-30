//! UI that shows the transaction history

use crate::core::{TransactionRich, WalletInfo, ZeusContext};
use crate::gui::SHARED_GUI;
use crate::utils::{RT, truncate_address};
use egui::{Align, Frame, Grid, Layout, Margin, RichText, ScrollArea, Sense, Ui, vec2};
use zeus_eth::{alloy_primitives::Address, types::ChainId};
use zeus_theme::Theme;
use zeus_widgets::{Button, ComboBox, Label};

const DEFAULT_TXS_PER_PAGE: usize = 20;

pub struct TxHistory {
   open: bool,
   /// True while the redb is being opened/loaded off the UI thread
   loading: bool,
   /// True once `open_tx_db` finished for this session
   db_ready: bool,
   pub current_page: usize,
   pub txs_per_page: usize,
   selected_wallet: Option<WalletInfo>,
   selected_chain: Option<ChainId>,
   /// Filtered list for the current filters (avoids cloning every frame)
   cached_txs: Vec<TransactionRich>,
   /// Fingerprint of the last cache build so we rebuild only when needed
   cache_key: CacheKey,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct CacheKey {
   wallet: Option<Address>,
   chain: Option<u64>,
}

impl CacheKey {
   /// Sentinel that never matches a real filter selection.
   /// Used to force a rebuild after the DB finishes loading.
   fn invalid() -> Self {
      Self {
         wallet: None,
         chain: Some(u64::MAX),
      }
   }
}

impl TxHistory {
   pub fn new() -> Self {
      Self {
         open: false,
         loading: false,
         db_ready: false,
         current_page: 0,
         txs_per_page: DEFAULT_TXS_PER_PAGE,
         selected_wallet: None,
         selected_chain: None,
         cached_txs: Vec::new(),
         cache_key: CacheKey::default(),
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   /// Mark the view open and start loading the tx DB off the UI thread.
   pub fn open(&mut self) {
      if self.open {
         return;
      }
      self.open = true;
      self.loading = true;
      self.db_ready = false;
      self.cached_txs.clear();
      self.cache_key = CacheKey::default();
      self.current_page = 0;

      RT.spawn_blocking(move || {
         let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
         // Bail if the user already navigated away
         let still_open = SHARED_GUI.read(|gui| gui.tx_history.is_open());
         if !still_open {
            return;
         }
         ctx.open_tx_db();
         SHARED_GUI.write(|gui| {
            if gui.tx_history.is_open() {
               gui.tx_history.on_db_ready();
            } else {
               // Closed while loading, drop anything we just opened
               gui.ctx.close_tx_db();
            }
            gui.request_repaint();
         });
      });
   }

   /// Close the view, drop the UI cache, and unload the tx DB from memory.
   pub fn close(&mut self, ctx: &mut ZeusContext) {
      if !self.open && !self.db_ready && self.cached_txs.is_empty() {
         return;
      }
      self.open = false;
      self.loading = false;
      self.db_ready = false;
      self.cached_txs = Vec::new();
      self.cache_key = CacheKey::default();
      ctx.tx_db.close();
   }

   fn on_db_ready(&mut self) {
      self.loading = false;
      self.db_ready = true;
      self.cached_txs.clear();
      // Must NOT leave cache_key equal to the current filters (Default == All/All),
      // or rebuild_cache early-returns and never fills the list on first open.
      self.cache_key = CacheKey::invalid();
   }

   fn wallet_name_or_address(&self, ctx: &mut ZeusContext, address: Address) -> String {
      let name_opt = ctx.get_wallet_name(address);
      if let Some(name) = name_opt {
         name
      } else {
         truncate_address(address.to_string())
      }
   }

   fn current_cache_key(&self) -> CacheKey {
      CacheKey {
         wallet: self.selected_wallet.as_ref().map(|w| w.address),
         chain: self.selected_chain.map(|c| c.id()),
      }
   }

   fn rebuild_cache(&mut self) {
      if !self.db_ready {
         self.cached_txs.clear();
         return;
      }

      let key = self.current_cache_key();
      // Already built (or a build is in-flight) for this filter set
      if key == self.cache_key {
         return;
      }

      // Claim the key *before* spawning so we don't queue a rebuild every frame
      // while the worker is still running.
      self.cache_key = key.clone();

      let selected_wallet = self.selected_wallet.clone();
      let selected_chain = self.selected_chain;

      RT.spawn_blocking(move || {
         let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
         let tx_db = ctx.read(|ctx| ctx.tx_db.clone());
         let tx_count = tx_db.txs_count();

         let mut txs = Vec::with_capacity(tx_count);
         let wallets = ctx.get_all_wallets_info();

         for wallet in wallets {
            if let Some(selected) = &selected_wallet {
               if selected.address != wallet.address {
                  continue;
               }
            }

            let chains_to_check: Vec<ChainId> = if let Some(chain) = selected_chain {
               vec![chain]
            } else {
               ChainId::supported_chains()
            };

            for chain in chains_to_check {
               if ctx.is_chain_disabled(chain.id()) {
                  continue;
               }

               if let Some(wallet_txs) = tx_db.get_txs(chain.id(), wallet.address) {
                  txs.extend(wallet_txs.iter().cloned());
               }
            }
         }

         txs.sort_unstable_by(|a, b| b.timestamp.cmp(&a.timestamp));

         SHARED_GUI.write(|gui| {
            // Drop stale results if the user changed filters while we were building
            if gui.tx_history.cache_key == key {
               gui.tx_history.cached_txs = txs;
            }
            gui.request_repaint();
         });
      });
   }

   pub fn show(&mut self, ctx: &mut ZeusContext, theme: &Theme, ui: &mut Ui) {
      if !self.open {
         return;
      }

      self.rebuild_cache();

      Frame::new().inner_margin(Margin::same(10)).show(ui, |ui| {
         ui.set_width(ui.available_width());
         ui.set_height(ui.available_height());
         ui.spacing_mut().item_spacing = vec2(10.0, 15.0);
         ui.spacing_mut().button_padding = vec2(10.0, 8.0);

         ui.vertical_centered_justified(|ui| {
            ui.label(
               RichText::new("Transaction History")
                  .size(theme.text_sizes.heading)
                  .color(theme.colors.text),
            );
         });

         ui.add_space(10.0);

         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.spacing_mut().item_spacing.x = 20.0;
            ui.spacing_mut().button_padding = vec2(10.0, 8.0);

            let combo_visuals = theme.combo_box_visuals();
            let label_visuals = theme.label_visuals();
            let expansion = Some(6.0);

            // Wallet Filter
            let wallets = ctx.get_all_wallets_info();
            let selected_wallet_name =
               self.selected_wallet.clone().map_or("All Wallets".to_string(), |wallet| {
                  wallet.name_with_source()
               });

            let text = RichText::new(selected_wallet_name).size(theme.text_sizes.normal);
            let label = Label::new(text, None)
               .visuals(label_visuals)
               .fill_width(true)
               .sense(Sense::click())
               .expand(expansion);

            ComboBox::new("wallet_filter", label)
               .visuals(combo_visuals)
               .width(200.0)
               .show_ui(ui, |ui| {
                  ui.spacing_mut().item_spacing.y = 10.0;

                  let text = RichText::new("All Wallets").size(theme.text_sizes.normal);
                  let label = Label::new(text, None)
                     .visuals(label_visuals)
                     .fill_width(true)
                     .sense(Sense::click())
                     .expand(expansion);

                  if ui.add(label).clicked() {
                     if self.selected_wallet.is_some() {
                        self.selected_wallet = None;
                        self.current_page = 0;
                     }
                  }

                  for (_, wallet) in wallets {
                     let text =
                        RichText::new(&wallet.name_with_source()).size(theme.text_sizes.normal);
                     let label = Label::new(text, None)
                        .visuals(label_visuals)
                        .sense(Sense::click())
                        .fill_width(true)
                        .expand(expansion);

                     if ui.add(label).clicked() {
                        if self.selected_wallet != Some(wallet.clone()) {
                           self.selected_wallet = Some(wallet.clone());
                           self.current_page = 0;
                        }
                     }
                  }
               });

            // --- Chain Filter ---
            let selected_chain_name =
               self.selected_chain.map_or("All Chains".to_string(), |chain| {
                  chain.name().to_string()
               });

            let text = RichText::new(selected_chain_name).size(theme.text_sizes.normal);
            let label = Label::new(text, None)
               .visuals(label_visuals)
               .fill_width(true)
               .sense(Sense::click())
               .expand(expansion);

            ComboBox::new("chain_filter", label)
               .visuals(combo_visuals)
               .width(200.0)
               .show_ui(ui, |ui| {
                  ui.spacing_mut().item_spacing.y = 10.0;

                  let text = RichText::new("All Chains").size(theme.text_sizes.normal);
                  let label = Label::new(text, None)
                     .visuals(label_visuals)
                     .fill_width(true)
                     .sense(Sense::click())
                     .expand(expansion);

                  if ui.add(label).clicked() {
                     if self.selected_chain.is_some() {
                        self.selected_chain = None;
                        self.current_page = 0;
                     }
                  }

                  for chain in ChainId::supported_chains() {
                     if ctx.is_chain_disabled(chain.id()) {
                        continue;
                     }

                     let text = RichText::new(chain.name()).size(theme.text_sizes.normal);
                     let label = Label::new(text, None)
                        .visuals(label_visuals)
                        .sense(Sense::click())
                        .fill_width(true)
                        .expand(expansion);

                     if ui.add(label).clicked() {
                        if self.selected_chain != Some(chain) {
                           self.selected_chain = Some(chain);
                           self.current_page = 0;
                        }
                     }
                  }
               });

            #[cfg(feature = "dev")]
            if ui.add(Button::new("Reload TxDB")).clicked() {
               self.loading = true;
               self.db_ready = false;
               self.cached_txs.clear();
               RT.spawn_blocking(move || {
                  let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
                  ctx.close_tx_db();
                  ctx.open_tx_db();
                  SHARED_GUI.write(|gui| {
                     gui.tx_history.on_db_ready();
                     gui.request_repaint();
                  });
               });
            }
         });

         ui.add_space(10.0);
         ui.separator();
         ui.add_space(10.0);

         if self.loading {
            ui.vertical_centered(|ui| {
               ui.label(
                  RichText::new("Loading transactions…")
                     .size(theme.text_sizes.large)
                     .color(theme.colors.text),
               );
            });
            return;
         }

         // Rebuild after filter widgets may have changed selection
         self.rebuild_cache();

         if self.cached_txs.is_empty() {
            ui.vertical_centered(|ui| {
               ui.label(
                  RichText::new("No transactions match your filters.")
                     .size(theme.text_sizes.large)
                     .color(theme.colors.text),
               );
            });
            return;
         }

         let total_txs = self.cached_txs.len();
         let total_pages = (total_txs as f64 / self.txs_per_page as f64).ceil() as usize;
         // Ensure current page is valid
         self.current_page = self.current_page.min(total_pages.saturating_sub(1));

         let button_visuals = theme.button_visuals();

         // --- Pagination Controls ---
         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            // Next Page Button
            let next_enabled = (self.current_page + 1) < total_pages;
            let text = RichText::new("Next").size(theme.text_sizes.normal);
            let next_button = Button::new(text).visuals(button_visuals);
            if ui.add_enabled(next_enabled, next_button).clicked() {
               self.current_page += 1;
            }

            // Page Number Display
            ui.label(format!(
               "Page {} of {}",
               self.current_page + 1,
               total_pages.max(1)
            ));

            // Previous Page Button
            let prev_enabled = self.current_page > 0;
            let text = RichText::new("Previous").size(theme.text_sizes.normal);
            let prev_button = Button::new(text).visuals(button_visuals);
            if ui.add_enabled(prev_enabled, prev_button).clicked() {
               self.current_page -= 1;
            }
         });
         ui.add_space(5.0);

         ui.vertical_centered(|ui| {
            ui.label(
               RichText::new(format!("{} transactions found", total_txs))
                  .size(theme.text_sizes.normal)
                  .color(theme.colors.text),
            );
         });
         ui.add_space(20.0);

         ScrollArea::vertical()
            .id_salt("tx_history_scroll_area")
            .auto_shrink([false; 2])
            .max_height(ui.available_height() * 0.8)
            .show(ui, |ui| {
               ui.set_width(ui.available_width());

               let start = self.current_page * self.txs_per_page;
               let end = start.saturating_add(self.txs_per_page).min(total_txs);
               let txs_on_page = if start < end {
                  &self.cached_txs[start..end]
               } else {
                  &[]
               };

               let column_widths = [
                  ui.available_width() * 0.2,
                  ui.available_width() * 0.2,
                  ui.available_width() * 0.2,
                  ui.available_width() * 0.2,
               ];

               ui.horizontal(|ui| {
                  ui.add_space((ui.available_width() - column_widths.iter().sum::<f32>()) / 2.0);

                  Grid::new("tx_history_grid")
                     .spacing([20.0, 10.0])
                     .num_columns(4)
                     .striped(true)
                     .show(ui, |ui| {
                        ui.label(
                           RichText::new("Wallet")
                              .strong()
                              .size(theme.text_sizes.large)
                              .color(theme.colors.text),
                        );

                        ui.label(
                           RichText::new("Action")
                              .strong()
                              .size(theme.text_sizes.large)
                              .color(theme.colors.text),
                        );

                        ui.label(
                           RichText::new("Age")
                              .strong()
                              .size(theme.text_sizes.large)
                              .color(theme.colors.text),
                        );

                        ui.label(
                           RichText::new("Details")
                              .strong()
                              .size(theme.text_sizes.large)
                              .color(theme.colors.text),
                        );
                        ui.end_row();

                        for tx in txs_on_page {
                           // Wallet Name Column
                           // TODO: Tweak this its very bad
                           let name = self.wallet_name_or_address(ctx, tx.sender());
                           ui.horizontal(|ui| {
                              ui.set_width(column_widths[0]);
                              ui.label(
                                 RichText::new(name)
                                    .size(theme.text_sizes.normal)
                                    .color(theme.colors.text),
                              );
                           });

                           // Action Name Column
                           ui.horizontal(|ui| {
                              ui.set_width(column_widths[1]);
                              ui.label(
                                 RichText::new(tx.main_event.name())
                                    .size(theme.text_sizes.normal)
                                    .color(theme.colors.text),
                              );
                           });

                           // Age Column
                           ui.horizontal(|ui| {
                              ui.set_width(column_widths[2]);
                              ui.label(
                                 RichText::new(tx.timestamp.to_relative())
                                    .size(theme.text_sizes.small)
                                    .color(theme.colors.text),
                              );
                           });

                           // Details Button Column
                           let text = RichText::new("Details").size(theme.text_sizes.normal);
                           let details_button = Button::new(text).visuals(button_visuals);
                           ui.horizontal(|ui| {
                              ui.set_width(column_widths[3]);
                              if ui.add(details_button).clicked() {
                                 let tx_clone = tx.clone();
                                 RT.spawn_blocking(move || {
                                    SHARED_GUI.write(|gui| {
                                       gui.tx_window.open(Some(tx_clone));
                                    });
                                 });
                              }
                           });

                           ui.end_row();
                        }
                     });
               });
            });
      });
   }
}
