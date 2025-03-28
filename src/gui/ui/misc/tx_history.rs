use std::sync::Arc;

use crate::assets::icons::Icons;
use crate::core::ZeusCtx;
use egui::{Align2, Button, Frame, Grid, RichText, ScrollArea, Ui, Window, vec2};
use egui_theme::{Theme, utils::widget_visuals};
use zeus_eth::utils::truncate_address;

use crate::core::utils::tx::{TxDetails, TxMethod};

pub struct TxHistory {
   pub open: bool,
   pub details_open: bool,
   pub selected_tx: Option<TxDetails>,
   pub current_page: usize,
   pub txs_per_page: usize,
}

impl TxHistory {
   pub fn new() -> Self {
      Self {
         open: false,
         details_open: false,
         selected_tx: None,
         current_page: 0,
         txs_per_page: 50,
      }
   }

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      if !self.open {
         return;
      }

      ui.vertical_centered(|ui| {
         ui.spacing_mut().item_spacing.y = 20.0;
         ui.spacing_mut().button_padding = vec2(10.0, 8.0);

         #[cfg(feature = "dev")]
         if ui.add(Button::new("Add Dummy Tx")).clicked() {
            ctx.write(|ctx| {
               let chain = ctx.chain.clone();
               let owner = ctx.account.current_wallet.address();
               ctx.tx_db.add_tx(chain.id(), owner, TxDetails::default());
            });
         }

         #[cfg(feature = "dev")]
         if ui.add(Button::new("Add 50 Dummy Txs")).clicked() {
            ctx.write(|ctx| {
               let chain = ctx.chain.clone();
               let owner = ctx.account.current_wallet.address();
               for _ in 0..50 {
                  ctx.tx_db.add_tx(chain.id(), owner, TxDetails::default());
               }
            });
         }

         let ctx_clone = ctx.clone();
         ctx.read(|ctx| {
            let account = &ctx.account;
            let chain = ctx.chain;
            let all_txs = ctx
               .tx_db
               .get_txs(chain.id(), account.current_wallet.address());

            if all_txs.is_none() {
               ui.label(RichText::new("No transactions found").size(theme.text_sizes.large));
               return;
            }
            let all_txs = all_txs.unwrap();

            let total_txs = all_txs.len();
            let total_pages = (total_txs as f32 / self.txs_per_page as f32).ceil() as usize;

            // Calculate the start and end indices for the current page
            let start = self.current_page * self.txs_per_page;
            let end = ((self.current_page + 1) * self.txs_per_page).min(total_txs);
            let txs = &all_txs[start..end];

            // Pagination controls
            ui.vertical_centered(|ui| {
               ui.label(RichText::new(format!("Found {} transactions", total_txs)).size(theme.text_sizes.normal));
               if self.current_page > 0 {
                  if ui
                     .add(Button::new(
                        RichText::new("Previous").size(theme.text_sizes.normal),
                     ))
                     .clicked()
                  {
                     self.current_page -= 1;
                  }
               }
               ui.label(format!("Page {} of {}", self.current_page + 1, total_pages));
               if (self.current_page + 1) < total_pages {
                  if ui
                     .add(Button::new(
                        RichText::new("Next").size(theme.text_sizes.normal),
                     ))
                     .clicked()
                  {
                     self.current_page += 1;
                  }
               }
            });

            ScrollArea::vertical()
               .id_salt("tx_history_scroll_area")
               .auto_shrink([false; 2])
               .show(ui, |ui| {
                  let column_width = ui.available_width() * 0.8;
                  let column_height = 50.0;

                  let frame = theme.frame1;
                  let bg_color = frame.fill;
                  for tx in txs {
                     frame.show(ui, |ui| {
                        ui.set_width(column_width);
                        ui.set_height(column_height);
                        ui.spacing_mut().item_spacing.x = 10.0;

                        ui.horizontal(|ui| {
                           // Calculate total content width to center
                           let action_width = 350.0;
                           let button_width = 80.0;
                           let total_content_width = action_width + button_width + ui.spacing().item_spacing.x;
                           let space = (column_width - total_content_width).max(0.0) / 2.0;
                           ui.add_space(space);

                           // Transaction Action
                           ui.add_sized([action_width, column_height], |ui: &mut Ui| {
                              ui.horizontal(|ui| {
                                 ui.label(RichText::new(tx.method.as_str()).size(theme.text_sizes.normal));
                                 ui.add_space(5.0);
                                 self.tx_method(ctx_clone.clone(), theme, icons.clone(), tx, ui);
                              });
                              ui.allocate_rect(ui.min_rect(), egui::Sense::hover())
                           });

                           // Details Button
                           let button = Button::new(RichText::new("Details").size(theme.text_sizes.normal))
                              .min_size(vec2(button_width, 30.0));
                           let visuals = theme.get_button_visuals(bg_color);
                           widget_visuals(ui, visuals);
                           if ui.add(button).clicked() {
                              self.details_open = true;
                              self.selected_tx = Some(tx.clone());
                           }
                        });
                     });
                  }
               });
         });
      });

