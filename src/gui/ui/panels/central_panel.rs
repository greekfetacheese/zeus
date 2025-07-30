use crate::{
   assets::icons::Icons,
   core::{
      TransactionAction, TransactionAnalysis, ZeusCtx,
      utils::{RT, sign::SignMsgType},
   },
   gui::{GUI, SHARED_GUI},
};
use eframe::egui::{Button, Frame, Ui, Window, vec2};
use egui::RichText;
use egui_theme::Theme;
use std::sync::Arc;

use zeus_eth::utils::NumericValue;

pub fn show(ui: &mut Ui, gui: &mut GUI) {
   let ctx = gui.ctx.clone();
   let logged_in = ctx.logged_in();
   let account_exists = ctx.account_exists();
   let theme = &gui.theme;
   let icons = gui.icons.clone();
   let token_selection = &mut gui.token_selection;
   let recipient_selection = &mut gui.recipient_selection;
   let contacts_ui = &mut gui.settings.contacts_ui;

   gui.tx_confirmation_window.show(ctx.clone(), theme, icons.clone(), ui);

   gui.tx_window.show(ctx.clone(), theme, icons.clone(), ui);

   gui.confirm_window.show(theme, ui);

   gui.testing_window.show(theme, icons.clone(), ui);

   gui.progress_window.show(theme, ui);

   gui.msg_window.show(theme, ui);

   gui.loading_window.show(ui);

   gui.sign_msg_window.show(ctx.clone(), theme, icons.clone(), ui);

   gui.ui_testing.show(ctx.clone(), theme, icons.clone(), ui);

   if !account_exists {
      gui.portofolio.open = false;
   }

   gui.register.show(ctx.clone(), theme, icons.clone(), ui);
   gui.login.show(ctx.clone(), theme, icons.clone(), ui);

   if account_exists && !logged_in {
      gui.portofolio.open = false;
   }

   gui.across_bridge.show(
      ctx.clone(),
      theme,
      icons.clone(),
      recipient_selection,
      contacts_ui,
      ui,
   );

   gui.send_crypto.show(
      ctx.clone(),
      icons.clone(),
      theme,
      token_selection,
      recipient_selection,
      contacts_ui,
      ui,
   );

   gui.portofolio.show(
      ctx.clone(),
      theme,
      icons.clone(),
      token_selection,
      ui,
   );

   gui.uniswap.show(
      ctx.clone(),
      theme,
      icons.clone(),
      token_selection,
      ui,
   );

   gui.settings.show(ctx.clone(), icons.clone(), theme, ui);

   gui.wallet_ui.show(ctx.clone(), theme, icons.clone(), ui);
   gui.tx_history.show(ctx.clone(), theme, ui);

   #[cfg(feature = "dev")]
   gui.sync_pools_ui.show(ctx.clone(), theme, ui);

   #[cfg(feature = "dev")]
   gui.fps_metrics.show(ui);

   #[cfg(feature = "dev")]
   {
      let theme = gui.editor.show(&mut gui.theme, ui);
      if let Some(theme) = theme {
         gui.theme = theme;
      }
   }
}

pub struct UiTesting {
   pub show: bool,
}

impl UiTesting {
   pub fn new() -> Self {
      Self { show: false }
   }

   pub fn show(&mut self, ctx: ZeusCtx, _theme: &Theme, _icons: Arc<Icons>, ui: &mut Ui) {
      if !self.show {
         return;
      }

      ui.vertical_centered(|ui| {
         ui.set_min_size(vec2(500.0, 500.0));
         ui.spacing_mut().item_spacing.y = 10.0;
         let btn_size = vec2(100.0, 25.0);

         if ui.button("Data Syncing").clicked() {
            ctx.write(|ctx| ctx.data_syncing = !ctx.data_syncing);
         }

         if ui.button("On Startup Syncing").clicked() {
            ctx.write(|ctx| ctx.on_startup_syncing = !ctx.on_startup_syncing);
         }

         let button = Button::new("Unknown Tx Analysis 1").min_size(btn_size);
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

         let button = Button::new("Unknown Tx Analysis 2").min_size(btn_size);
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

         let button = Button::new("Wrap ETH Analysis").min_size(btn_size);
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

         let button = Button::new("Unwrap WETH Analysis").min_size(btn_size);
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

         let button = Button::new("Swap Tx Analysis").min_size(btn_size);
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

         let button = Button::new("Transfer Analysis").min_size(btn_size);
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

         let button = Button::new("ERC20 Transfer Analysis").min_size(btn_size);
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

         let button = Button::new("ERC20 Approval Analysis").min_size(btn_size);
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

         let button = Button::new("Uniswap AddLiquidity V3 Analysis").min_size(btn_size);
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

         let button = Button::new("Bridge Analysis").min_size(btn_size);
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

         let button = Button::new("Sign Message").min_size(btn_size);
         if ui.add(button).clicked() {
            RT.spawn_blocking(move || {
               let msg = SignMsgType::dummy_permit2();
               SHARED_GUI.write(|gui| {
                  gui.sign_msg_window.open("app.uniswap.org".to_string(), 8453, msg);
               });
            });
         }

         let progress_window = Button::new("Progress Window").min_size(btn_size);
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

         let close = Button::new("Close").min_size(btn_size);
         if ui.add(close).clicked() {
            self.show = false;
         }
      });
   }
}

pub struct FPSMetrics {
   pub open: bool,
   pub max_fps: f64,
   pub time_ms: f64,
   pub time_ns: u128,
}

impl FPSMetrics {
   pub fn new() -> Self {
      Self {
         open: false,
         max_fps: 0.0,
         time_ms: 0.0,
         time_ns: 0,
      }
   }

   pub fn update(&mut self, time_ns: u128) {
      self.time_ns = time_ns;

      if self.time_ns > 0 {
         self.max_fps = 1_000_000_000.0 / self.time_ns as f64;

         self.time_ms = self.time_ns as f64 / 1_000_000.0;
      } else {
         self.max_fps = 0.0;
         self.time_ms = 0.0;
      }
   }

   pub fn show(&mut self, ui: &mut Ui) {
      let mut open = self.open;

      let title = RichText::new("FPS Metrics").size(18.0);
      Window::new(title)
         .open(&mut open)
         .resizable(true)
         .collapsible(true)
         .movable(true)
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_width(150.0);
            ui.set_height(100.0);

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing = vec2(0.0, 5.0);

               let max_fps = RichText::new(format!("Max FPS: {:.2}", self.max_fps)).size(14.0);
               ui.label(max_fps);

               let time_ms = RichText::new(format!("Time: {:.4} ms", self.time_ms)).size(14.0);
               ui.label(time_ms);

               let time_ns = RichText::new(format!("Time: {} ns", self.time_ns)).size(14.0);
               ui.label(time_ns);
            });
         });

      self.open = open;
   }
}
