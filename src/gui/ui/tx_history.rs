//! UI that shows the transaction history

use crate::core::{TransactionRich, WalletInfo, ZeusContext};
use crate::gui::{
   SHARED_GUI,
   ui::{
      show_with_fade,
      tx::spent_note_window::{SpentHistoryRow, spent_age},
   },
};
use crate::utils::{RT, truncate_address};
use egui::{Align, Frame, Layout, Margin, RichText, ScrollArea, Sense, Spinner, Ui, vec2};
use egui_elements::{Button, ComboBox, Label, Theme};
use elegance::{Badge, BadgeTone};
use zeus_eth::{
   alloy_primitives::{Address, U256},
   types::ChainId,
   utils::NumericValue,
};
use zeus_railgun::{PrivateHistoryEntry, PrivateHistoryKind};

const DEFAULT_TXS_PER_PAGE: usize = 10;

const ZEUS_TIP: &str = "Zeus only shows transactions that have been been made in-app.\n
It cannot track transactions made from other wallets.";

const RAILGUN_TIP: &str = "Private history shows what actually left your 0zk.\n
Change from spent UTXOs is subtracted so a 1 USDC transfer shows as 1 USDC.";

pub struct TxHistory {
   open: bool,
   /// Reserved for a future loading state
   loading: bool,
   /// True once the history view is ready to read the vault tx cache
   db_ready: bool,
   pub current_page: usize,
   pub txs_per_page: usize,
   selected_wallet: Option<WalletInfo>,
   selected_chain: Option<ChainId>,
   /// Filtered list for the current filters (avoids cloning every frame)
   cached_txs: Vec<TransactionRich>,
   cached_spent: Vec<SpentHistoryRow>,
   /// Fingerprint of the last cache build so we rebuild only when needed
   cache_key: CacheKey,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct CacheKey {
   wallet: Option<Address>,
   chain: Option<u64>,
   privacy: bool,
}

impl CacheKey {
   /// Sentinel that never matches a real filter selection.
   /// Used to force a rebuild after the DB finishes loading.
   fn invalid() -> Self {
      Self {
         wallet: None,
         chain: Some(u64::MAX),
         privacy: false,
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
         cached_spent: Vec::new(),
         cache_key: CacheKey::default(),
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   /// Mark the view open. Tx history is already in the vault (loaded on unlock).
   pub fn open(&mut self) {
      if self.open {
         return;
      }
      self.open = true;
      self.loading = false;
      self.db_ready = true;
      self.cached_txs.clear();
      self.cached_spent.clear();
      self.cache_key = CacheKey::invalid();
      self.current_page = 0;
   }

   /// Close the view and drop the UI cache.
   pub fn close(&mut self, _ctx: &mut ZeusContext) {
      if !self.open && !self.db_ready && self.cached_txs.is_empty() {
         return;
      }
      self.open = false;
      self.loading = false;
      self.db_ready = false;
      self.cached_txs = Vec::new();
      self.cached_spent = Vec::new();
      self.cache_key = CacheKey::default();
      self.selected_chain = None;
      self.selected_wallet = None;
   }

   fn wallet_name_or_address(&self, ctx: &mut ZeusContext, address: Address) -> String {
      let name_opt = ctx.get_wallet_name(address);
      if let Some(name) = name_opt {
         name
      } else {
         truncate_address(address.to_string())
      }
   }

   /// Fixed-size cell with vertically centered content so every column
   /// shares one baseline across framed rows.
   fn row_cell(ui: &mut Ui, width: f32, height: f32, add_contents: impl FnOnce(&mut Ui)) {
      ui.allocate_ui_with_layout(
         vec2(width, height),
         Layout::left_to_right(Align::Center),
         |ui| {
            ui.set_min_size(vec2(width, height));
            ui.set_max_size(vec2(width, height));
            add_contents(ui);
         },
      );
   }

   fn current_cache_key(&self, privacy: bool) -> CacheKey {
      CacheKey {
         wallet: self.selected_wallet.as_ref().map(|w| w.address),
         chain: self.selected_chain.map(|c| c.id()),
         privacy,
      }
   }

   fn rebuild_cache(&mut self, privacy: bool) {
      if !self.db_ready {
         self.cached_txs.clear();
         self.cached_spent.clear();
         return;
      }

      let key = self.current_cache_key(privacy);
      if key == self.cache_key {
         return;
      }

      self.cache_key = key.clone();

      let selected_wallet = self.selected_wallet.clone();
      let selected_chain = self.selected_chain;

      if privacy {
         self.rebuild_spent_cache(key, selected_wallet, selected_chain);
      } else {
         self.rebuild_public_cache(key, selected_wallet, selected_chain);
      }
   }

   fn rebuild_public_cache(
      &mut self,
      key: CacheKey,
      selected_wallet: Option<WalletInfo>,
      selected_chain: Option<ChainId>,
   ) {
      self.loading = true;

      RT.spawn_blocking(move || {
         let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
         let tx_db = ctx.tx_db();
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
            gui.tx_history.loading = false;
            if gui.tx_history.cache_key == key {
               gui.tx_history.cached_txs = txs;
               gui.tx_history.cached_spent.clear();
            }
            gui.request_repaint();
         });
      });
   }

