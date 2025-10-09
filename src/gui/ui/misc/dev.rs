use bincode::{config::standard, decode_from_slice, encode_to_vec};
use eframe::egui::{Align2, Button, Frame, Order, RichText, ScrollArea, Ui, Window, vec2};
use egui_widgets::Label;

use crate::assets::Icons;
use crate::core::{
   TransactionAnalysis, ZeusCtx,
   context::db::currencies::TokenData,
   transaction::*,
   utils::{RT, sign::SignMsgType},
};
use crate::gui::{SHARED_GUI, ui::notification::NotificationType};

use zeus_eth::{
   alloy_primitives::Address,
   amm::uniswap::{DexKind, UniswapPool},
   currency::ERC20Token,
   types::SUPPORTED_CHAINS,
   utils::NumericValue,
};

use super::sync::SyncPoolsUi;
use zeus_theme::Theme;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct DevUi {
   pub open: bool,
   pub sync_pools: SyncPoolsUi,
   pub ui_testing: UiTesting,
   pub loaded_token_data: Vec<TokenData>,
   pub filtering_tokens: bool,
   pub size: (f32, f32),
}

impl DevUi {
   pub fn new() -> Self {
      Self {
         open: false,
         sync_pools: SyncPoolsUi::new(),
         ui_testing: UiTesting::new(),
         loaded_token_data: Vec::new(),
         filtering_tokens: false,
         size: (550.0, 500.0),
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

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      if !self.open {
         return;
      }

      self.show_sync_pools(ctx.clone(), theme, ui);
      self.show_ui_testing(ctx.clone(), theme, icons, ui);

      ui.vertical_centered(|ui| {
         ui.set_width(self.size.0);
         ui.set_height(self.size.1);
         ui.spacing_mut().item_spacing = vec2(0.0, 20.0);

         let button_size = vec2(self.size.0, 50.0);
         let text_size = theme.text_sizes.normal;
         let header = RichText::new("Dev UI").size(theme.text_sizes.heading);

         ui.label(header);

         let button =
            Button::new(RichText::new("Ui Testing").size(text_size)).min_size(button_size);

         if ui.add(button).clicked() {
            self.ui_testing.open();
         }

         let button =
            Button::new(RichText::new("Sync Pools").size(text_size)).min_size(button_size);

         if ui.add(button).clicked() {
            self.sync_pools.open();
         }

         let button =
            Button::new(RichText::new("Filter Tokens").size(text_size)).min_size(button_size);

         if ui.add(button).clicked() {
            self.filtering_tokens = true;
            let tokens = self.loaded_token_data.clone();
            RT.spawn(async move {
               SHARED_GUI.write(|gui| {
                  gui.loading_window.open("Filtering Tokens");
               });

               match filter_tokens(ctx.clone(), tokens).await {
                  Ok(_) => {
                     SHARED_GUI.write(|gui| {
                        gui.dev.filtering_tokens = false;
                        gui.loading_window.reset();
                     });
                  }
                  Err(e) => {
                     tracing::error!("Error filtering tokens: {:?}", e);
                     SHARED_GUI.write(|gui| {
                        gui.dev.filtering_tokens = false;
                        gui.loading_window.reset();
                     });
                  }
               }
            });
         }

         ui.ctx().input(|i| {
            if let Some(dropped_file) = i.raw.dropped_files.first() {
               let path = dropped_file.path.clone();

               RT.spawn_blocking(move || {
                  if let Some(path) = path {
                     let data = std::fs::read(path).unwrap();
                     let (token_data, _b): (Vec<TokenData>, usize) =
                        decode_from_slice(&data, standard()).unwrap();

                     tracing::info!("Loaded {} tokens", token_data.len());
                     SHARED_GUI.write(|gui| {
                        gui.dev.loaded_token_data = token_data;
                     });
                  }
               });
            }
         });
      });
   }

   fn show_sync_pools(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      let mut open = self.sync_pools.is_open();
      let title = RichText::new("Sync Pools").size(theme.text_sizes.heading);
      Window::new(title)
         .open(&mut open)
         .resizable(false)
         .collapsible(false)
         .order(Order::Foreground)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            self.sync_pools.show(ctx.clone(), theme, ui);
         });

