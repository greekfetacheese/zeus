//! UI that shows the transaction history

use std::collections::HashMap;

use crate::core::{TransactionRich, WalletInfo, ZeusContext};
use crate::gui::{
   SHARED_GUI,
   ui::{
      chain_label,
      common::{
         address::{drawn_and_spoken, name_address},
         name_text, spoken_wallet_name,
      },
      show_with_fade,
      tx::spent_note_window::{SpentHistoryRow, spent_age},
   },
};
use crate::utils::truncate_address;
use egui::{Align, Id, Layout, Margin, Response, RichText, Sense, TextWrapMode, Ui, vec2};
use egui_elements::{
   Button, Chip, ChipRole, ComboBox, Frame, Label, Spinner, Theme,
   layout::{Column, Table, TableHeader, TableScroll},
   visuals::ButtonVisuals,
};
use zeus_eth::{
   alloy_primitives::{Address, B256, U256},
   currency::Currency,
   types::ChainId,
   utils::NumericValue,
};
use zeus_railgun::{PrivateHistoryEntry, PrivateHistoryKind, caip::AssetId};

const DEFAULT_TXS_PER_PAGE: usize = 10;

const ZEUS_TIP: &str = "Zeus only shows transactions that have been been made in-app.\n
It cannot track transactions made from other wallets.";

const RAILGUN_TIP: &str = "Private history is rebuilt from your own Railgun notes:\n
what left your 0zk, what came back as change, and what arrived from another 0zk account.";

pub struct TxHistory {
   open: bool,
   /// Reserved for a future loading state
   loading: bool,
   /// True once the history view is ready to read the tx db
   db_ready: bool,
   pub current_page: usize,
   pub txs_per_page: usize,
   selected_wallet: Option<WalletInfo>,
   selected_chain: Option<ChainId>,
   /// Filtered list for the current filters (avoids cloning every frame)
   cached_txs: Vec<PublicHistoryRow>,
   cached_spent: Vec<SpentHistoryRow>,
   /// Fingerprint of the last cache build so we rebuild only when needed
   cache_key: CacheKey,
   /// The result count this page has already said out loud.
   ///
   /// The count under the filters is the whole answer to a filter change or a refresh, and it can
   /// move without anything else on the page moving. Held so a repaint of the same list does not
   /// say the same number twice.
   spoken_count: Option<usize>,
   /// The words the page hands back to the frame, which owns the announcement channel.
   ///
   /// The page draws from a borrowed theme and cannot announce, and the channel keeps only the last
   /// announcement of a frame, so the sentence waits here until a frame drains it.
   notice: Option<String>,
}

