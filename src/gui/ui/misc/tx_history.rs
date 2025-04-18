use std::sync::Arc;

use crate::assets::icons::Icons;
use crate::core::ZeusCtx;
use egui::{Align2, Button, Frame, Grid, RichText, ScrollArea, Ui, Window, vec2};
use egui_theme::{Theme, utils::widget_visuals};
use zeus_eth::utils::truncate_address;

use crate::core::utils::tx::{TxSummary, TxMethod};

pub struct TxHistory {
   pub open: bool,
   pub details_open: bool,
   pub selected_tx: Option<TxSummary>,
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

        // #[cfg(feature = "dev")]
         if ui.add(Button::new("Add Dummy Tx")).clicked() {
            let wallet = ctx.current_wallet();
            ctx.write(|ctx| {
               let chain = ctx.chain.clone();
               ctx.tx_db
                  .add_tx(chain.id(), wallet.address, TxSummary::default());
            });
         }

       //  #[cfg(feature = "dev")]
         if ui.add(Button::new("Add 50 Dummy Txs")).clicked() {
            let wallet = ctx.current_wallet();
            ctx.write(|ctx| {
               let chain = ctx.chain.clone();
               for _ in 0..50 {
                  ctx.tx_db
                     .add_tx(chain.id(), wallet.address, TxSummary::default());
               }
            });
         }

        // #[cfg(feature = "dev")]
         if ui.add(Button::new("Save TxDB")).clicked() {
            ctx.save_tx_db();
         }

         let current_wallet = ctx.current_wallet();
         let chain = ctx.chain();
         let all_txs = ctx.read(|ctx| {
            ctx.tx_db
               .get_txs(chain.id(), current_wallet.address)
               .cloned()
         });

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
                              ui.label(RichText::new(tx.action.name()).size(theme.text_sizes.normal));
                              ui.add_space(5.0);
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

      
   }



}
