use crate::{
   assets::icons::Icons,
   core::{
      utils::{sign::SignMsgType, tx::TxSummary, RT}, ZeusCtx
   },
   gui::{GUI, SHARED_GUI},
};
use eframe::egui::{Button, Ui, vec2};
use egui_theme::Theme;
use std::sync::Arc;

pub fn show(ui: &mut Ui, gui: &mut GUI) {
   let ctx = gui.ctx.clone();
   let logged_in = ctx.logged_in();
   let account = ctx.account_exists();
   let theme = &gui.theme;
   let icons = gui.icons.clone();
   let token_selection = &mut gui.token_selection;
   let recipient_selection = &mut gui.recipient_selection;
   let contacts_ui = &mut gui.settings.contacts_ui;

   gui.tx_confirm_window
      .show(ctx.clone(), theme, icons.clone(), ui);
   gui.confirm_window.show(theme, ui);
   gui.testing_window.show(theme, icons.clone(), ui);
   gui.progress_window.show(theme, ui);
   gui.msg_window.show(theme, ui);
   gui.loading_window.show(ui);

   gui.sign_msg_window.show(theme, icons.clone(), ui);
   gui.ui_testing.show(ctx.clone(), theme, icons.clone(), ui);

   if !account {
      gui.register.show(ctx.clone(), theme, ui);
      gui.portofolio.open = false;
      // ! We could early return but for some reason the whole window becomes transparent
   }

   if account && !logged_in {
      gui.login.show(ctx.clone(), theme, ui);
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
   gui.swap_ui.show(
      ctx.clone(),
      icons.clone(),
      theme,
      token_selection,
      ui,
   );
   gui.settings.show(ctx.clone(), icons.clone(), theme, ui);

   gui.wallet_ui.show(ctx.clone(), theme, icons.clone(), ui);
   gui.tx_history.show(ctx.clone(), theme, ui);

   #[cfg(feature = "dev")]
   {
      let theme = gui.editor.show(&mut gui.theme, ui);
      if let Some(theme) = theme {
         gui.theme = theme;
      }
   }
}

#[allow(dead_code)]
fn should_show_overlay(gui: &mut GUI) {
   if gui.settings.credentials.open {
      gui.show_overlay = true;
   } else if gui.msg_window.open {
      gui.show_overlay = true;
   } else if gui.loading_window.open {
      gui.show_overlay = true;
   } else if gui.settings.contacts_ui.open {
      gui.show_overlay = true;
   } else if gui.settings.encryption.open {
      gui.show_overlay = true;
   } else if gui.token_selection.open {
      gui.show_overlay = true;
   } else {
      gui.show_overlay = false;
   }
}

pub struct UiTesting {
   pub show: bool,
}

impl UiTesting {
   pub fn new() -> Self {
      Self { show: false }
   }

   pub fn show(&mut self, _ctx: ZeusCtx, _theme: &Theme, _icons: Arc<Icons>, ui: &mut Ui) {
      if !self.show {
         return;
      }

      ui.vertical_centered(|ui| {
         ui.set_min_size(vec2(500.0, 500.0));
         ui.spacing_mut().item_spacing.y = 10.0;
         let btn_size = vec2(100.0, 25.0);

         let button = Button::new("Swap Transaction Summary").min_size(btn_size);
         if ui.add(button).clicked() {
            RT.spawn_blocking(move || {
               let summary = TxSummary::dummy_swap();
               SHARED_GUI.write(|gui| {
                  gui.tx_confirm_window.open_with_summary(summary);
               });
            });
         }

         let button = Button::new("Token Approval Transaction Summary").min_size(btn_size);
         if ui.add(button).clicked() {
            RT.spawn_blocking(move || {
               let summary = TxSummary::dummy_token_approve();
               SHARED_GUI.write(|gui| {
                  gui.tx_confirm_window.open_with_summary(summary);
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