      if self.details_open {
         self.show_tx_details(ctx, theme, ui);
      }
   }

   fn tx_method(&self, ctx: ZeusCtx, theme: &Theme, icons: Arc<Icons>, tx: &TxDetails, ui: &mut Ui) {
      match &tx.method {
         TxMethod::Transfer(currency) => {
            let icon = icons.native_currency_icon(currency);
            ui.add(icon);
            ui.label(RichText::new(&currency.symbol).size(theme.text_sizes.normal));
            ui.label(RichText::new(tx.value.formatted()).size(theme.text_sizes.normal));
            ui.label(RichText::new("->").size(theme.text_sizes.normal));
            let name = self.get_recipient_name(ctx, tx);
            ui.label(
               RichText::new(name.unwrap_or(truncate_address(&tx.to.to_string(), 16))).size(theme.text_sizes.small),
            );
         }
         TxMethod::ERC20Transfer((token, amount)) => {
            let icon = icons.token_icon(token.address, token.chain_id);
            ui.add(icon);
            ui.label(RichText::new(&token.symbol).size(theme.text_sizes.normal));
            ui.label(RichText::new(amount.formatted()).size(theme.text_sizes.normal));
            ui.label(RichText::new("->").size(theme.text_sizes.normal));
            let name = self.get_recipient_name(ctx, tx);
            ui.label(
               RichText::new(name.unwrap_or(truncate_address(&tx.to.to_string(), 16))).size(theme.text_sizes.small),
            );
         }
         TxMethod::Bridge((currency, amount)) => {
            let icon = icons.currency_icon(currency);
            ui.add(icon);
            ui.label(RichText::new(currency.symbol()).size(theme.text_sizes.normal));
            ui.label(RichText::new(amount.formatted()).size(theme.text_sizes.normal));
         }
         TxMethod::Swap(swap_details) => {
            let icon0 = icons.currency_icon(&swap_details.token_in);
            let icon1 = icons.currency_icon(&swap_details.token_out);
            ui.add(icon0);
            ui.label(RichText::new(swap_details.token_in.symbol()).size(theme.text_sizes.normal));
            ui.label(RichText::new("->").size(theme.text_sizes.normal));
            ui.add(icon1);
            ui.label(RichText::new(swap_details.token_out.symbol()).size(theme.text_sizes.normal));
         }
         TxMethod::Other => {
            ui.label(RichText::new("Other").size(theme.text_sizes.normal));
         }
      }
   }

   fn get_recipient_name(&self, ctx: ZeusCtx, tx: &TxDetails) -> Option<String> {
      ctx.read(|ctx| {
         let contacts = &ctx.contact_db.contacts;
         let account = &ctx.account;
         if let Some(contact) = contacts.iter().find(|c| c.address == tx.to.to_string()) {
            Some(contact.name.clone())
         } else if let Some(wallet) = account.wallets.iter().find(|w| w.address() == tx.to) {
            Some(wallet.name.clone())
         } else {
            None
         }
      })
   }

   fn show_tx_details(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      if let Some(tx) = &self.selected_tx {
         let mut is_open = self.details_open; // Need mutable copy for window open state
         Window::new(RichText::new("Transaction Details").size(theme.text_sizes.heading))
            .open(&mut is_open)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .resizable(false)
            .collapsible(false)
            .default_width(450.0)
            .frame(Frame::window(ui.style()))
            .show(ui.ctx(), |ui| {
               let chain = ctx.chain();

               ScrollArea::vertical()
                  .auto_shrink([false, false])
                  .show(ui, |ui| {
                     Grid::new("tx_details_grid")
                        .num_columns(2)
                        .spacing([40.0, 10.0])
                        .striped(true)
                        .show(ui, |ui| {
                           // Transaction Hash
                           ui.label(RichText::new("Hash:").size(theme.text_sizes.normal));
                           ui.horizontal(|ui| {
                              let text = RichText::new(truncate_address(&tx.hash.to_string(), 16))
                                 .size(theme.text_sizes.small)
                                 .strong();
                              let link = format!("{}/tx/{}", chain.block_explorer(), tx.hash.to_string());
                              ui.hyperlink_to(text, link);
                           });
                           ui.end_row();

                           // Status
                           ui.label(RichText::new("Status:").size(theme.text_sizes.normal));
                           // Add color/icon based on status
                           let status_text = RichText::new(tx.success_str()).size(theme.text_sizes.normal);
                           let status_text = if tx.success {
                              status_text.color(egui::Color32::GREEN)
                           } else {
                              status_text.color(egui::Color32::RED)
                           };
                           ui.label(status_text);
                           ui.end_row();

                           // Block
                           ui.label(RichText::new("Block:").size(theme.text_sizes.normal));
                           ui.label(RichText::new(format!("{}", tx.block)).size(theme.text_sizes.normal));
                           ui.end_row();

                           // TODO: Add Timestamp
                           // ui.label(RichText::new("Timestamp:").size(theme.text_sizes.normal));
                           // let formatted_time = format_timestamp(tx.timestamp); // Implement this
                           // ui.label(RichText::new(formatted_time).size(theme.text_sizes.normal));
                           // ui.end_row();

                           // From
                           ui.label(RichText::new("From:").size(theme.text_sizes.normal));
                           ui.horizontal(|ui| {
                              ui.label(RichText::new(&tx.from.to_string()).size(theme.text_sizes.small));
                              if ui.button("📋").on_hover_text("Copy Address").clicked() {
                                 ui.ctx().copy_text(tx.from.to_string());
                              }
                              let link = format!("{}/address/{}", chain.block_explorer(), tx.from.to_string());
                              ui.hyperlink_to("↗", link).on_hover_text("View on Explorer");
                           });
                           ui.end_row();

                           // To
                           ui.label(RichText::new("To:").size(theme.text_sizes.normal));
                           ui.horizontal(|ui| {
                              ui.label(RichText::new(&tx.to.to_string()).size(theme.text_sizes.small));
                              if ui.button("📋").on_hover_text("Copy Address").clicked() {
                                 ui.ctx().copy_text(tx.to.to_string());
                              }
                              let link = format!("{}/address/{}", chain.block_explorer(), tx.to.to_string());
                              ui.hyperlink_to("↗", link).on_hover_text("View on Explorer");
                           });
                           ui.end_row();

                           // Value
                           ui.label(RichText::new("Value:").size(theme.text_sizes.normal));
                           ui.vertical(|ui| {
                              // Use vertical for multi-line values
                              ui.label(
                                 RichText::new(format!("{} {}", tx.value.formatted(), chain.coin_symbol()))
                                    .size(theme.text_sizes.normal),
                              );
                              // USD Value
                              ui.label(
                                 RichText::new(format!("~${}", tx.value_in_usd().formatted()))
                                    .size(theme.text_sizes.small)
                                    .weak(),
                              );
                           });
                           ui.end_row();

                           // Transaction Fee
                           ui.label(RichText::new("Fee:").size(theme.text_sizes.normal));
                           ui.vertical(|ui| {
                              ui.label(
                                 RichText::new(format!(
                                    "{} {}",
                                    tx.fee_in_eth().formatted(),
                                    chain.coin_symbol()
                                 ))
                                 .size(theme.text_sizes.normal),
                              );
                              // USD Value
                              ui.label(
                                 RichText::new(format!("~${}", tx.fee_in_usd().formatted()))
                                    .size(theme.text_sizes.small)
                                    .weak(),
                              );
                           });
                           ui.end_row();

                           // Base Fee
                           ui.label(RichText::new("Base Fee:").size(theme.text_sizes.normal));
                           ui.label(
                              RichText::new(format!("{} Gwei", tx.base_fee.formatted())).size(theme.text_sizes.normal),
                           );
                           ui.end_row();

                           // Priority Fee
                           ui.label(RichText::new("Priority Fee:").size(theme.text_sizes.normal));
                           ui.label(
                              RichText::new(format!("{} Gwei", tx.priority_fee.formatted()))
                                 .size(theme.text_sizes.normal),
                           );
                           ui.end_row();

                           // Gas Used
                           ui.label(RichText::new("Gas Used:").size(theme.text_sizes.normal));
                           ui.label(RichText::new(format!("{}", tx.gas_used)).size(theme.text_sizes.normal));
                           ui.end_row();
                        });
                  });
            });
         self.details_open = is_open;
         if !is_open {
            self.selected_tx = None;
         }
      }
   }
}
