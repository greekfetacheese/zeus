use crate::gui::GUI;
use egui::{Align, Layout, RichText, Spinner, Ui, vec2};

const DATA_SYNCING_MSG: &str = "Zeus is still syncing important data, do not close the app yet!";
const DEX_SYNCING_MSG: &str = "Zeus is still syncing DEX data, do not close the app yet!";
const ON_STARTUP_SYNC_MSG: &str = "Zeus is syncing your wallets state, do not close the app yet!";
const VAULT_SAVE_IN_PROGRESS_MSG: &str = "Saving vault in progress, do not close the app yet!";
const WALLET_DISCOVERY_IN_PROGRESS_MSG: &str =
   "Wallet discovery in progress, do not close the app yet!";

pub fn show(gui: &mut GUI, ui: &mut Ui) {
   let ctx = gui.ctx.clone();
   let data_syncing = ctx.read(|ctx| ctx.data_syncing);
   let dex_syncing = ctx.read(|ctx| ctx.dex_syncing);
   let on_startup_syncing = ctx.read(|ctx| ctx.on_startup_syncing);
   let icons = gui.icons.clone();
   let theme = &gui.theme;
   let frame = theme.frame2;

   ui.spacing_mut().item_spacing = vec2(0.0, 10.0);
   ui.spacing_mut().button_padding = vec2(10.0, 8.0);

   ui.horizontal(|ui| {
      ui.vertical(|ui| {
         gui.header.show(ctx.clone(), theme, icons.clone(), ui);
      });

      // ui.add_space(240.0);

      gui.notification.show(&gui.theme, icons, ui);

      if data_syncing && !on_startup_syncing {
         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            frame.show(ui, |ui| {
               ui.label(RichText::new(DATA_SYNCING_MSG).size(theme.text_sizes.normal));
               ui.add_space(10.0);
               ui.add(Spinner::new().size(20.0));
            });
         });
      }

      if dex_syncing && !data_syncing && !on_startup_syncing {
         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            frame.show(ui, |ui| {
               ui.label(RichText::new(DEX_SYNCING_MSG).size(theme.text_sizes.normal));
               ui.add_space(10.0);
               ui.add(Spinner::new().size(20.0));
            });
         });
      }

      if on_startup_syncing && !data_syncing {
         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            frame.show(ui, |ui| {
               ui.label(RichText::new(ON_STARTUP_SYNC_MSG).size(theme.text_sizes.normal));
               ui.add_space(10.0);
               ui.add(Spinner::new().size(20.0));
            });
         });
      }

      if ctx.wallet_discovery_in_progress() {
         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            frame.show(ui, |ui| {
               ui.label(
                  RichText::new(WALLET_DISCOVERY_IN_PROGRESS_MSG).size(theme.text_sizes.normal),
               );
               ui.add_space(10.0);
               ui.add(Spinner::new().size(20.0));
            });
         });
      }

      if ctx.save_vault_in_progress() {
         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            frame.show(ui, |ui| {
               ui.label(RichText::new(VAULT_SAVE_IN_PROGRESS_MSG).size(theme.text_sizes.normal));
               ui.add_space(10.0);
               ui.add(Spinner::new().size(20.0));
            });
         });
      }
   });
}
