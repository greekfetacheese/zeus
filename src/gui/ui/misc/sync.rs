use crate::core::{
   context::ZeusCtx,
   utils::{RT, pool_data_dir, pool_data_full_dir},
};
use crate::gui::{SHARED_GUI, Theme};
use egui::{Button, Color32, RichText, ScrollArea, Spinner, Ui, vec2};
use zeus_eth::{
   amm::{DexKind, pool_manager::PoolManagerHandle},
   types::*,
};

pub struct SyncPoolsUi {
   pub open: bool,
   pub syncing: bool,
   pub updating_state: bool,
}

impl SyncPoolsUi {
   pub fn new() -> Self {
      Self {
         open: false,
         syncing: false,
         updating_state: false,
      }
   }

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      if !self.open {
         return;
      }

      ui.vertical_centered(|ui| {
         ui.set_width(ui.available_width() * 0.8);
         ui.spacing_mut().item_spacing.y = 10.0;
         ui.spacing_mut().button_padding = vec2(10.0, 8.0);

         let load_btn = Button::new(RichText::new("Load Pool Data").size(theme.text_sizes.normal));
         if ui.add(load_btn).clicked() {
            let manager = ctx.pool_manager();
            RT.spawn_blocking(move || {
               match load_pools(manager) {
                  Ok(_) => {}
                  Err(e) => {
                     SHARED_GUI.write(|gui| {
                        gui.msg_window
                           .open("Failed to load pool data", e.to_string());
                     });
                     return;
                  }
               };
            });
         }

         let button = Button::new(RichText::new("Sync Pool Data").size(theme.text_sizes.normal));
         let enabled = !self.syncing && !self.updating_state;
         if ui.add_enabled(enabled, button).clicked() {
            self.syncing = true;
            let ctx2 = ctx.clone();
            RT.spawn(async move {
               match sync_v4_pools(ctx2.clone()).await {
                  Ok(_) => {
                     SHARED_GUI.write(|gui| {
                        gui.sync_pools_ui.syncing = false;
                     });
                  }
                  Err(e) => {
                     SHARED_GUI.write(|gui| {
                        gui.sync_pools_ui.syncing = false;
                     });
                     tracing::error!("Error syncing pools {:?}", e);
                  }
               }
            });
         }

         let button =
            Button::new(RichText::new("Update and Cleanup Pools").size(theme.text_sizes.normal));
         if ui.add_enabled(enabled, button).clicked() {
            self.updating_state = true;
            let ctx2 = ctx.clone();
            RT.spawn(async move {
               match update_and_cleanup_pools(ctx2.clone()).await {
                  Ok(_) => {
                     SHARED_GUI.write(|gui| {
                        gui.sync_pools_ui.updating_state = false;
                     });
                  }
                  Err(e) => {
                     SHARED_GUI.write(|gui| {
                        gui.sync_pools_ui.updating_state = false;
                     });
                     tracing::error!("Error updating pool state {:?}", e);
                  }
               }
            });
         }

         if self.syncing {
            ui.label(RichText::new("Syncing Pools...").size(theme.text_sizes.normal));
            ui.add(Spinner::new().size(17.0).color(Color32::WHITE));
         }

         if self.updating_state {
            ui.label(RichText::new("Updating Pool State...").size(theme.text_sizes.normal));
            ui.add(Spinner::new().size(17.0).color(Color32::WHITE));
         }

         let manager = ctx.pool_manager();

         let checkpoints = manager.get_all_checkpoints();

         ui.add_space(30.0);
         ui.horizontal(|ui| {
            ui.add_space(250.0);

            ui.vertical(|ui| {
               ui.visuals_mut().widgets.noninteractive.bg_stroke.width = 1.0;
               ui.set_width(ui.available_width() * 0.8);
               ui.set_height(400.0);

               ScrollArea::vertical().show(ui, |ui| {
                  for checkpoint in checkpoints {
                     let ctx = ctx.clone();
                     ui.separator();

                     let button =
                        Button::new(RichText::new("Delete").size(theme.text_sizes.normal));
                     if ui.add(button).clicked() {
                        manager.remove_checkpoint(checkpoint.chain_id, checkpoint.dex);
                        RT.spawn_blocking(move || {
                           let _ = ctx.save_pool_manager();
                        });
                     }

                     ui.horizontal(|ui| {
                        let chain = ChainId::new(checkpoint.chain_id).unwrap();
                        let chain = format!("Chain: {}", chain.name());
                        ui.label(RichText::new(chain).size(theme.text_sizes.normal));
                     });

                     ui.horizontal(|ui| {
                        let dex = format!("Dex: {}", checkpoint.dex.to_str());
                        ui.label(RichText::new(dex).size(theme.text_sizes.normal));
                     });

                     ui.horizontal(|ui| {
                        let last_synced = format!("Last synced block: {}", checkpoint.block);
                        ui.label(RichText::new(last_synced).size(theme.text_sizes.normal));
                     });

                     ui.horizontal(|ui| {
                        let pools = manager.v2_pools_len(checkpoint.chain_id);
                        let pools = format!("V2 Pools: {}", pools);
                        ui.label(RichText::new(pools).size(theme.text_sizes.normal));
                     });

                     ui.horizontal(|ui| {
                        let pools = manager.v3_pools_len(checkpoint.chain_id);
                        let pools = format!("V3 Pools: {}", pools);
                        ui.label(RichText::new(pools).size(theme.text_sizes.normal));
                     });

                     ui.horizontal(|ui| {
                        let pools = manager.v4_pools_len(checkpoint.chain_id);
                        let pools = format!("V4 Pools: {}", pools);
                        ui.label(RichText::new(pools).size(theme.text_sizes.normal));
                     });
                  }
               });
            });
         });
      });
   }
}

