//! A ComboBox to select a wallet

use crate::assets::icons::Icons;
use crate::core::ZeusContext;
use eframe::egui::{RichText, Sense, Ui, Vec2};
use std::sync::Arc;

use zeus_theme::Theme;
use zeus_wallet::Wallet;
use zeus_widgets::{ComboBox, Label};

/// A ComboBox to select a wallet
pub struct WalletSelect {
   pub id: &'static str,
   /// Selected Wallet
   pub wallet: Wallet,
   pub size: Vec2,
   pub expansion: Option<f32>,
}

impl WalletSelect {
   pub fn new(id: &'static str) -> Self {
      Self {
         id,
         wallet: Wallet::new_rng("I should not be here2".to_string()),
         size: (200.0, 25.0).into(),
         expansion: Some(6.0),
      }
   }

   pub fn size(mut self, size: impl Into<Vec2>) -> Self {
      self.size = size.into();
      self
   }

   pub fn expansion(mut self, expansion: f32) -> Self {
      self.expansion = Some(expansion);
      self
   }

   /// Show the ComboBox
   ///
   /// Returns true if the wallet was changed
   pub fn show(
      &mut self,
      theme: &Theme,
      ctx: &mut ZeusContext,
      icons: Arc<Icons>,
      ui: &mut Ui,
   ) -> bool {
      let mut clicked = false;
      let expansion = self.expansion;

      let combo_visuals = theme.combo_box_visuals();
      let label_visuals = theme.label_visuals();
      let wallet_icon = icons.wallet_main_x24();
      let text = RichText::new(&self.wallet.name_with_id_short()).size(theme.text_sizes.normal);

      let current_wallet_label = Label::new(text, Some(wallet_icon))
         .image_on_left()
         .expand(expansion)
         .visuals(label_visuals)
         .sense(Sense::click());

      ComboBox::new(self.id, current_wallet_label)
         .visuals(combo_visuals)
         .width(self.size.x)
         .show_ui(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 14.0;

            for wallet in ctx.vault_ref().all_wallets() {
               let is_selected = wallet.address() == self.wallet.address();
               let text = RichText::new(wallet.name_with_id_short()).size(theme.text_sizes.normal);

               let wallet_label = Label::new(text, None)
                  .fill_width(true)
                  .expand(expansion)
                  .selected(is_selected)
                  .visuals(label_visuals)
                  .sense(Sense::click());

               if ui.add(wallet_label).clicked() {
                  self.wallet = wallet.clone();
                  clicked = true;
               }
            }
         });

      clicked
   }
}
