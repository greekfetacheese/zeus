//! A ComboBox to select a chain

use crate::assets::icons::Icons;
use eframe::egui::{RichText, Sense, Ui, Vec2};
use std::sync::Arc;

use zeus_eth::types::ChainId;
use zeus_theme::Theme;
use zeus_widgets::{ComboBox, Label};

/// A ComboBox to select a chain
pub struct ChainSelect {
   pub id: &'static str,
   pub chain: ChainId,
   pub size: Vec2,
   pub show_icon: bool,
   pub expansion: Option<f32>,
}

impl ChainSelect {
   pub fn new(id: &'static str, default_chain: u64) -> Self {
      Self {
         id,
         chain: ChainId::new(default_chain).unwrap(),
         size: (200.0, 25.0).into(),
         show_icon: true,
         expansion: Some(4.0),
      }
   }

   pub fn size(mut self, size: impl Into<Vec2>) -> Self {
      self.size = size.into();
      self
   }

   pub fn show_icon(mut self, show: bool) -> Self {
      self.show_icon = show;
      self
   }

   /// Show the ComboBox
   ///
   /// Returns true if the chain was changed
   pub fn show(
      &mut self,
      ignore_chain: u64,
      theme: &Theme,
      icons: Arc<Icons>,
      ui: &mut Ui,
   ) -> bool {
      let current_chain = self.chain;
      let mut clicked = false;
      let supported_chains = ChainId::supported_chains();
      let expansion = self.expansion;

      let text_size = theme.text_sizes.normal;
      let combo_visuals = theme.combo_box_visuals();
      let label_visuals = theme.label_visuals();
      let tint = theme.image_tint_recommended;
      let icon = icons.chain_icon(current_chain.id(), tint);

      let current_chain_label = Label::new(
         RichText::new(current_chain.name()).size(text_size),
         Some(icon),
      )
      .image_on_left()
      .sense(Sense::click())
      .visuals(label_visuals);

      ComboBox::new(self.id, current_chain_label)
         .visuals(combo_visuals)
         .width(self.size.x)
         .show_ui(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 10.0;

            for chain in supported_chains {
               if chain.id() == ignore_chain {
                  continue;
               }

               let text = RichText::new(chain.name()).size(text_size);
               let icon = icons.chain_icon(chain.id(), tint);

               let is_selected = chain == current_chain;
               let chain_label = Label::new(text.clone(), Some(icon))
                  .image_on_left()
                  .expand(expansion)
                  .fill_width(true)
                  .selected(is_selected)
                  .visuals(label_visuals)
                  .sense(Sense::click());

               if ui.add(chain_label).clicked() {
                  self.chain = chain;
                  clicked = true;
               }
            }
         });
      clicked
   }
}
