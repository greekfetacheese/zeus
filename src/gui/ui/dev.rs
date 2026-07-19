use eframe::egui::{Align2, Button, Frame, Order, RichText, ScrollArea, Ui, Window, vec2};
use zeus_widgets::Label;

use crate::assets::Icons;
use crate::core::{DecodedEvent, SignMsgType, TransactionAnalysis, ZeusContext};
use crate::gui::{SHARED_GUI, ui::notification::NotificationType};
use crate::utils::RT;
use crate::utils::self_update::UpdateInfo;

use zeus_eth::currency::ERC20Token;

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use zeus_theme::Theme;

pub struct DevUi {
   pub open: bool,
   pub ui_testing: UiTesting,
   pub size: (f32, f32),
}

impl DevUi {
   pub fn new() -> Self {
      Self {
         open: false,
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

   pub fn show(&mut self, ctx: &mut ZeusContext, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      if !self.open {
         return;
      }

      self.show_ui_testing(ctx, theme, icons, ui);

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
      });
   }

   fn show_ui_testing(
      &mut self,
      ctx: &mut ZeusContext,
      theme: &Theme,
      icons: Arc<Icons>,
      ui: &mut Ui,
   ) {
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
            self.ui_testing.show(ctx, theme, icons, ui);
         });

      if !open {
         self.ui_testing.close();
      }
   }
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

   pub fn show(&mut self, ctx: &mut ZeusContext, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      if !self.open {
         return;
      }

      self.icon_window(theme, icons, ui);

      ui.vertical_centered(|ui| {
         ui.set_width(self.size.0);
         ui.set_height(self.size.1);
         ui.spacing_mut().item_spacing.y = 20.0;
         let button_size = vec2(self.size.0, 50.0);

         let text_size = theme.text_sizes.normal;

         ScrollArea::vertical().show(ui, |ui| {
            let button =
               Button::new(RichText::new("Icon Window").size(text_size)).min_size(button_size);
            if ui.add(button).clicked() {
               self.icon_window_open = true;
            }

            let button =
               Button::new(RichText::new("Update Window").size(text_size)).min_size(button_size);
            if ui.add(button).clicked() {
               RT.spawn_blocking(move || {
                  SHARED_GUI.write(|gui| {
                     let mut info = UpdateInfo::default();
                     info.available = true;
                     gui.update_window.open(info);
                  });
               });
            }

            let button =
               Button::new(RichText::new("Confirm Window").size(text_size)).min_size(button_size);

            if ui.add(button).clicked() {
               RT.spawn_blocking(move || {
                  SHARED_GUI.write(|gui| {
                     let msg2 = "Continue without MEV protection?";
                     gui.confirm_window.open("No available MEV protect RPC found");
                     gui.confirm_window.set_msg2(msg2);
                  });
               });
            }

            let button =
               Button::new(RichText::new("Msg Window").size(text_size)).min_size(button_size);

            if ui.add(button).clicked() {
               RT.spawn_blocking(move || {
                  SHARED_GUI.write(|gui| {
                     gui.msg_window.open("Error", "Error message goes here");
                  });
               });
            }

            let button =
               Button::new(RichText::new("Swap Notification With Progress Bar").size(text_size))
                  .min_size(button_size);

            let dummy_swap = DecodedEvent::dummy_swap().clone();
            let dummy_bridge = DecodedEvent::dummy_bridge().clone();
            let dummy_transfer = DecodedEvent::dummy_transfer().clone();
            let dummy_approval = DecodedEvent::dummy_token_approve().clone();
            let dummy_shield = DecodedEvent::dummy_shield().clone();

            let swap_title = dummy_swap.name();
            let bridge_title = dummy_bridge.name();
            let transfer_title = dummy_transfer.name();
            let approval_title = dummy_approval.name();
            let shield_title = dummy_shield.name();

            let swap_notification = NotificationType::from_main_event(dummy_swap);
            let bridge_notification = NotificationType::from_main_event(dummy_bridge);
            let transfer_notification = NotificationType::from_main_event(dummy_transfer);
            let approval_notification = NotificationType::from_main_event(dummy_approval);
            let shield_notification = NotificationType::from_main_event(dummy_shield);

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

            let button = Button::new(RichText::new("Shield with Progress Bar").size(text_size))
               .min_size(button_size);

            if ui.add(button).clicked() {
               let notification_clone = shield_notification.clone();
               RT.spawn_blocking(move || {
                  let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
                  let finish_on = now + 5;
                  SHARED_GUI.write(|gui| {
                     gui.notification.open_with_progress_bar(
                        now,
                        finish_on,
                        shield_title,
                        notification_clone,
                        None,
                     );
                  });
               });
            }

            let button =
               Button::new(RichText::new("Data Syncing").size(text_size)).min_size(button_size);

            if ui.add(button).clicked() {
               ctx.data_syncing = !ctx.data_syncing;
            }

            let button = Button::new(RichText::new("On Startup Syncing").size(text_size))
               .min_size(button_size);

            if ui.add(button).clicked() {
               ctx.on_startup_syncing = !ctx.on_startup_syncing;
            }

            let button =
               Button::new(RichText::new("Railgun Syncing").size(text_size)).min_size(button_size);

            if ui.add(button).clicked() {
               let chain = ctx.chain;
               let is_syncing = ctx.is_railgun_provider_syncing(chain.id());
               ctx.railgun_provider_syncing.insert(chain.id(), !is_syncing);
            }

            let button =
               Button::new(RichText::new("Railgun Unshield").size(text_size)).min_size(button_size);

            if ui.add(button).clicked() {
               let analysis = TransactionAnalysis::dummy_unshield();
               RT.spawn_blocking(move || {
                  SHARED_GUI.write(|gui| {
                     let ctx = gui.ctx.clone();

                     gui.tx_confirmation_window.open(
                        ctx.clone(),
                        "".to_string(),
                        ctx.chain(),
                        analysis,
                        "1".to_string(),
                        false,
                        true,
                     );
                  });
               });
            }

            let button =
               Button::new(RichText::new("Railgun Shield").size(text_size)).min_size(button_size);

            if ui.add(button).clicked() {
               let analysis = TransactionAnalysis::dummy_shield();
               RT.spawn_blocking(move || {
                  SHARED_GUI.write(|gui| {
                     let ctx = gui.ctx.clone();

                     gui.tx_confirmation_window.open(
                        ctx.clone(),
                        "".to_string(),
                        ctx.chain(),
                        analysis,
                        "1".to_string(),
                        false,
                        false,
                     );
                  });
               });
            }

            let button =
               Button::new(RichText::new("EOA Delegate").size(text_size)).min_size(button_size);

            if ui.add(button).clicked() {
               let analysis = TransactionAnalysis::dummy_eoa_delegate();
               RT.spawn_blocking(move || {
                  SHARED_GUI.write(|gui| {
                     let ctx = gui.ctx.clone();

                     gui.tx_confirmation_window.open(
                        ctx.clone(),
                        "".to_string(),
                        ctx.chain(),
                        analysis,
                        "1".to_string(),
                        false,
                        false,
                     );
                  });
               });
            }

            let button = Button::new(RichText::new("Unknown Tx Analysis").size(text_size))
               .min_size(button_size);

            if ui.add(button).clicked() {
               RT.spawn_blocking(move || {
                  let analysis = TransactionAnalysis::unknown_tx_1();
                  SHARED_GUI.write(|gui| {
                     let ctx = gui.ctx.clone();

                     gui.tx_confirmation_window.open(
                        ctx.clone(),
                        "".to_string(),
                        ctx.chain(),
                        analysis,
                        "1".to_string(),
                        true,
                        false,
                     );
                  });
               });
            }

            let button = Button::new(RichText::new("Wrap ETH Analysis").size(text_size))
               .min_size(button_size);

            if ui.add(button).clicked() {
               RT.spawn_blocking(move || {
                  let analysis = TransactionAnalysis::dummy_wrap_eth();
                  SHARED_GUI.write(|gui| {
                     let ctx = gui.ctx.clone();

                     gui.tx_confirmation_window.open(
                        ctx.clone(),
                        "".to_string(),
                        ctx.chain(),
                        analysis,
                        "1".to_string(),
                        true,
                        false,
                     );
                  });
               });
            }

            let button = Button::new(RichText::new("Unwrap WETH Analysis").size(text_size))
               .min_size(button_size);

            if ui.add(button).clicked() {
               RT.spawn_blocking(move || {
                  let analysis = TransactionAnalysis::dummy_unwrap_weth();
                  SHARED_GUI.write(|gui| {
                     let ctx = gui.ctx.clone();

                     gui.tx_confirmation_window.open(
                        ctx.clone(),
                        "".to_string(),
                        ctx.chain(),
                        analysis,
                        "1".to_string(),
                        true,
                        false,
                     );
                  });
               });
            }

            let button =
               Button::new(RichText::new("Swap Tx Analysis").size(text_size)).min_size(button_size);

            if ui.add(button).clicked() {
               RT.spawn_blocking(move || {
                  let analysis = TransactionAnalysis::dummy_swap();
                  SHARED_GUI.write(|gui| {
                     let ctx = gui.ctx.clone();

                     gui.tx_confirmation_window.open(
                        ctx.clone(),
                        "".to_string(),
                        ctx.chain(),
                        analysis,
                        "1".to_string(),
                        true,
                        false,
                     );
                  });
               });
            }

            let button = Button::new(RichText::new("Transfer Analysis").size(text_size))
               .min_size(button_size);

            if ui.add(button).clicked() {
               RT.spawn_blocking(move || {
                  let analysis = TransactionAnalysis::dummy_transfer();
                  SHARED_GUI.write(|gui| {
                     let ctx = gui.ctx.clone();

                     gui.tx_confirmation_window.open(
                        ctx.clone(),
                        "".to_string(),
                        ctx.chain(),
                        analysis,
                        "1".to_string(),
                        true,
                        false,
                     );
                  });
               });
            }

            let button = Button::new(RichText::new("ERC20 Transfer Analysis").size(text_size))
               .min_size(button_size);

            if ui.add(button).clicked() {
               RT.spawn_blocking(move || {
                  let analysis = TransactionAnalysis::dummy_erc20_transfer();
                  SHARED_GUI.write(|gui| {
                     let ctx = gui.ctx.clone();

                     gui.tx_confirmation_window.open(
                        ctx.clone(),
                        "".to_string(),
                        ctx.chain(),
                        analysis,
                        "1".to_string(),
                        true,
                        false,
                     );
                  });
               });
            }

            let button = Button::new(RichText::new("ERC20 Approval Analysis").size(text_size))
               .min_size(button_size);

            if ui.add(button).clicked() {
               RT.spawn_blocking(move || {
                  let analysis = TransactionAnalysis::dummy_token_approval();
                  SHARED_GUI.write(|gui| {
                     let ctx = gui.ctx.clone();

                     gui.tx_confirmation_window.open(
                        ctx.clone(),
                        "".to_string(),
                        ctx.chain(),
                        analysis,
                        "1".to_string(),
                        true,
                        false,
                     );
                  });
               });
            }

            let button =
               Button::new(RichText::new("Permit Analysis").size(text_size)).min_size(button_size);
            if ui.add(button).clicked() {
               RT.spawn_blocking(move || {
                  let analysis = TransactionAnalysis::dummy_permit();
                  SHARED_GUI.write(|gui| {
                     let ctx = gui.ctx.clone();

                     gui.tx_confirmation_window.open(
                        ctx.clone(),
                        "".to_string(),
                        ctx.chain(),
                        analysis,
                        "1".to_string(),
                        true,
                        false,
                     );
                  });
               });
            }

            let button =
               Button::new(RichText::new("Uniswap AddLiquidity V3 Analysis").size(text_size))
                  .min_size(button_size);
            if ui.add(button).clicked() {
               RT.spawn_blocking(move || {
                  let analysis = TransactionAnalysis::dummy_uniswap_position_operation();
                  SHARED_GUI.write(|gui| {
                     let ctx = gui.ctx.clone();

                     gui.tx_confirmation_window.open(
                        ctx.clone(),
                        "".to_string(),
                        ctx.chain(),
                        analysis,
                        "1".to_string(),
                        true,
                        false,
                     );
                  });
               });
            }

            let button =
               Button::new(RichText::new("Bridge Analysis").size(text_size)).min_size(button_size);

            if ui.add(button).clicked() {
               RT.spawn_blocking(move || {
                  let analysis = TransactionAnalysis::dummy_bridge();
                  SHARED_GUI.write(|gui| {
                     let ctx = gui.ctx.clone();

                     gui.tx_confirmation_window.open(
                        ctx.clone(),
                        "".to_string(),
                        ctx.chain(),
                        analysis,
                        "1".to_string(),
                        true,
                        false,
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
                     let ctx = gui.ctx.clone();

                     ctx.write(|ctx| {
                        gui.sign_msg_window.open(ctx, "app.uniswap.org".to_string(), 8453, msg);
                     });
                  });
               });
            }
         });
      });
   }

   fn icon_window(&mut self, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
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
            tracing::info!(
               "Token Icon With Tint took {} μs",
               time.elapsed().as_micros()
            );

            let time = std::time::Instant::now();
            let weth_without_tint = icons.token_icon_x32(weth.address, weth.chain_id, false);
            tracing::info!(
               "Token Icon Without Tint took {} μs",
               time.elapsed().as_micros()
            );

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
