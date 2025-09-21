use bincode::{config::standard, decode_from_slice, encode_to_vec};
use eframe::egui::{Align2, Button, Frame, Order, RichText, ScrollArea, Ui, Window, vec2};
use zeus_eth::amm::uniswap::DexKind;

use crate::assets::Icons;
use crate::core::{
   TransactionAnalysis, ZeusCtx,
   transaction::*,
   utils::{RT, sign::SignMsgType},
   context::db::currencies::TokenData,
};
use crate::gui::SHARED_GUI;

use zeus_eth::{
   alloy_primitives::Address, amm::uniswap::UniswapPool, currency::ERC20Token,
   types::SUPPORTED_CHAINS, utils::NumericValue,
};

use super::sync::SyncPoolsUi;
use egui_theme::Theme;

use std::str::FromStr;
use std::sync::Arc;


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

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, _icons: Arc<Icons>, ui: &mut Ui) {
      if !self.open {
         return;
      }

      self.show_sync_pools(ctx.clone(), theme, ui);
      self.show_ui_testing(ctx.clone(), theme, ui);

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

   fn show_ui_testing(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
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
            self.ui_testing.show(ctx.clone(), theme, ui);
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
   size: (f32, f32),
}

impl UiTesting {
   pub fn new() -> Self {
      Self {
         open: false,
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

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      if !self.open {
         return;
      }

      ui.vertical_centered(|ui| {
         ui.set_width(self.size.0);
         ui.set_height(self.size.1);
         ui.spacing_mut().item_spacing.y = 20.0;
         let button_size = vec2(self.size.0, 50.0);

         let text_size = theme.text_sizes.normal;
         let button =
            Button::new(RichText::new("Data Syncing").size(text_size)).min_size(button_size);

         ScrollArea::vertical().show(ui, |ui| {
            if ui.add(button).clicked() {
               ctx.write(|ctx| ctx.data_syncing = !ctx.data_syncing);
            }

            let button = Button::new(RichText::new("On Startup Syncing").size(text_size))
               .min_size(button_size);

            if ui.add(button).clicked() {
               ctx.write(|ctx| ctx.on_startup_syncing = !ctx.on_startup_syncing);
            }

            let button = Button::new(RichText::new("Unknown Tx Analysis 1").size(text_size))
               .min_size(button_size);

            if ui.add(button).clicked() {
               let ctx_clone = ctx.clone();

               RT.spawn_blocking(move || {
                  let params =
                     TransactionAction::dummy_erc20_transfer().erc20_transfer_params().clone();
                  let analysis = TransactionAnalysis {
                     chain: 1,
                     contract_interact: true,
                     decoded_selector: "Unknown".to_string(),
                     erc20_transfers: vec![params.clone(), params.clone(), params],
                     gas_used: 160_000,
                     value: NumericValue::parse_to_wei("1", 18).wei(),
                     ..Default::default()
                  };

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

            let button = Button::new(RichText::new("Unknown Tx Analysis 2").size(text_size))
               .min_size(button_size);

            if ui.add(button).clicked() {
               let ctx_clone = ctx.clone();

               RT.spawn_blocking(move || {
                  let params =
                     TransactionAction::dummy_erc20_transfer().erc20_transfer_params().clone();
                  let unwrap = TransactionAction::dummy_unwrap_weth().unwrap_weth_params().clone();
                  let analysis = TransactionAnalysis {
                     chain: 1,
                     contract_interact: true,
                     decoded_selector: "Unknown".to_string(),
                     erc20_transfers: vec![params.clone(), params.clone()],
                     weth_unwraps: vec![unwrap],
                     gas_used: 160_000,
                     eth_balance_before: NumericValue::default().wei(),
                     eth_balance_after: NumericValue::parse_to_wei("1", 18).wei(),
                     ..Default::default()
                  };

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
                  let params = TransactionAction::dummy_wrap_eth().wrap_eth_params().clone();
                  let analysis = TransactionAnalysis {
                     chain: 1,
                     contract_interact: true,
                     decoded_selector: "Deposit".to_string(),
                     eth_wraps: vec![params],
                     gas_used: 160_000,
                     ..Default::default()
                  };

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
                  let params = TransactionAction::dummy_unwrap_weth().unwrap_weth_params().clone();
                  let analysis = TransactionAnalysis {
                     chain: 1,
                     contract_interact: true,
                     decoded_selector: "Withdraw".to_string(),
                     weth_unwraps: vec![params],
                     gas_used: 160_000,
                     ..Default::default()
                  };

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
                  let params = TransactionAction::dummy_swap().swap_params().clone();
                  let analysis = TransactionAnalysis {
                     chain: 1,
                     contract_interact: true,
                     decoded_selector: "Swap".to_string(),
                     swaps: vec![params],
                     gas_used: 160_000,
                     ..Default::default()
                  };

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
                  let analysis = TransactionAnalysis {
                     chain: 1,
                     value: NumericValue::parse_to_wei("1", 18).wei(),
                     contract_interact: false,
                     decoded_selector: "Transfer".to_string(),
                     gas_used: 21_000,
                     ..Default::default()
                  };
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
                  let params =
                     TransactionAction::dummy_erc20_transfer().erc20_transfer_params().clone();
                  let analysis = TransactionAnalysis {
                     chain: 1,
                     contract_interact: true,
                     decoded_selector: "ERC20 Transfer".to_string(),
                     gas_used: 50_000,
                     erc20_transfers: vec![params],
                     ..Default::default()
                  };
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
                  let params =
                     TransactionAction::dummy_token_approve().token_approval_params().clone();
                  let analysis = TransactionAnalysis {
                     chain: 1,
                     contract_interact: true,
                     decoded_selector: "Approve".to_string(),
                     gas_used: 50_000,
                     token_approvals: vec![params],
                     ..Default::default()
                  };
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
                  let params = TransactionAction::dummy_uniswap_position_operation()
                     .uniswap_position_params()
                     .clone();
                  let analysis = TransactionAnalysis {
                     chain: 1,
                     contract_interact: true,
                     decoded_selector: "AddLiquidity".to_string(),
                     gas_used: 120_000,
                     positions_ops: vec![params],
                     ..Default::default()
                  };
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
                  let params = TransactionAction::dummy_bridge().bridge_params().clone();
                  let analysis = TransactionAnalysis {
                     chain: 1,
                     contract_interact: true,
                     decoded_selector: "Bridge".to_string(),
                     gas_used: 120_000,
                     bridge: vec![params],
                     ..Default::default()
                  };
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
                     gui.sign_msg_window.open("app.uniswap.org".to_string(), 8453, msg);
                  });
               });
            }

            let progress_window =
               Button::new(RichText::new("Progress Window").size(text_size)).min_size(button_size);

            if ui.add(progress_window).clicked() {
               RT.spawn_blocking(move || {
                  SHARED_GUI.write(|gui| {
                     gui.progress_window.open_test();
                  });

                  std::thread::sleep(std::time::Duration::from_secs(1));
                  SHARED_GUI.write(|gui| {
                     gui.progress_window.proceed_with("step2");
                  });

                  std::thread::sleep(std::time::Duration::from_secs(1));
                  SHARED_GUI.write(|gui| {
                     gui.progress_window.proceed_with("step3");
                  });

                  std::thread::sleep(std::time::Duration::from_secs(1));
                  SHARED_GUI.write(|gui| {
                     gui.progress_window.finish_last_step();
                  });
               });
            }
         });
      });
   }
}
