use crate::gui::GUI;
use eframe::egui::Ui;

pub fn show(ui: &mut Ui, gui: &mut GUI) {
   let ctx = gui.ctx.clone();
   let logged_in = ctx.logged_in();
   let account = ctx.account_exists();
   let theme = &gui.theme;
   let icons = gui.icons.clone();
   let token_selection = &mut gui.token_selection;
   let recipient_selection = &mut gui.recipient_selection;

   gui.testing_window.show(theme, ui);
   gui.msg_window.show(theme, ui);
   gui.loading_window.show(ui);
   gui.send_crypto.tx_success_window.show(theme, ui);

   if !account {
      gui.register.show(ctx.clone(), theme, ui);
      gui.portofolio.open = false;
      // ! We could early return but for some reason the whole window becomes transparent
   }

   if account && !logged_in {
      gui.login.show(ctx.clone(), theme, ui);
      gui.portofolio.open = false;
   }

   gui.portofolio
      .show(ctx.clone(), theme, icons.clone(), token_selection, ui);
   gui.swap_ui
      .show(ctx.clone(), icons.clone(), theme, token_selection, ui);
   gui.settings.show(ctx.clone(), icons.clone(), theme, ui);
   gui.send_crypto
      .show(ctx.clone(), icons.clone(), theme, token_selection, recipient_selection, ui);

   gui.wallet_ui.show(ctx.clone(), theme, icons.clone(), ui);
   gui.tx_history.show(ctx.clone(), theme, icons.clone(), ui);
   gui.across_bridge.show(ctx.clone(), theme, icons.clone(), recipient_selection, ui);

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
