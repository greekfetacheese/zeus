use crate::core::ZeusContext;
use crate::gui::{GUI, SHARED_GUI};
use crate::utils::RT;
use egui::{Align, Layout, Margin, RichText, Spinner, Ui, vec2};
use zeus_widgets::{Button, Label};

use std::time::{SystemTime, UNIX_EPOCH};

const DATA_SYNCING_MSG: &str = "Zeus is still syncing important data";
const DEX_SYNCING_MSG: &str = "Zeus is still syncing DEX data";
const ON_STARTUP_SYNC_MSG: &str = "Zeus is syncing your wallets state";
const VAULT_SAVE_IN_PROGRESS_MSG: &str = "Saving vault in progress, do not close the app yet!";
const RAILGUN_SYNCING_MSG: &str = "Railgun state sync in progress, do not close the app yet!";

const AVAILABLE_RPCS_CHECK_THRESHOLD: u128 = 100;
const RAILGUN_CHECK_THRESHOLD: u128 = 250;

pub fn show(gui: &mut GUI, ctx: &mut ZeusContext, ui: &mut Ui) {
   let chain = ctx.chain;

   let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis();

   let has_available_rpcs =
      ctx.check_for_available_rpcs(now, chain.id(), AVAILABLE_RPCS_CHECK_THRESHOLD);

   let should_check_railgun_provider_sync =
      ctx.should_check_railgun_provider_sync(now, chain.id(), RAILGUN_CHECK_THRESHOLD);

   if should_check_railgun_provider_sync {
      check_railgun(chain.id());
   }

   let icons = gui.icons.clone();
   let theme = &gui.theme;

   let frame = theme.frame1.outer_margin(Margin::same(10));

   ui.spacing_mut().item_spacing = vec2(0.0, 10.0);
   ui.spacing_mut().button_padding = vec2(10.0, 8.0);

   let available_width = ui.available_width();

   ui.horizontal(|ui| {
      ui.vertical(|ui| {
         gui.header.show(ctx, theme, icons.clone(), ui);
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

      let is_railgun_syncing = ctx.is_railgun_provider_syncing(chain.id());

      let status_msg = if ctx.data_syncing {
         Some(DATA_SYNCING_MSG)
      } else if is_railgun_syncing {
         Some(RAILGUN_SYNCING_MSG)
      } else if ctx.dex_syncing {
         Some(DEX_SYNCING_MSG)
      } else if ctx.on_startup_syncing {
         Some(ON_STARTUP_SYNC_MSG)
      } else if ctx.save_vault_in_progress {
         Some(VAULT_SAVE_IN_PROGRESS_MSG)
      } else {
         None
      };

      if let Some(msg) = status_msg {
         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            frame.show(ui, |ui| {
               ui.label(RichText::new(msg).size(theme.text_sizes.normal));
               ui.add_space(10.0);
               ui.add(Spinner::new().size(20.0).color(theme.colors.text));
            });
         });
      }
   });
}

fn check_railgun(chain: u64) {
   RT.spawn(async move {
      let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
      if !ctx.railgun_is_supported(chain.into()) {
         return;
      }

      let railgun_provider = match ctx.get_railgun_provider(chain).await {
         Ok(provider) => provider,
         Err(e) => {
            tracing::error!("Error getting Railgun provider: {:?}", e);
            return;
         }
      };

      let is_syncing = railgun_provider.is_syncing().await;

      ctx.write(|ctx| {
         ctx.railgun_provider_syncing.insert(chain, is_syncing);
      });
   });
}
