use egui::{
   Align, Align2, Button, Color32, ComboBox, FontId, Frame, Grid, Layout, Margin, Order, RichText,
   ScrollArea, Slider, Spinner, TextEdit, Ui, Vec2, Window, vec2,
};
use zeus_eth::currency::{Currency, ERC20Token, NativeCurrency};
use zeus_eth::types::ChainId;
use zeus_eth::utils::NumericValue;

use super::{UniswapSettingsUi, swap::InOrOut};
use crate::assets::icons::Icons;
use crate::core::{
   ZeusCtx,
   utils::{RT, eth, update},
};
use crate::gui::ui::dapps::uniswap::ProtocolVersion;
use crate::gui::{SHARED_GUI, ui::TokenSelectionWindow};
use egui_theme::{Theme, utils::*};
use egui_widgets::LabelWithImage;
use std::sync::Arc;

use std::time::Instant;

pub struct ViewPositionsUi {
   pub open: bool,
   pub size: (f32, f32),
   pub syncing: bool,
}

impl ViewPositionsUi {
   pub fn new() -> Self {
      Self {
         open: false,
         size: (600.0, 700.0),
         syncing: false,
      }
   }

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      if !self.open {
         return;
      }

      ui.vertical_centered(|ui| {
         ui.set_width(self.size.0);
         ui.set_height(self.size.1);
         ui.spacing_mut().item_spacing = vec2(0.0, 15.0);

         if self.syncing {
            ui.add(Spinner::new().size(20.0).color(Color32::WHITE));
         }

         let owner = ctx.current_wallet().address;
         let chain = ctx.chain();
         let positions = ctx.get_v3_positions(chain.id(), owner);

         if positions.is_empty() {
            let text = RichText::new("No positions found").size(theme.text_sizes.normal);
            ui.label(text);
            return;
         }

         let frame = theme.frame1;

         ScrollArea::vertical().show(ui, |ui| {
            ui.vertical_centered(|ui| {
               for position in &positions {
                  frame.show(ui, |ui| {
                     ui.vertical(|ui| {
                        let pair = RichText::new(format!(
                           "{}-{}",
                           position.token0.symbol(),
                           position.token1.symbol()
                        ))
                        .size(theme.text_sizes.normal);
                        ui.label(pair);

                        let fee = RichText::new(format!("{}%", position.fee.fee_percent()))
                           .size(theme.text_sizes.normal);
                        ui.label(fee);

                        let id =
                           RichText::new(format!("{}", position.id)).size(theme.text_sizes.normal);
                        ui.label(id);

                        let lower_tick = RichText::new(format!("{}", position.tick_lower))
                           .size(theme.text_sizes.normal);
                        ui.label(lower_tick);

                        let upper_tick = RichText::new(format!("{}", position.tick_upper))
                           .size(theme.text_sizes.normal);
                        ui.label(upper_tick);
                     });
                  });
               }
            });
         });
      });
   }
}
