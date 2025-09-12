use eframe::egui::{
   Align2, Button, Frame, Order, RichText, ScrollArea, Ui, Window, vec2,
};

use crate::assets::Icons;
use crate::core::{
   TransactionAnalysis, ZeusCtx,
   transaction::*,
   utils::{RT, sign::SignMsgType},
};
use crate::gui::SHARED_GUI;

use zeus_eth::utils::NumericValue;

use super::sync::SyncPoolsUi;
use egui_theme::Theme;

use std::sync::Arc;

pub struct DevUi {
   pub open: bool,
   pub sync_pools: SyncPoolsUi,
   pub ui_testing: UiTesting,
   pub size: (f32, f32),
}

impl DevUi {
   pub fn new() -> Self {
      Self {
         open: false,
         sync_pools: SyncPoolsUi::new(),
         ui_testing: UiTesting::new(),
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