      if !open {
         self.sync_pools.close();
      }
   }

   fn show_ui_testing(&mut self, ctx: ZeusCtx, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      let mut open = self.ui_testing.is_open();
      let title = RichText::new("Ui Testing").size(theme.text_sizes.heading);
      Window::new(title)
         .open(&mut open)
         .resizable(false)
         .collapsible(false)
         .order(Order::Foreground)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            self.ui_testing.show(ctx.clone(), theme, icons, ui);
         });

      if !open {
         self.ui_testing.close();
      }
   }
}

async fn filter_tokens(ctx: ZeusCtx, mut token_data: Vec<TokenData>) -> Result<(), anyhow::Error> {
   if token_data.is_empty() {
      RT.spawn_blocking(move || {
         SHARED_GUI.write(|gui| {
            gui.open_msg_window("Token Data is empty", "");
         });
      });
      return Ok(());
   }

   let mut good_tokens = Vec::new();

   for chain in SUPPORTED_CHAINS {
      if chain == 56 {
         continue;
      }

      if chain == 1 {
         ctx.write(|ctx| ctx.pool_manager.set_concurrency(4));
      } else {
         ctx.write(|ctx| ctx.pool_manager.set_concurrency(2));
      }

      tracing::info!("Processing ChainId {}", chain);
      let manager = ctx.pool_manager();
      let tokens = token_data.iter().filter(|token| token.chain_id == chain);

      let mut erc20_tokens = Vec::new();
      let _dexes = DexKind::main_dexes(chain);

      for token in tokens {
         let address = Address::from_str(&token.address)?;

         let erc20 = ERC20Token {
            chain_id: chain,
            address,
            name: token.name.clone(),
            symbol: token.symbol.clone(),
            decimals: token.decimals,
            total_supply: Default::default(),
         };

         erc20_tokens.push(erc20);
      }

      tracing::info!(
         "Syncing pools for {} tokens on ChainId {}",
         erc20_tokens.len(),
         chain
      );

      /*
      match manager.sync_pools_for_tokens(ctx.clone(), erc20_tokens.clone(), dexes).await {
         Ok(_) => {
            tracing::info!("Synced pools for ChainId {}", chain);
         }
         Err(e) => {
            tracing::error!("Failed to sync pools for ChainId {}: {}", chain, e);
         }
      }
      */

      let currencies = erc20_tokens.iter().map(|t| t.clone().into()).collect();
      match manager.update_for_currencies(ctx.clone(), chain, currencies).await {
         Ok(_) => {
            tracing::info!("Updated pools for ChainId {}", chain);
         }
         Err(e) => {
            tracing::error!(
               "Failed to update pools for ChainId {}: {}",
               chain,
               e
            );
         }
      }

      for token in erc20_tokens {
         let pools = manager.get_pools_that_have_currency(&token.clone().into());

         if pools.is_empty() {
            tracing::info!(
               "No pools found for token {}, skipping",
               token.symbol
            );
            continue;
         }

         for pool in &pools {
            if pool.state().is_none() {
               tracing::info!("Pool for token {} is not synced", token.symbol);
               continue;
            }

            let threshold = 50_000.0;
            let base_balance = pool.base_balance();
            let base_price = ctx.get_token_price(&pool.base_currency().to_erc20());
            let base_value = NumericValue::value(base_balance.f64(), base_price.f64());

            if base_value.f64() >= threshold {
               tracing::info!(
                  "Token {} is good, found at least 1 pool",
                  token.symbol
               );
               good_tokens.push(token.clone());
               break;
            } else {
               tracing::info!("Token {} is not good", token.symbol);
            }
         }
      }
   }

   tracing::info!("Found {} good tokens", good_tokens.len());

   // Save the good tokens
   token_data.retain(|token| {
      good_tokens.contains(&ERC20Token {
         chain_id: token.chain_id,
         address: Address::from_str(&token.address).unwrap(),
         name: token.name.clone(),
         symbol: token.symbol.clone(),
         decimals: token.decimals,
         total_supply: Default::default(),
      })
   });

   let file_name = "token_data.data";
   let data = encode_to_vec(&token_data, standard())?;
   let dir = std::env::current_dir()?;
   std::fs::write(dir.join(file_name), data)?;
   ctx.save_pool_manager();
   tracing::info!("Token data saved successfully!");

   Ok(())
}

pub struct UiTesting {
   open: bool,
   icon_window_open: bool,
   size: (f32, f32),
}