   fn rebuild_spent_cache(
      &mut self,
      key: CacheKey,
      selected_wallet: Option<WalletInfo>,
      selected_chain: Option<ChainId>,
   ) {
      self.loading = true;

      RT.spawn(async move {
         let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
         let wallets = ctx.get_all_wallets_info();
         let mut rows = Vec::new();

         for wallet in wallets {
            if let Some(selected) = &selected_wallet {
               if selected.address != wallet.address {
                  continue;
               }
            }

            let Some(zk) = wallet.railgun_address.clone() else {
               continue;
            };

            let chains_to_check: Vec<ChainId> = if let Some(chain) = selected_chain {
               vec![chain]
            } else {
               ChainId::supported_chains()
            };

            for chain in chains_to_check {
               if ctx.is_chain_disabled(chain.id()) || !ctx.railgun_is_supported(chain) {
                  continue;
               }

               let Ok(provider) = ctx.get_railgun_provider(chain.id(), false).await else {
                  continue;
               };

               let notes = provider.private_history(zk.clone()).await;

               for entry in notes {
                  let action = spent_action(&ctx, chain.id(), &entry);
                  rows.push(SpentHistoryRow {
                     wallet: wallet.address,
                     chain: chain.id(),
                     action,
                     spent_block: entry.spent_block,
                     spent_timestamp: entry.spent_timestamp,
                     entry,
                  });
               }
            }
         }

         rows.sort_unstable_by(|a, b| {
            b.spent_timestamp
               .cmp(&a.spent_timestamp)
               .then_with(|| b.spent_block.cmp(&a.spent_block))
         });

         SHARED_GUI.write(|gui| {
            gui.tx_history.loading = false;
            if gui.tx_history.cache_key == key {
               gui.tx_history.cached_spent = rows;
               gui.tx_history.cached_txs.clear();
            }
            gui.request_repaint();
         });
      });
   }

