use crate::gui::GUI;
use egui::{Align, Layout, Margin, RichText, Spinner, Ui, vec2};
use zeus_widgets::{Button, Label};

const DATA_SYNCING_MSG: &str = "Zeus is still syncing important data";
const DEX_SYNCING_MSG: &str = "Zeus is still syncing DEX data";
const ON_STARTUP_SYNC_MSG: &str = "Zeus is syncing your wallets state";
const VAULT_SAVE_IN_PROGRESS_MSG: &str = "Saving vault in progress, do not close the app yet!";

const AVAILABLE_RPCS_CHECK_THRESHOLD: u128 = 500;

pub fn show(gui: &mut GUI, ui: &mut Ui) {
   let ctx = gui.ctx.clone();

   let (
      save_vault_in_progress,
      data_syncing,
      dex_syncing,
      on_startup_syncing,
      has_available_rpcs,
      chain,
   ) = ctx.write(|ctx| {
      let chain = ctx.chain;
      (
         ctx.save_vault_in_progress,
         ctx.data_syncing,
         ctx.dex_syncing,
         ctx.on_startup_syncing,
         ctx.check_for_available_rpcs(chain.id(), AVAILABLE_RPCS_CHECK_THRESHOLD),
         chain,
      )
   });

   let icons = gui.icons.clone();
   let theme = &gui.theme;

   let frame = theme.frame1.outer_margin(Margin::same(10));

   ui.spacing_mut().item_spacing = vec2(0.0, 10.0);
   ui.spacing_mut().button_padding = vec2(10.0, 8.0);

   let available_width = ui.available_width();

   ui.horizontal(|ui| {
      ui.vertical(|ui| {
         gui.header.show(ctx.clone(), theme, icons.clone(), ui);
      });

      if !has_available_rpcs {
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.set_width(available_width * 0.6);

            frame.show(ui, |ui| {
               ui.set_max_height(50.0);

               ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                  let text = format!(
                     "No functional or enabled RPC for the {} network",
                     chain.name()
                  );
                  let rich_text = RichText::new(text).size(theme.text_sizes.normal);
                  ui.add(Label::new(rich_text, None).interactive(false));

                  ui.add_space(10.0);

                  let text = RichText::new("Open Network Settings").size(theme.text_sizes.normal);
                  let button = Button::new(text).visuals(theme.button_visuals());
                  if ui.add(button).clicked() {
                     gui.settings.open_network_settings();
                  }
               });
            });
         });
      }

      gui.notification.show(&gui.theme, icons, ui);

      if data_syncing && !on_startup_syncing {
         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            frame.show(ui, |ui| {
               ui.label(RichText::new(DATA_SYNCING_MSG).size(theme.text_sizes.normal));
               ui.add_space(10.0);
               ui.add(Spinner::new().size(20.0).color(theme.colors.text));
            });
         });
      }

      if dex_syncing && !data_syncing && !on_startup_syncing {
         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            frame.show(ui, |ui| {
               ui.label(RichText::new(DEX_SYNCING_MSG).size(theme.text_sizes.normal));
               ui.add_space(10.0);
               ui.add(Spinner::new().size(20.0).color(theme.colors.text));
            });
         });
      }

      if on_startup_syncing && !data_syncing {
         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            frame.show(ui, |ui| {
               ui.label(RichText::new(ON_STARTUP_SYNC_MSG).size(theme.text_sizes.normal));
               ui.add_space(10.0);
               ui.add(Spinner::new().size(20.0).color(theme.colors.text));
            });
         });
      }

      if save_vault_in_progress {
         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            frame.show(ui, |ui| {
               ui.label(RichText::new(VAULT_SAVE_IN_PROGRESS_MSG).size(theme.text_sizes.normal));
               ui.add_space(10.0);
               ui.add(Spinner::new().size(20.0).color(theme.colors.text));
            });
         });
      }
   });
}
