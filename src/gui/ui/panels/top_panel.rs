use crate::assets::icons::Icons;
use crate::core::{ZeusCtx, utils::RT};
use crate::gui::{
   GUI, SHARED_GUI,
   ui::{ChainSelect, WalletSelect},
};
use egui::{Align, Grid, Layout, RichText, SelectableLabel, Spinner, Ui, vec2};
use egui_theme::{Theme, utils::*};
use std::sync::Arc;
use zeus_eth::currency::{Currency, NativeCurrency};

const DATA_SYNCING_MSG: &str = "Zeus is still syncing important data, do not close the app yet!";
const ON_STARTUP_SYNC_MSG: &str = "Zeus is syncing your wallets state, do not close the app yet!";

pub fn show(gui: &mut GUI, ui: &mut Ui) {
   let ctx = gui.ctx.clone();
   let data_syncing = ctx.read(|ctx| ctx.data_syncing);
   let on_startup_syncing = ctx.read(|ctx| ctx.on_startup_syncing);
   let icons = gui.icons.clone();
   let theme = &gui.theme;

   if data_syncing && !on_startup_syncing {
      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         ui.label(RichText::new(DATA_SYNCING_MSG).size(theme.text_sizes.normal));
         ui.add(Spinner::new().size(20.0));
      });
   }

   if on_startup_syncing && !data_syncing {
      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         ui.label(RichText::new(ON_STARTUP_SYNC_MSG).size(theme.text_sizes.normal));
         ui.add(Spinner::new().size(20.0));
      });
   }

   // For now no need to call ctx.request_repaint() here
   // because the spinner does that even when the window is minimized
   if gui
      .wallet_ui
      .export_key_ui
      .exporter
      .key_copied_time
      .is_some()
   {
      ui.vertical_centered(|ui| {
         Grid::new("key_copied_grid")
            .spacing([0.0, 0.0])
            .show(ui, |ui| {
               ui.add(Spinner::new().size(20.0));
               gui.wallet_ui
                  .export_key_ui
                  .exporter
                  .update(theme, ui.ctx().clone(), ui);
               ui.end_row();
            });
      });
   }

   ui.spacing_mut().item_spacing = vec2(0.0, 10.0);
   ui.spacing_mut().button_padding = vec2(10.0, 8.0);

   ui.horizontal(|ui| {
      if gui.tx_confirm_window.is_open() {
         ui.disable();
      }

      if gui.sign_msg_window.is_open() {
         ui.disable();
      }

      gui.chain_selection
         .show(ctx.clone(), theme, icons.clone(), ui);
   });

   ui.horizontal(|ui| {
      // Disable the wallet selection when we are in the review window
      // To avoid any mistakes
      if gui.tx_confirm_window.is_open() {
         ui.disable();
      }

      if gui.sign_msg_window.is_open() {
         ui.disable();
      }

      gui.wallet_selection
         .show(ctx.clone(), theme, icons.clone(), ui);
   });

   ui.horizontal(|ui| {
      let wallet = ctx.current_wallet();
      let address = wallet.address_truncated();

      let address_text = RichText::new(address).size(theme.text_sizes.normal);
      if ui.add(SelectableLabel::new(false, address_text)).clicked() {
         ui.ctx().copy_text(wallet.address_string());
      }
   });
}

pub struct ChainSelection {
   pub open: bool,
   pub chain_select: ChainSelect,
}

impl ChainSelection {
   pub fn new() -> Self {
      Self {
         open: false,
         chain_select: ChainSelect::new("main_chain_select", 1),
      }
   }

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      if !self.open {
         return;
      }

      ui.vertical(|ui| {
         ui.spacing_mut().item_spacing = vec2(0.0, 15.0);
         ui.spacing_mut().button_padding = vec2(10.0, 8.0);
         widget_visuals(
            ui,
            theme.get_widget_visuals(theme.colors.bg_color),
         );

         // Chain Select
         let clicked = self.chain_select.show(0, theme, icons.clone(), ui);
         if clicked {
            let ctx_clone = ctx.clone();
            let new_chain = self.chain_select.chain.clone();

            RT.spawn_blocking(move || {
               ctx_clone.write(|ctx| {
                  ctx.chain = new_chain;
               });

               SHARED_GUI.write(|gui| {
                  let currency =
                     Currency::from(NativeCurrency::from_chain_id(new_chain.id()).unwrap());
                  gui.send_crypto.set_currency(currency.clone());
                  gui.uniswap.swap_ui.default_currency_in(new_chain.id());
                  gui.uniswap.swap_ui.default_currency_out(new_chain.id());
                  gui.uniswap.position_ui.default_currency_a(new_chain.id());
                  gui.uniswap.position_ui.default_currency_b(new_chain.id());
               });
            });
         }
      });
   }
}

pub struct WalletSelection {
   pub open: bool,
   pub wallet_select: WalletSelect,
}

impl WalletSelection {
   pub fn new() -> Self {
      Self {
         open: false,
         wallet_select: WalletSelect::new("main_wallet_select"),
      }
   }

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      if !self.open {
         return;
      }

      ui.vertical(|ui| {
         ui.spacing_mut().item_spacing = vec2(0.0, 15.0);
         ui.spacing_mut().button_padding = vec2(10.0, 8.0);
         widget_visuals(
            ui,
            theme.get_widget_visuals(theme.colors.bg_color),
         );

         // Wallet Select
         ui.spacing_mut().button_padding = vec2(10.0, 12.0);
         let wallets = ctx.wallets_info();
         let clicked = self.wallet_select.show(theme, &wallets, icons.clone(), ui);
         if clicked {
            // update the wallet
            ctx.write_account(|account| {
               account.current_wallet = self.wallet_select.wallet.clone();
            });
         }
      });
   }
}
