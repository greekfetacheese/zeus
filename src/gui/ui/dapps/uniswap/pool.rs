use egui::{FontId, Frame, Grid, Margin, RichText, ScrollArea, Sense, TextEdit, Ui, vec2};
use zeus_widgets::{ComboBox, Label};

use crate::assets::icons::Icons;
use crate::utils::truncate_symbol_or_name;
use crate::core::ZeusCtx;
use zeus_theme::Theme;
use std::sync::Arc;
use zeus_eth::amm::uniswap::{AnyUniswapPool, UniswapPool};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Version {
   All,
   V2,
   V3,
   V4,
}

impl Version {
   fn all() -> Vec<Self> {
      vec![Self::All, Self::V2, Self::V3, Self::V4]
   }

   fn is_all(&self) -> bool {
      matches!(self, Self::All)
   }

   fn is_v2(&self) -> bool {
      matches!(self, Self::V2)
   }

   fn is_v3(&self) -> bool {
      matches!(self, Self::V3)
   }

   fn is_v4(&self) -> bool {
      matches!(self, Self::V4)
   }

   fn to_str(&self) -> &'static str {
      match self {
         Self::All => "All",
         Self::V2 => "V2",
         Self::V3 => "V3",
         Self::V4 => "V4",
      }
   }
}

pub struct PoolsUi {
   pub open: bool,
   pub search_query: String,
   version: Option<Version>,
   pub size: (f32, f32),
}

impl PoolsUi {
   pub fn new() -> Self {
      Self {
         open: false,
         search_query: String::new(),
         version: None,
         size: (600.0, 500.0),
      }
   }

   fn select_version(&mut self, theme: &Theme, ui: &mut Ui) {
      let all_versions = Version::all();
      let selected_text = self.version.map(|v| v.to_str()).unwrap_or("Select Version");
      let selected_text = RichText::new(selected_text).size(theme.text_sizes.normal);
      let selected_label = Label::new(selected_text, None).sense(Sense::click()).interactive(false);

      ComboBox::new("pool_explore_combobox", selected_label)
         .width(200.0)
         .show_ui(ui, |ui| {
            for version in all_versions {
               let text = RichText::new(version.to_str()).size(theme.text_sizes.normal);
               let label = Label::new(text, None).sense(Sense::click()).interactive(false);
               if ui.add(label).clicked() {
                  self.version = Some(version);
               }
            }
         });
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

         ui.scope(|ui| {
            self.select_version(theme, ui);
         });

         ui.add_space(20.0);

         let manager = ctx.pool_manager();

         let column_width = ui_width / 5.0;
         let query = &self.search_query;

         ScrollArea::vertical().show(ui, |ui| {
            ui.set_width(ui_width);

            Grid::new("pools_ui").spacing(vec2(25.0, 15.0)).show(ui, |ui| {
               // Header

               // Pool
               ui.label(RichText::new("Pool").size(theme.text_sizes.large));

               // Protocol
               ui.label(RichText::new("Protocol").size(theme.text_sizes.large));

               // Fee
               ui.label(RichText::new("Fee").size(theme.text_sizes.large));

               ui.label(RichText::new("Has State").size(theme.text_sizes.large));

               ui.end_row();

               let selected_version = self.version.unwrap_or(Version::All);
               let chain = ctx.chain().id();

               manager.read(|manager| {
                  let pools = manager.pools.iter().filter(|(_, pool)| {
                     selected_version.is_all() && pool.chain_id() == chain
                        || selected_version.is_v2()
                           && pool.dex_kind().is_v2()
                           && pool.chain_id() == chain
                        || selected_version.is_v3()
                           && pool.dex_kind().is_v3()
                           && pool.chain_id() == chain
                        || selected_version.is_v4()
                           && pool.dex_kind().is_v4()
                           && pool.chain_id() == chain
                  });

                  for (_key, pool) in pools {
                     let valid_search = valid_search(pool, query);
                     if !valid_search {
                        continue;
                     }

                     // Pool
                     ui.scope(|ui| {
                        ui.set_width(column_width);
                        ui.spacing_mut().item_spacing.x = 5.0;
                        let tint = theme.image_tint_recommended;

                        let token0 = pool.currency0();
                        let token1 = pool.currency1();

                        let icon0 = icons.currency_icon(token0, tint);
                        let icon1 = icons.currency_icon(token1, tint);

                        let token0_symbol = truncate_symbol_or_name(token0.symbol(), 10);
                        let token1_symbol = truncate_symbol_or_name(token1.symbol(), 10);

                        let label0 = Label::new(
                           RichText::new(token0_symbol).size(theme.text_sizes.normal),
                           Some(icon0),
                        )
                        .image_on_left()
                        .interactive(false);

                        let label1 = Label::new(
                           RichText::new(token1_symbol).size(theme.text_sizes.normal),
                           Some(icon1),
                        ).interactive(false);

                        Frame::new().inner_margin(Margin::same(5)).show(ui, |ui| {
                           ui.horizontal(|ui| {
                              ui.add(label0);
                              ui.add(Label::new(
                                 RichText::new("/").size(theme.text_sizes.normal),
                                 None,
                              ).interactive(false));
                              ui.add(label1);
                           });
                        });
                     });

                     // Protocol Version
                     ui.scope(|ui| {
                        ui.set_width(column_width);
                        let text = if pool.dex_kind().is_v2() {
                           RichText::new("V2").size(theme.text_sizes.normal)
                        } else if pool.dex_kind().is_v3() {
                           RichText::new("V3").size(theme.text_sizes.normal)
                        } else {
                           RichText::new("V4").size(theme.text_sizes.normal)
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
                        let text = RichText::new(if has_state { "Yes" } else { "No" })
                           .size(theme.text_sizes.normal);
                        ui.label(text);
                     });

                     ui.end_row();
                  }
               });
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
      if pool.id().to_string().to_lowercase().contains(&query) {
         return true;
      }
   };

   false
}
