use crate::core::{PoolManagerHandle, context::ZeusCtx, utils::RT};
use crate::gui::{SHARED_GUI, Theme};
use egui::{Button, Color32, RichText, ScrollArea, Spinner, Ui, vec2};
use std::{collections::HashMap, path::PathBuf};
use zeus_eth::{
   amm::uniswap::{DexKind, sync::Checkpoint},
   types::*,
};

const POOL_DATA_FILTERED: &str = "pool_data_filtered.json";
const POOL_DATA_FULL: &str = "pool_data_full.json";

pub struct SyncPoolsUi {
   open: bool,
   pub syncing: bool,
   pub updating_state: bool,
   pool_manager: Option<PoolManagerHandle>,
   checkpoints: Vec<Checkpoint>,
   dropped_file_path: Option<PathBuf>,
   v2_pools_len: HashMap<u64, usize>,
   v3_pools: HashMap<u64, usize>,
   v4_pools: HashMap<u64, usize>,
}

impl SyncPoolsUi {
   pub fn new() -> Self {
      Self {
         open: false,
         syncing: false,
         updating_state: false,
         pool_manager: None,
         checkpoints: Vec::new(),
         dropped_file_path: None,
         v2_pools_len: HashMap::new(),
         v3_pools: HashMap::new(),
         v4_pools: HashMap::new(),
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self) {
      self.open = true;
   }

   pub fn close(&mut self) {
      self.open = false;
   }

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      if !self.open {
         return;
      }

      ui.vertical_centered(|ui| {
         ui.set_width(ui.available_width() * 0.8);
         ui.spacing_mut().item_spacing.y = 10.0;
         ui.spacing_mut().button_padding = vec2(10.0, 8.0);

         // Collect dropped file
         ui.ctx().input(|i| {
            if let Some(dropped_file) = i.raw.dropped_files.first() {
               let path = dropped_file.path.clone();
               self.dropped_file_path = path.clone();

               RT.spawn_blocking(move || {
                  let path = path.unwrap();
                  let manager = PoolManagerHandle::from_dir(&path).unwrap();
                  let checkpoints = manager.get_all_checkpoints();
                  let mut v2_pools_len = HashMap::new();
                  let mut v3_pools_len = HashMap::new();
                  let mut v4_pools_len = HashMap::new();

                  for chain in ChainId::supported_chains() {
                     let v2_pools = manager.v2_pools_len(chain.id());
                     v2_pools_len.insert(chain.id(), v2_pools);

                     let v3_pools = manager.v3_pools_len(chain.id());
                     v3_pools_len.insert(chain.id(), v3_pools);

                     let v4_pools = manager.v4_pools_len(chain.id());
                     v4_pools_len.insert(chain.id(), v4_pools);
                  }

                  SHARED_GUI.write(|gui| {
                     gui.dev.sync_pools.pool_manager = Some(manager);
                     gui.dev.sync_pools.checkpoints = checkpoints;
                     gui.dev.sync_pools.v2_pools_len = v2_pools_len;
                     gui.dev.sync_pools.v3_pools = v3_pools_len;
                     gui.dev.sync_pools.v4_pools = v4_pools_len;
                  });
               });
            }
         });

         if let Some(path) = &self.dropped_file_path {
            ui.label(
               RichText::new(format!("Loaded Manager from: {}", path.display()))
                  .size(theme.text_sizes.normal),
            );
         }

         let text = RichText::new("Load Pool Manager to CTX").size(theme.text_sizes.normal);
         let button = Button::new(text);
         if ui.add(button).clicked() {
            let ctx_clone = ctx.clone();
            let manager = self.pool_manager.clone();
            RT.spawn_blocking(move || {
               if let Some(manager) = manager {
                  tracing::info!("Loaded pool manager to CTX");
                  ctx_clone.write(|ctx| {
                     ctx.pool_manager = manager;
                  });
               }
            });
         }

         let button = Button::new(RichText::new("Sync V4 Pool Data").size(theme.text_sizes.normal));
         let enabled = !self.syncing && !self.updating_state;

         if ui.add_enabled(enabled, button).clicked() {
            self.syncing = true;
            let ctx2 = ctx.clone();
            let manager = self.pool_manager.clone().unwrap();

            RT.spawn(async move {
               match sync_v4_pools(ctx2.clone(), manager).await {
                  Ok(_) => {
                     SHARED_GUI.write(|gui| {
                        gui.dev.sync_pools.syncing = false;
                     });
                  }
                  Err(e) => {
                     SHARED_GUI.write(|gui| {
                        gui.dev.sync_pools.syncing = false;
                     });
                     tracing::error!("Error syncing pools {:?}", e);
                  }
               }
            });
         }

         let button = Button::new(RichText::new("Cleanup Pools").size(theme.text_sizes.normal));
         if ui.add_enabled(enabled, button).clicked() {
            self.updating_state = true;
            let ctx2 = ctx.clone();
            let manager = self.pool_manager.clone().unwrap();

            RT.spawn(async move {
               match cleanup_pools(ctx2.clone(), manager).await {
                  Ok(_) => {
                     SHARED_GUI.write(|gui| {
                        gui.dev.sync_pools.updating_state = false;
                     });
                  }
                  Err(e) => {
                     SHARED_GUI.write(|gui| {
                        gui.dev.sync_pools.updating_state = false;
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

         ui.add_space(30.0);
         ui.horizontal(|ui| {
            ui.add_space(250.0);

            ui.vertical(|ui| {
               ui.visuals_mut().widgets.noninteractive.bg_stroke.width = 1.0;
               ui.set_width(ui.available_width() * 0.8);
               ui.set_height(400.0);

               ScrollArea::vertical().show(ui, |ui| {
                  for checkpoint in &self.checkpoints {
                     ui.separator();

                     ui.horizontal(|ui| {
                        let chain = ChainId::new(checkpoint.chain_id).unwrap();
                        let chain = format!("Chain: {}", chain.name());
                        ui.label(RichText::new(chain).size(theme.text_sizes.normal));
                     });

                     ui.horizontal(|ui| {
                        let dex = format!("Dex: {}", checkpoint.dex.as_str());
                        ui.label(RichText::new(dex).size(theme.text_sizes.normal));
                     });

                     ui.horizontal(|ui| {
                        let last_synced = format!("Last synced block: {}", checkpoint.block);
                        ui.label(RichText::new(last_synced).size(theme.text_sizes.normal));
                     });

                     ui.horizontal(|ui| {
                        let pools = self.v2_pools_len.get(&checkpoint.chain_id).unwrap_or(&0);
                        let pools = format!("V2 Pools: {}", pools);
                        ui.label(RichText::new(pools).size(theme.text_sizes.normal));
                     });

                     ui.horizontal(|ui| {
                        let pools = self.v3_pools.get(&checkpoint.chain_id).unwrap_or(&0);
                        let pools = format!("V3 Pools: {}", pools);
                        ui.label(RichText::new(pools).size(theme.text_sizes.normal));
                     });

                     ui.horizontal(|ui| {
                        let pools = self.v4_pools.get(&checkpoint.chain_id).unwrap_or(&0);
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

async fn cleanup_pools(ctx: ZeusCtx, manager: PoolManagerHandle) -> Result<(), anyhow::Error> {
   let mut tasks: Vec<tokio::task::JoinHandle<Result<(), anyhow::Error>>> = Vec::new();
   let current_dir = std::env::current_dir()?;
   let pool_data_dir = current_dir.join(POOL_DATA_FILTERED);

   manager.remove_v4_pools_with_no_base_token();
   manager.remove_v4_pools_with_high_fee();

   for chain in ChainId::supported_chains() {
      if !chain.is_ethereum() {
         continue;
      }

      let ctx = ctx.clone();
      let manager = manager.clone();

      let task = RT.spawn(async move {
         manager.update(ctx, chain.id()).await?;

         tracing::info!("Pool state is updated for chain {}", chain.id());
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

   manager.save_to_dir(&pool_data_dir)?;

   Ok(())
}

async fn sync_v4_pools(ctx: ZeusCtx, manager: PoolManagerHandle) -> Result<(), anyhow::Error> {
   let dex = DexKind::UniswapV4;
   let mut tasks: Vec<tokio::task::JoinHandle<Result<(), anyhow::Error>>> = Vec::new();

   let current_dir = std::env::current_dir()?;
   let pool_data_dir = current_dir.join(POOL_DATA_FULL);

   manager.write(|manager| {
      manager.ignore_chains.clear();
   });

   for chain in ChainId::supported_chains() {
      if chain.is_bsc() {
         continue;
      }

      let ctx = ctx.clone();
      let manager = manager.clone();
      let dir = Some(pool_data_dir.clone());
      let task = RT.spawn(async move {
         tracing::info!("Started syncing pools for chain {}", chain.id());
         manager.sync_pools(ctx.clone(), chain, dex, dir).await?;
         tracing::info!("Synced pools for chain {}", chain.id());

         Ok(())
      });
      tasks.push(task);
   }

   for task in tasks {
      match task.await {
         Ok(_) => {}
         Err(e) => tracing::error!("Error syncing pools: {:?}", e),
      }
   }

   manager.save_to_dir(&pool_data_dir)?;

   SHARED_GUI.write(|gui| {
      gui.dev.sync_pools.pool_manager = Some(manager.clone());
   });

   tracing::info!("Synced V4 pools!");

   Ok(())
}
