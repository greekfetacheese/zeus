use crate::core::{
   WalletInfo,
   TransactionRich, ZeusCtx,
   utils::{RT, truncate_address},
};
use crate::gui::SHARED_GUI;
use egui::{Align, Button, ComboBox, Frame, Grid, Layout, Margin, RichText, ScrollArea, Ui, vec2};
use egui_theme::Theme;
use zeus_eth::{alloy_primitives::Address, types::ChainId};

use std::time::{Duration, SystemTime, UNIX_EPOCH};

const DEFAULT_TXS_PER_PAGE: usize = 20;

pub struct TxHistory {
   pub open: bool,
   pub current_page: usize,
   pub txs_per_page: usize,
   selected_wallet: Option<WalletInfo>,
   selected_chain: Option<ChainId>,
}

impl TxHistory {
   pub fn new() -> Self {
      Self {
         open: false,
         current_page: 0,
         txs_per_page: DEFAULT_TXS_PER_PAGE,
         selected_wallet: None,
         selected_chain: None,
      }
   }

   fn wallet_name_or_address(&self, ctx: ZeusCtx, address: Address) -> String {
      let wallet_info = ctx.get_wallet_info_by_address(address);
      if let Some(info) = wallet_info {
         info.name()
      } else {
         truncate_address(address.to_string())
      }
   }

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      if !self.open {
         return;
      }

