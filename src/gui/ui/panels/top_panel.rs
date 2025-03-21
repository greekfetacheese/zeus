use crate::assets::icons::Icons;
use crate::core::ZeusCtx;
use crate::gui::{
   GUI,
   ui::{ChainSelect, WalletSelect},
};
use egui::{Align, Grid, Layout, RichText, SelectableLabel, Spinner, Ui, vec2};
use egui_theme::{Theme, utils::*};
use std::sync::Arc;

const DATA_SYNCING_MSG: &str = "Zeus is still syncing important data, do not close the app yet!";

pub fn show(gui: &mut GUI, ui: &mut Ui) {
   let ctx = gui.ctx.clone();
   let syncing = ctx.read(|ctx| ctx.data_syncing);
   let icons = gui.icons.clone();
   let theme = &gui.theme;

   if syncing {
      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         ui.label(RichText::new(DATA_SYNCING_MSG).size(theme.text_sizes.normal));
         ui.add(Spinner::new().size(20.0));
      });
   }

   // For now no need to call ctx.request_repaint() here
   // because the spinner does that even when the window is minimized
   if gui.wallet_ui.view_key_ui.exporter.key_copied_time.is_some() {
      ui.vertical_centered(|ui| {
         Grid::new("key_copied_grid")
            .spacing([0.0, 0.0])
            .show(ui, |ui| {
               ui.add(Spinner::new().size(20.0));
               gui.wallet_ui
                  .view_key_ui
                  .exporter
                  .update(theme, ui.ctx().clone(), ui);
               ui.end_row();
            });
      });
   }

   ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
      gui.top_left_area.show(ctx, icons, theme, ui);
   });
}

pub struct TopLeftArea {
   pub open: bool,
   pub chain_select: ChainSelect,
   pub wallet_select: WalletSelect,
   pub size: (f32, f32),
}

impl TopLeftArea {
   pub fn new() -> Self {
      Self {
         open: false,
         chain_select: ChainSelect::new("main_chain_select"),
         wallet_select: WalletSelect::new("main_wallet_select"),
         size: (300.0, 140.0),
      }
   }

   pub fn show(&mut self, ctx: ZeusCtx, icons: Arc<Icons>, theme: &Theme, ui: &mut Ui) {
      if !self.open {
         return;
      }

      ui.vertical(|ui| {
         ui.set_width(self.size.0);
         ui.set_height(self.size.1);

         ui.spacing_mut().item_spacing = vec2(0.0, 15.0);
         ui.spacing_mut().button_padding = vec2(10.0, 8.0);
         widget_visuals(ui, theme.get_widget_visuals(theme.colors.bg_color));

         // Chain Select
         let clicked = self.chain_select.show(theme, icons.clone(), ui);
         if clicked {
            // update the chain
            ctx.write(|ctx| {
               ctx.chain = self.chain_select.chain.clone();
            });
         }

         // Wallet Select
         let wallets = ctx.account().wallets;
         let clicked = self.wallet_select.show(theme, &wallets, icons.clone(), ui);
         if clicked {
            // update the wallet
            ctx.write(|ctx| {
               ctx.account.current_wallet = self.wallet_select.wallet.clone();
            });
         }
         ui.end_row();

         let wallet = ctx.account().current_wallet;
         let address = wallet.address_truncated();

         let address_text = RichText::new(address).size(theme.text_sizes.normal);
         if ui.add(SelectableLabel::new(false, address_text)).clicked() {
            ui.ctx().copy_text(wallet.address_string());
         }
      });
   }
}
