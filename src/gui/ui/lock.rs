//! The auto-lock screen shown when Zeus is idle-locked.
//!
//! Re-verifies the credentials against the in-memory vault — no Argon2, no
//! disk. Same shape as the vault-unlock window, but it is a *re-login*: the
//! wallet stays in memory, we only gate the UI.

use crate::core::ZeusContext;
use crate::core::types::CredentialCheck;
use crate::gui::SHARED_GUI;
use crate::utils::{RT, TimeStamp};
use egui::{Align2, RichText, Ui, Window, vec2};
use egui_elements::{Button, CredentialsForm, Theme};
use ncrypt_me::Credentials;

pub struct LockScreen {
   credentials_form: CredentialsForm,
   size: (f32, f32),
}

impl LockScreen {
   pub fn new() -> Self {
      let form_size = vec2(550.0 * 0.6, 20.0);
      let credentials_form = CredentialsForm::new()
         .with_min_size(form_size)
         .with_open(true)
         .with_enabled_virtual_keyboard();
      Self {
         credentials_form,
         size: (550.0, 300.0),
      }
   }

   pub fn erase(&mut self) {
      self.credentials_form.erase();
   }

   pub fn show(&mut self, ctx: &mut ZeusContext, theme: &Theme, ui: &mut Ui) {
      if !ctx.locked {
         return;
      }

      let frame = theme.frame1;

      Window::new("Lock_Screen")
         .title_bar(false)
         .movable(false)
         .resizable(false)
         .frame(frame)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .show(ui.ctx(), |ui| {
            ui.set_min_size(vec2(self.size.0, self.size.1));

            let button_visuals = theme.button_visuals();

            ui.vertical_centered(|ui| {
               ui.add_space(10.0);
               ui.spacing_mut().item_spacing.y = theme.spacing.xl;
               ui.spacing_mut().button_padding = theme.button_padding;
               let ui_width = ui.available_width();

               ui.label(RichText::new("Zeus is locked").size(theme.typography.heading));
               ui.label(
                  RichText::new("Enter your credentials to continue")
                     .size(theme.typography.normal)
                     .color(theme.colors.text_muted),
               );

               ui.scope(|ui| {
                  ui.spacing_mut().button_padding = vec2(theme.spacing.xs, theme.spacing.xs);
                  self.credentials_form.show(ui);
               });

               let text = RichText::new("Unlock").size(theme.typography.large);
               let button =
                  Button::new(text).visuals(button_visuals).min_size(vec2(ui_width * 0.50, 35.0));

               if ui.add(button).clicked() {
                  let username = self.credentials_form.username();
                  let password = self.credentials_form.password();
                  let confirm_password = self.credentials_form.confirm_password();
                  let credentials = Credentials::new(username, password, confirm_password);
                  on_unlock(credentials);
               }
            });
         });
   }
}

/// Verify the credentials against the in-memory vault and unlock on success.
///
/// Uses [`crate::core::Vault::credentials_match_login`] (username + password),
/// not `credentials_match`, which also compares the confirm field the login
/// form does not have.
fn on_unlock(credentials: Credentials) {
   RT.spawn_blocking(move || {
      let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());

      match ctx.check_credentials(&credentials, true) {
         CredentialCheck::Matched => {}
         CredentialCheck::Mismatch => {
            SHARED_GUI.write(|gui| {
               gui.open_msg_window("Incorrect credentials");
               gui.request_repaint();
            });
            return;
         }
         // The attempt cap was reached: Zeus is shutting down.
         CredentialCheck::LockedOut => return,
      }

      let now = TimeStamp::now_as_millis().unwrap_or_default().timestamp();

      ctx.write(|ctx| {
         ctx.locked = false;
         ctx.last_activity_ms = now;
      });

      SHARED_GUI.write(|gui| {
         gui.lock_screen.erase();
         gui.request_repaint();
      });
   });
}
