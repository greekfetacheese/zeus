use egui::{
   CornerRadius, FontId, Frame, Grid, Margin, RichText, ScrollArea, Sense, TextEdit, Ui, vec2,
};
use egui_widgets::Label;

use crate::assets::icons::Icons;
use crate::core::ZeusCtx;
use egui_theme::{Theme, utils::*};
use std::sync::Arc;
use zeus_eth::amm::{AnyUniswapPool, UniswapPool};

pub struct PoolsUi {
   pub open: bool,
   pub search_query: String,
   pub size: (f32, f32),
}

impl PoolsUi {
   pub fn new() -> Self {
      Self {
         open: false,
         search_query: String::new(),
         size: (600.0, 500.0),
      }
   }

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      if !self.open {
         return;
      }

      ui.vertical_centered(|ui| {
         ui.set_width(self.size.0);
         ui.set_max_height(self.size.1);
         ui.spacing_mut().item_spacing = vec2(10.0, 15.0);
         ui.spacing_mut().button_padding = vec2(10.0, 8.0);
         let ui_width = ui.available_width();

         TextEdit::singleline(&mut self.search_query)
            .hint_text(RichText::new("Search pools"))
            .desired_width(ui_width * 0.5)
            .margin(Margin::same(10))
            .font(FontId::proportional(theme.text_sizes.normal))
            .show(ui);

         let manager = ctx.pool_manager();
         let v2_pools = manager.get_v2_pools_for_chain(ctx.chain().id());
         let v3_pools = manager.get_v3_pools_for_chain(ctx.chain().id());
         let mut pools = Vec::new();
         pools.extend(v3_pools);
         pools.extend(v2_pools);

         let column_width = ui_width / 5.0;
         let query = &self.search_query;

         ScrollArea::vertical().show(ui, |ui| {
            ui.set_width(ui_width);

            Grid::new("pools_ui")
               .spacing(vec2(25.0, 15.0))
               .show(ui, |ui| {
                  // Header

                  // Pool
                  ui.label(RichText::new("Pool").size(theme.text_sizes.large));

                  // Protocol
                  ui.label(RichText::new("Protocol").size(theme.text_sizes.large));

                  // Fee
                  ui.label(RichText::new("Fee").size(theme.text_sizes.large));

                  ui.label(RichText::new("Has State").size(theme.text_sizes.large));

                  // TVL
                  ui.label(RichText::new("TVL").size(theme.text_sizes.large));

                  ui.end_row();

                  for (_i, pool) in pools.into_iter().enumerate() {
                     let valid_search = valid_search(&pool, &query);
                     if !valid_search {
                        continue;
                     }

                     // Pool
                     ui.scope(|ui| {
                        ui.set_width(column_width);
                        ui.spacing_mut().item_spacing.x = 5.0;

                        let token0 = pool.currency0();
                        let token1 = pool.currency1();

                        let icon0 = icons.currency_icon_x24(&token0);
                        let icon1 = icons.currency_icon_x24(&token1);

                        let label0 = Label::new(
                           RichText::new(token0.symbol()).size(theme.text_sizes.normal),
                           Some(icon0),
                        )
                        .image_on_left();

                        let label1 = Label::new(
                           RichText::new(token1.symbol()).size(theme.text_sizes.normal),
                           Some(icon1),
                        );

                        let mut frame = Frame::new()
                           .corner_radius(CornerRadius::same(10))
                           .inner_margin(Margin::same(10));
                        let visuals = theme.frame1_visuals.clone();
                        let res = frame_it(&mut frame, Some(visuals), ui, |ui| {
                           ui.horizontal(|ui| {
                              ui.add(label0);
                              ui.add(Label::new(
                                 RichText::new("/").size(theme.text_sizes.normal),
                                 None,
                              ));
                              ui.add(label1);
                           });
                        });

                        if res.interact(Sense::click()).clicked() {
                           println!("Clicked");
                        }
                     });

                     // Protocol Version
                     ui.scope(|ui| {
                        ui.set_width(column_width);
                        let text = if pool.dex_kind().is_v2() {
                           RichText::new("V2").size(theme.text_sizes.normal)
                        } else {
                           RichText::new("V3").size(theme.text_sizes.normal)
                        };
                        ui.label(text);
                     });

                     // Fee
                     ui.scope(|ui| {
                        ui.set_width(column_width);

                        let fee = pool.fee().fee_percent();
                        let text = RichText::new(format!("{fee}%")).size(theme.text_sizes.normal);
                        ui.label(text);
                     });

                        ui.scope(|ui| {
                        ui.set_width(column_width);

                        let has_state = pool.state().is_some();
                        let text = RichText::new(if has_state { "Yes" } else { "No" }).size(theme.text_sizes.normal);
                        ui.label(text);
                     });

                     // TVL
                     ui.scope(|ui| {
                        ui.set_width(column_width);
                        ui.label(RichText::new("TODO").size(theme.text_sizes.normal));
                     });

                     ui.end_row();
                  }
               });
         });
      });
   }
}

fn valid_search(pool: &AnyUniswapPool, query: &str) -> bool {
   let query = query.to_lowercase();

   if query.is_empty() {
      return true;
   }

   if pool.currency0().symbol().to_lowercase().contains(&query) {
      return true;
   }

   if pool.currency1().symbol().to_lowercase().contains(&query) {
      return true;
   }

   if pool.dex_kind().is_v2() || pool.dex_kind().is_v3() {
      if pool.address().to_string().to_lowercase().contains(&query) {
         return true;
      }
   } else {
      if pool.pool_id().to_string().to_lowercase().contains(&query) {
         return true;
      }
   };

   false
}