      Frame::new().inner_margin(Margin::same(10)).show(ui, |ui| {
         ui.set_width(ui.available_width());
         ui.set_height(ui.available_height());
         ui.spacing_mut().item_spacing = vec2(10.0, 15.0);
         ui.spacing_mut().button_padding = vec2(10.0, 8.0);

         ui.vertical_centered_justified(|ui| {
            ui.label(
               RichText::new("Transaction History")
                  .size(theme.text_sizes.heading)
                  .color(theme.colors.text_color),
            );
         });

         ui.add_space(10.0);

         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.spacing_mut().item_spacing.x = 20.0;
            ui.spacing_mut().button_padding = vec2(10.0, 8.0);

            // Wallet Filter
            let wallets = ctx.get_all_wallets_info();
            let selected_wallet_name =
               self.selected_wallet.clone().map_or("All Wallets".to_string(), |wallet| {
                  wallet.name()
               });

            ComboBox::from_id_salt("wallet_filter")
               .width(200.0)
               .selected_text(RichText::new(selected_wallet_name).size(theme.text_sizes.small))
               .show_ui(ui, |ui| {
                  if ui
                     .selectable_label(
                        self.selected_wallet.is_none(),
                        RichText::new("All Wallets").size(theme.text_sizes.small),
                     )
                     .clicked()
                  {
                     if self.selected_wallet.is_some() {
                        self.selected_wallet = None;
                        self.current_page = 0;
                     }
                  }

                  for wallet in wallets {
                     if ui
                        .selectable_label(
                           self.selected_wallet == Some(wallet.clone()),
                           RichText::new(&wallet.name()).size(theme.text_sizes.small),
                        )
                        .clicked()
                     {
                        if self.selected_wallet != Some(wallet.clone()) {
                           self.selected_wallet = Some(wallet);
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

            ComboBox::from_id_salt("chain_filter")
               .width(200.0)
               .selected_text(RichText::new(selected_chain_name).size(theme.text_sizes.small))
               .show_ui(ui, |ui| {
                  if ui
                     .selectable_label(
                        self.selected_chain.is_none(),
                        RichText::new("All Chains").size(theme.text_sizes.small),
                     )
                     .clicked()
                  {
                     if self.selected_chain.is_some() {
                        self.selected_chain = None;
                        self.current_page = 0;
                     }
                  }

                  for chain in ChainId::supported_chains() {
                     if ui
                        .selectable_label(
                           self.selected_chain == Some(chain),
                           RichText::new(chain.name()).size(theme.text_sizes.small),
                        )
                        .clicked()
                     {
                        if self.selected_chain != Some(chain) {
                           self.selected_chain = Some(chain);
                           self.current_page = 0;
                        }
                     }
                  }
               });

            /*
            #[cfg(feature = "dev")]
            if ui.add(Button::new("Add Dummy Tx")).clicked() {
               let wallet = ctx.current_wallet();
               ctx.write(|ctx| {
                  let chain = ctx.chain.clone();
                  ctx.tx_db.add_tx(
                     chain.id(),
                     wallet.address,
                     TxSummary::dummy_swap2(wallet.address),
                  );
               });
            }
            */

            /*
            #[cfg(feature = "dev")]
            if ui.add(Button::new("Add 50 Dummy Txs")).clicked() {
               let wallet = ctx.current_wallet();
               ctx.write(|ctx| {
                  let chain = ctx.chain.clone();
                  for _ in 0..50 {
                     ctx.tx_db.add_tx(
                        chain.id(),
                        wallet.address,
                        TxSummary::dummy_swap2(wallet.address),
                     );
                  }
               });
            }
            */

            #[cfg(feature = "dev")]
            if ui.add(Button::new("Save TxDB")).clicked() {
               ctx.save_tx_db();
            }
         });

         ui.add_space(10.0);
         ui.separator();
         ui.add_space(10.0);

         // --- Transaction Data Fetching and Filtering ---
         let all_wallets = ctx.get_all_wallets_info();
         let filtered_txs: Vec<TransactionRich> = ctx.read(|ctx_read| {
            let mut txs = Vec::new();
            
            for wallet in &all_wallets {
               if self.selected_wallet.is_some() && self.selected_wallet != Some(wallet.clone()) {
                  continue;
               }

               let chains_to_check: Vec<ChainId> = if let Some(chain) = self.selected_chain {
                  vec![chain]
               } else {
                  ChainId::supported_chains()
               };

               for chain in chains_to_check {
                  if let Some(wallet_txs) = ctx_read.tx_db.get_txs(chain.id(), wallet.address) {
                     txs.extend(wallet_txs.iter().cloned());
                  }
               }
            }
            // Sort all collected transactions by block number (newest first)
            txs.sort_unstable_by(|a, b| b.block.cmp(&a.block));
            txs
         });

         if filtered_txs.is_empty() {
            ui.vertical_centered(|ui| {
               ui.label(
                  RichText::new("No transactions match your filters.")
                     .size(theme.text_sizes.large)
                     .color(theme.colors.text_secondary),
               );
            });
            return;
         }

         let total_txs = filtered_txs.len();
         let total_pages = (total_txs as f64 / self.txs_per_page as f64).ceil() as usize;
         // Ensure current page is valid
         self.current_page = self.current_page.min(total_pages.saturating_sub(1));

         // --- Pagination Controls ---
         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            // Next Page Button
            let next_enabled = (self.current_page + 1) < total_pages;
            let next_button = Button::new(RichText::new("Next").size(theme.text_sizes.normal));
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
            let prev_button = Button::new(RichText::new("Previous").size(theme.text_sizes.normal));
            if ui.add_enabled(prev_enabled, prev_button).clicked() {
               self.current_page -= 1;
            }
         });
         ui.add_space(5.0);

         ui.vertical_centered(|ui| {
            ui.label(
               RichText::new(format!("{} transactions found", total_txs))
                  .size(theme.text_sizes.normal)
                  .color(theme.colors.text_secondary),
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
                  &filtered_txs[start..end]
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
                              .color(theme.colors.text_secondary),
                        );

                        ui.label(
                           RichText::new("Action")
                              .strong()
                              .size(theme.text_sizes.large)
                              .color(theme.colors.text_secondary),
                        );

                        ui.label(
                           RichText::new("Age")
                              .strong()
                              .size(theme.text_sizes.large)
                              .color(theme.colors.text_secondary),
                        );

                        ui.label(
                           RichText::new("Details")
                              .strong()
                              .size(theme.text_sizes.large)
                              .color(theme.colors.text_secondary),
                        );
                        ui.end_row();

                        // let bg_color = theme.frame2.fill;

                        for tx in txs_on_page {
                           // Wallet Name Column
                           let name = self.wallet_name_or_address(ctx.clone(), tx.sender());
                           ui.horizontal(|ui| {
                              ui.set_width(column_widths[0]);
                              ui.label(
                                 RichText::new(name)
                                    .size(theme.text_sizes.normal)
                                    .color(theme.colors.text_color),
                              );
                           });

                           // Action Name Column
                           ui.horizontal(|ui| {
                              ui.set_width(column_widths[1]);
                              ui.label(
                                 RichText::new(tx.action.name())
                                    .size(theme.text_sizes.normal)
                                    .color(theme.colors.text_color),
                              );
                           });

                           // Age Column
                           let age_text = format_age(tx.timestamp);
                           ui.horizontal(|ui| {
                              ui.set_width(column_widths[2]);
                              ui.label(
                                 RichText::new(age_text)
                                    .size(theme.text_sizes.small)
                                    .color(theme.colors.text_secondary),
                              );
                           });

                           // Details Button Column
                           let details_button =
                              Button::new(RichText::new("Details").size(theme.text_sizes.small));
                           // let visuals = theme.get_button_visuals(bg_color);
                           // widget_visuals(ui, visuals);
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

/// Formats a Unix timestamp into a relative age string.
fn format_age(timestamp: u64) -> String {
   let now = SystemTime::now();
   let tx_time = UNIX_EPOCH + Duration::from_secs(timestamp);
   match now.duration_since(tx_time) {
      Ok(duration) => {
         let secs = duration.as_secs();
         if secs < 60 {
            format!("{}s ago", secs)
         } else if secs < 3600 {
            format!("{}m ago", secs / 60)
         } else if secs < 86400 {
            format!("{}h ago", secs / 3600)
         } else if secs < 2_592_000 {
            // ~30 days
            format!("{}d ago", secs / 86400)
         } else {
            let months = secs / 2_592_000;
            format!("{}mo ago", months)
         }
      }
      Err(_) => "Just now".to_string(),
   }
}