   pub fn show(&mut self, ctx: &mut ZeusContext, theme: &Theme, ui: &mut Ui) {
      show_with_fade(ui, "tx_history_ui_fade", self.open, |ui| {
         Frame::new().inner_margin(Margin::same(10)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_height(ui.available_height());
            ui.spacing_mut().item_spacing = vec2(10.0, 15.0);
            ui.spacing_mut().button_padding = vec2(10.0, 8.0);

            self.rebuild_cache(ctx.privacy_mode);

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
                     wallet.name_with_id_short()
                  });

               let text = RichText::new(selected_wallet_name).size(theme.typography.normal);
               let label = Label::new(text, None)
                  .visuals(label_visuals)
                  .interactive(true)
                  .fill_width(true)
                  .sense(Sense::click())
                  .expand(expansion);

               ComboBox::new("wallet_filter", label)
                  .visuals(combo_visuals)
                  .width(200.0)
                  .show_ui(ui, |ui| {
                     ui.spacing_mut().item_spacing.y = 10.0;

                     let text = RichText::new("All Wallets").size(theme.typography.normal);
                     let label = Label::new(text, None)
                        .visuals(label_visuals)
                        .interactive(true)
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
                           RichText::new(&wallet.name_with_source()).size(theme.typography.normal);
                        let label = Label::new(text, None)
                           .visuals(label_visuals)
                           .sense(Sense::click())
                           .interactive(true)
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

               let text = RichText::new(selected_chain_name).size(theme.typography.normal);
               let label = Label::new(text, None)
                  .visuals(label_visuals)
                  .interactive(true)
                  .fill_width(true)
                  .sense(Sense::click())
                  .expand(expansion);

               ComboBox::new("chain_filter", label)
                  .visuals(combo_visuals)
                  .width(200.0)
                  .show_ui(ui, |ui| {
                     ui.spacing_mut().item_spacing.y = 10.0;

                     let text = RichText::new("All Chains").size(theme.typography.normal);
                     let label = Label::new(text, None)
                        .visuals(label_visuals)
                        .interactive(true)
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

                        let text = RichText::new(chain.name()).size(theme.typography.normal);
                        let label = Label::new(text, None)
                           .visuals(label_visuals)
                           .sense(Sense::click())
                           .interactive(true)
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
            });

            ui.separator();

            if self.loading {
               ui.vertical_centered(|ui| {
                  ui.add(Spinner::new().size(20.0).color(theme.colors.text));
               });
               return;
            }

            let privacy = ctx.privacy_mode;
            let empty = if privacy {
               self.cached_spent.is_empty()
            } else {
               self.cached_txs.is_empty()
            };
            let tip = if privacy { RAILGUN_TIP } else { ZEUS_TIP };
            let empty_label = if privacy {
               "No spent notes match your filters"
            } else {
               "No transactions match your filters"
            };

            if empty {
               ui.horizontal(|ui| {
                  ui.add_space(400.0);
                  ui.spacing_mut().item_spacing.x = 5.0;

                  ui.label(
                     RichText::new(empty_label)
                        .size(theme.typography.large)
                        .color(theme.colors.text),
                  );

                  let q_mark = RichText::new("?").size(theme.typography.normal);
                  let info_tip = Badge::new(q_mark, BadgeTone::Info);
                  ui.add(info_tip).on_hover_text(tip);
               });
               return;
            }

            let total_txs = if privacy {
               self.cached_spent.len()
            } else {
               self.cached_txs.len()
            };
            let total_pages = (total_txs as f64 / self.txs_per_page as f64).ceil() as usize;
            // Ensure current page is valid
            self.current_page = self.current_page.min(total_pages.saturating_sub(1));

            let button_visuals = theme.button_visuals();

            // Count centered above the list
            ui.horizontal(|ui| {
               // Pagination centered and vertically aligned with the buttons
               ui.horizontal(|ui| {
                  ui.spacing_mut().item_spacing.x = 12.0;
                  ui.spacing_mut().button_padding = vec2(4.0, 6.0);

                  let prev_enabled = self.current_page > 0;
                  let text = RichText::new("Previous").size(theme.typography.small);
                  let prev_button = Button::new(text).visuals(button_visuals);
                  if ui.add_enabled(prev_enabled, prev_button).clicked() {
                     self.current_page -= 1;
                  }

                  ui.label(
                     RichText::new(format!(
                        "Page {} of {}",
                        self.current_page + 1,
                        total_pages.max(1)
                     ))
                     .size(theme.typography.small)
                     .color(theme.colors.text),
                  );

                  let next_enabled = (self.current_page + 1) < total_pages;
                  let text = RichText::new("Next").size(theme.typography.small);
                  let next_button = Button::new(text).visuals(button_visuals);
                  if ui.add_enabled(next_enabled, next_button).clicked() {
                     self.current_page += 1;
                  }
               });

               ui.add_space(200.0);
               ui.spacing_mut().item_spacing.x = 5.0;

               ui.label(
                  RichText::new(if privacy {
                     format!("{} spent notes found", total_txs)
                  } else {
                     format!("{} transactions found", total_txs)
                  })
                  .size(theme.typography.large)
                  .color(theme.colors.text),
               );

               let q_mark = RichText::new("?").size(theme.typography.normal);
               let info_tip = Badge::new(q_mark, BadgeTone::Info);
               ui.add(info_tip).on_hover_text(tip);
            });

            ui.add_space(10.0);

            ScrollArea::vertical()
               .id_salt("tx_history_scroll_area")
               .auto_shrink([false; 2])
               .max_height(ui.available_height() * 0.85)
               .show(ui, |ui| {
                  ui.set_width(ui.available_width());

                  let start = self.current_page * self.txs_per_page;
                  let end = start.saturating_add(self.txs_per_page).min(total_txs);

                  // Fixed content height for every column so icon-less rows
                  // and the Details button share one baseline.
                  let row_height = 40.0;
                  let col_spacing = 20.0;
                  let column_widths = [
                     ui.available_width() * 0.22, // Wallet
                     ui.available_width() * 0.28, // Action
                     ui.available_width() * 0.22, // Age
                     ui.available_width() * 0.16, // Details
                  ];

                  let row_width: f32 = column_widths.iter().sum::<f32>()
                     + col_spacing * (column_widths.len() as f32 - 1.0);

                  // --- Header (same widths as body cells; not inside a frame) ---
                  ui.horizontal(|ui| {
                     ui.add_space((ui.available_width() - row_width).max(0.0) / 2.0);
                     ui.spacing_mut().item_spacing.x = col_spacing;
                     for (i, header) in ["Wallet", "Action", "Age", ""].into_iter().enumerate() {
                        Self::row_cell(ui, column_widths[i], 28.0, |ui| {
                           if !header.is_empty() {
                              ui.label(
                                 RichText::new(header)
                                    .strong()
                                    .size(theme.typography.large)
                                    .color(theme.colors.text),
                              );
                           }
                        });
                     }
                  });

                  ui.add_space(8.0);

                  // --- Body: one frame2 card per row ---
                  let row_frame = theme.frame2.outer_margin(Margin::ZERO);

                  ui.vertical_centered(|ui| {
                     ui.spacing_mut().item_spacing.y = 10.0;

                     if privacy {
                        let rows_on_page = if start < end {
                           &self.cached_spent[start..end]
                        } else {
                           &[]
                        };
                        for row in rows_on_page {
                           ui.allocate_ui(vec2(row_width, row_height + 16.0), |ui| {
                              row_frame.show(ui, |ui| {
                                 ui.set_width(row_width);
                                 ui.spacing_mut().item_spacing.x = col_spacing;

                                 ui.horizontal(|ui| {
                                    Self::row_cell(ui, column_widths[0], row_height, |ui| {
                                       let name = self.wallet_name_or_address(ctx, row.wallet);
                                       ui.label(
                                          RichText::new(name)
                                             .size(theme.typography.normal)
                                             .color(theme.colors.text),
                                       )
                                       .on_hover_text(row.wallet.to_string());
                                    });

                                    Self::row_cell(ui, column_widths[1], row_height, |ui| {
                                       ui.label(
                                          RichText::new(&row.action)
                                             .size(theme.typography.normal)
                                             .color(theme.colors.text),
                                       );
                                    });

                                    Self::row_cell(ui, column_widths[2], row_height, |ui| {
                                       ui.label(
                                          RichText::new(spent_age(ctx, row))
                                             .size(theme.typography.normal)
                                             .color(theme.colors.text),
                                       );
                                    });

                                    Self::row_cell(ui, column_widths[3], row_height, |ui| {
                                       let text =
                                          RichText::new("Details").size(theme.typography.normal);
                                       let details_button =
                                          Button::new(text).visuals(button_visuals);
                                       if ui.add(details_button).clicked() {
                                          let row = row.clone();
                                          RT.spawn_blocking(move || {
                                             SHARED_GUI.write(|gui| {
                                                gui.spent_note_window.open(row);
                                             });
                                          });
                                       }
                                    });
                                 });
                              });
                           });
                        }
                     } else {
                        let txs_on_page = if start < end {
                           &self.cached_txs[start..end]
                        } else {
                           &[]
                        };
                        for tx in txs_on_page {
                           ui.allocate_ui(vec2(row_width, row_height + 16.0), |ui| {
                              row_frame.show(ui, |ui| {
                                 ui.set_width(row_width);
                                 ui.spacing_mut().item_spacing.x = col_spacing;

                                 ui.horizontal(|ui| {
                                    Self::row_cell(ui, column_widths[0], row_height, |ui| {
                                       let name = self.wallet_name_or_address(ctx, tx.sender());
                                       ui.label(
                                          RichText::new(name)
                                             .size(theme.typography.normal)
                                             .color(theme.colors.text),
                                       )
                                       .on_hover_text(tx.sender().to_string());
                                    });

                                    Self::row_cell(ui, column_widths[1], row_height, |ui| {
                                       ui.label(
                                          RichText::new(tx.summary_name())
                                             .size(theme.typography.normal)
                                             .color(theme.colors.text),
                                       );
                                    });

                                    Self::row_cell(ui, column_widths[2], row_height, |ui| {
                                       ui.label(
                                          RichText::new(tx.timestamp.to_relative())
                                             .size(theme.typography.normal)
                                             .color(theme.colors.text),
                                       );
                                    });

                                    Self::row_cell(ui, column_widths[3], row_height, |ui| {
                                       let text =
                                          RichText::new("Details").size(theme.typography.normal);
                                       let details_button =
                                          Button::new(text).visuals(button_visuals);
                                       if ui.add(details_button).clicked() {
                                          let tx_clone = tx.clone();
                                          RT.spawn_blocking(move || {
                                             SHARED_GUI.write(|gui| {
                                                gui.tx_window.open(Some(tx_clone));
                                             });
                                          });
                                       }
                                    });
                                 });
                              });
                           });
                        }
                     }
                  });
               });
         });
      });
   }
}

fn spent_action(
   ctx: &crate::core::context::ZeusCtx,
   chain: u64,
   entry: &PrivateHistoryEntry,
) -> String {
   if entry.kind == PrivateHistoryKind::Merge {
      return "Merged notes".to_string();
   }
   let (symbol, decimals) = match entry.asset.erc20_address() {
      Some(addr) => ctx.read(|c| {
         c.currency_db
            .get_erc20_token(chain, addr)
            .map(|t| (t.symbol.to_string(), t.decimals))
            .unwrap_or_else(|| (truncate_address(addr.to_string()), 18))
      }),
      None => (entry.asset.to_string(), 18),
   };
   let amount = NumericValue::format_wei(U256::from(entry.amount), decimals);
   let sent = format!("Sent {} {}", amount.abbreviated(), symbol);
   if entry.memo.is_empty() {
      sent
   } else {
      format!("{} · {}", sent, entry.memo)
   }
}