async fn update_and_cleanup_pools(ctx: ZeusCtx) -> Result<(), anyhow::Error> {
   let mut tasks: Vec<tokio::task::JoinHandle<Result<(), anyhow::Error>>> = Vec::new();
   for chain in SUPPORTED_CHAINS {
      let ctx = ctx.clone();
      let task = RT.spawn(async move {
         let manager = ctx.pool_manager();
         let client = ctx.get_client(chain).await?;

         manager.update(client, chain).await?;

         tracing::info!("Pool state is updated for chain {}", chain);
         Ok(())
      });
      tasks.push(task);
   }

   for task in tasks {
      match task.await {
         Ok(_) => {}
         Err(e) => tracing::error!("Error updating pool state {:?}", e),
      }
   }

   let manager = ctx.pool_manager();
   manager.cleanup_pools();
   manager.save_to_dir(&pool_data_dir()?)?;

   Ok(())
}

async fn sync_v4_pools(ctx: ZeusCtx) -> Result<(), anyhow::Error> {
   let dex = DexKind::UniswapV4;
   let mut tasks: Vec<tokio::task::JoinHandle<Result<(), anyhow::Error>>> = Vec::new();

   for chain in ChainId::supported_chains() {
      if chain.is_bsc() {
         continue;
      }

      let ctx = ctx.clone();
      let task = RT.spawn(async move {
         let client = ctx.get_archive_client(chain.id()).await?;
         let manager = ctx.pool_manager();

         manager
            .sync_pools(client.clone(), chain.id(), vec![dex])
            .await?;

         Ok(())
      });
      tasks.push(task);
   }

   for task in tasks {
      let _ = task.await;
   }

   let manager = ctx.pool_manager();
   let manager_string = manager
      .to_string()
      .map_err(|e| anyhow::anyhow!("Failed to serialize pool manager: {:?}", e))?;
   std::fs::write(pool_data_full_dir()?, manager_string)?;

   Ok(())
}

fn load_pools(current_manager: PoolManagerHandle) -> Result<(), anyhow::Error> {
   let dir = pool_data_full_dir()?;
   let manager = PoolManagerHandle::from_dir(&dir)?;

   let mut all_pools = Vec::new();
   for chain in ChainId::supported_chains() {
      let pools = manager.get_pools_for_chain(chain.id());
      all_pools.extend(pools);
   }

   let checkpoints = manager.get_all_checkpoints();
   for checkpoint in checkpoints {
      current_manager.add_checkpoint(checkpoint.chain_id, checkpoint.dex, checkpoint);
   }

   current_manager.add_pools(all_pools);
   Ok(())
}
