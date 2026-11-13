//! UI that shows the transaction history

use std::collections::HashMap;

use crate::core::{TransactionRich, WalletInfo, ZeusContext};
use crate::gui::{
   SHARED_GUI,
   ui::{
      chain_label, show_with_fade,
      tx::spent_note_window::{SpentHistoryRow, spent_age},
   },
};
use crate::utils::truncate_address;
use egui::{Align, Layout, Margin, Response, RichText, Sense, TextWrapMode, Ui, vec2};
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
      }
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
   }

   fn wallet_name_or_address(&self, ctx: &mut ZeusContext, address: Address) -> String {
      let name_opt = ctx.get_wallet_name(address);
      if let Some(name) = name_opt {
         name
      } else {
         truncate_address(address.to_string())
      }
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

                     if ui.add(label).clicked() && self.selected_wallet != Some(wallet.clone()) {
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
                  ui.add(Spinner::new().size(20.0).color(theme.colors.text));
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
                  ui.spacing_mut().item_spacing.x = theme.spacing.xs;

                  ui.label(
                     RichText::new(empty_label)
                        .size(theme.typography.large)
                        .color(theme.colors.text),
                  );

                  let q_mark = RichText::new("?").size(theme.typography.normal);
                  let info_tip = Chip::new(q_mark).role(ChipRole::Info);
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

               let q_mark = RichText::new("?").size(theme.typography.normal);
               let info_tip = Chip::new(q_mark).role(ChipRole::Info);
               ui.add(info_tip).on_hover_text(tip);
            });

            ui.add_space(10.0);

            let start = self.current_page * self.txs_per_page;
            let end = start.saturating_add(self.txs_per_page).min(total_txs);

            let columns = [
               Column::Fr(2.2),      // Wallet
               Column::Fr(1.6),      // Chain
               Column::Fr(4.0),      // Action
               Column::Auto,         // Age
               Column::Fixed(100.0), // Details
            ];

            // One card per row. Its margins become part of the row pitch, so the cells keep the
            // height the table measured and the header still shares their left edge.
            let row_frame = Frame::new()
               .interactive(true)
               .inner_margin(theme.inner_margin)
               .corner_radius(theme.corner_radius)
               .visuals(theme.frame1_visuals());

            let mut details_tx: Option<TransactionRich> = None;
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
                           let name = self.wallet_name_or_address(ctx, row.wallet);
                           history_cell(ui, theme, &name)
                              .on_hover_text(format!("{}\n{}", name, row.wallet));
                        });

                        r.cell(|ui| {
                           // A spent-note row's chain comes from the Railgun store and is not
                           // re-validated, so it is named rather than converted.
                           let name = chain_label(row.chain);
                           history_cell(ui, theme, &name).on_hover_text(&name);
                        });

                        r.cell(|ui| {
                           history_cell(ui, theme, &row.action).on_hover_text(&row.action);
                        });

                        r.cell(|ui| {
                           let age = spent_age(ctx, row);
                           history_cell(ui, theme, &age).on_hover_text(&age);
                        });

                        r.cell(|ui| {
                           if details_cell(ui, theme, button_visuals) {
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
                           let name = self.wallet_name_or_address(ctx, row.wallet);
                           history_cell(ui, theme, &name)
                              .on_hover_text(format!("{}\n{}", name, row.wallet));
                        });

                        r.cell(|ui| {
                           // A stored transaction's chain is a plain serde field that
                           // `load_all` does not re-validate, so it is named rather than converted.
                           let name = chain_label(row.tx.chain);
                           history_cell(ui, theme, &name).on_hover_text(&name);
                        });

                        r.cell(|ui| {
                           let action = row.tx.summary_name();
                           history_cell(ui, theme, &action).on_hover_text(&action);
                        });

                        r.cell(|ui| {
                           let age = row.tx.timestamp.to_relative();
                           history_cell(ui, theme, &age).on_hover_text(&age);
                        });

                        r.cell(|ui| {
                           if details_cell(ui, theme, button_visuals) {
                              details_tx = Some(row.tx.clone());
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

            if let Some(tx) = details_tx {
               crate::runtime::rt().spawn_blocking(move || {
                  SHARED_GUI.write(|gui| {
                     gui.tx_window.open(Some(tx));
                  });
               });
            }
         });
      });
   }
}

/// One text cell of the history tables: themed, truncating, and the full value on hover.
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

/// The header line both history tables share.
fn history_header(theme: &Theme, header: &mut TableHeader<'_>) {
   for text in ["Wallet", "Chain", "Action", "Age", ""] {
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
fn details_cell(ui: &mut Ui, theme: &Theme, visuals: ButtonVisuals) -> bool {
   let mut clicked = false;
   ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
      let text = RichText::new("Details").size(theme.typography.normal);
      clicked = ui.add(Button::new(text).visuals(visuals)).clicked();
   });
   clicked
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
}
