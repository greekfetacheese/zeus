use crate::assets::icons::Icons;
use crate::core::ZeusCtx;
use crate::gui::{
   GUI,
   ui::{ChainSelect, WalletSelect},
};
use egui::{Align, Color32, Grid, Layout, RichText, Spinner, Ui, vec2};
use egui_theme::Theme;
use std::sync::Arc;

const DATA_SYNCING_MSG: &str = "Zeus is still loading token data, do not close the app yet!";

pub fn show(gui: &mut GUI, ui: &mut Ui) {
   let ctx = gui.ctx.clone();
   let syncing = ctx.read(|ctx| ctx.pool_data_syncing);
   let icons = gui.icons.clone();

   if syncing {
      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         ui.label(RichText::new(DATA_SYNCING_MSG).size(14.0));
         ui.add(Spinner::new().size(20.0));
      });
   }

   ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
      gui.top_left_area.show(ctx, icons, &gui.theme, ui);
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
         chain_select: ChainSelect::new("main_chain_select", Some(Color32::TRANSPARENT), None).width(100.0),
         wallet_select: WalletSelect::new("main_wallet_select", Some(Color32::TRANSPARENT), None).width(100.0),
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

         ui.spacing_mut().item_spacing = vec2(0.0, 20.0);

         // Chain Select
         let clicked = self.chain_select.show(ui, icons.clone());
         if clicked {
            // if we select a new chain update the necessary state
            let chain = self.chain_select.chain.clone();
            // update the chain
            ctx.write(|ctx| {
               ctx.chain = chain.clone();
            });
         }

         // Wallet Select
         Grid::new("main_wallet_select").show(ui, |ui| {
            ui.add(icons.wallet());
            let wallets = ctx.profile().wallets;
            let clicked = self.wallet_select.show(&wallets, ui);
            if clicked {
               // if we select a new wallet update the necessary state
               let wallet = self.wallet_select.wallet.clone();
               // update the wallet
               ctx.write(|ctx| {
                  ctx.profile.current_wallet = wallet.clone();
               });
            }
            ui.end_row();
         });

         let wallet = ctx.profile().current_wallet;
         let address = wallet.address_truncated();

         if ui
            .selectable_label(false, RichText::new(address).size(14.0))
            .clicked()
         {
            ui.ctx().copy_text(wallet.address());
         }
      });
   }
}