impl UiTesting {
   pub fn new() -> Self {
      Self {
         open: false,
         icon_window_open: false,
         size: (500.0, 400.0),
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

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      if !self.open {
         return;
      }

      self.icon_window(ctx.clone(), theme, icons, ui);

      ui.vertical_centered(|ui| {
         ui.set_width(self.size.0);
         ui.set_height(self.size.1);
         ui.spacing_mut().item_spacing.y = 20.0;
         let button_size = vec2(self.size.0, 50.0);

         let text_size = theme.text_sizes.normal;

         ScrollArea::vertical().show(ui, |ui| {

            let button = Button::new(RichText::new("Icon Window").size(text_size)).min_size(button_size);
            if ui.add(button).clicked() {
               self.icon_window_open = true;
            }

            let button =
               Button::new(RichText::new("Swap Notification With Progress Bar").size(text_size))
                  .min_size(button_size);

            let dummy_swap = DecodedEvent::dummy_swap().clone();
            let dummy_bridge = DecodedEvent::dummy_bridge().clone();
            let dummy_transfer = DecodedEvent::dummy_transfer().clone();
            let dummy_approval = DecodedEvent::dummy_token_approve().clone();

            let swap_title = dummy_swap.name();
            let bridge_title = dummy_bridge.name();
            let transfer_title = dummy_transfer.name();
            let approval_title = dummy_approval.name();

            let swap_notification = NotificationType::from_main_event(dummy_swap);
            let bridge_notification = NotificationType::from_main_event(dummy_bridge);
            let transfer_notification = NotificationType::from_main_event(dummy_transfer);
            let approval_notification = NotificationType::from_main_event(dummy_approval);

            if ui.add(button).clicked() {
               let title = swap_title.clone();
               let notification_clone = swap_notification.clone();
               RT.spawn_blocking(move || {
                  let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
                  let finish_on = now + 5;
                  SHARED_GUI.write(|gui| {
                     gui.notification.open_with_progress_bar(
                        now,
                        finish_on,
                        title,
                        notification_clone,
                        None,
                     );
                  });
               });
            }

            let button =
               Button::new(RichText::new("Swap Notification With Spinner").size(text_size))
                  .min_size(button_size);

            if ui.add(button).clicked() {
               RT.spawn_blocking(move || {
                  SHARED_GUI.write(|gui| {
                     gui.notification.open_with_spinner(swap_title, swap_notification);
                  });
               });
            }

            let button =
               Button::new(RichText::new("Bridge Notification With Progress Bar").size(text_size))
                  .min_size(button_size);

            if ui.add(button).clicked() {
               let notification_clone = bridge_notification.clone();
               let title = bridge_title.clone();
               RT.spawn_blocking(move || {
                  let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
                  let finish_on = now + 5;
                  SHARED_GUI.write(|gui| {
                     gui.notification.open_with_progress_bar(
                        now,
                        finish_on,
                        title,
                        notification_clone,
                        None,
                     );
                  });
               });
            }

            let button =
               Button::new(RichText::new("Bridge Notification With Spinner").size(text_size))
                  .min_size(button_size);

            if ui.add(button).clicked() {
               RT.spawn_blocking(move || {
                  SHARED_GUI.write(|gui| {
                     gui.notification.open_with_spinner(bridge_title, bridge_notification);
                  });
               });
            }

            let button = Button::new(
               RichText::new("Transfer Notification With Progress Bar").size(text_size),
            )
            .min_size(button_size);

            if ui.add(button).clicked() {
               let notification_clone = transfer_notification.clone();
               RT.spawn_blocking(move || {
                  let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
                  let finish_on = now + 5;
                  SHARED_GUI.write(|gui| {
                     gui.notification.open_with_progress_bar(
                        now,
                        finish_on,
                        transfer_title,
                        notification_clone,
                        None,
                     );
                  });
               });
            }

            let button = Button::new(RichText::new("Approval with Progress Bar").size(text_size))
               .min_size(button_size);

            if ui.add(button).clicked() {
               let notification_clone = approval_notification.clone();
               RT.spawn_blocking(move || {
                  let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
                  let finish_on = now + 5;
                  SHARED_GUI.write(|gui| {
                     gui.notification.open_with_progress_bar(
                        now,
                        finish_on,
                        approval_title,
                        notification_clone,
                        None,
                     );
                  });
               });
            }

            let button =
               Button::new(RichText::new("Data Syncing").size(text_size)).min_size(button_size);

            if ui.add(button).clicked() {
               ctx.write(|ctx| ctx.data_syncing = !ctx.data_syncing);
            }

            let button = Button::new(RichText::new("On Startup Syncing").size(text_size))
               .min_size(button_size);

            if ui.add(button).clicked() {
               ctx.write(|ctx| ctx.on_startup_syncing = !ctx.on_startup_syncing);
            }

            let button =
               Button::new(RichText::new("EOA Delegate").size(text_size)).min_size(button_size);

            if ui.add(button).clicked() {
               let analysis = TransactionAnalysis::dummy_eoa_delegate();
               let ctx_clone = ctx.clone();
               RT.spawn_blocking(move || {
                  SHARED_GUI.write(|gui| {
                     gui.tx_confirmation_window.open(
                        ctx_clone.clone(),
                        "".to_string(),
                        ctx_clone.chain(),
                        analysis,
                        "1".to_string(),
                        true,
                     );
                  });
               });
            }

            let button = Button::new(RichText::new("Unknown Tx Analysis").size(text_size))
               .min_size(button_size);

            if ui.add(button).clicked() {
               let ctx_clone = ctx.clone();

               RT.spawn_blocking(move || {
                  let analysis = TransactionAnalysis::unknown_tx_1();
                  SHARED_GUI.write(|gui| {
                     gui.tx_confirmation_window.open(
                        ctx_clone.clone(),
                        "".to_string(),
                        ctx_clone.chain(),
                        analysis,
                        "1".to_string(),
                        true,
                     );
                  });
               });
            }

            let button = Button::new(RichText::new("Wrap ETH Analysis").size(text_size))
               .min_size(button_size);

            if ui.add(button).clicked() {
               let ctx_clone = ctx.clone();

               RT.spawn_blocking(move || {
                  let analysis = TransactionAnalysis::dummy_wrap_eth();
                  SHARED_GUI.write(|gui| {
                     gui.tx_confirmation_window.open(
                        ctx_clone.clone(),
                        "".to_string(),
                        ctx_clone.chain(),
                        analysis,
                        "1".to_string(),
                        true,
                     );
                  });
               });
            }

            let button = Button::new(RichText::new("Unwrap WETH Analysis").size(text_size))
               .min_size(button_size);

            if ui.add(button).clicked() {
               let ctx_clone = ctx.clone();

               RT.spawn_blocking(move || {
                  let analysis = TransactionAnalysis::dummy_unwrap_weth();
                  SHARED_GUI.write(|gui| {
                     gui.tx_confirmation_window.open(
                        ctx_clone.clone(),
                        "".to_string(),
                        ctx_clone.chain(),
                        analysis,
                        "1".to_string(),
                        true,
                     );
                  });
               });
            }

            let button =
               Button::new(RichText::new("Swap Tx Analysis").size(text_size)).min_size(button_size);

            if ui.add(button).clicked() {
               let ctx_clone = ctx.clone();

               RT.spawn_blocking(move || {
                  let analysis = TransactionAnalysis::dummy_swap();
                  SHARED_GUI.write(|gui| {
                     gui.tx_confirmation_window.open(
                        ctx_clone.clone(),
                        "".to_string(),
                        ctx_clone.chain(),
                        analysis,
                        "1".to_string(),
                        true,
                     );
                  });
               });
            }

            let button = Button::new(RichText::new("Transfer Analysis").size(text_size))
               .min_size(button_size);

            if ui.add(button).clicked() {
               let ctx_clone = ctx.clone();
               RT.spawn_blocking(move || {
                  let analysis = TransactionAnalysis::dummy_transfer();
                  SHARED_GUI.write(|gui| {
                     gui.tx_confirmation_window.open(
                        ctx_clone.clone(),
                        "".to_string(),
                        ctx_clone.chain(),
                        analysis,
                        "1".to_string(),
                        true,
                     );
                  });
               });
            }

            let button = Button::new(RichText::new("ERC20 Transfer Analysis").size(text_size))
               .min_size(button_size);

            if ui.add(button).clicked() {
               let ctx_clone = ctx.clone();
               RT.spawn_blocking(move || {
                  let analysis = TransactionAnalysis::dummy_erc20_transfer();
                  SHARED_GUI.write(|gui| {
                     gui.tx_confirmation_window.open(
                        ctx_clone.clone(),
                        "".to_string(),
                        ctx_clone.chain(),
                        analysis,
                        "1".to_string(),
                        true,
                     );
                  });
               });
            }

            let button = Button::new(RichText::new("ERC20 Approval Analysis").size(text_size))
               .min_size(button_size);

            if ui.add(button).clicked() {
               let ctx_clone = ctx.clone();
               RT.spawn_blocking(move || {
                  let analysis = TransactionAnalysis::dummy_token_approval();
                  SHARED_GUI.write(|gui| {
                     gui.tx_confirmation_window.open(
                        ctx_clone.clone(),
                        "".to_string(),
                        ctx_clone.chain(),
                        analysis,
                        "1".to_string(),
                        true,
                     );
                  });
               });
            }

            let button =
               Button::new(RichText::new("Permit Analysis").size(text_size)).min_size(button_size);
            if ui.add(button).clicked() {
               let ctx_clone = ctx.clone();
               RT.spawn_blocking(move || {
                  let analysis = TransactionAnalysis::dummy_permit();
                  SHARED_GUI.write(|gui| {
                     gui.tx_confirmation_window.open(
                        ctx_clone.clone(),
                        "".to_string(),
                        ctx_clone.chain(),
                        analysis,
                        "1".to_string(),
                        true,
                     );
                  });
               });
            }

            let button =
               Button::new(RichText::new("Uniswap AddLiquidity V3 Analysis").size(text_size))
                  .min_size(button_size);
            if ui.add(button).clicked() {
               let ctx_clone = ctx.clone();
               RT.spawn_blocking(move || {
                  let analysis = TransactionAnalysis::dummy_uniswap_position_operation();
                  SHARED_GUI.write(|gui| {
                     gui.tx_confirmation_window.open(
                        ctx_clone.clone(),
                        "".to_string(),
                        ctx_clone.chain(),
                        analysis,
                        "1".to_string(),
                        true,
                     );
                  });
               });
            }

            let button =
               Button::new(RichText::new("Bridge Analysis").size(text_size)).min_size(button_size);

            if ui.add(button).clicked() {
               let ctx_clone = ctx.clone();
               RT.spawn_blocking(move || {
                  let analysis = TransactionAnalysis::dummy_bridge();
                  SHARED_GUI.write(|gui| {
                     gui.tx_confirmation_window.open(
                        ctx_clone.clone(),
                        "".to_string(),
                        ctx_clone.chain(),
                        analysis,
                        "1".to_string(),
                        true,
                     );
                  });
               });
            }

            let button =
               Button::new(RichText::new("Sign Message").size(text_size)).min_size(button_size);

            if ui.add(button).clicked() {
               RT.spawn_blocking(move || {
                  let msg = SignMsgType::dummy_permit2();
                  SHARED_GUI.write(|gui| {
                     gui.sign_msg_window.open(
                        ctx.clone(),
                        "app.uniswap.org".to_string(),
                        8453,
                        msg,
                     );
                  });
               });
            }
         });
      });
   }

   fn icon_window(&mut self, _ctx: ZeusCtx, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      let mut open = self.icon_window_open;

      let title = RichText::new("Icons").size(theme.text_sizes.heading);
      Window::new(title)
         .open(&mut open)
         .resizable(false)
         .collapsible(false)
         .order(Order::Foreground)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);
            ui.spacing_mut().item_spacing = vec2(0.0, 20.0);

            let weth = ERC20Token::weth();
            let dai = ERC20Token::dai();

            let time = std::time::Instant::now();
            let weth_with_tint = icons.token_icon_x32(weth.address, weth.chain_id, true);
            tracing::info!("Token Icon With Tint took {} μs", time.elapsed().as_micros());

            let time = std::time::Instant::now();
            let weth_without_tint = icons.token_icon_x32(weth.address, weth.chain_id, false);
            tracing::info!("Token Icon Without Tint took {} μs", time.elapsed().as_micros());
            
            let dai_with_tint = icons.token_icon_x32(dai.address, dai.chain_id, true);
            let dai_without_tint = icons.token_icon_x32(dai.address, dai.chain_id, false);

            let label = Label::new("With tint", Some(weth_with_tint));
            ui.add(label);

            let label = Label::new("Without tint", Some(weth_without_tint));
            ui.add(label);

            let label = Label::new("With tint", Some(dai_with_tint));
            ui.add(label);

            let label = Label::new("Without tint", Some(dai_without_tint));
            ui.add(label);
         });

         self.icon_window_open = open;
   }
}