/// Public history row: Zeus wallet that owns the record, plus the stored tx.
///
/// The Wallet column uses [`Self::wallet`] (the `tx_db` key), not
/// [`TransactionRich::sender`] — sponsored unshields set `analysis.sender`
/// to the bundler EOA from the receipt.
#[derive(Clone)]
struct PublicHistoryRow {
   wallet: Address,
   tx: TransactionRich,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
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
         spoken_count: None,
         notice: None,
      }
   }

   /// A page already open on `rows`, with the state that keeps `show` from starting a fetch.
   ///
   /// The tx database a real page reads is not built in a unit test, so the public rows are seeded
   /// directly and every other part of the page — filters, table, count, notice channel — is the
   /// real one.
   #[cfg(test)]
   fn for_test(rows: Vec<PublicHistoryRow>) -> Self {
      let mut page = Self::new();
      page.open = true;
      page.db_ready = true;
      page.cached_txs = rows;

      // The key `rebuild_cache` computes for the unfiltered, public list a test context has.
      let key = page.current_cache_key(false);
      page.cache_key = key;

      page
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   /// Mark the view open. Tx history is loaded with the vault (redb).
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
      // Leaving re-arms the count: coming back is a new arrival, and the number may have moved
      // while the page was away.
      self.spoken_count = None;
   }

   /// The words this page should say, drained by the frame right after it is drawn.
   pub fn take_notice(&mut self) -> Option<String> {
      self.notice.take()
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

      crate::runtime::rt().spawn_blocking(move || {
         let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
         let tx_db = ctx.tx_db();
         let tx_count = tx_db.txs_count();

         let mut txs = Vec::with_capacity(tx_count);
         let wallets = ctx.get_all_wallets_info();

         for wallet in wallets {
            if let Some(selected) = &selected_wallet
               && selected.address != wallet.address
            {
               continue;
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
                  txs.extend(
                     wallet_txs.iter().cloned().map(|tx| PublicHistoryRow {
                        wallet: wallet.address,
                        tx,
                     }),
                  );
               }
            }
         }

         txs.sort_unstable_by_key(|a| std::cmp::Reverse(a.tx.timestamp));

         SHARED_GUI.write(|gui| {
            // A superseded fetch owns nothing here, not even the flag: a newer one is in flight, and
            // clearing `loading` would let "no transactions match your filters" flash before its rows
            // land.
            if gui.tx_history.cache_key != key {
               return;
            }
            gui.tx_history.cached_txs = txs;
            gui.tx_history.cached_spent.clear();
            gui.tx_history.loading = false;
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

      crate::runtime::rt().spawn(async move {
         let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
         let wallets = ctx.get_all_wallets_info();
         let mut rows = Vec::new();

         let chains_to_check: Vec<ChainId> = if let Some(chain) = selected_chain {
            vec![chain]
         } else {
            ChainId::supported_chains()
         };

         for chain in chains_to_check {
            if ctx.is_chain_disabled(chain.id())
               || !ctx.railgun_is_supported(chain)
               || !ctx.is_railgun_enabled(chain.id())
            {
               continue;
            }

            let Ok(provider) = ctx.get_railgun_provider(chain.id(), false).await else {
               continue;
            };

            // Every vault wallet's history on this chain: one provider, in-memory reads. Wallets outside
            // the filter are still read — they are how a receive from our own account gets named.
            let mut per_wallet: Vec<(Address, Vec<PrivateHistoryEntry>)> = Vec::new();
            for wallet in &wallets {
               let Some(zk) = wallet.railgun_address.clone() else {
                  continue;
               };
               let entries = provider.private_history(zk).await;
               if !entries.is_empty() {
                  per_wallet.push((wallet.address, entries));
               }
            }

            // (chain, tx) -> vault wallets that spent in it.
            let mut senders: HashMap<(u64, B256), Vec<Address>> = HashMap::new();
            for (address, entries) in &per_wallet {
               for entry in entries {
                  if entry.kind == PrivateHistoryKind::Receive || entry.tx_hash == B256::ZERO {
                     continue;
                  }
                  let list = senders.entry((chain.id(), entry.tx_hash)).or_default();
                  if !list.contains(address) {
                     list.push(*address);
                  }
               }
            }

            for (address, entries) in per_wallet {
               if let Some(selected) = &selected_wallet
                  && selected.address != address
               {
                  continue;
               }

               for entry in entries {
                  let action = private_action(&ctx, chain.id(), &entry);
                  // A receive from our own vault names that wallet; an outside sender's 0zk is not
                  // recoverable from the note itself, so it stays anonymous.
                  let from = if entry.kind == PrivateHistoryKind::Receive {
                     senders
                        .get(&(chain.id(), entry.tx_hash))
                        .and_then(|senders| senders.iter().copied().find(|a| *a != address))
                  } else {
                     None
                  };

                  rows.push(SpentHistoryRow {
                     wallet: address,
                     chain: chain.id(),
                     action,
                     from,
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

   /// Say how many rows the page is showing when that number changes, and only then.
   ///
   /// A filter change or a refresh replaces the whole list, and the count under the filters is the
   /// one thing on the page that says how much of it there is: left silent, a reader has no way of
   /// knowing the number moved. Callers reach this only once a build has landed — until then the
   /// list is the previous page's, so its length is not this page's count.
   fn announce_count(&mut self, total: usize, privacy: bool) {
      if self.spoken_count == Some(total) {
         return;
      }

      self.spoken_count = Some(total);

      // An empty list draws its own sentence; a number said over it would describe a label that is
      // not on the page. The record still moves, so the next count is a change against zero.
      if total == 0 {
         return;
      }

      self.notice = Some(if privacy {
         format!("{total} private history entries.")
      } else {
         format!("{total} transactions found.")
      });
   }

   pub fn show(&mut self, ctx: &mut ZeusContext, theme: &Theme, ui: &mut Ui) {
      show_with_fade(ui, "tx_history_ui_fade", self.open, |ui| {
         Frame::new().inner_margin(Margin::same(10)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_height(ui.available_height());
            ui.spacing_mut().item_spacing = vec2(theme.spacing.sm, theme.spacing.md);
            ui.spacing_mut().button_padding = theme.button_padding;

            self.rebuild_cache(ctx.privacy_mode);

            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
               ui.spacing_mut().item_spacing.x = theme.spacing.xl;
               ui.spacing_mut().button_padding = theme.button_padding;

               let expansion = Some(6.0);

               // Wallet Filter
               let wallets = ctx.all_wallets_info_ordered();
               let selected_wallet_name =
                  self.selected_wallet.clone().map_or("All Wallets".to_string(), |wallet| {
                     wallet.name_with_id_short()
                  });

               let text = RichText::new(selected_wallet_name).size(theme.typography.normal);
               let label = Label::new(text, None)
                  .interactive(true)
                  .fill_width(true)
                  .sense(Sense::click())
                  .expand(expansion);

               ComboBox::new("wallet_filter", label).width(200.0).show_ui(ui, |ui| {
                  ui.spacing_mut().item_spacing.y = theme.spacing.sm;

                  let text = RichText::new("All Wallets").size(theme.typography.normal);
                  let label = Label::new(text, None)
                     .interactive(true)
                     .fill_width(true)
                     .sense(Sense::click())
                     .expand(expansion);

                  if ui.add(label).clicked() && self.selected_wallet.is_some() {
                     self.selected_wallet = None;
                     self.current_page = 0;
                  }

                  for wallet in wallets {
                     let text =
                        RichText::new(wallet.name_with_source()).size(theme.typography.normal);
                     let label = Label::new(text, None)
                        .sense(Sense::click())
                        .interactive(true)
                        .fill_width(true)
                        .expand(expansion);

                     let response = ui.add(label);

                     name_text(ui, &response, spoken_wallet_name(wallet));

                     if response.clicked() && self.selected_wallet != Some(wallet.clone()) {
                        self.selected_wallet = Some(wallet.clone());
                        self.current_page = 0;
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
                  .interactive(true)
                  .fill_width(true)
                  .sense(Sense::click())
                  .expand(expansion);

               ComboBox::new("chain_filter", label).width(200.0).show_ui(ui, |ui| {
                  ui.spacing_mut().item_spacing.y = theme.spacing.sm;

                  let text = RichText::new("All Chains").size(theme.typography.normal);
                  let label = Label::new(text, None)
                     .interactive(true)
                     .fill_width(true)
                     .sense(Sense::click())
                     .expand(expansion);

                  if ui.add(label).clicked() && self.selected_chain.is_some() {
                     self.selected_chain = None;
                     self.current_page = 0;
                  }

                  for chain in ChainId::supported_chains() {
                     if ctx.is_chain_disabled(chain.id()) {
                        continue;
                     }

                     let text = RichText::new(chain.name()).size(theme.typography.normal);
                     let label = Label::new(text, None)
                        .sense(Sense::click())
                        .interactive(true)
                        .fill_width(true)
                        .expand(expansion);

                     if ui.add(label).clicked() && self.selected_chain != Some(chain) {
                        self.selected_chain = Some(chain);
                        self.current_page = 0;
                     }
                  }
               });
            });

            ui.separator();

            if self.loading {
               ui.vertical_centered(|ui| {
                  ui.horizontal(|ui| {
                     ui.add(Spinner::new().size(20.0));
                     // A bare spinner says nothing about what the wait is for.
                     ui.label(
                        RichText::new("Loading transaction history")
                           .size(theme.typography.normal)
                           .color(theme.colors.text),
                     );
                  });
               });
               return;
            }

            let privacy = ctx.privacy_mode;

            if privacy && !ctx.is_railgun_enabled(ctx.chain.id()) {
               ui.vertical_centered(|ui| {
                  let text = RichText::new("Railgun is disabled")
                     .size(theme.typography.large)
                     .color(theme.colors.warning);
                  ui.label(text);
                  ui.label(
                     RichText::new("Enable it in Settings/Railgun to see private transactions.")
                        .size(theme.typography.normal),
                  );
               });
               return;
            }

            let total_txs = if privacy {
               self.cached_spent.len()
            } else {
               self.cached_txs.len()
            };
            let empty = total_txs == 0;

            // Said before the empty state returns: an empty result moves the record too.
            self.announce_count(total_txs, privacy);

            let tip = if privacy { RAILGUN_TIP } else { ZEUS_TIP };
            let empty_label = if privacy {
               "No spent notes match your filters"
            } else {
               "No transactions match your filters"
            };

            if empty {
               ui.horizontal(|ui| {
                  ui.add_space(400.0);
                  ui.spacing_mut().item_spacing.x = theme.spacing.xs;

                  ui.label(
                     RichText::new(empty_label)
                        .size(theme.typography.large)
                        .color(theme.colors.text),
                  );

                  history_tip(ui, theme, tip);
               });
               return;
            }

            let total_pages = (total_txs as f64 / self.txs_per_page as f64).ceil() as usize;
            // Ensure current page is valid
            self.current_page = self.current_page.min(total_pages.saturating_sub(1));

            let button_visuals = theme.button_visuals();

            // Count centered above the list
            ui.horizontal(|ui| {
               // Pagination centered and vertically aligned with the buttons
               ui.horizontal(|ui| {
                  ui.spacing_mut().item_spacing.x = theme.spacing.md;
                  ui.spacing_mut().button_padding = vec2(theme.spacing.xs, theme.spacing.sm);

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
               ui.spacing_mut().item_spacing.x = theme.spacing.xs;

               ui.label(
                  RichText::new(if privacy {
                     format!("{} private history entries", total_txs)
                  } else {
                     format!("{} transactions found", total_txs)
                  })
                  .size(theme.typography.large)
                  .color(theme.colors.text),
               );

               history_tip(ui, theme, tip);
            });

            ui.add_space(10.0);

            let start = self.current_page * self.txs_per_page;
            let end = start.saturating_add(self.txs_per_page).min(total_txs);

            let columns = [
               Column::Fr(2.2),      // Wallet
               Column::Fr(1.6),      // Chain
               Column::Fr(4.0),      // Action
               Column::Auto,         // Age
               Column::Auto,         // Status
               Column::Fixed(100.0), // Details
            ];

            // One card per row. Its margins become part of the row pitch, so the cells keep the
            // height the table measured and the header still shares their left edge.
            let row_frame = Frame::new()
               .interactive(true)
               .inner_margin(theme.inner_margin)
               .corner_radius(theme.corner_radius)
               .visuals(theme.frame1_visuals());

            let mut details_tx: Option<(TransactionRich, Id)> = None;
            let mut details_spent: Option<SpentHistoryRow> = None;

            if privacy {
               let rows: &[SpentHistoryRow] = if start < end {
                  &self.cached_spent[start..end]
               } else {
                  &[]
               };

               Table::new("tx_history_spent", rows)
                  .columns(columns)
                  .gap(20.0)
                  // 12 pt, not xs: a themed `Button` paints ~6 pt above and below its band.
                  .row_gap(theme.spacing.md)
                  .scroll(
                     TableScroll::new()
                        .auto_shrink([false; 2])
                        .content_margin(15)
                        .max_height(ui.available_height() * 0.85),
                  )
                  .row_frame(row_frame)
                  .header_rule(true)
                  .key((&self.cache_key, self.current_page))
                  .show(ui, |t| {
                     t.header(|h| history_header(theme, h));

                     t.rows(|r, row| {
                        r.cell(|ui| {
                           let name = ctx.get_wallet_name(row.wallet);
                           let (drawn, spoken) =
                              drawn_and_spoken(name.as_deref(), &row.wallet.to_string());
                           // `name_address` names the address and keeps the address's second form
                           // in the description; the old hover only repeated those two things.
                           let response = history_cell(ui, theme, &drawn);
                           name_address(ui, &response, &spoken);
                        });

                        r.cell(|ui| {
                           // A spent-note row's chain comes from the Railgun store and is not
                           // re-validated, so it is named rather than converted.
                           let name = chain_label(row.chain);
                           let response = history_cell(ui, theme, &name).on_hover_text(&name);
                           // The drawn name can be truncated; the full one stays on the node.
                           describe(ui, &response, name);
                        });

                        r.cell(|ui| {
                           let response =
                              history_cell(ui, theme, &row.action).on_hover_text(&row.action);
                           // The drawn action can be truncated; the full one stays on the node.
                           describe(ui, &response, row.action.clone());
                        });

                        r.cell(|ui| {
                           let age = spent_age(ctx, row);
                           let response = history_cell(ui, theme, &age).on_hover_text(&age);
                           // The drawn age can be truncated; the full one stays on the node.
                           describe(ui, &response, age);
                        });

                        r.cell(|ui| {
                           // The private list holds spent notes: the spend is in a block, so
                           // every row it lists is confirmed.
                           let status = status_text(true);
                           let response = history_cell(ui, theme, status).on_hover_text(status);
                           // The drawn status can be truncated; the full one stays on the node.
                           describe(ui, &response, status);
                        });

                        r.cell(|ui| {
                           if details_cell(ui, theme, button_visuals, &row.action).clicked() {
                              details_spent = Some(row.clone());
                           }
                        });
                     });
                  });
            } else {
               let rows: &[PublicHistoryRow] = if start < end {
                  &self.cached_txs[start..end]
               } else {
                  &[]
               };

               Table::new("tx_history_public", rows)
                  .columns(columns)
                  .gap(20.0)
                  .row_gap(theme.spacing.md)
                  .scroll(
                     TableScroll::new()
                        .auto_shrink([false; 2])
                        .content_margin(15)
                        .max_height(ui.available_height() * 0.85),
                  )
                  .row_frame(row_frame)
                  .header_rule(true)
                  .key((&self.cache_key, self.current_page))
                  .show(ui, |t| {
                     t.header(|h| history_header(theme, h));

                     t.rows(|r, row| {
                        r.cell(|ui| {
                           let name = ctx.get_wallet_name(row.wallet);
                           let (drawn, spoken) =
                              drawn_and_spoken(name.as_deref(), &row.wallet.to_string());
                           // `name_address` names the address and keeps the address's second form
                           // in the description; the old hover only repeated those two things.
                           let response = history_cell(ui, theme, &drawn);
                           name_address(ui, &response, &spoken);
                        });

                        r.cell(|ui| {
                           // A stored transaction's chain is a plain serde field that
                           // `load_all` does not re-validate, so it is named rather than converted.
                           let name = chain_label(row.tx.chain);
                           let response = history_cell(ui, theme, &name).on_hover_text(&name);
                           // The drawn name can be truncated; the full one stays on the node.
                           describe(ui, &response, name);
                        });

                        r.cell(|ui| {
                           let action = row.tx.summary_name();
                           let response = history_cell(ui, theme, &action).on_hover_text(&action);
                           // The drawn action can be truncated; the full one stays on the node.
                           describe(ui, &response, action);
                        });

                        r.cell(|ui| {
                           let age = row.tx.timestamp.to_relative();
                           let response = history_cell(ui, theme, &age).on_hover_text(&age);
                           // The drawn age can be truncated; the full one stays on the node.
                           describe(ui, &response, age);
                        });

                        r.cell(|ui| {
                           let status = status_text(row.tx.success);
                           let response = history_cell(ui, theme, status).on_hover_text(status);
                           // The drawn status can be truncated; the full one stays on the node.
                           describe(ui, &response, status);
                        });

                        r.cell(|ui| {
                           let details =
                              details_cell(ui, theme, button_visuals, &row.tx.summary_name());

                           if details.clicked() {
                              details_tx = Some((row.tx.clone(), details.id));
                           }
                        });
                     });
                  });
            }

            if let Some(row) = details_spent {
               crate::runtime::rt().spawn_blocking(move || {
                  SHARED_GUI.write(|gui| {
                     gui.spent_note_window.open(row);
                  });
               });
            }

            if let Some((tx, opener)) = details_tx {
               crate::runtime::rt().spawn_blocking(move || {
                  SHARED_GUI.write(|gui| {
                     gui.tx_window.open(Some(tx), Some(opener));
                  });
               });
            }
         });
      });
   }
}

/// One text cell of the history tables: themed and truncating.
///
/// Truncation is a paint-time cut, so the drawn text is only shorter than the value on screen: the
/// callers put that value on the cell's own node through [`describe`], which is the copy a reader
/// gets, and keep a hover for the pointer.
fn history_cell(ui: &mut Ui, theme: &Theme, text: &str) -> Response {
   ui.add(
      Label::new(
         RichText::new(text).size(theme.typography.normal).color(theme.colors.text),
         None,
      )
      .wrap_mode(TextWrapMode::Truncate)
      .visuals(theme.label_visuals()),
   )
}

/// Put a sentence a drawn cell only shows on hover on the node the label already made.
///
/// A tooltip is a pointer-only surface: a reader never asks for one, so the full value a truncated
/// cell carries has to be part of the cell's own node.
fn describe(ui: &Ui, response: &Response, text: impl Into<String>) {
   let text = text.into();
   ui.ctx().accesskit_node_builder(response.id, move |node| {
      node.set_description(text);
   });
}

/// The `?` that explains where the history comes from.
///
/// The chip keeps the drawn question mark it is, but its node carries the name and the sentence:
/// the tip is the whole point of the chip, and a hover is not reachable without a pointer.
fn history_tip(ui: &mut Ui, theme: &Theme, tip: &str) -> Response {
   let q_mark = RichText::new("?").size(theme.typography.normal);
   let info_tip = Chip::new(q_mark).role(ChipRole::Info);
   let response = ui.add(info_tip).on_hover_text(tip);

   name_text(ui, &response, "About transaction history");
   describe(ui, &response, tip);

   response
}

/// The word a row uses for the transaction's own outcome.
fn status_text(success: bool) -> &'static str {
   match success {
      true => "Confirmed",
      false => "Failed",
   }
}

/// The header line both history tables share.
fn history_header(theme: &Theme, header: &mut TableHeader<'_>) {
   for text in ["Wallet", "Chain", "Action", "Age", "Status", ""] {
      header.cell(|ui| {
         if !text.is_empty() {
            ui.label(
               RichText::new(text)
                  .strong()
                  .size(theme.typography.large)
                  .color(theme.colors.text),
            );
         }
      });
   }
}

/// The Details button both history tables carry, hugging the right edge of its column.
///
/// Every row draws the same word, so the node says which row this one opens: a reader that lists
/// the table otherwise hears a column of identical "Details" buttons. The response is handed back
/// so the button's id can be remembered as the opener the details window gives the focus to.
fn details_cell(ui: &mut Ui, theme: &Theme, visuals: ButtonVisuals, action: &str) -> Response {
   ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
      let text = RichText::new("Details").size(theme.typography.normal);
      let button = Button::new(text).visuals(visuals);
      let name = format!("Details for {action}");
      ui.add(button).accessible_name(name)
   })
   .inner
}

/// How a private-history amount reads: the value, the name it goes by, and the token when Zeus prices
/// it (so a caller can add a USD value).
pub(crate) struct PrivateAssetDisplay {
   symbol: String,
   currency: Option<Currency>,
   /// `None` for an NFT, whose note amount is a **count** and must never be wei-scaled.
   decimals: Option<u8>,
}

impl PrivateAssetDisplay {
   pub(crate) fn symbol(&self) -> &str {
      &self.symbol
   }

   pub(crate) fn currency(&self) -> Option<&Currency> {
      self.currency.as_ref()
   }

   /// The amount scaled to the asset's own decimals — `None` for an NFT, whose amount is a count.
   pub(crate) fn value(&self, amount: u128) -> Option<NumericValue> {
      self
         .decimals
         .map(|decimals| NumericValue::format_wei(U256::from(amount), decimals))
   }

   /// The note amount as the private history prints it: a scaled value, or the raw count for an NFT.
   ///
   /// `precision` is a *character* precision — the callers format [`NumericValue::abbreviated`], so it
   /// caps the string, not a decimal count.
   pub(crate) fn amount_text(&self, amount: u128, precision: usize) -> String {
      match self.decimals {
         Some(_) => {
            let value = self.value(amount).expect("fungible");
            format!("{:.precision$}", value.abbreviated())
         }
         None => amount.to_string(),
      }
   }
}

/// Name and scale an asset the way the private history shows it.
///
/// NFTs are the trap this exists for: they are not in `CurrencyDB`, so the old fallback put
/// [`AssetId`]'s own `Display` next to an 18-decimal value, and every spent NFT note read
/// `Sent 0 erc721:0xaf5aa7b670ef209e23d3f7b39a8f42f84bd002ac/1071` — the note's raw asset, and an
/// amount formatted as if it were wei. An NFT is named by its **collection** (`get_address_name`
/// resolves it from `NftDB`) and its amount is a count.
pub(crate) fn private_asset_display(
   ctx: &ZeusContext,
   chain: u64,
   asset: AssetId,
) -> PrivateAssetDisplay {
   let collection = |address: Address| {
      ctx.get_address_name(chain, address).map_or_else(
         || truncate_address(address.to_string()),
         |name| name.to_string(),
      )
   };

   match asset {
      AssetId::Erc721(address, id) => PrivateAssetDisplay {
         symbol: format!("{} #{id}", collection(address)),
         currency: None,
         decimals: None,
      },
      AssetId::Erc1155(address, id) => PrivateAssetDisplay {
         symbol: format!("{} #{id}", collection(address)),
         currency: None,
         decimals: None,
      },
      AssetId::Erc20(address) => {
         let token = ctx.currency_db.get_erc20_token(chain, address);
         let decimals = token.as_ref().map_or(18, |token| token.decimals);
         let symbol = token.as_ref().map_or_else(
            || truncate_address(address.to_string()),
            |token| token.symbol.to_string(),
         );
         let currency = token.map(Currency::from);

         PrivateAssetDisplay {
            symbol,
            currency,
            decimals: Some(decimals),
         }
      }
   }
}

fn private_action(
   ctx: &crate::core::context::ZeusCtx,
   chain: u64,
   entry: &PrivateHistoryEntry,
) -> String {
   if entry.kind == PrivateHistoryKind::Merge {
      return "Merged notes".to_string();
   }

   let display = ctx.read(|c| private_asset_display(c, chain, entry.asset));
   let amount = display.amount_text(entry.amount, 5);
   let symbol = display.symbol();

   match entry.kind {
      PrivateHistoryKind::Unshield => format!("Unshield {amount} {symbol}"),
      PrivateHistoryKind::Send => format!("Transfer {amount} {symbol}"),
      PrivateHistoryKind::Spend => format!("Sent {amount} {symbol}"),
      PrivateHistoryKind::Receive => format!("Received {amount} {symbol}"),
      PrivateHistoryKind::Merge => "Merged notes".to_string(),
      _ => "Unknown".to_string(),
   }
}

#[cfg(test)]
mod tests {
   use super::*;
   use zeus_eth::alloy_primitives::address;

   /// An NFT note is *named*, not identified: `Sent 1 <collection> #1071`, never the note's raw asset.
   ///
   /// Needs this machine's `data/` — the collection name comes from `NftDB` — so it is ignored by
   /// default. Run it alone: `cargo test -p zeus -- --ignored an_nft_note_is_named`.
   #[test]
   #[ignore = "needs the machine's data/ for the NFT collection name"]
   fn an_nft_note_is_named() {
      let ctx = crate::tests::unlock_ctx();
      let entry = PrivateHistoryEntry {
         asset: AssetId::Erc721(
            address!("0xaf5aa7b670ef209e23d3f7b39a8f42f84bd002ac"),
            U256::from(1071),
         ),
         amount: 1,
         change_amount: 0,
         input_count: 1,
         spent_block: 11837089,
         spent_timestamp: 0,
         tx_hash: Default::default(),
         memo: String::new(),
         kind: PrivateHistoryKind::Spend,
      };

      let action = private_action(&ctx, 11155111, &entry);
      println!("action = {action}");

      assert!(action.starts_with("Sent 1 "), "{action}");
      assert!(action.ends_with("#1071"), "{action}");
      assert!(!action.contains("erc721"), "{action}");
   }

   /// The status cell says the transaction's outcome in words, which is all a reader gets: the
   /// table draws no colour for a failed one.
   #[test]
   fn the_status_word_says_whether_the_transaction_succeeded() {
      assert_eq!(status_text(true), "Confirmed");
      assert_eq!(status_text(false), "Failed");
   }

   /// The page's accessible tree: what a reader is handed for a page holding known rows.
   ///
   /// `PublicHistoryRow` and `TxHistory` are private to this file, so the fixtures live here rather
   /// than widening the row type for the shared accessibility suite. The frame below reads the same
   /// `platform_output.accesskit_update` value that suite reads.
   mod accessibility {
      use super::*;
      use crate::core::ZeusCtx;
      use egui::accesskit::{Node, NodeId, Role};
      use egui::{Context, RawInput, Rect, pos2, vec2};
      use egui_elements::ThemeKind;
      use zeus_eth::alloy_primitives::address;

      const WALLET: Address = address!("1111111111111111111111111111111111111111");

      /// Run one AccessKit-enabled frame and return the node ids and nodes it produced.
      fn frame_nodes(ctx: &Context, add: impl FnMut(&mut Ui)) -> Vec<(NodeId, Node)> {
         ctx.enable_accesskit();

         let mut output = ctx.run_ui(
            RawInput {
               screen_rect: Some(Rect::from_min_size(
                  pos2(0.0, 0.0),
                  vec2(1290.0, 950.0),
               )),
               ..Default::default()
            },
            add,
         );

         let update = output
            .platform_output
            .accesskit_update
            .take()
            .expect("an AccessKit-enabled frame must produce a tree update");

         let nodes = update.nodes.into_iter().collect();
         output.drop_without_applying_deltas();

         nodes
      }

      /// The pieces the page needs: the installed theme and a context with no wallet behind it.
      fn page_harness() -> (Context, ZeusCtx, Theme) {
         let ctx = Context::default();
         let mut theme = Theme::new(ThemeKind::TokyoNight);
         theme.install(&ctx);

         let handle = ZeusCtx::new(crate::runtime::test_handle());

         (ctx, handle, theme)
      }

      /// Draw the page for four frames and return the tree of the last one: the content fades in
      /// and the table lays its rows out after the first pass. Every sentence the page hands back
      /// is collected in `said`.
      fn render_page(
         page: &mut TxHistory,
         ctx: &Context,
         handle: &ZeusCtx,
         theme: &Theme,
         said: &mut Vec<String>,
      ) -> Vec<(NodeId, Node)> {
         let mut nodes = Vec::new();

         for _ in 0..4 {
            nodes = frame_nodes(ctx, |ui| {
               handle.write(|zeus| {
                  page.show(zeus, theme, ui);

                  if let Some(text) = page.take_notice() {
                     said.push(text);
                  }
               });
            });
         }

         nodes
      }

      /// Whether a node is named by the given word, either way egui spells a label for AccessKit.
      fn is_named(node: &Node, name: &str) -> bool {
         node.label() == Some(name) || node.value() == Some(name)
      }

      /// Every name and description in the tree, for a failure message.
      fn spoken(nodes: &[(NodeId, Node)]) -> Vec<String> {
         nodes
            .iter()
            .flat_map(|(_, node)| [node.label(), node.value(), node.description()])
            .flatten()
            .map(str::to_string)
            .collect()
      }

      /// The label node that carries `text` as its description.
      fn label_with_description<'a>(nodes: &'a [(NodeId, Node)], text: &str) -> &'a Node {
         nodes
            .iter()
            .filter(|(_, node)| node.role() == Role::Label)
            .map(|(_, node)| node)
            .find(|node| node.description() == Some(text))
            .unwrap_or_else(|| {
               panic!(
                  "no label carries {text:?} as its description: {:?}",
                  spoken(nodes)
               )
            })
      }

      /// One public history row: the clear-signed fixture, which this page calls "Supply".
      fn supply_row() -> PublicHistoryRow {
         PublicHistoryRow {
            wallet: WALLET,
            tx: TransactionRich::dummy_clear_signed(),
         }
      }

      /// The same stored transaction without its ERC-7730 view, so the page has no heading and
      /// falls back to the decoded event's own name: a second action without a second fixture.
      fn unknown_row() -> PublicHistoryRow {
         let mut tx = TransactionRich::dummy_clear_signed();
         tx.clear_display = None;

         PublicHistoryRow { wallet: WALLET, tx }
      }

      /// Every row draws the same word, so the button's node must say which row it opens: a reader
      /// that lists the table otherwise hears a column of identical "Details" buttons.
      #[test]
      fn each_details_button_names_the_row_it_opens() {
         let (ctx, handle, theme) = page_harness();
         let rows = vec![supply_row(), unknown_row()];
         let actions: Vec<String> = rows.iter().map(|row| row.tx.summary_name()).collect();
         assert_ne!(
            actions[0], actions[1],
            "the fixture must exercise two different actions"
         );

         let mut page = TxHistory::for_test(rows);
         let nodes = render_page(&mut page, &ctx, &handle, &theme, &mut Vec::new());

         let names: Vec<&str> = nodes
            .iter()
            .filter(|(_, node)| node.role() == Role::Button)
            .filter_map(|(_, node)| node.label())
            .collect();

         for action in &actions {
            let expected = format!("Details for {action}");
            assert!(
               names.contains(&expected.as_str()),
               "the row's button must name its action: {names:?}"
            );
         }

         assert!(
            !names.contains(&"Details"),
            "no button may be left with the bare word: {names:?}"
         );
      }

      /// The cells truncate, and each caller used to keep the full value only in a hover: a reader
      /// gets that value on the cell's own node, with the drawn text untouched.
      #[test]
      fn the_cells_say_what_their_hovers_carried() {
         let (ctx, handle, theme) = page_harness();
         let row = supply_row();
         let chain = chain_label(row.tx.chain);
         let action = row.tx.summary_name();
         let age = row.tx.timestamp.to_relative();
         let (_, spoken_wallet) = drawn_and_spoken(None, &row.wallet.to_string());
         let wallet_description = spoken_wallet.description.clone().expect("a spoken address");

         let mut page = TxHistory::for_test(vec![row]);
         let nodes = render_page(&mut page, &ctx, &handle, &theme, &mut Vec::new());

         for text in [chain.as_str(), action.as_str(), age.as_str()] {
            label_with_description(&nodes, text);
            assert!(
               nodes.iter().any(|(_, node)| is_named(node, text)),
               "the cell must keep drawing {text:?}: {:?}",
               spoken(&nodes)
            );
         }

         // The wallet cell's hover only repeated what `name_address` already put on the node.
         let wallet_node = label_with_description(&nodes, &wallet_description);
         assert_eq!(
            wallet_node.label(),
            Some(spoken_wallet.name.as_str()),
            "the wallet cell keeps its spoken name: {:?}",
            spoken(&nodes)
         );
      }

      /// A row's outcome is a column of its own: a reader has only the words, so the cell says the
      /// status rather than leaving it to a colour.
      #[test]
      fn the_history_page_says_each_rows_status() {
         let (ctx, handle, theme) = page_harness();

         let mut failed = supply_row();
         failed.tx.success = false;

         let mut page = TxHistory::for_test(vec![supply_row(), failed]);
         let nodes = render_page(&mut page, &ctx, &handle, &theme, &mut Vec::new());

         for status in ["Confirmed", "Failed"] {
            assert!(
               nodes.iter().any(|(_, node)| node.role() == Role::Label
                  && is_named(node, status)
                  && node.description() == Some(status)),
               "the status cell must draw and describe {status:?}: {:?}",
               spoken(&nodes)
            );
         }
      }

      /// The `?` chip is the only explanation of where the list comes from, and a hover is not
      /// reachable without a pointer: the tip is the chip's description, next to its name.
      #[test]
      fn the_help_chip_is_named_and_carries_its_tip() {
         let (ctx, handle, theme) = page_harness();

         let mut empty = TxHistory::for_test(Vec::new());
         let nodes = render_page(&mut empty, &ctx, &handle, &theme, &mut Vec::new());
         let tip = nodes
            .iter()
            .find(|(_, node)| is_named(node, "About transaction history"))
            .map(|(_, node)| node)
            .unwrap_or_else(|| {
               panic!(
                  "the empty state's ? chip must be named: {:?}",
                  spoken(&nodes)
               )
            });

         assert_eq!(
            tip.description(),
            Some(ZEUS_TIP),
            "the tip must be the chip's description, not only a hover"
         );

         let mut page = TxHistory::for_test(vec![supply_row()]);
         let nodes = render_page(&mut page, &ctx, &handle, &theme, &mut Vec::new());

         assert!(
            nodes.iter().any(
               |(_, node)| is_named(node, "About transaction history")
                  && node.description() == Some(ZEUS_TIP)
            ),
            "the count row's chip carries the same sentence: {:?}",
            spoken(&nodes)
         );
      }

      /// A bare spinner says nothing about what is being waited for, and the previous list's count
      /// must not be read out while a build is in flight.
      #[test]
      fn the_loading_page_says_loading_transaction_history() {
         let (ctx, handle, theme) = page_harness();
         let mut page = TxHistory::for_test(vec![supply_row()]);
         page.loading = true;

         let mut said = Vec::new();
         let nodes = render_page(&mut page, &ctx, &handle, &theme, &mut said);

         assert!(
            nodes.iter().any(|(_, node)| is_named(node, "Loading transaction history")),
            "the loading page must say what it is waiting for: {:?}",
            spoken(&nodes)
         );
         assert!(
            said.is_empty(),
            "a count must not be said while the rows are still loading: {said:?}"
         );
      }

      /// A filter change or a refresh replaces the count without anything else on the page moving.
      /// The page says the number once, and a repaint of the same list says nothing.
      #[test]
      fn the_result_count_is_said_once_per_change() {
         let (ctx, handle, theme) = page_harness();
         let mut page = TxHistory::for_test(vec![supply_row(), unknown_row()]);
         let mut said = Vec::new();

         render_page(&mut page, &ctx, &handle, &theme, &mut said);
         assert_eq!(
            said.as_slice(),
            ["2 transactions found."],
            "the number is said once, after the rows have landed"
         );

         render_page(&mut page, &ctx, &handle, &theme, &mut said);
         assert_eq!(
            said.as_slice(),
            ["2 transactions found."],
            "a repaint of the same list must not repeat the number"
         );

         page.cached_txs.push(supply_row());
         render_page(&mut page, &ctx, &handle, &theme, &mut said);
         assert_eq!(
            said.as_slice(),
            ["2 transactions found.", "3 transactions found."],
            "a changed count is the one thing said"
         );
      }

      /// The private page counts entries rather than transactions, says it once, and leaves the
      /// empty state's own sentence to speak for zero.
      #[test]
      fn a_private_count_is_named_as_entries() {
         let mut page = TxHistory::for_test(Vec::new());
         page.announce_count(12, true);

         assert_eq!(
            page.take_notice().as_deref(),
            Some("12 private history entries.")
         );
         assert_eq!(page.take_notice(), None, "the frame says it once");

         page.announce_count(12, true);
         assert_eq!(
            page.take_notice(),
            None,
            "the same number is not said twice, even on the private page"
         );

         let mut empty = TxHistory::for_test(Vec::new());
         empty.announce_count(0, false);
         assert_eq!(
            empty.take_notice(),
            None,
            "an empty list draws its own sentence instead of a count"
         );
      }
   }
}
